//! HGVS support for tgv, built on ferro-hgvs.
//!
//! - [`query`] parses HGVS search input, such as `KRAS:p.12` or `NM_004985.5:c.35G>A`.
//! - [`prepare`] downloads the reference data for `tgv download`.
//! - [`worker`] owns the loaded data on a dedicated thread. It resolves searches to genomic
//!   coordinates and describes variants and bases in HGVS.
//!
//! All coordinates in this crate's API are 1-based and inclusive.

pub mod assembly;
pub mod error;
pub mod prepare;
pub mod query;
pub mod worker;

pub use assembly::{HgvsBuild, HgvsDataDir};
pub use error::HgvsError;
pub use query::HgvsQuery;
pub use worker::{Engine, HgvsHandle, HgvsService, HgvsStatus};
