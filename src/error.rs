use std::fmt;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TGVError {
    CliError(String),

    IOError(String),

    StateError(String),

    ParsingError(String),
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
            _ => write!(f, "{}", self),
        }
    }
}
