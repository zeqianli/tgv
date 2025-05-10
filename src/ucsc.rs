use chrono::Local;
use clap::ValueEnum;

#[derive(Debug, Clone, Eq, PartialEq)]
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

    /// Choose the host based on the local timezone.
    pub fn auto() -> Self {
        let offset = Local::now().offset().local_minus_utc() / 3600;
        if offset >= -12 && offset <= 0 {
            UcscHost::Us
        } else {
            UcscHost::Eu
        }
    }
}
