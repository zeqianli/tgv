//! Saving, writing, and closing the viewer.

use strum::Display;

/// Saving, writing, and closing the viewer.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum SessionAction {
    Quit,
    ForceQuit,
    SaveSession(Option<String>),
    SaveAndQuit(Option<String>),
    WriteTrack(Option<String>),
    WriteAll,
    WriteAndQuit(Option<String>),
    WriteAllAndQuit,
}
