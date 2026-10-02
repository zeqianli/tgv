#[derive(Clone, Debug)]
#[allow(non_snake_case)]
pub struct BaseCoverage {
    pub A: usize,
    pub T: usize,
    pub C: usize,
    pub G: usize,

    pub N: usize,

    // total coverage, exluding softclips
    pub total: usize,

    // Softclip count
    pub softclip: usize,

    // reference_base
    pub reference_base: u8,
}

impl BaseCoverage {
    pub const MAX_DISPLAY_ALLELE_FREQUENCY_RECIPROCOL: usize = 100;
    pub fn new(reference_base: u8) -> Self {
        Self {
            A: 0,
            T: 0,
            C: 0,
            G: 0,
            N: 0,
            total: 0,
            softclip: 0,
            reference_base,
        }
    }

    pub fn update(&mut self, base: u8) {
        match base {
            b'A' | b'a' => self.A += 1,
            b'T' | b't' => self.T += 1,
            b'C' | b'c' => self.C += 1,
            b'G' | b'g' => self.G += 1,

            _ => self.N += 1,
        }

        self.total += 1;
    }

    pub fn update_softclip(&mut self, _base: u8) {
        self.softclip += 1
    }

    pub fn add(&mut self, other: &BaseCoverage) {
        self.A += other.A;
        self.T += other.T;
        self.C += other.C;
        self.G += other.G;
        self.total += other.total;
        self.softclip += other.softclip;
    }

    pub fn max_alt_depth(&self) -> Option<usize> {
        match self.reference_base {
            b'A' | b'a' => Some(usize::max(self.C, self.T)),
            b'T' | b't' => Some(usize::max(self.A, self.C)),
            b'C' | b'c' => Some(usize::max(self.A, self.T)),
            b'G' | b'g' => Some(usize::max(self.C, self.T)),
            _ => None,
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "A:{}, T:{}, C:{}, G:{}, N:{}, total:{}",
            self.A, self.T, self.C, self.G, self.N, self.total
        )
    }
}

impl Default for BaseCoverage {
    fn default() -> Self {
        DEFAULT_COVERAGE.clone()
    }
}

pub static DEFAULT_COVERAGE: BaseCoverage = BaseCoverage {
    A: 0,
    T: 0,
    C: 0,
    G: 0,
    N: 0,
    total: 0,
    softclip: 0,
    reference_base: b'N',
};
