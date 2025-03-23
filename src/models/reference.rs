use crate::error::TGVError;
use std::fmt;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Reference {
    Hg19,
    Hg38,
}

impl Reference {
    pub const HG19: &str = "hg19";
    pub const HG38: &str = "hg38";
    pub const SUPPORTED_REFERENCES: [&str; 2] = [Self::HG19, Self::HG38];

    pub fn from_str(s: &str) -> Result<Self, TGVError> {
        match s {
            Self::HG19 => Ok(Self::Hg19),
            Self::HG38 => Ok(Self::Hg38),
            _ => Err(TGVError::ParsingError(format!("Invalid reference: {}", s))),
        }
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hg19 => write!(f, "{}", Self::HG19),
            Self::Hg38 => write!(f, "{}", Self::HG38),
        }
    }
}
