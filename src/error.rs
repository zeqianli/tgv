use std::fmt;

use sqlx::Error as SqlxError;
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TGVError {
    CliError(String),

    IOError(String),

    StateError(String),

    ParsingError(String),

    ValueError(String),
}

impl TGVError {
    pub fn is_same_type(&self, other: &TGVError) -> bool {
        matches!(self, other)
    }
}

impl fmt::Display for TGVError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TGVError::CliError(e) => write!(f, "{}", e),
            TGVError::IOError(e) => write!(f, "{}", e),
            TGVError::StateError(e) => write!(f, "{}", e),
            TGVError::ParsingError(e) => write!(f, "{}", e),
            TGVError::ValueError(e) => write!(f, "{}", e),
        }
    }
}

// sqlx error automatic conversion
impl From<SqlxError> for TGVError {
    fn from(e: SqlxError) -> Self {
        TGVError::IOError(e.to_string())
    }
}

// TODO: tracing
