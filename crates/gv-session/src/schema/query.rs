//! SQL query request and response types, and the table catalog.

use super::inspect::{InspectInterval, InspectWarning};
use crate::error::SessionError;
use crate::functions::{CatalogFunction, OpFunction, OpFunctions};
use crate::tables::{CatalogTable, TABLES};
use gv_core::prelude::*;
use polars::{prelude::*, sql::SQLContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::sync::Arc;

/// Requests a read-only SQL query, optionally over region-scoped tables.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryRequest {
    /// The region that region-scoped tables cover. Omit it to query only the dataset's
    /// `tracks` table.
    pub region: Option<InspectInterval>,
    /// A Polars SQL `SELECT` or `WITH` statement.
    pub sql: String,
    /// The maximum number of rows to return, from 1 to 10000. Defaults to 500.
    pub limit: Option<usize>,
}

impl QueryRequest {
    pub const DEFAULT_LIMIT: usize = 500;
    pub const MAX_LIMIT: usize = 10_000;

    /// Validates the row limit.
    pub fn limit(&self) -> Result<usize, SessionError> {
        let limit = self.limit.unwrap_or(Self::DEFAULT_LIMIT);
        if !(1..=Self::MAX_LIMIT).contains(&limit) {
            return Err(SessionError::InvalidInput {
                field: "limit",
                message: format!("The limit must be 1–{}.", Self::MAX_LIMIT),
            });
        }
        Ok(limit)
    }

