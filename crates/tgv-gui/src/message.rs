use gv_viewer::message::Action;

#[derive(Debug, Clone, PartialEq)]
pub enum GuiAction {
    Viewer(Action),
    Pan { pixels: f32 },
    ZoomAround { factor: f64, x: f32 },
    Hover(Option<String>),
}

impl From<Action> for GuiAction {
    fn from(action: Action) -> Self {
        GuiAction::Viewer(action)
    }
}
