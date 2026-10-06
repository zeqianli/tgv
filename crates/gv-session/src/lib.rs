//! The session that owns a loaded dataset and serves typed commands to front ends and agents.
//!
//! One worker owns the dataset, so the TUI, a GUI, and agent servers can share it without
//! shared mutable state. Callers send commands through a [`SessionHandle`], and the worker
//! handles them one at a time. The session also builds the SQL tables that agents query.

mod error;
mod schema;
mod session;
mod tables;

pub use error::SessionError;
pub use schema::*;
pub use session::{
    DataRequest, Dataset, Request, Requests, Responder, Session, SessionHandle, ViewRequest,
};
pub use tables::{CatalogColumn, CatalogTable, TableScope};
