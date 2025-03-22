pub enum Reference {
    Hg19,
    Hg38,
}

impl Reference {
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "hg19" => Ok(Self::Hg19),
            "hg38" => Ok(Self::Hg38),
            _ => Err(format!("Inalid reference: {}", s)),
        }
    }
}
