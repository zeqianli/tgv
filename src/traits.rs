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

pub trait IntervalCollection<T: GenomeInterval>: GenomeInterval {
    fn get(&self, idx: usize) -> Option<&T>;
    fn intervals(&self) -> &Vec<T>;
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn idx_at(&self, position: usize) -> Option<usize> {
        if self.is_empty() {
            return None;
        }

        if position < self.start() {
            return None;
        }

        if position > self.end() {
            return None;
        }
        self.intervals()
            .iter()
            .position(|interval| interval.covers(position))
    }

    fn get_at(&self, position: usize) -> Option<(usize, &T)> {
        if self.is_empty() {
            return None;
        }

        if position < self.start() {
            return None;
        }

        if position > self.end() {
            return None;
        }

        for (i, interval) in self.intervals().iter().enumerate() {
            if interval.covers(position) {
                return Some((i, interval));
            }
        }
        None
    }

    fn get_k_before(&self, position: usize, k: usize) -> Option<(usize, &T)> {
        if k == 0 {
            return self.get_at(position);
        }

        if self.is_empty() {
            return None;
        }

        if position < self.start() {
            return None;
        }

        if position > self.end() {
            if self.len() < k {
                return None;
            }
            let idx = self.len() - k;
            return Some((idx, self.get(idx).unwrap()));
        }

        for (i, interval) in self.intervals().iter().enumerate() {
            if interval.end() < position {
                continue;
            }
            if i < k {
                return None;
            }

            let idx = i - k;
            return Some((idx, interval));
        }

        None
    }

    fn get_k_after(&self, position: usize, k: usize) -> Option<(usize, &T)> {
        if k == 0 {
            return self.get_at(position);
        }

        if self.is_empty() {
            return None;
        }

        if position > self.end() {
            return None;
        }

        if position < self.start() {
            if self.len() < k {
                return None;
            }
            let idx = k - 1;
            return Some((idx, self.get(idx).unwrap()));
        }

        for (i, interval) in self.intervals().iter().enumerate() {
            if interval.start() > position {
                continue;
            }
            if i + k > self.len() {
                return None;
            }
            let idx = i + k - 1;
            return Some((idx, self.get(idx).unwrap()));
        }

        None
    }

    fn get_saturating_k_before(&self, position: usize, k: usize) -> Option<(usize, &T)> {
        if k == 0 {
            return self.get_at(position);
        }

        if self.is_empty() {
            return None;
        }

        match self.get_k_before(position, k) {
            Some((idx, interval)) => Some((idx, interval)),
            None => Some((0, self.get(0).unwrap())),
        }
    }

    fn get_saturating_k_after(&self, position: usize, k: usize) -> Option<(usize, &T)> {
        if k == 0 {
            return self.get_at(position);
        }

        if self.is_empty() {
            return None;
        }

        match self.get_k_after(position, k) {
            Some((idx, interval)) => Some((idx, interval)),
            None => Some((self.len() - 1, self.get(self.len() - 1).unwrap())),
        }
    }
}