    /// Runs the statement over the given tables and collects up to `limit` rows.
    pub fn execute(
        &self,
        tables: Vec<(&'static str, LazyFrame)>,
        limit: usize,
    ) -> Result<(DataFrame, bool), SessionError> {
        // A fresh context per query keeps `CREATE TABLE` and similar statements from persisting.
        // Rejecting them as well keeps the tool's contract read-only.
        let statement = self.sql.trim_start().to_ascii_lowercase();
        if !["select", "with", "("]
            .iter()
            .any(|prefix| statement.starts_with(prefix))
        {
            return Err(SessionError::InvalidInput {
                field: "sql",
                message: "Use a single SELECT or WITH statement.".to_owned(),
            });
        }
        let has_region = self.region.is_some();
        let invalid = |error: PolarsError| {
            let message = error.to_string();
            let hint = if !has_region && message.contains("relation") {
                ". Region tables need the `region` argument; call describe_tables to list them."
            } else {
                ""
            };
            SessionError::InvalidInput {
                field: "sql",
                message: format!("{message}{hint}"),
            }
        };
        let mut context = SQLContext::new().with_function_registry(Arc::new(OpFunctions));
        for (name, frame) in tables {
            context.register(name, frame);
        }
        // One extra row reveals whether the result is truncated.
        let mut frame = context
            .execute(&self.sql)
            .map_err(invalid)?
            .limit(limit as IdxSize + 1)
            .collect()
            .map_err(invalid)?;
        let truncated = frame.height() > limit;
        if truncated {
            frame = frame.head(Some(limit));
        }
        Ok((frame, truncated))
    }
}

/// Names and types one result column.
#[derive(Serialize)]
pub struct QueryColumn {
    pub name: String,
    pub dtype: String,
}

/// Returns query results as column metadata and row arrays.
#[derive(Serialize)]
pub struct QueryResponse {
    pub region: Option<InspectInterval>,
    pub columns: Vec<QueryColumn>,
    pub rows: Vec<Vec<Value>>,
    pub row_count: usize,
    pub truncated: bool,
    pub warnings: Vec<InspectWarning>,
}

impl QueryResponse {
    /// Converts a result frame into JSON rows.
    pub fn from_frame(
        region: Option<InspectInterval>,
        frame: &DataFrame,
        truncated: bool,
        warnings: Vec<InspectWarning>,
    ) -> Result<Self, TGVError> {
        let columns = frame
            .columns()
            .iter()
            .map(|column| QueryColumn {
                name: column.name().to_string(),
                dtype: column.dtype().to_string(),
            })
            .collect();
        let rows = (0..frame.height())
            .map(|row| {
                frame
                    .columns()
                    .iter()
                    .map(|column| Ok(json_value(column.get(row)?)))
                    .collect::<Result<Vec<_>, TGVError>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            region,
            columns,
            row_count: frame.height(),
            rows,
            truncated,
            warnings,
        })
    }
}

/// Converts one Polars value to JSON, keeping numbers, booleans, strings, and lists native.
fn json_value(value: AnyValue) -> Value {
    match value {
        AnyValue::Null => Value::Null,
        AnyValue::Boolean(value) => Value::Bool(value),
        AnyValue::UInt8(value) => value.into(),
        AnyValue::UInt16(value) => value.into(),
        AnyValue::UInt32(value) => value.into(),
        AnyValue::UInt64(value) => value.into(),
        AnyValue::Int8(value) => value.into(),
        AnyValue::Int16(value) => value.into(),
        AnyValue::Int32(value) => value.into(),
        AnyValue::Int64(value) => value.into(),
        AnyValue::Float32(value) => {
            Number::from_f64(f64::from(value)).map_or(Value::Null, Value::Number)
        }
        AnyValue::Float64(value) => Number::from_f64(value).map_or(Value::Null, Value::Number),
        AnyValue::String(value) => value.into(),
        AnyValue::StringOwned(value) => value.as_str().into(),
        AnyValue::List(series) => Value::Array(series.iter().map(json_value).collect()),
        AnyValue::Struct(_, _, fields) => Value::Object(
            fields
                .iter()
                .zip(value._iter_struct_av())
                .map(|(field, value)| (field.name().to_string(), json_value(value)))
                .collect::<Map<_, _>>(),
        ),
        AnyValue::StructOwned(payload) => {
            let (values, fields) = *payload;
            Value::Object(
                fields
                    .iter()
                    .zip(values)
                    .map(|(field, value)| (field.name().to_string(), json_value(value)))
                    .collect::<Map<_, _>>(),
            )
        }
        other => other.to_string().into(),
    }
}

/// Describes every curated table, with usage notes and example queries.
#[derive(Serialize)]
pub struct TablesResponse {
    pub tables: Vec<CatalogTable>,
    pub functions: Vec<CatalogFunction>,
    pub notes: &'static [&'static str],
    pub examples: &'static [QueryExample],
}

/// Shows one example query.
#[derive(Serialize)]
pub struct QueryExample {
    pub description: &'static str,
    pub sql: &'static str,
}

impl TablesResponse {
    pub const NOTES: [&'static str; 8] = [
        "Coordinates are 1-based, and interval ends are inclusive.",
        "Coordinates, counts, and IDs are signed 64-bit integers, so subtracting them can go negative.",
        "For per-base questions, call the functions in `functions` with `cigar_ops.op` and a 1-based position, such as `allele_at(c.op, 88108)`. Filter with `c.ref_start <= pos AND c.ref_end >= pos` first so they read only covering operations; `insertion_after` needs `c.kind = 'I' AND c.ref_start = pos + 1` instead. `offset_at` and `read_offset` are zero-based.",
        "Queries use Polars SQL. CTEs, subqueries, GROUP BY, window functions, and INNER, LEFT, RIGHT, FULL, CROSS, SEMI, and ANTI joins are supported.",
        "An ON clause with inequalities, such as an overlap test, runs as an efficient range join, but only as an inner join. For a left overlap join, aggregate the inner join in a CTE, then LEFT JOIN it back on the left table's key.",
        "Region tables hold data for the `region` argument, at most 100,000 bases. Only the `tracks` table is available without a region.",
        "`reads` holds reads whose aligned span overlaps the region, while `coverage` counts all loaded reads except duplicates and QC failures, so coverage near the region edges can include reads outside `reads`.",
        "Results return at most `limit` rows; `truncated` reports whether more rows exist. Aggregate in SQL instead of returning raw rows when possible.",
    ];

