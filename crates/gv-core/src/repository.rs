use crate::{
    alignment::AlignmentRepositoryEnum,
    bed::BEDRepository,
    contig_header::{ContigHeader, ContigSource},
    error::TGVError,
    reference::Reference,
    sequence::SequenceRepositoryEnum,
    settings::{FilePath, Settings},
    tracks::{TrackService, TrackServiceEnum},
    variant::VariantRepository,
};

use itertools::Itertools;
use std::path::Path;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RepositoryFileIndex {
    Alignment(usize),
    Variant(usize),
    Bed(usize),
}

pub struct Repository {
    pub alignment_repositories: Vec<AlignmentRepositoryEnum>,

    pub variant_repositories: Vec<VariantRepository>,

    pub bed_repositories: Vec<BEDRepository>,

    pub track_service: Option<TrackServiceEnum>,

    pub sequence_service: Option<SequenceRepositoryEnum>,
}

impl Repository {
    pub async fn new(
        settings: &Settings,
    ) -> Result<(Self, ContigHeader, Vec<RepositoryFileIndex>), TGVError> {
        let mut track_service = TrackServiceEnum::new(settings).await?;
        let mut sequence_service = SequenceRepositoryEnum::new(settings)?;
        let mut alignment_repositories = Vec::new();
        let mut variant_repositories = Vec::new();
        let mut bed_repositories = Vec::new();
        let mut repository_file_indexes = Vec::new();

        for file_path in &settings.file_paths {
            match file_path {
                FilePath::AlignmentPath(alignment_path) => {
                    let index = alignment_repositories.len();
                    alignment_repositories
                        .push(AlignmentRepositoryEnum::new(alignment_path).await?);
                    repository_file_indexes.push(RepositoryFileIndex::Alignment(index));
                }
                FilePath::VariantPath(vcf_path) => {
                    let index = variant_repositories.len();
                    variant_repositories.push(VariantRepository {
                        vcf_path: vcf_path.clone(),
                    });
                    repository_file_indexes.push(RepositoryFileIndex::Variant(index));
                }
                FilePath::BedPath(bed_path) => {
                    let index = bed_repositories.len();
                    bed_repositories.push(BEDRepository {
                        bed_path: bed_path.clone(),
                    });
                    repository_file_indexes.push(RepositoryFileIndex::Bed(index));
                }
            }
        }

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
                        .filter_map(|(_contig_index, path)| path.as_ref())
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

        for repository_file_index in &repository_file_indexes {
            match repository_file_index {
                RepositoryFileIndex::Alignment(index) => {
                    // FIXME
                    // Warning when the reference contig is not present in the BAM header.
                    alignment_repositories[*index]
                        .read_header()?
                        .into_iter()
                        .for_each(|(name, length)| {
                            contig_header.update_or_add_contig(
                                name,
                                length.map(|l| l as u64),
                                Vec::new(),
                                ContigSource::Alignment,
                            );
                        });
                }
                RepositoryFileIndex::Variant(index) => {
                    variant_repositories[*index]
                        .read_contigs()?
                        .into_iter()
                        .for_each(|(name, length)| {
                            contig_header.update_or_add_contig(
                                name,
                                length,
                                Vec::new(),
                                ContigSource::Annotation,
                            );
                        });
                }
                RepositoryFileIndex::Bed(index) => {
                    bed_repositories[*index]
                        .read_contigs()?
                        .into_iter()
                        .for_each(|(name, length)| {
                            contig_header.update_or_add_contig(
                                name,
                                length,
                                Vec::new(),
                                ContigSource::Annotation,
                            );
                        });
                }
            }
        }

        // PERF: async
        Ok((
            Self {
                alignment_repositories,
                variant_repositories,
                bed_repositories,
                track_service,
                sequence_service,
            },
            contig_header,
            repository_file_indexes,
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
