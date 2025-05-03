use crate::contig::Contig;
use crate::error::TGVError;
use crate::reference::Reference;
use csv::Reader;
use ratatui::style::Color;
use serde::Deserialize;
use std::io::BufReader;

const VALID_CHROMOSOMES: [&str; 25] = [
    "chr1", "chr2", "chr3", "chr4", "chr5", "chr6", "chr7", "chr8", "chr9", "chr10", "chr11",
    "chr12", "chr13", "chr14", "chr15", "chr16", "chr17", "chr18", "chr19", "chr20", "chr21",
    "chr22", "chrX", "chrY", "chrMT",
];

// Include the csv files as static bytes
const HG19_CYTOBAND: &[u8] = include_bytes!("resources/hg19_cytoband.csv");
const HG38_CYTOBAND: &[u8] = include_bytes!("resources/hg38_cytoband.csv");

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
    Ok(Contig::chrom(&s))
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
    pub contig: Contig,
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
    pub contig: Contig,
    pub segments: Vec<CytobandSegment>,
}

impl Cytoband {
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

impl Cytoband {
    /// Human csvs are pre-saved.
    pub fn from_human_reference(reference: &Reference) -> Result<Vec<Self>, TGVError> {
        let mut cytobands: Vec<Cytoband> = Vec::new();

        let content = match reference {
            Reference::Hg19 => HG19_CYTOBAND,
            Reference::Hg38 => HG38_CYTOBAND,
            _ => {
                // TODO
                return Err(TGVError::ValueError(format!(
                    "Does not support loading cytobands from csv for this reference: {}. Use the UCSC API.",
                    reference
                )));
            }
        };

        let reader = BufReader::new(content);
        let mut csv_reader = Reader::from_reader(reader);

        for result in csv_reader.records() {
            let record = result.map_err(|e| TGVError::ParsingError(e.to_string()))?;

            // only keep chr + digits
            let contig_string = record[0].to_string();
            if !VALID_CHROMOSOMES.contains(&contig_string.as_str()) {
                continue;
            }

            let contig = Contig::chrom(&contig_string);
            let start = record[1]
                .parse::<usize>()
                .map_err(|e| TGVError::ParsingError(e.to_string()))?;
            let end = record[2]
                .parse::<usize>()
                .map_err(|e| TGVError::ParsingError(e.to_string()))?;
            let name = record[3].to_string();
            let stain =
                Stain::from(&record[4]).map_err(|e| TGVError::ParsingError(e.to_string()))?;

            let segment = CytobandSegment {
                contig,
                start: start + 1,
                end,
                name,
                stain,
            };

            if cytobands.is_empty() || cytobands.last().unwrap().contig != segment.contig {
                let cytoband = Cytoband {
                    reference: Some(reference.clone()),
                    contig: segment.contig.clone(),
                    segments: Vec::new(),
                };
                cytobands.push(cytoband);
            }

            cytobands.last_mut().unwrap().segments.push(segment);
        }
        Ok(cytobands)
    }

    pub fn from_non_reference(
        contigs: &[Contig],
        lengths: Vec<usize>,
    ) -> Result<Vec<Self>, TGVError> {
        Ok(contigs
            .iter()
            .zip(lengths.iter())
            .map(|(contig, length)| Cytoband {
                reference: None,
                contig: contig.clone(),
                segments: vec![CytobandSegment {
                    contig: contig.clone(),
                    start: 1,
                    end: *length,
                    name: "".to_string(),
                    stain: Stain::Other("unknown".to_string()),
                }],
            })
            .collect())
    }
}
