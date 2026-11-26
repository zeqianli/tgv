use std::path::{Path, PathBuf};
use std::sync::Arc;

use ferro_hgvs::data::projection::Projector;
use ferro_hgvs::prepare::{PrepareConfig, prepare_references};
use ferro_hgvs::reference::multi_fasta::MultiFastaProvider;
use ferro_hgvs::{HgvsVariant, VariantProjector, parse_hgvs};

use crate::Error;
use crate::reference::Reference;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GenomicSpan {
    pub contig: String,
    pub start: u64,
    pub end: u64,
}

pub fn split(input: &str) -> Option<(&str, &str)> {
    input.match_indices(':').find_map(|(index, _)| {
        let rest = &input[index + 1..];
        let mut chars = rest.chars();
        let axis = chars.next()?.to_ascii_lowercase();
        (matches!(axis, 'g' | 'c' | 'n' | 'm' | 'r' | 'p') && chars.next() == Some('.'))
            .then(|| (&input[..index], rest))
    })
}

pub fn data_dir(reference: &Reference, cache_dir: &str) -> PathBuf {
    Path::new(&reference.cache_dir(cache_dir)).join("hgvs")
}

pub fn prepare(reference: &Reference, cache_dir: &str) -> Result<(), Error> {
    let grch38 = match reference {
        Reference::Hg38 => true,
        Reference::Hg19 => false,
        _ => return Err(unsupported(reference)),
    };
    let output_dir = data_dir(reference, cache_dir);
    prepare_references(&PrepareConfig {
        output_dir: output_dir.clone(),
        download_transcripts: true,
        download_proteins: false,
        download_genome: grch38,
        download_genome_grch37: !grch38,
        download_refseqgene: false,
        download_lrg: false,
        download_cdot: grch38,
        download_cdot_grch37: !grch38,
        download_ensembl: false,
        skip_existing: true,
        genome: if grch38 { "grch38" } else { "grch37" }.to_string(),
        ..PrepareConfig::default()
    })?;

    eprintln!("Building the HGVS transcript cache...");
    MultiFastaProvider::from_manifest(output_dir.join("manifest.json"))?;
    Ok(())
}

fn unsupported(reference: &Reference) -> Error {
    Error::ValueError(format!(
        "HGVS is only available for hg38 and hg19, not {reference}."
    ))
}

pub struct Engine {
    chromosomes: &'static [(&'static str, &'static str)],
    provider: Arc<MultiFastaProvider>,
    projector: VariantProjector<Arc<MultiFastaProvider>>,
}

impl Engine {
    pub fn load(reference: &Reference, cache_dir: &str) -> Result<Self, Error> {
        let chromosomes = match reference {
            Reference::Hg38 => GRCH38_CHROMOSOMES,
            Reference::Hg19 => GRCH37_CHROMOSOMES,
            _ => return Err(unsupported(reference)),
        };
        let dir = data_dir(reference, cache_dir);
        let manifest = dir.join("manifest.json");
        if !manifest.is_file() {
            return Err(Error::StateError(format!(
                "HGVS data for {reference} is not prepared in {}. Run `tgv download {reference}` to enable HGVS.",
                dir.display()
            )));
        }
        let provider = Arc::new(MultiFastaProvider::from_manifest(manifest)?);
        let cdot = provider.cdot_mapper().cloned().ok_or_else(|| {
            Error::StateError(format!(
                "HGVS is unavailable: {} has no transcript models. Run `tgv download {reference}` again.",
                dir.display()
            ))
        })?;
        Ok(Self {
            chromosomes,
            projector: VariantProjector::new(Projector::new(cdot), provider.clone()),
            provider,
        })
    }

