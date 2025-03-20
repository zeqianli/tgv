#[derive(Debug, Clone)]
pub enum Contig {
    Chromosome { name: String },
    Contig { name: String },
}

impl Contig {
    const APPREVIATABLE_CHROMOSOMES: [&'static str; 25] = [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
        "17", "18", "19", "20", "21", "22", "X", "Y", "MT",
    ];

    pub fn chrom(s: &String) -> Self {
        if Contig::APPREVIATABLE_CHROMOSOMES.contains(&s.as_str()) {
            Contig::Chromosome {
                name: format!("chr{}", s),
            }
        } else {
            Contig::Chromosome { name: s.clone() }
        }
    }

    pub fn contig(s: &String) -> Self {
        Contig::Contig { name: s.clone() }
    }

    /// Full name with the "chr" prefix, if applicable.
    pub fn full_name(&self) -> String {
        match self {
            Contig::Chromosome { name } => name.clone(),
            Contig::Contig { name } => name.clone(),
        }
    }

    pub fn abbreviated_name(&self) -> String {
        match self {
            Contig::Chromosome { name } => {
                if name.starts_with("chr") {
                    name[3..].to_string()
                } else {
                    name.clone()
                }
            }
            Contig::Contig { name } => name.clone(),
        }
    }
}

impl Eq for Contig {}

impl PartialEq for Contig {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Contig::Chromosome { name: name1 }, Contig::Chromosome { name: name2 }) => {
                name1 == name2
            }
            (Contig::Contig { name: name1 }, Contig::Contig { name: name2 }) => name1 == name2,
            _ => false,
        }
    }
}
