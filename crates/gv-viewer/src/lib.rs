//! The viewer logic that TGV's front ends share.
//!
//! A front end, such as the terminal UI in `tgv` or the window in `tgv-gui`, owns a [`Viewer`]
//! and translates its own input events into [`Action`]s. The viewer applies them to the
//! dataset, the viewport, the focus, and the modes, and tells the front end what to redraw.
//! Front ends keep only what depends on their screen: layout geometry, input decoding, widgets,
//! and drawing.
//!
//! [`Viewer`]: viewer::Viewer
//! [`Action`]: message::Action

pub mod colors;
pub mod command;
pub mod jumps;
pub mod keys;
pub mod layout;
pub mod menu;
pub mod message;
pub mod popup;
pub mod prepare;
pub mod register;
pub mod session;
pub mod settings;
pub mod viewer;
pub mod viewport;

/// Common types for front ends that use the shared viewer.
pub mod prelude {
    pub use crate::{
        layout::AreaType,
        message::Action,
        register::{KeyRegisterType, Registers},
        viewer::{Effect, Scene, Viewer},
        viewport::{ColumnScale, PixelScale, Scale, Viewport},
    };
}
