//! Data preparation for drawing, independent of the screen: what to show and where, in bases
//! and screen units. Each front end draws the results with its own primitives.
//!
//! The modules are named after the rendering files that use them.

pub mod alignment;
pub mod coordinate;
pub mod coverage;
pub mod hover;
pub mod sidebar;
pub mod track;
