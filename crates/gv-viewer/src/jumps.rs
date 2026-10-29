//! The jump history behind `u` (back) and `Ctrl-r` (forward), like a vim jumplist.

use crate::viewport::Scale;
use gv_core::intervals::Focus;

/// A position and zoom to return to.
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

    /// Records a new jump.
    pub fn record(&mut self, from: ViewPoint<Z>) {
        if self.back.len() == Self::CAPACITY {
            self.back.remove(0);
        }
        self.back.push(from);
        self.forward.clear();
    }

    /// Returns the view before `current`, and keeps `current` for going forward.
    pub fn back(&mut self, current: ViewPoint<Z>) -> Option<ViewPoint<Z>> {
        let target = self.back.pop()?;
        self.forward.push(current);
        Some(target)
    }

    /// Returns the view after `current`, and keeps `current` for going back.
    pub fn forward(&mut self, current: ViewPoint<Z>) -> Option<ViewPoint<Z>> {
        let target = self.forward.pop()?;
        self.back.push(current);
        Some(target)
    }
}
