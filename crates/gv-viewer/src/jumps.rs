use crate::viewport::Scale;
use gv_core::intervals::Focus;

#[derive(Debug, Clone, PartialEq)]
pub struct ViewPoint<Z: Scale> {
    pub focus: Focus,
    pub zoom: Z,
}

/// The jump history behind `u` (back) and `Ctrl-r` (forward), like a vim jumplist.
#[derive(Debug)]
pub struct JumpList<Z: Scale> {
    back: Vec<ViewPoint<Z>>,
    forward: Vec<ViewPoint<Z>>,
}

impl<Z: Scale> Default for JumpList<Z> {
    fn default() -> Self {
        Self {
            back: Vec::new(),
            forward: Vec::new(),
        }
    }
}

impl<Z: Scale> JumpList<Z> {
    const CAPACITY: usize = 100;

    pub fn record(&mut self, from: ViewPoint<Z>) {
        if self.back.len() == Self::CAPACITY {
            self.back.remove(0);
        }
        self.back.push(from);
        self.forward.clear();
    }

    pub fn back(&mut self, current: ViewPoint<Z>) -> Option<ViewPoint<Z>> {
        let target = self.back.pop()?;
        self.forward.push(current);
        Some(target)
    }

    pub fn forward(&mut self, current: ViewPoint<Z>) -> Option<ViewPoint<Z>> {
        let target = self.forward.pop()?;
        self.back.push(current);
        Some(target)
    }
}
