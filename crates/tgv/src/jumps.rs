//! The jump history behind `u` (back) and `Ctrl-r` (forward), like a vim jumplist.

use gv_core::intervals::Focus;

/// A position and zoom to return to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewPoint {
    pub focus: Focus,
    pub zoom: u64,
}

/// Views left by jumps, such as searches and agent navigation.
#[derive(Debug, Default)]
pub struct JumpList {
    back: Vec<ViewPoint>,
    forward: Vec<ViewPoint>,
}

impl JumpList {
    /// The number of views kept for going back.
    const CAPACITY: usize = 100;

    /// Records the view a jump left. A new jump discards the views to go forward to.
    pub fn record(&mut self, from: ViewPoint) {
        if self.back.len() == Self::CAPACITY {
            self.back.remove(0);
        }
        self.back.push(from);
        self.forward.clear();
    }

    /// Returns the view before `current`, and keeps `current` for going forward.
    pub fn back(&mut self, current: ViewPoint) -> Option<ViewPoint> {
        let target = self.back.pop()?;
        self.forward.push(current);
        Some(target)
    }

    /// Returns the view after `current`, and keeps `current` for going back.
    pub fn forward(&mut self, current: ViewPoint) -> Option<ViewPoint> {
        let target = self.forward.pop()?;
        self.back.push(current);
        Some(target)
    }
}
