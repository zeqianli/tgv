use crate::contig::Contig;
use crate::region::Region;
/// Sequences of a genome region.
pub struct Sequence {
    /// 1-based genome coordinate of sequence[0].
    /// 1-based, inclusive.
    pub start: usize,

    /// Genome sequence
    pub sequence: String,

    /// Contig name
    pub contig: Contig,
}

impl Sequence {
    pub fn new(start: usize, sequence: String, contig: Contig) -> Result<Self, ()> {
        if usize::MAX - start < sequence.len() {
            return Err(());
        }

        Ok(Self {
            contig,
            start,
            sequence,
        })
    }

    /// Sequence start. 1-based, inclusive.
    pub fn start(&self) -> usize {
        self.start
    }

    /// Sequence length.
    pub fn len(&self) -> usize {
        self.sequence.len()
    }

    /// Sequence end. 1-based, inclusive.
    pub fn end(&self) -> usize {
        self.start + self.sequence.len() - 1
    }
}

impl Sequence {
    /// Get the sequence in [left, right].
    /// 1-based, inclusive.
    pub fn get_sequence(&self, region: &Region) -> Option<String> {
        if !self.has_complete_data(region) {
            return None;
        }

        Some(
            self.sequence
                .get(region.start - self.start..region.end - self.start + 1)
                .unwrap()
                .to_string(),
        )
    }

    /// Whether the sequence has complete data in [left, right].
    /// 1-based, inclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        (region.contig == self.contig)
            && ((region.start >= self.start()) && (region.end <= self.end()))
    }
}
