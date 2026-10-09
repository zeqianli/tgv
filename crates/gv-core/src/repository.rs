use crate::{
    alignment::{AlignmentRepository, QualityEncodingSetting},
    bed::BedRepositoryEnum,
    contig_header::{ContigHeader, ContigSource},
    error::TGVError,
    reference::Reference,
    sequence::SequenceRepositoryEnum,
    settings::{FilePath, Settings},
    tracks::{TrackService, TrackServiceEnum},
    variant::VariantRepositoryEnum,
};

use itertools::Itertools;
use std::{path::Path, time::Instant};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum RepositoryFileIndex {
    Alignment(usize),
    Variant(usize),
    Bed(usize),
}

impl RepositoryFileIndex {
    /// This index after `removed` leaves the per-kind vectors: `None` for the removed file
    /// itself, and one lower for later files of the same kind.
    pub fn after_removal(self, removed: Self) -> Option<Self> {
        match (self, removed) {
            _ if self == removed => None,
            (Self::Alignment(index), Self::Alignment(gone)) if index > gone => {
                Some(Self::Alignment(index - 1))
            }
            (Self::Variant(index), Self::Variant(gone)) if index > gone => {
                Some(Self::Variant(index - 1))
            }
            (Self::Bed(index), Self::Bed(gone)) if index > gone => Some(Self::Bed(index - 1)),
            _ => Some(self),
        }
    }
}

pub struct Repository {
    pub alignment_repositories: Vec<AlignmentRepository>,

    pub variant_repositories: Vec<VariantRepositoryEnum>,

    pub bed_repositories: Vec<BedRepositoryEnum>,

    pub track_service: Option<TrackServiceEnum>,

    pub sequence_service: Option<SequenceRepositoryEnum>,
}

impl Repository {
    pub fn file_path(&self, index: RepositoryFileIndex) -> &str {
        match index {
            RepositoryFileIndex::Alignment(index) => {
                self.alignment_repositories[index].source.path()
            }
            RepositoryFileIndex::Variant(index) => self.variant_repositories[index].path(),
            RepositoryFileIndex::Bed(index) => self.bed_repositories[index].path(),
        }
    }

    /// Opens the reference services and reads the reference's contigs. Track files open later,
    /// with [`Repository::open_file`].
    pub async fn new(settings: &Settings) -> Result<(Self, ContigHeader), TGVError> {
        let started = Instant::now();
        log::info!(
            "Initializing repository resources: reference={}",
            settings.reference,
        );

        let mut repository = Self {
            alignment_repositories: Vec::new(),
            variant_repositories: Vec::new(),
            bed_repositories: Vec::new(),
            track_service: TrackServiceEnum::new(settings).await?,
            sequence_service: SequenceRepositoryEnum::new(settings)?,
        };

        // Contig header collect contigs from multiple sources.
        // - If the reference is a ucsc genome: ucsc database (local, mariadb, or api)
        // - If the reference is a custom indexed fasta or a 2bit file: from the reference file
        // Track files add theirs when they open, through `merge_contigs`.
        let mut contig_header = ContigHeader::new(settings.reference.clone());

        // Sync contigs from tracks
        match &settings.reference {
            Reference::Hg19
            | Reference::Hg38
            | Reference::UcscGenome(_)
            | Reference::UcscAccession(_) => {
                repository
                    .track_service
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
                            ContigSource::GeneTrack,
                        );
                    });
            }
            Reference::BYOIndexedFasta(_) => {
                if let Some(SequenceRepositoryEnum::IndexedFasta(fasta_sr)) =
                    repository.sequence_service.as_mut()
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
                if let Some(SequenceRepositoryEnum::TwoBit(twobit_sr)) =
                    repository.sequence_service.as_mut()
                {
                    twobit_sr.add_2bit_file(path)?;
                } else {
                    unreachable!()
                }
            }
            _ => {}
        }

        // Sync sequence repositories
        match &settings.reference {
            Reference::Hg19
            | Reference::Hg38
            | Reference::UcscGenome(_)
            | Reference::UcscAccession(_) => {
                if let Some(SequenceRepositoryEnum::TwoBit(twobit_sr)) =
                    repository.sequence_service.as_mut()
                {
                    repository
                        .track_service
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
                            twobit_sr.add_2bit_file(twobit_file_path)
                        })?;
                }
            }
            Reference::BYOIndexedFasta(_) => {
                if let Some(SequenceRepositoryEnum::IndexedFasta(fasta_sr)) =
                    repository.sequence_service.as_mut()
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
                if let Some(SequenceRepositoryEnum::TwoBit(twobit_sr)) =
                    repository.sequence_service.as_mut()
                {
                    twobit_sr.add_2bit_file(path)?;
                } else {
                    unreachable!()
                }
            }
            _ => {}
        }

        if let Some(sr) = repository.sequence_service.as_mut() {
            sr.get_all_contigs().await?.into_iter().for_each(|contig| {
                contig_header.update_or_add_contig(
                    contig.name,
                    contig.length,
                    contig.aliases,
                    ContigSource::Sequence,
                );
            })
        }

        log::info!(
            "Repository resources are ready: contigs={} elapsed_ms={}",
            contig_header.contigs.len(),
            started.elapsed().as_millis(),
        );

        // PERF: async
        Ok((repository, contig_header))
    }

    /// Opens a track file and appends its repository to the vector of its kind.
    pub async fn open_file(
        &mut self,
        file_path: &FilePath,
        quality_encoding: QualityEncodingSetting,
    ) -> Result<RepositoryFileIndex, TGVError> {
        Ok(match file_path {
            FilePath::AlignmentPath(alignment_path) => {
                self.alignment_repositories
                    .push(AlignmentRepository::new(alignment_path, quality_encoding).await?);
                RepositoryFileIndex::Alignment(self.alignment_repositories.len() - 1)
            }
            FilePath::VariantPath(vcf_path) => {
                self.variant_repositories
                    .push(VariantRepositoryEnum::new(vcf_path)?);
                RepositoryFileIndex::Variant(self.variant_repositories.len() - 1)
            }
            FilePath::BedPath(bed_path) => {
                self.bed_repositories
                    .push(BedRepositoryEnum::new(bed_path)?);
                RepositoryFileIndex::Bed(self.bed_repositories.len() - 1)
            }
        })
    }

    /// Removes a track file's repository. Later repositories of the same kind shift down by one.
    pub fn remove(&mut self, index: RepositoryFileIndex) {
        match index {
            RepositoryFileIndex::Alignment(index) => {
                self.alignment_repositories.remove(index);
            }
            RepositoryFileIndex::Variant(index) => {
                self.variant_repositories.remove(index);
            }
            RepositoryFileIndex::Bed(index) => {
                self.bed_repositories.remove(index);
            }
        }
    }

    /// Merges the contigs that a track file names into a contig header, recording the name
    /// the file uses for each.
    pub fn merge_contigs(
        &self,
        index: RepositoryFileIndex,
        contig_header: &mut ContigHeader,
    ) -> Result<(), TGVError> {
        let contigs = match index {
            RepositoryFileIndex::Alignment(i) => self.alignment_repositories[i]
                .source
                .read_header()?
                .into_iter()
                .map(|(name, length)| (name, length.map(|length| length as u64)))
                .collect(),
            RepositoryFileIndex::Variant(i) => self.variant_repositories[i].read_contigs(),
            RepositoryFileIndex::Bed(i) => self.bed_repositories[i].read_contigs(),
        };
        for (name, length) in contigs {
            contig_header.update_or_add_contig(name, length, Vec::new(), ContigSource::File(index));
        }
        Ok(())
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