    pub fn locate(&self, accession: &str, description: &str) -> Result<GenomicSpan, Error> {
        let invalid = |reason: &str| {
            Error::ParsingError(format!(
                "Invalid HGVS search {accession}:{description}: {reason}"
            ))
        };
        let genomic_axis = description.starts_with(['g', 'G']);
        let transcript = self
            .provider
            .cdot_mapper()
            .and_then(|cdot| cdot.get_transcript(accession));
        let reference = match transcript {
            Some(_) if genomic_axis => {
                return Err(invalid(
                    "g. positions need a chromosome accession, such as NC_000012.12",
                ));
            }
            Some(model) => format!("{}({accession})", model.contig),
            None if self
                .chromosomes
                .iter()
                .any(|(candidate, _)| *candidate == accession) =>
            {
                if !genomic_axis {
                    return Err(invalid("a chromosome accession only takes g. positions"));
                }
                accession.to_string()
            }
            None => return Err(invalid("the accession is not in the HGVS data")),
        };
        let description = match description
            .strip_prefix("p.")
            .or_else(|| description.strip_prefix("P."))
        {
            Some(codons) => {
                let codon = |text: &str| {
                    text.parse::<u64>()
                        .ok()
                        .filter(|codon| *codon > 0)
                        .and_then(|codon| codon.checked_mul(3))
                        .ok_or_else(|| {
                            invalid("protein search takes a codon number, such as KRAS:p.12")
                        })
                };
                let (start, end) = codons.split_once('_').unwrap_or((codons, codons));
                let (start, end) = (codon(start)?, codon(end)?);
                if end < start {
                    return Err(invalid("the range ends before it starts"));
                }
                format!("c.{}_{end}=", start - 2)
            }
            None => description.to_string(),
        };
        let expression = format!("{reference}:{description}");
        let variant = parse_hgvs(&expression)
            .or_else(|error| parse_hgvs(&format!("{expression}=")).map_err(|_| error))
            .map_err(|_| invalid("it is not a valid HGVS description"))?;
        let HgvsVariant::Genome(genomic) =
            self.projector.project_to_genomic_normalized(&variant)?
        else {
            unreachable!("projecting to the genome yields a g. variant")
        };
        let location = &genomic.loc_edit.location;
        let (Some(start), Some(end)) = (location.start.inner(), location.end.inner()) else {
            return Err(invalid("the position is uncertain"));
        };
        let accession = genomic.accession.to_string();
        let chromosome = self
            .chromosomes
            .iter()
            .find(|(candidate, _)| *candidate == accession)
            .map(|(_, chromosome)| *chromosome)
            .ok_or_else(|| {
                Error::ValueError(format!(
                    "Contig {accession} has no RefSeq accession on this reference."
                ))
            })?;
        Ok(GenomicSpan {
            contig: format!("chr{chromosome}"),
            start: start.base.min(end.base),
            end: start.base.max(end.base),
        })
    }
}

const GRCH38_CHROMOSOMES: &[(&str, &str)] = &[
    ("NC_000001.11", "1"),
    ("NC_000002.12", "2"),
    ("NC_000003.12", "3"),
    ("NC_000004.12", "4"),
    ("NC_000005.10", "5"),
    ("NC_000006.12", "6"),
    ("NC_000007.14", "7"),
    ("NC_000008.11", "8"),
    ("NC_000009.12", "9"),
    ("NC_000010.11", "10"),
    ("NC_000011.10", "11"),
    ("NC_000012.12", "12"),
    ("NC_000013.11", "13"),
    ("NC_000014.9", "14"),
    ("NC_000015.10", "15"),
    ("NC_000016.10", "16"),
    ("NC_000017.11", "17"),
    ("NC_000018.10", "18"),
    ("NC_000019.10", "19"),
    ("NC_000020.11", "20"),
    ("NC_000021.9", "21"),
    ("NC_000022.11", "22"),
    ("NC_000023.11", "X"),
    ("NC_000024.10", "Y"),
    ("NC_012920.1", "M"),
];

const GRCH37_CHROMOSOMES: &[(&str, &str)] = &[
    ("NC_000001.10", "1"),
    ("NC_000002.11", "2"),
    ("NC_000003.11", "3"),
    ("NC_000004.11", "4"),
    ("NC_000005.9", "5"),
    ("NC_000006.11", "6"),
    ("NC_000007.13", "7"),
    ("NC_000008.10", "8"),
    ("NC_000009.11", "9"),
    ("NC_000010.10", "10"),
    ("NC_000011.9", "11"),
    ("NC_000012.11", "12"),
    ("NC_000013.10", "13"),
    ("NC_000014.8", "14"),
    ("NC_000015.9", "15"),
    ("NC_000016.9", "16"),
    ("NC_000017.10", "17"),
    ("NC_000018.9", "18"),
    ("NC_000019.9", "19"),
    ("NC_000020.10", "20"),
    ("NC_000021.8", "21"),
    ("NC_000022.10", "22"),
    ("NC_000023.10", "X"),
    ("NC_000024.9", "Y"),
];
