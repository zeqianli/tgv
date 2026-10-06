//! Explicit column names and data types for core DataFrames.
//!
//! Schema types are declarations, independent of the tables' data and application state.
//! Polars schemas describe types without enforcing nullability or genomic invariants.

use polars::prelude::{DataFrame, SchemaRef};

/// Describes the meaning of one column for users of a table.
pub struct ColumnDoc {
    pub name: &'static str,
    pub description: &'static str,
}

/// The ordered column schema of a DataFrame.
pub trait TableSchema {
    fn schema() -> SchemaRef;

    /// Describe the columns that are meaningful outside the core, in schema order.
    ///
    /// Columns without a description are internal, such as layout and rendering state, and
    /// are not exposed to external queries.
    fn column_docs() -> &'static [ColumnDoc] {
        &[]
    }

    /// Construct an empty DataFrame with the declared column types.
    fn empty() -> DataFrame {
        DataFrame::full_null(&Self::schema(), 0)
    }
}
