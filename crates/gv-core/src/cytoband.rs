use crate::{error::TGVError, reference::Reference};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Stain {
    Gneg,
    Gpos(u8),
    Acen,
    Gvar,
    Stalk,
    Other(String),
}

impl TryFrom<&str> for Stain {
    type Error = TGVError;

    fn try_from(s: &str) -> Result<Self, TGVError> {
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
}

#[derive(Debug, Clone)]
pub struct CytobandSegment {
    pub contig_index: usize,
    pub start: u64,
    pub end: u64,
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
        contig_length: u64,
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

    pub fn start(&self) -> u64 {
        1
    }

    pub fn end(&self) -> u64 {
        self.segments.last().unwrap().end
    }

    pub fn length(&self) -> u64 {
        self.end()
    }
}
