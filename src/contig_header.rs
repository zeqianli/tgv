use crate::error::TGVError;
use crate::{cytoband::Cytoband, reference::Reference};
use std::collections::HashMap;
use std::fmt;
use std::fmt::Display;

/// None: Contig not in the data source
/// Some(None): Contig.name is in the data source
/// Some(Some(i)): Contig.aliases[i] is in the data source
type ContigNameSourceIndex = Option<Option<usize>>;

#[derive(Debug, Clone)]
pub struct Contig {
    /// Name for displya
    pub name: String,

    /// Aliases:
    /// - chr1 -> 1
    /// - chromAlias table in the UCSC database
    pub aliases: Vec<String>,
    pub length: Option<usize>,      // Length
    pub cytoband: Option<Cytoband>, // Cytoband

    cytoband_loaded: bool, // Whether this contig's cytoband has been quried.

    /// Name used for the track database query
    track_name_index: ContigNameSourceIndex,
    sequence_name_index: ContigNameSourceIndex,
    alignment_name_index: ContigNameSourceIndex,
}

impl Contig {
    const APPREVIATABLE_CHROMOSOMES: [&'static str; 25] = [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
        "17", "18", "19", "20", "21", "22", "X", "Y", "MT",
    ];

    pub fn new(name: &str, length: Option<usize>) -> Self {
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
            cytoband: None,
            cytoband_loaded: false,

            track_name_index: None,
            sequence_name_index: None,
            alignment_name_index: None,
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

    fn get_name_by_source_index(&self, index: &ContigNameSourceIndex) -> Option<&String> {
        index.map(|inner| match inner {
            None => &self.name,
            Some(i) => &self.aliases[i],
        })
    }

    pub fn get_alignment_name(&self) -> Option<&String> {
        self.get_name_by_source_index(&self.alignment_name_index)
    }

    pub fn get_sequence_name(&self) -> Option<&String> {
        self.get_name_by_source_index(&self.sequence_name_index)
    }

    pub fn get_track_name(&self) -> Option<&String> {
        self.get_name_by_source_index(&self.track_name_index)
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

pub enum ContigSource {
    Sequence,
    Alignment,
    Track,
}

/// A collection of contigs. This helps relative contig movements.
#[derive(Debug)]
pub struct ContigHeader {
    reference: Reference,
    pub contigs: Vec<Contig>,

    /// contig name / aliases -> index
    contig_lookup: HashMap<String, usize>,

    /// What the contig name is in the bam header.
    /// - None: contig is not in the bam
    /// - Some(None): contig name is the bam header is the main name
    /// - Some(Some(i)): contig name is the ith alias
    bam_contig_str: Vec<Option<Option<usize>>>,
}

impl ContigHeader {
    pub fn new(reference: Reference) -> Self {
        Self {
            reference,
            contigs: Vec::new(),
            contig_lookup: HashMap::new(),
            bam_contig_str: Vec::new(),
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

    pub fn try_update_cytoband(
        &mut self,
        contig_index: usize,
        cytoband: Option<Cytoband>,
    ) -> Result<(), TGVError> {
        if contig_index >= self.contigs.len() {
            return Err(TGVError::StateError(format!(
                "Contig index out of bounds: {}",
                contig_index
            )));
        }

        self.contigs[contig_index].cytoband = cytoband;
        self.contigs[contig_index].cytoband_loaded = true; // can be None
        Ok(())
    }

    pub fn update_or_add_contig(
        &mut self,
        name: String,
        length: Option<usize>,
        aliases: Vec<String>,
        source: ContigSource,
    ) -> usize {
        let contig_index = self.contig_lookup.get(&name).cloned().unwrap_or({
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

        let source_index = if &name == &contig.name {
            None
        } else {
            contig.aliases.iter().position(|x| x == &name)
        };

        match source {
            ContigSource::Alignment => contig.alignment_name_index = Some(source_index),
            ContigSource::Sequence => contig.sequence_name_index = Some(source_index),

            ContigSource::Track => contig.track_name_index = Some(source_index),
        }

        contig_index
    }

    pub fn next(&self, contig_index: &usize, k: usize) -> usize {
        (contig_index + k) % self.contigs.len() // TODO: bound check
    }

    pub fn previous(&self, contig_index: &usize, k: usize) -> usize {
        (contig_index + self.contigs.len() - k % self.contigs.len()) % self.contigs.len()
        // TODO: bound check
    }

    pub fn cytoband_is_loaded(&self, contig_index: usize) -> Result<bool, TGVError> {
        Ok(self.try_get(contig_index)?.cytoband_loaded)
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