    pub const EXAMPLES: [QueryExample; 6] = [
        QueryExample {
            description: "Allele counts by strand at one position, with base and mapping quality, excluding duplicate, secondary, and supplementary reads. Deletions count as `*`.",
            sql: "SELECT c.track_id, allele_at(c.op, 88108) AS allele, r.reverse, count(*) AS reads, avg(qual_at(c.op, 88108)) AS mean_bq, avg(r.mapq) AS mean_mapq FROM cigar_ops c JOIN reads r ON c.track_id = r.track_id AND c.read_id = r.read_id WHERE c.ref_start <= 88108 AND c.ref_end >= 88108 AND c.kind IN ('M', '=', 'X', 'D') AND NOT r.duplicate AND NOT r.secondary AND NOT r.supplementary GROUP BY 1, 2, 3 ORDER BY 1, 2, 3",
        },
        QueryExample {
            description: "Alleles at every position of a window, using `coverage` as the list of positions.",
            sql: "SELECT v.pos, allele_at(c.op, v.pos) AS allele, count(*) AS reads FROM coverage v JOIN cigar_ops c ON c.ref_start <= v.pos AND c.ref_end >= v.pos WHERE v.track_id = c.track_id AND c.kind IN ('M', '=', 'X', 'D') GROUP BY 1, 2 ORDER BY 1, 2",
        },
        QueryExample {
            description: "Mean depth for every BED target in the region, including targets with no coverage.",
            sql: "WITH depth AS (SELECT b.track_id AS bed_track, b.row_id, avg(c.total) AS mean_depth FROM bed b JOIN coverage c ON c.pos >= b.start AND c.pos <= b.end GROUP BY 1, 2) SELECT b.row_id, b.name, b.start, b.end, coalesce(d.mean_depth, 0) AS mean_depth FROM bed b LEFT JOIN depth d ON b.track_id = d.bed_track AND b.row_id = d.row_id ORDER BY b.start",
        },
        QueryExample {
            description: "Fragment-length histogram in 50-base bins. `/` divides as floating point, so floor it for integer bins.",
            sql: "SELECT CAST(floor(abs(tlen) / 50) AS BIGINT) * 50 AS bin, count(*) AS fragments FROM reads WHERE proper_pair AND first_segment AND NOT duplicate AND NOT secondary AND NOT supplementary GROUP BY bin ORDER BY bin",
        },
        QueryExample {
            description: "Insertions and deletions, with the reads that carry them.",
            sql: "SELECT o.kind, o.op_len, o.ref_start, count(*) AS reads FROM cigar_ops o JOIN reads r ON o.track_id = r.track_id AND o.read_id = r.read_id WHERE o.kind IN ('I', 'D') AND NOT r.duplicate GROUP BY 1, 2, 3 ORDER BY reads DESC",
        },
        QueryExample {
            description: "Passing variants in the region, with the depth at each.",
            sql: "SELECT v.start, v.reference, v.alternate, v.quality_score, c.total FROM variants v JOIN coverage c ON c.pos = v.start WHERE array_contains(v.filters, 'PASS') ORDER BY v.start",
        },
    ];

