pub mod alignment;
pub mod bed;
pub mod command;
pub mod contig_header;
pub mod cytoband;
pub mod error;
pub mod intervals;
pub mod logging;
pub mod message;
pub mod normal;
/// Common core types shared by the TUI and server.
pub mod prelude {
    pub use crate::{
        contig_header::ContigHeader,
        error::TGVError,
        intervals::{Focus, GenomeInterval, IntervalTable, Region},
        reference::Reference,
        repository::{Repository, RepositoryFileIndex},
        settings::FilePath,
        state::{CachePolicy, LoadRequest, State},
        table_schema::TableSchema,
    };
}
pub mod gene;
pub mod reference;
pub mod repository;
pub mod sequence;
pub mod settings;
pub mod state;
pub mod strand;
pub mod table_schema;
pub mod tracks;
pub mod variant;
