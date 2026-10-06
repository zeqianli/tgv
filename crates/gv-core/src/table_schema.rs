//! Explicit column names and data types for core DataFrames.
//!
//! Schema types are declarations, independent of the tables' data and application state.
//! Polars schemas describe types without enforcing nullability or genomic invariants.

use polars::prelude::{DataFrame, SchemaRef};

/// The ordered column schema of a DataFrame.
pub trait TableSchema {
    fn schema() -> SchemaRef;

    /// Construct an empty DataFrame with the declared column types.
    fn empty() -> DataFrame {
        DataFrame::full_null(&Self::schema(), 0)
    }
}
