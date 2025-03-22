use crate::models::contig::Contig;
use crate::models::{
    message::StateMessage, reference::Reference, services::tracks::TrackService,
    window::ViewingWindow,
};
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
    pub reference: Option<Reference>,

    initial_state_messages: Vec<StateMessage>,
}

impl Settings {
    const SUPPORTED_REFERENCES: [&str; 2] = ["hg19", "hg38"];

    pub fn new(cli: Cli) -> Result<Self, String> {
        if !Self::SUPPORTED_REFERENCES.contains(&cli.reference.as_str()) {
            return Err(format!("Unsupported reference: {}", cli.reference));
        }

        let mut initial_state_messages = Vec::new();

        let mut bam_path = None;
        let mut vcf_path = None;
        let mut bed_path = None;
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

        // Referennce
        let reference = match Reference::from_str(&cli.reference) {
            Ok(reference) => Some(reference),
            Err(e) => {
                initial_state_messages.push(StateMessage::Error(e));
                None
            }
        };

        // Initial region
        match Self::translate_initial_region(&cli.region) {
            Ok(state_message) => initial_state_messages.push(state_message),
            Err(e) => initial_state_messages.push(StateMessage::CommandModeRegisterError(e)),
        }

        Ok(Self {
            bam_path,
            vcf_path,
            bed_path,
            reference,
            initial_state_messages,
        })
    }

    fn translate_initial_region(&region_string: &String) -> Result<StateMessage, String> {
        let region_string = region_string.trim();

        // Interpretation 1: empty input (go to a default location)
        if region_string.is_empty() {
            return Ok(StateMessage::GoToDefault);
        }

        /// Check format
        let split = region_string.split(":").collect::<Vec<&str>>();
        if split.len() > 2 {
            return Err(format!("Cannot interpret the region: {}", region_string));
        }

        // Interpretation 2: genome:position
        if split.len() == 2 {
            match split[1].parse::<usize>() {
                Ok(n) => {
                    return Ok(StateMessage::GotoContigCoordinate(
                        Contig::chrom(&split[0].to_string()),
                        n,
                    ))
                }
                Err(_) => return Err(format!("Invalid genome region: {}", region_string)),
            }
        }

        // Interpretation 3: gene name
        return Ok(StateMessage::GoToGene(region_string.to_string()));
    }
}
