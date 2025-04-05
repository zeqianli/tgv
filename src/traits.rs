use crate::models::contig::Contig;

pub trait GenomeInterval {
    fn start(&self) -> usize;
    fn end(&self) -> usize;
    fn contig(&self) -> &Contig;

    fn length(&self) -> usize {
        self.end() - self.start() + 1
    }

    fn covers(&self, position: usize) -> bool {
        self.start() <= position && self.end() >= position
    }

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

    fn middle(&self) -> usize {
        (self.start() + self.end() + 1) / 2
    }
}
