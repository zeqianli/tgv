use crate::error::TGVError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Strand {
    Forward,
    Reverse,
}

impl Strand {
    pub fn from_str(s: String) -> Result<Self, TGVError> {
        match s.as_str() {
            "+" => Ok(Strand::Forward),
            "-" => Ok(Strand::Reverse),
            _ => Err(TGVError::ValueError(format!("Invalid strand: {}", s))),
        }
    }

    pub fn reverse(self) -> Self {
        match self {
            Strand::Forward => Strand::Reverse,
            Strand::Reverse => Strand::Forward,
        }
    }
}
