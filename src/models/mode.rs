use strum::Display;

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum InputMode {
    Normal,
    Command,
    Help,
}
