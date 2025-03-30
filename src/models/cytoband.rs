use std::fs::File;
use std::io::{BufRead, BufReader};

// Include the csv files directly as static strings
static HG19_CYTOBAND: &[u8] = include_bytes!("../resources/hg19_cytoband.csv");
static HG38_CYTOBAND: &[u8] = include_bytes!("../resources/hg38_cytoband.csv");

#[derive(Debug, Clone)]
pub enum Stain {
    Gneg,
    Gpos25,
    Gpos50,
    Gpos75,
    Gpos100,
    Acen,
    Gvar,
    Stalk,
}

impl From<&str> for Stain {
    fn from(s: &str) -> Self {
        match s {
            "gneg" => Stain::Gneg,
            "gpos25" => Stain::Gpos25,
            "gpos50" => Stain::Gpos50,
            "gpos75" => Stain::Gpos75,
            "gpos100" => Stain::Gpos100,
            "acen" => Stain::Acen,
            "gvar" => Stain::Gvar,
            "stalk" => Stain::Stalk,
            _ => Stain::Gneg, // Default case
        }
    }
}

#[derive(Debug, Clone)]
pub struct CytobandSegment {
    pub chromosome: String,
    pub start: u32,
    pub end: u32,
    pub name: String,
    pub stain: Stain,
}

#[derive(Debug)]
pub struct Cytoband {
    pub chromosome: String,
    pub segments: Vec<CytobandSegment>,
}

impl Cytoband {
    pub fn new(chromosome: &str) -> Self {
        Cytoband {
            chromosome: chromosome.to_string(),
            segments: Vec::new(),
        }
    }

    pub fn load_from_file(assembly: &str, chromosome: &str) -> Result<Self, std::io::Error> {
        let mut cytoband = Cytoband::new(chromosome);

        let content = match assembly {
            "hg19" => HG19_CYTOBAND,
            "hg38" => HG38_CYTOBAND,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Invalid assembly",
                ))
            }
        };

        let reader = BufReader::new(content);

        // Skip header
        let mut lines = reader.lines();
        let _ = lines.next();

        for line in lines {
            let line = line?;
            let fields: Vec<&str> = line.split(',').map(|s| s.trim_matches('"')).collect();

            if fields.len() >= 5 && fields[0] == chromosome {
                let segment = CytobandSegment {
                    chromosome: fields[0].to_string(),
                    start: fields[1].parse().unwrap_or(0),
                    end: fields[2].parse().unwrap_or(0),
                    name: fields[3].to_string(),
                    stain: Stain::from(fields[4]),
                };

                cytoband.segments.push(segment);
            }
        }

        Ok(cytoband)
    }
}