    pub fn new() -> Result<Self, SessionError> {
        Ok(Self {
            tables: TABLES
                .iter()
                .map(|table| table.catalog())
                .collect::<Result<_, _>>()?,
            functions: OpFunction::ALL.map(OpFunction::catalog).into(),
            notes: &Self::NOTES,
            examples: &Self::EXAMPLES,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gv_core::{alignment::CoverageSchema, bed::BedSchema};
    use rstest::rstest;

    fn request(sql: &str, limit: Option<usize>) -> QueryRequest {
        QueryRequest {
            region: None,
            sql: sql.to_owned(),
            limit,
        }
    }

    fn tables() -> Vec<(&'static str, LazyFrame)> {
        let frame = DataFrame::new(
            4,
            vec![
                Column::new(CoverageSchema::POS.into(), [1i64, 2, 3, 4]),
                Column::new(CoverageSchema::TOTAL.into(), [0i64, 5, 7, 9]),
            ],
        )
        .unwrap();
        vec![("coverage", frame.lazy())]
    }

    #[rstest]
    #[case::create("CREATE TABLE x AS SELECT 1")]
    #[case::drop("DROP TABLE coverage")]
    #[case::explain("EXPLAIN SELECT * FROM coverage")]
    #[case::file_function("SELECT * FROM read_csv('/etc/passwd')")]
    #[case::unknown_table("SELECT * FROM reads")]
    fn rejects_statements(#[case] sql: &str) {
        let error = request(sql, None).execute(tables(), 10).unwrap_err();
        assert!(matches!(
            error,
            SessionError::InvalidInput { field: "sql", .. }
        ));
    }

    /// Tables concatenate per-track frames, and `count(*)` over a union needs 128-bit counts.
    #[test]
    fn counts_concatenated_tables() {
        let base = tables().remove(0).1;
        let doubled = concat([base.clone(), base], UnionArgs::default()).unwrap();
        let (frame, _) = request("SELECT count(*) FROM coverage", None)
            .execute(vec![("coverage", doubled)], 10)
            .unwrap();
        let counts: Vec<_> = frame.columns()[0]
            .cast(&DataType::Int64)
            .unwrap()
            .i64()
            .unwrap()
            .into_no_null_iter()
            .collect();
        assert_eq!(counts, [8]);
    }

    #[rstest]
    #[case::under_limit(10, 4, false)]
    #[case::at_limit(4, 4, false)]
    #[case::over_limit(3, 3, true)]
    fn truncates_at_limit(#[case] limit: usize, #[case] rows: usize, #[case] truncated: bool) {
        let (frame, actual) = request("SELECT * FROM coverage ORDER BY pos", None)
            .execute(tables(), limit)
            .unwrap();
        assert_eq!((frame.height(), actual), (rows, truncated));
    }

    #[rstest]
    #[case::default(None, Ok(QueryRequest::DEFAULT_LIMIT))]
    #[case::max(Some(QueryRequest::MAX_LIMIT), Ok(QueryRequest::MAX_LIMIT))]
    #[case::zero(Some(0), Err(()))]
    #[case::above_max(Some(QueryRequest::MAX_LIMIT + 1), Err(()))]
    fn validates_limit(#[case] limit: Option<usize>, #[case] expected: Result<usize, ()>) {
        assert_eq!(request("SELECT 1", limit).limit().map_err(|_| ()), expected);
    }

    #[test]
    fn joins_on_inequalities() {
        let targets = DataFrame::new(
            2,
            vec![
                Column::new(BedSchema::START.into(), [1i64, 10]),
                Column::new(BedSchema::END.into(), [3i64, 12]),
            ],
        )
        .unwrap();
        let mut tables = tables();
        tables.push(("targets", targets.lazy()));
        let sql = "WITH d AS (SELECT t.start, sum(c.total) AS total FROM targets t JOIN coverage c ON c.pos >= t.start AND c.pos <= t.end GROUP BY t.start) SELECT t.start, coalesce(d.total, 0) AS total FROM targets t LEFT JOIN d ON t.start = d.start ORDER BY t.start";
        let (frame, _) = request(sql, None).execute(tables, 10).unwrap();
        let depths: Vec<i64> = frame
            .column(CoverageSchema::TOTAL)
            .unwrap()
            .i64()
            .unwrap()
            .into_no_null_iter()
            .collect();
        assert_eq!(depths, [12, 0]);
    }
}
