use crate::models::contig::Contig;

pub trait GenomeInterval {
    fn contig(&self) -> &Contig;
    fn start(&self) -> usize;
    fn end(&self) -> usize;
    fn length(&self) -> usize {
        self.end() - self.start() + 1
    }

    fn covers(&self, position: usize) -> bool {
        self.start() <= position && self.end() >= position
    }

    #[allow(dead_code)]
    fn overlaps(&self, other: &impl GenomeInterval) -> bool {
        self.contig() == other.contig()
            && self.start() <= other.end()
            && self.end() >= other.start()
    }

    fn contains(&self, other: &impl GenomeInterval) -> bool {
        self.contig() == other.contig()
            && self.start() <= other.start()
            && self.end() >= other.end()
    }

    // The region ends at the end of the genome. Inclusive.
    #[allow(dead_code)]
    fn is_properly_bounded(&self, end: Option<usize>) -> bool {
        match end {
            Some(e) => self.start() <= self.end() && self.end() <= e,
            None => self.start() <= self.end(),
        }
    }

    #[allow(dead_code)]
    fn middle(&self) -> usize {
        (self.start() + self.end()).div_ceil(2)
    }
}
