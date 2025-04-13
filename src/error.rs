use std::fmt;

use sqlx::Error as SqlxError;
#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum TGVError {
    CliError(String),

    IOError(String),

    StateError(String),

    ParsingError(String),

    ValueError(String),
}

impl TGVError {
    pub fn is_same_type(&self, _other: &TGVError) -> bool {
        matches!(self, _other)
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

impl PartialEq for TGVError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TGVError::CliError(_), TGVError::CliError(_)) => true,
            (TGVError::IOError(_), TGVError::IOError(_)) => true,
            (TGVError::StateError(_), TGVError::StateError(_)) => true,
            (TGVError::ParsingError(_), TGVError::ParsingError(_)) => true,
            (TGVError::ValueError(_), TGVError::ValueError(_)) => true,
            _ => false,
        }
    }
}

impl Eq for TGVError {}

// TODO: tracing
