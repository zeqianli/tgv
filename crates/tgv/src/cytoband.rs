use crate::{error::TGVError, reference::Reference};
use ratatui::style::Color;

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
    pub fn get_color(&self) -> Color {
        // FIXME: This function is AI code. I haven't verified the correctness.
        // FIXME: Mvoe to Pallete.
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

#[derive(Debug, Clone)]
pub struct CytobandSegment {
    pub contig_index: usize,
    pub start: usize,
    pub end: usize,
    pub name: String,
    pub stain: Stain,
}

#[derive(Debug, Clone)]
pub struct Cytoband {
    pub reference: Option<Reference>,
    pub contig_index: usize,
    pub segments: Vec<CytobandSegment>,
}

impl Cytoband {
    pub fn default(
        reference: &Reference,
        contig_index: usize,
        contig_length: usize,
        contig_name: &str,
    ) -> Self {
        Self {
            reference: Some(reference.clone()),
            contig_index,
            segments: vec![CytobandSegment {
                contig_index,
                start: 1,
                end: contig_length,
                name: contig_name.to_string(),
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
