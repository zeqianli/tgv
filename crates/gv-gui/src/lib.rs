//! A graphical genome viewer with TGV's keys and layout.
//!
//! The modules mirror the terminal UI in the `tgv` crate: `app`, `keys`, `layout`, `mouse`,
//! `menu`, `settings`, and `rendering` do in a window what their namesakes do in a terminal.
//! `worker` has no terminal counterpart: it runs the shared viewer on its own thread, so
//! loading data never blocks the window.

pub mod app;
pub mod keys;
pub mod layout;
pub mod menu;
pub mod message;
pub mod mouse;
pub mod pinch;
pub mod rendering;
pub mod settings;
pub mod worker;
