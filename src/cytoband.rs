use crate::{error::TGVError, reference::Reference};
use ratatui::style::Color;
use serde::Deserialize;

// const VALID_CHROMOSOMES: [&str; 25] = [
//     "chr1", "chr2", "chr3", "chr4", "chr5", "chr6", "chr7", "chr8", "chr9", "chr10", "chr11",
//     "chr12", "chr13", "chr14", "chr15", "chr16", "chr17", "chr18", "chr19", "chr20", "chr21",
//     "chr22", "chrX", "chrY", "chrMT",
// ];

// // Include the csv files as static bytes
// const HG19_CYTOBAND: &[u8] = include_bytes!("resources/hg19_cytoband.csv");
// const HG38_CYTOBAND: &[u8] = include_bytes!("resources/hg38_cytoband.csv");

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Stain {
    Gneg,
    Gpos(u8),
    Acen,
    Gvar,
    Stalk,
    Other(String),
}

impl Stain {
    pub fn from(s: &str) -> Result<Self, TGVError> {
        match s {
            "gneg" => Ok(Stain::Gneg),
            "acen" => Ok(Stain::Acen),
            "gvar" => Ok(Stain::Gvar),
            "stalk" => Ok(Stain::Stalk),
            "" => Ok(Stain::Other("unknown".to_string())),
            stain => {
                if stain.starts_with("gpos") {
                    let percentage = stain.get(4..).unwrap_or("").parse::<u8>().unwrap_or(0);
                    if percentage <= 100 {
                        Ok(Stain::Gpos(percentage))
                    } else {
                        Ok(Stain::Other(stain.to_string()))
                    }
                } else {
                    Ok(Stain::Other(stain.to_string()))
                }
            }
        }
    }

    /// Returns the color associated with the stain type.
    /// AI code.
    /// TODO: move to colors.rs
    pub fn get_color(&self) -> Color {
        match self {
            Stain::Gneg => Color::from_u32(0xffffff),
            Stain::Gpos(p) => {
                let start_r = 240.0;
                let start_g = 253.0;
                let start_b = 244.0;
                let end_r = 5.0;
                let end_g = 46.0;
                let end_b = 22.0;

                let t = *p as f32 / 100.0;

                let r = (start_r * (1.0 - t) + end_r * t).round() as u8;
                let g = (start_g * (1.0 - t) + end_g * t).round() as u8;
                let b = (start_b * (1.0 - t) + end_b * t).round() as u8;

                Color::Rgb(r, g, b)
            }
            Stain::Acen => Color::from_u32(0xdc2626),
            Stain::Gvar => Color::from_u32(0x60a5fa),
            Stain::Stalk => Color::from_u32(0xc026d3),
            Stain::Other(_) => Color::from_u32(0x4b5563),
        }
    }
}

fn deserialize_stain_from_string<'de, D>(deserializer: D) -> Result<Stain, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Stain::from(&s).map_err(serde::de::Error::custom)
}

fn deserialize_contig_from_string<'de, D>(deserializer: D) -> Result<Contig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(Contig::new(&s))
}

fn deserialize_start_from_0_based<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let start_0_based = usize::deserialize(deserializer)?;
    Ok(start_0_based + 1)
}

#[derive(Debug, Clone, Deserialize)]
pub struct CytobandSegment {
    #[serde(rename = "chrom", deserialize_with = "deserialize_contig_from_string")]
    pub contig: usize,
    #[serde(
        rename = "chromStart",
        deserialize_with = "deserialize_start_from_0_based"
    )]
    pub start: usize,
    #[serde(rename = "chromEnd")]
    pub end: usize,
    pub name: String,
    #[serde(
        rename = "gieStain",
        deserialize_with = "deserialize_stain_from_string"
    )]
    pub stain: Stain,
}

#[derive(Debug, Clone)]
pub struct Cytoband {
    pub reference: Option<Reference>,
    pub contig: usize,
    pub segments: Vec<CytobandSegment>,
}

impl Cytoband {
    pub fn default(reference: &Reference, contig: usize, contig_length: usize) -> Self {
        Self {
            reference: Some(reference.clone()),
            contig: contig.clone(),
            segments: vec![CytobandSegment {
                contig: contig.clone(),
                start: 1,
                end: contig_length,
                name: contig.name.clone(),
                stain: Stain::Other("unknown".to_string()),
            }],
        }
    }

    pub fn start(&self) -> usize {
        1
    }

    pub fn end(&self) -> usize {
        self.segments.last().unwrap().end
    }

    pub fn length(&self) -> usize {
        self.end()
    }
}
