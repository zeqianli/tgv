use clap::ValueEnum;

#[derive(Debug, Clone, Eq, PartialEq, ValueEnum)]
pub enum UcscHost {
    Us,
    Eu,
}

impl UcscHost {
    pub fn url(&self) -> String {
        match self {
            UcscHost::Us => "genome-mysql.soe.ucsc.edu".to_string(),
            UcscHost::Eu => "genome-euro-mysql.soe.ucsc.edu".to_string(),
        }
    }
}
