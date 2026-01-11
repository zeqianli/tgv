use crate::reference::Reference;
use crate::tracks::UcscHost;
use clap::ValueEnum;

#[derive(Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum BackendType {
    /// Always use UCSC DB / API.
    Ucsc,

    /// Always use local database.
    Local,

    /// If local cache is available, use it. Otherwise, use UCSC DB / API.
    Default,
}

impl Default for BackendType {
    fn default() -> Self {
        BackendType::Default
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Settings {
    /// bam path, bai path
    pub bam_path: Option<(String, String)>,
    pub vcf_path: Option<String>,
    pub bed_path: Option<String>,
    pub reference: Reference,
    pub backend: BackendType,

    pub ucsc_host: UcscHost,

    pub cache_dir: String,
    //pub palette: Palette,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            bam_path: None,
            vcf_path: None,
            bed_path: None,
            reference: Reference::default(),
            backend: BackendType::default(), // Default backend
            ucsc_host: UcscHost::default(),
            cache_dir: shellexpand::tilde("~/.tgv").to_string(),
        }
    }
}
