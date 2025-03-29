use crate::error::TGVError;
use crate::models::{message::StateMessage, reference::Reference};
use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// BAM file path. Must be sorted and indexed (with .bai file in the same directory).
    /// If not provided, only reference genome will be displayed.
    #[arg(value_name = "PATHS")]
    paths: Vec<String>,

    /// Index file path.
    /// If not provided, .bai in the same directory as the BAM file will be used.
    #[arg(short = 'i', long = "index", value_name = "PATH", default_value = "")]
    index: String,

    /// Starting region. Supported formats: [chr]:[pos] (e.g. 12:25398142); [gene] (e.g. TP53).
    /// If not provided, TGV will find a default starting region.
    #[arg(short = 'r', long = "region", default_value = "")]
    region: String,

    /// Reference genome. Supported values: hg38; hg19.
    #[arg(short = 'g', long = "reference", default_value = Reference::HG38)]
    reference: String,

    /// Do not display the reference genome.
    /// This flag cannot be used when no BAM file is provided.
    #[arg(long)]
    no_reference: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Settings {
    pub bam_path: Option<String>,
    pub bai_path: Option<String>,
    // pub vcf_path: Option<String>,
    // pub bed_path: Option<String>,
    pub reference: Option<Reference>,

    pub initial_state_messages: Vec<StateMessage>,
}

impl Settings {
    pub fn new(cli: Cli) -> Result<Self, TGVError> {
        let mut bam_path = None;
        // let mut vcf_path = None;
        // let mut bed_path = None;
        for path in cli.paths {
            if path.ends_with(".bam") {
                bam_path = Some(path.clone());
            } else {
                return Err(TGVError::CliError(format!(
                    "Unsupported file type: {}",
                    path
                )));
            }
        }

        let bai_path = match cli.index.is_empty() {
            true => None,
            false => Some(cli.index),
        };

        // Reference
        let reference = if cli.no_reference {
            None
        } else {
            Some(Reference::from_str(&cli.reference)?)
        };

        // Initial messages
        let initial_state_messages =
            Self::translate_initial_state_messages(&cli.region, reference.as_ref())?;

        // Additional validations:
        // 1. If no reference is provided, the initial state messages cannot contain GoToGene
        if reference.is_none() {
            for m in initial_state_messages.iter() {
                if let StateMessage::GoToGene(gene_name) = m {
                    return Err(TGVError::CliError(format!(
                        "The initial region cannot not be a gene name {} when no reference is provided. ",
                        gene_name
                    )));
                }
            }
        }

        // 2. If no bam file is provided, the initial state message cannot be GoToContigCoordinate
        if bam_path.is_none() {
            for m in initial_state_messages.iter() {
                if let StateMessage::GotoContigCoordinate(_, _) = m {
                    return Err(TGVError::CliError(
                        "Bam file is required to go to a contig coordinate".to_string(),
                    ));
                }
            }
        }

        // 3. bam file and reference cannot both be none
        if bam_path.is_none() && reference.is_none() {
            return Err(TGVError::CliError(
                "Bam file and reference cannot both be none".to_string(),
            ));
        }

        Ok(Self {
            bam_path,
            bai_path,
            // vcf_path,
            // bed_path,
            reference,
            initial_state_messages,
        })
    }

    fn translate_initial_state_messages(
        region_string: &String,
        reference: Option<&Reference>,
    ) -> Result<Vec<StateMessage>, TGVError> {
        let region_string = region_string.trim();

        // Interpretation 1: empty input (go to a default location)
        if region_string.is_empty() {
            return Ok(vec![StateMessage::GoToDefault]);
        }

        // Check format
        let split = region_string.split(":").collect::<Vec<&str>>();
        if split.len() > 2 {
            return Err(TGVError::CliError(format!(
                "Cannot interpret the region: {}",
                region_string
            )));
        }

        // Interpretation 2: genome:position
        if split.len() == 2 {
            match split[1].parse::<usize>() {
                Ok(n) => {
                    return Ok(vec![StateMessage::GotoContigCoordinate(
                        split[0].to_string(),
                        n,
                    )]);
                }
                Err(_) => {
                    return Err(TGVError::CliError(format!(
                        "Invalid genome region: {}",
                        region_string
                    )))
                }
            }
        }

        // Interpretation 3: gene name
        Ok(vec![StateMessage::GoToGene(region_string.to_string())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::models::message::StateMessage;
    use crate::models::reference::Reference;
    use rstest::rstest;

    #[rstest]
    #[case("tgv", Ok(Settings {
        bam_path: None,
        reference: Some(Reference::Hg38),
        initial_state_messages: vec![StateMessage::GoToDefault],
    }))] // empty input: no bam file and no reference: browse hg38
    #[case("tgv input.bam", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        reference: Some(Reference::Hg38),
        initial_state_messages: vec![StateMessage::GoToDefault],
    }))]
    #[case("tgv wrong.extension", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam -r chr1:12345", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        reference: Some(Reference::Hg38),
        initial_state_messages: vec![StateMessage::GotoContigCoordinate("chr1".to_string(), 12345)],
    }))]
    #[case("tgv input.bam -r chr1:invalid", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam -r chr1:12:12345", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam -r TP53", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        reference: Some(Reference::Hg38),
        initial_state_messages: vec![StateMessage::GoToGene("TP53".to_string())],
    }))]
    #[case("tgv input.bam -r TP53 -g hg19", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        reference: Some(Reference::Hg19),
        initial_state_messages: vec![StateMessage::GoToGene("TP53".to_string())],
    }))]
    #[case("tgv input.bam -r TP53 -g hg100", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam -r 1:12345 --no-reference", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        reference: None,
        initial_state_messages: vec![StateMessage::GotoContigCoordinate("1".to_string(), 12345)],
    }))]
    #[case("tgv input.bam -r TP53 -g hg19 --no-reference", Err(TGVError::CliError("".to_string())))]
    #[case("tgv --no-reference", Err(TGVError::CliError("".to_string())))]
    #[case("tgv -r 1:12345", Err(TGVError::CliError("".to_string())))]
    fn test_cli_parsing(
        #[case] command_line: &str,
        #[case] expected_settings: Result<Settings, TGVError>,
    ) {
        let cli = Cli::parse_from(shlex::split(command_line).unwrap());
        println!("{:?}", cli.paths);

        match (Settings::new(cli), expected_settings) {
            (Ok(settings), Ok(expected)) => assert_eq!(settings, expected),
            (Err(e), Err(expected)) => assert!(e.is_same_type(&expected)),
            _ => assert!(false),
        }
    }
}
