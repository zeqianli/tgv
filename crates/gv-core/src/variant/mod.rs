mod repository;
mod variant;
pub use repository::{Bcf, IndexedVcf, PlainVcf, VariantRepositoryEnum};
pub use variant::{Variant, VariantSchema, VariantTable};
