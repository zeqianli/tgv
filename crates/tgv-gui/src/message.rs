//! The window's actions: the shared viewer actions, plus gestures the window predicts before
//! the viewer applies them.

use gv_viewer::message::Action;

#[derive(Debug, Clone, PartialEq)]
pub enum GuiAction {
    /// An action for the viewer.
    Viewer(Action),
    /// Pans the view by pixels; positive moves the content right. The window draws the pan at
    /// once, and sends the viewer whole bases as they add up.
    Pan { pixels: f32 },
    /// Zooms by a factor, keeping the base under an x position in place. A factor above 1
    /// zooms in.
    ZoomAround { factor: f64, x: f32 },
    /// Describes the data under the pointer in the message line, or stops describing it.
    Hover(Option<String>),
}

impl From<Action> for GuiAction {
    fn from(action: Action) -> Self {
        GuiAction::Viewer(action)
    }
}
