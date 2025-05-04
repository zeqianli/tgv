use std::default::Default;

#[derive(Debug, Clone, Default)]
pub struct Contig {
    // name should match with the UCSC genome browser.
    pub name: String,

    /// Aliases:
    /// - chr1 -> 1
    /// - chromAlias table in the UCSC database
    pub aliases: Vec<String>,
}

impl Contig {
    const APPREVIATABLE_CHROMOSOMES: [&'static str; 25] = [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
        "17", "18", "19", "20", "21", "22", "X", "Y", "MT",
    ];

    pub fn new(name: &str) -> Self {
        let mut aliases = Vec::new();
        if Contig::APPREVIATABLE_CHROMOSOMES.contains(&name) {
            aliases.push(format!("chr{}", name));
        }

        if name.starts_with("chr") && Contig::APPREVIATABLE_CHROMOSOMES.contains(&&name[3..]) {
            aliases.push(name[3..].to_string());
        }

        Contig {
            name: name.to_string(),
            aliases,
        }
    }

    pub fn alias(&mut self, alias: &str) {
        self.aliases.push(alias.to_string());
    }

    pub fn aliases(&mut self, aliases: Vec<String>) {
        self.aliases.extend(aliases);
    }

    pub fn all_aliases(&self) -> Vec<String> {
        let mut all_aliases = Vec::new();
        all_aliases.push(self.name.clone());
        all_aliases.extend(self.aliases.clone());
        all_aliases
    }

    // #[allow(clippy::self_named_constructors)]
    // pub fn contig(s: &str) -> Self {
    //     Contig::Contig { name: s.to_owned() }
    // }

    // /// Full name with the "chr" prefix, if applicable.
    // pub fn full_name(&self) -> String {
    //     match self {
    //         Contig::newosome { name } => name.clone(),
    //         Contig::Contig { name } => name.clone(),
    //     }
    // }

    // pub fn abbreviated_name(&self) -> String {
    //     match self {
    //         Contig::newosome { name } => {
    //             if let Some(stripped) = name.strip_prefix("chr") {
    //                 stripped.to_string()
    //             } else {
    //                 name.clone()
    //             }
    //         }
    //         Contig::Contig { name } => name.clone(),
    //     }
    // }
}

impl Eq for Contig {}

impl PartialEq for Contig {
    fn eq(&self, other: &Self) -> bool {
        if self.name == other.name {
            return true;
        }

        for alias in other.aliases.iter() {
            if alias == &self.name {
                return true;
            }
        }

        for alias in self.aliases.iter() {
            if alias == &other.name {
                return true;
            }

            for alias in other.aliases.iter() {
                if alias == &self.name {
                    return true;
                }
            }
        }

        false
    }
}
