use crate::models::contig::Contig;

/// A genomic region.
/// An alternative is the `noodles::core::Region` type.
/// Using this lightweight struct for now. Consider switching to noodles::core::Region in the future. That one is more robust to bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub contig: Contig,

    /// Start coordinate of a genome region.
    /// 1-based, inclusive.
    pub start: usize,

    /// End coordinate of a genome region.
    /// 1-based, inclusive.
    pub end: usize,
}

impl Region {
    pub fn new(contig: Contig, start: usize, end: usize) -> Result<Self, ()> {
        if end < start {
            return Err(());
        }

        Ok(Self { contig, start, end })
    }

    pub fn to_string(&self) -> String {
        format!("{}:{}-{}", self.contig.full_name(), self.start, self.end)
    }

    /// Width of a genome region.
    pub fn width(&self) -> usize {
        self.end - self.start + 1
    }

    /// Middle coordinate of a genome region.
    /// 1-based, inclusive.
    /// If the region has an even number of bases, this returns the right to the middle.
    /// This is to be consistent with the ViewingWindow::middle() method.
    pub fn middle(&self) -> usize {
        (self.start + self.end + 1) / 2
    }
}
