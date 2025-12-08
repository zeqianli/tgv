use crate::{
    alignment::AlignmentRepositoryEnum,
    bed::BEDIntervals,
    contig_header::{ContigHeader, ContigSource},
    error::TGVError,
    reference::Reference,
    sequence::{SequenceRepository, SequenceRepositoryEnum},
    settings::Settings,
    tracks::{TrackService, TrackServiceEnum},
    variant::VariantRepository,
};

use itertools::Itertools;
use std::path::Path;

pub struct Repository {
    pub alignment_repository: Option<AlignmentRepositoryEnum>,

    pub variant_repository: Option<VariantRepository>,

    pub bed_intervals: Option<BEDIntervals>,

    pub track_service: Option<TrackServiceEnum>,

    pub sequence_service: Option<SequenceRepositoryEnum>,
}

impl Repository {
    pub async fn new(settings: &Settings) -> Result<(Self, ContigHeader), TGVError> {
        let mut track_service = TrackServiceEnum::new(settings).await?;
        let mut sequence_service = SequenceRepositoryEnum::new(settings)?;
        let alignment_repository = AlignmentRepositoryEnum::new(settings).await?;

        // Contig header collect contigs from multiple sources.
        // - If the reference is a ucsc genome: ucsc database (local, mariadb, or api)
        // - If the reference is a custom indexed fasta or a 2bit file: from the reference file
        // - If bam file is provided: from bam header
        let mut contig_header = ContigHeader::new(settings.reference.clone());

        match &settings.reference {
            Reference::Hg19
            | Reference::Hg38
            | Reference::UcscGenome(_)
            | Reference::UcscAccession(_) => {
                track_service
                    .as_mut()
                    .unwrap()
                    .get_all_contigs(&settings.reference)
                    .await?
                    .into_iter()
                    .for_each(|contig| {
                        contig_header.update_or_add_contig(
                            contig.name,
                            contig.length,
                            contig.aliases,
                            ContigSource::Track,
                        );
                    });

                if let Some(SequenceRepositoryEnum::TwoBit(twobit_sr)) = sequence_service.as_mut() {
                    track_service
                        .as_mut()
                        .unwrap()
                        .get_contig_2bit_file_lookup(&settings.reference, &contig_header)
                        .await?
                        .iter()
                        .filter_map(|(contig_index, path)| path.as_ref())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .unique()
                        .try_for_each(|path| {
                            let twobit_file_path =
                                Path::new(&settings.reference.cache_dir(&settings.cache_dir))
                                    .join(path);
                            let twobit_file_path = twobit_file_path.to_str().unwrap();
                            twobit_sr.add_contig_path(twobit_file_path, &mut contig_header)
                        })?;
                }
            }
            Reference::BYOIndexedFasta(_) => {
                if let Some(SequenceRepositoryEnum::IndexedFasta(fasta_sr)) =
                    sequence_service.as_mut()
                {
                    fasta_sr
                        .get_all_contigs()
                        .await?
                        .into_iter()
                        .for_each(|contig| {
                            contig_header.update_or_add_contig(
                                contig.name,
                                contig.length,
                                contig.aliases,
                                ContigSource::Sequence,
                            );
                        });
                } else {
                    unreachable!()
                }
            }

            Reference::BYOTwoBit(path) => {
                if let Some(SequenceRepositoryEnum::TwoBit(twobit_sr)) = sequence_service.as_mut() {
                    twobit_sr.add_contig_path(path, &mut contig_header)?;
                } else {
                    unreachable!()
                }
            }
            _ => {}
        }

        // FIXME
        // Warning when the reference contig is not present in the BAM header.
        if let Some(bam) = alignment_repository.as_ref() {
            bam.read_header()?.into_iter().for_each(|(name, length)| {
                contig_header.update_or_add_contig(
                    name,
                    length,
                    Vec::new(),
                    ContigSource::Alignment,
                );
            })
        }

        let variant_repository = match &settings.vcf_path {
            Some(vcf_path) => Some(VariantRepository::from_vcf(vcf_path, &contig_header)?),
            None => None,
        };

        let bed_intervals = match &settings.bed_path {
            Some(bed_path) => Some(BEDIntervals::from_bed(bed_path, &contig_header)?),
            None => None,
        };

        Ok((
            Self {
                alignment_repository,
                variant_repository,
                bed_intervals,
                track_service,
                sequence_service,
            },
            contig_header,
        ))
    }

    pub fn track_service_checked(&mut self) -> Result<&mut TrackServiceEnum, TGVError> {
        match self.track_service.as_mut() {
            Some(track_service) => Ok(track_service),
            None => Err(TGVError::StateError(
                "Track service is not initialized".to_string(),
            )),
        }
    }

    pub fn sequence_service_checked(&mut self) -> Result<&mut SequenceRepositoryEnum, TGVError> {
        match self.sequence_service.as_mut() {
            Some(sequence_service) => Ok(sequence_service),
            None => Err(TGVError::StateError(
                "Sequence service is not initialized".to_string(),
            )),
        }
    }

    pub async fn close(&mut self) -> Result<(), TGVError> {
        if let Some(ts) = self.track_service.as_mut() {
            ts.close().await?;
        }
        if let Some(ss) = self.sequence_service.as_mut() {
            ss.close().await?;
        }
        Ok(())
    }
}
