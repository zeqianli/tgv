use crate::models::contig::Contig;
use crate::models::{services::tracks::TrackService, window::ViewingWindow};
use clap::Parser;
use core::str::FromStr;
use noodles_core::region::Region as NoodlesRegion;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// File paths (Currently supports: BAM (sorted and indexed))
    #[arg(value_name = "PATHS")]
    paths: Vec<String>,

    /// Region to view (Currently supports: "12:25398142", "TP53")
    #[arg(short = 'r', long = "region", required = true)]
    region: String,

    /// Reference genome (Currently supports: "hg19", "hg38")
    #[arg(short = 'g', long = "reference", required = true)]
    reference: String,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub bam_path: Option<String>,
    pub vcf_path: Option<String>,
    pub bed_path: Option<String>,
    pub reference: String,

    initial_region_str: String,
}

impl Settings {
    const SUPPORTED_REFERENCES: [&str; 2] = ["hg19", "hg38"];

    pub fn new(cli: Cli) -> Result<Self, String> {
        if !Self::SUPPORTED_REFERENCES.contains(&cli.reference.as_str()) {
            return Err(format!("Unsupported reference: {}", cli.reference));
        }

        let mut settings = Self {
            bam_path: None,
            vcf_path: None,
            bed_path: None,
            initial_region_str: cli.region,
            reference: cli.reference,
        };

        for path in cli.paths {
            if path.ends_with(".bam") && settings.bam_path.is_none() {
                settings.bam_path = Some(path.clone());
            }
            if path.ends_with(".vcf") && settings.vcf_path.is_none() {
                settings.vcf_path = Some(path.clone());
            }
            if path.ends_with(".bed") && settings.bed_path.is_none() {
                settings.bed_path = Some(path.clone());
            }
        }

        Ok(settings)
    }

    pub async fn initial_window(
        &self,
        track_service: &TrackService,
    ) -> Result<ViewingWindow, String> {
        // Option 1: interprete as a genome region
        let parsed_region: Result<NoodlesRegion, noodles_core::region::ParseError> =
            NoodlesRegion::from_str(self.initial_region_str.as_str());
        if let Ok(parsed_region) = parsed_region {
            if let Some(start) = parsed_region.interval().start() {
                return Ok(ViewingWindow::new_basewise_window(
                    Contig::chrom(&parsed_region.name().to_string()), // TODO: length is not used yet.
                    start.get(),
                    0,
                ));
            }
        }

        // Option 2: interprete as a gene name
        let gene = track_service
            .query_gene_name(&self.initial_region_str)
            .await;
        if let Ok(gene) = gene {
            let initial_window = ViewingWindow::new_basewise_window(gene.contig(), gene.start(), 0);
            return Ok(initial_window);
        }

        // Failed to interpret the region
        Err(format!(
            "Cannot interpret the region {}",
            self.initial_region_str
        ))
    }
}
