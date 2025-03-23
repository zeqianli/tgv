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

    pub initial_state_messages: Vec<StateMessage>,
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
            if path.ends_with(".bam") {
                bam_path = Some(path.clone());
            }
            if path.ends_with(".vcf") {
                vcf_path = Some(path.clone());
            }
            if path.ends_with(".bed") {
                bed_path = Some(path.clone());
            }
        }

        // Referennce
        let reference = match Reference::from_str(&cli.reference) {
            Ok(reference) => Some(reference),
            Err(e) => {
                initial_state_messages.push(StateMessage::NormalModeRegisterError(e));
                None
            }
        };

        // Initial region
        match Self::translate_initial_state_messages(&cli.region) {
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

    fn translate_initial_state_messages(region_string: &String) -> Result<StateMessage, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::contig::Contig;
    use crate::models::message::StateMessage;
    use crate::models::reference::Reference;

    #[test]
    fn test_cli_parsing_with_valid_inputs() {
        let cli = Cli {
            paths: vec![
                "test.bam".to_string(),
                "test.vcf".to_string(),
                "test.bed".to_string(),
            ],
            region: "chr1:12345".to_string(),
            reference: "hg38".to_string(),
        };

        let settings = Settings::new(cli).unwrap();

        assert_eq!(settings.bam_path, Some("test.bam".to_string()));
        assert_eq!(settings.vcf_path, Some("test.vcf".to_string()));
        assert_eq!(settings.bed_path, Some("test.bed".to_string()));
        assert_eq!(
            settings.reference,
            Some(Reference::from_str("hg38").unwrap())
        );

        // Check that the initial state message is correct
        assert!(matches!(
            settings.initial_state_messages[0],
            StateMessage::GotoContigCoordinate(contig, pos)
            if contig == Contig::chrom(&"chr1".to_string()) && pos == 12345
        ));
    }

    #[test]
    fn test_cli_with_invalid_reference() {
        let cli = Cli {
            paths: vec!["test.bam".to_string()],
            region: "chr1:12345".to_string(),
            reference: "invalid_ref".to_string(),
        };

        let result = Settings::new(cli);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Unsupported reference: invalid_ref");
    }

    #[test]
    fn test_translate_initial_state_messages_empty() {
        let result = Settings::translate_initial_state_messages(&"".to_string());
        assert!(matches!(result.unwrap(), StateMessage::GoToDefault));
    }

    #[test]
    fn test_translate_initial_state_messages_contig_coordinate() {
        let result = Settings::translate_initial_state_messages(&"chr1:12345".to_string());
        assert!(matches!(
            result.unwrap(),
            StateMessage::GotoContigCoordinate(contig, pos)
            if contig == Contig::chrom(&"chr1".to_string()) && pos == 12345
        ));
    }

    #[test]
    fn test_translate_initial_state_messages_invalid_format() {
        let result = Settings::translate_initial_state_messages(&"chr1:pos:extra".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_translate_initial_state_messages_invalid_position() {
        let result = Settings::translate_initial_state_messages(&"chr1:not_a_number".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_translate_initial_state_messages_gene_name() {
        let result = Settings::translate_initial_state_messages(&"TP53".to_string());
        assert!(matches!(
            result.unwrap(),
            StateMessage::GoToGene(gene) if gene == "TP53"
        ));
    }

    #[test]
    fn test_file_path_parsing() {
        let cli = Cli {
            paths: vec!["data.txt".to_string(), "sample.bam".to_string()],
            region: "chr1:12345".to_string(),
            reference: "hg19".to_string(),
        };

        let settings = Settings::new(cli).unwrap();
        assert_eq!(settings.bam_path, Some("sample.bam".to_string()));
        assert_eq!(settings.vcf_path, None);
        assert_eq!(settings.bed_path, None);
    }
}
