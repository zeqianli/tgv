use crate::error::TGVError;
use crate::{reference::Reference, repository::RepositoryFileIndex};
use std::collections::HashMap;
use std::fmt;
use std::fmt::Display;

/// Which of a contig's names a data source uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContigName {
    /// `Contig::name`.
    Primary,
    /// `Contig::aliases[i]`.
    Alias(usize),
}

#[derive(Debug, Clone)]
pub struct Contig {
    /// Name for displya
    pub name: String,

    /// Aliases:
    /// - chr1 -> 1
    /// - chromAlias table in the UCSC database
    pub aliases: Vec<String>,

    // TODO: drop the option and set it to u64::MAX if the length is unknown?
    pub length: Option<u64>,

    /// The names the reference sequence and the gene track use, if they have the contig.
    sequence_name: Option<ContigName>,
    gene_track_name: Option<ContigName>,
    /// The name each track file uses, for the files that have the contig. Files can name one
    /// contig differently, such as `chr1` and `1`, so each file keeps its own.
    file_names: HashMap<RepositoryFileIndex, ContigName>,
}

impl Contig {
    const APPREVIATABLE_CHROMOSOMES: [&'static str; 25] = [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
        "17", "18", "19", "20", "21", "22", "X", "Y", "MT",
    ];

    pub fn new(name: &str, length: Option<u64>) -> Self {
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
            length,

            sequence_name: None,
            gene_track_name: None,
            file_names: HashMap::new(),
        }
    }

    pub fn add_alias(&mut self, alias: &str) {
        self.aliases.push(alias.to_string());
    }

    // pub fn add_aliases(&mut self, aliases: Vec<String>) {
    //     self.aliases.extend(aliases);
    // }

    // pub fn all_aliases(&self) -> Vec<String> {
    //     let mut all_aliases = Vec::new();
    //     all_aliases.push(self.name.clone());
    //     all_aliases.extend(self.aliases.clone());
    //     all_aliases
    // }

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

    fn resolve(&self, name: ContigName) -> &str {
        match name {
            ContigName::Primary => &self.name,
            ContigName::Alias(i) => &self.aliases[i],
        }
    }

    pub fn get_sequence_name(&self) -> Option<&str> {
        self.sequence_name.map(|name| self.resolve(name))
    }

    pub fn get_gene_track_name(&self) -> Option<&str> {
        self.gene_track_name.map(|name| self.resolve(name))
    }

    /// The name a track file uses for this contig, or `None` if the file doesn't have it.
    pub fn file_name(&self, file: RepositoryFileIndex) -> Option<&str> {
        self.file_names.get(&file).map(|&name| self.resolve(name))
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

/// A data source that names contigs.
pub enum ContigSource {
    /// The reference sequence.
    Sequence,
    /// The gene annotation track of the reference.
    GeneTrack,
    /// A track file: a BAM, VCF, or BED file.
    File(RepositoryFileIndex),
}

/// A collection of contigs. This helps relative contig movements.
#[derive(Clone, Debug)]
pub struct ContigHeader {
    reference: Reference,
    pub contigs: Vec<Contig>,

    /// contig name / aliases -> index
    contig_lookup: HashMap<String, usize>,
}

impl ContigHeader {
    pub fn new(reference: Reference) -> Self {
        Self {
            reference,
            contigs: Vec::new(),
            contig_lookup: HashMap::new(),
        }
    }

    pub fn first(&self) -> Result<usize, TGVError> {
        if self.contigs.is_empty() {
            return Err(TGVError::StateError("No contigs found".to_string()));
        }
        Ok(0)
    }

    pub fn last(&self) -> Result<usize, TGVError> {
        if self.contigs.is_empty() {
            return Err(TGVError::StateError("No contigs found".to_string()));
        }
        Ok(self.contigs.len() - 1)
    }

    pub fn try_get(&self, index: usize) -> Result<&Contig, TGVError> {
        self.get(index).ok_or(TGVError::StateError(format!(
            "Contig index out of bounds: {}",
            index
        )))
    }

    pub fn get(&self, index: usize) -> Option<&Contig> {
        self.contigs.get(index)
    }

    pub fn try_get_index_by_str(&self, contig_name: &str) -> Result<usize, TGVError> {
        self.contig_lookup
            .get(contig_name)
            .cloned()
            .ok_or(TGVError::StateError(format!(
                "Contig {} not found",
                contig_name
            )))
    }

    pub fn update_or_add_contig(
        &mut self,
        name: String,
        length: Option<u64>,
        aliases: Vec<String>,
        source: ContigSource,
    ) -> usize {
        let contig_index = self.contig_lookup.get(&name).cloned().unwrap_or_else(|| {
            // add a new contig
            let contig = Contig::new(&name, length);

            self.contig_lookup.insert(name.clone(), self.contigs.len());

            contig.aliases.iter().for_each(|alias| {
                self.contig_lookup.insert(alias.clone(), self.contigs.len());
            });
            self.contigs.push(contig);

            self.contigs.len() - 1
        });

        let contig = &mut self.contigs[contig_index];

        if length.is_some() {
            contig.length = length
        }

        aliases.into_iter().for_each(|alias| {
            if !contig.aliases.contains(&alias) {
                contig.add_alias(&alias);
                self.contig_lookup.insert(alias.clone(), contig_index);
            }
        });

        // The contig was found or created by this name, so it is the name or an alias.
        let source_name = if name == contig.name {
            ContigName::Primary
        } else {
            ContigName::Alias(
                contig
                    .aliases
                    .iter()
                    .position(|alias| *alias == name)
                    .expect("a looked-up name is the contig's name or an alias"),
            )
        };

        match source {
            ContigSource::Sequence => contig.sequence_name = Some(source_name),
            ContigSource::GeneTrack => contig.gene_track_name = Some(source_name),
            ContigSource::File(file) => {
                contig.file_names.insert(file, source_name);
            }
        }

        contig_index
    }

    /// Forgets the names a removed track file used, and shifts later files of the same kind,
    /// matching the per-kind vectors. Contigs stay, so contig indexes remain valid.
    pub fn remove_file(&mut self, removed: RepositoryFileIndex) {
        for contig in &mut self.contigs {
            contig.file_names = contig
                .file_names
                .drain()
                .filter_map(|(file, name)| Some((file.after_removal(removed)?, name)))
                .collect();
        }
    }

    pub fn next(&self, contig_index: usize, k: usize) -> usize {
        (contig_index + k) % self.contigs.len() // TODO: bound check
    }

    pub fn previous(&self, contig_index: usize, k: usize) -> usize {
        (contig_index + self.contigs.len() - k % self.contigs.len()) % self.contigs.len()
        // TODO: bound check
    }
}

impl Display for ContigHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for contig in &self.contigs {
            writeln!(f, "{}: {:?}", contig.name, contig.length)?;
        }
        Ok(())
    }
}
