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

    /// Helper function to sort contigs by name.
    /// 1. chromosomes start with "chr" comes first.
    /// 2. Then, if it follows numbers, sort by numbers.
    /// 3. chrX, chrY, chrM / chrMT comes next.
    /// 4. Otherwise, sort by the alphabetical order.
    pub fn contigs_sort(contigs: Vec<Contig>) -> Vec<Contig> {
        let mut sorted_contigs = contigs;
        sorted_contigs.sort_by(Contig::contigs_compare);
        sorted_contigs
    }

    pub fn contigs_compare(a: &Contig, b: &Contig) -> std::cmp::Ordering {
        let a_name = &a.name;
        let b_name = &b.name;

        let a_is_chr = a_name.starts_with("chr");
        let b_is_chr = b_name.starts_with("chr");

        if a_is_chr && !b_is_chr {
            return std::cmp::Ordering::Less;
        } else if !a_is_chr && b_is_chr {
            return std::cmp::Ordering::Greater;
        }

        let numeric_part = |s: &String| -> Option<i32> {
            if s.starts_with("chr") {
                s[3..].parse().ok()
            } else {
                s.parse().ok()
            }
        };

        let a_num = numeric_part(a_name);
        let b_num = numeric_part(b_name);

        if let (Some(na), Some(nb)) = (a_num, b_num) {
            return na.cmp(&nb);
        }
        if a_num.is_some() {
            return std::cmp::Ordering::Less;
        }
        if b_num.is_some() {
            return std::cmp::Ordering::Greater;
        }

        let rank = |s: &String| {
            let s_lower = s.to_lowercase();
            if s_lower == "chrx" {
                1
            } else if s_lower == "chry" {
                2
            } else if s_lower == "chrm" || s_lower == "chrm" {
                3
            } else {
                4
            }
        };

        let a_rank = rank(a_name);
        let b_rank = rank(b_name);

        if a_rank != b_rank {
            return a_rank.cmp(&b_rank);
        }

        a_name.cmp(b_name)
    }
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
