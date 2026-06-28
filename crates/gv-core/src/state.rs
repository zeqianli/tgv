use crate::sequence::SequenceRepositoryEnum;
use crate::tracks::{TrackService, TrackServiceEnum};
use crate::variant::VariantRepository;
use crate::{
    alignment::{Alignment, AlignmentRepositoryEnum, PairedAlignment},
    bed::{BedRepository, BedTrack},
    contig_header::ContigHeader,
    cytoband::Cytoband,
    error::TGVError,
    feature::Gene,
    intervals::{Focus, GenomeInterval, Region},
    message::{AlignmentDisplayOption, AlignmentFilter, AlignmentSort, Movement},
    reference::Reference,
    //register::Registers,
    //rendering::{MainLayout, layout::resize_node},
    repository::Repository,
    sequence::Sequence,
    track::Track,
    variant::VariantTrack,
};
use itertools::Itertools;
use std::time::Instant;

/// Holds states of the application.
pub struct State {
    pub messages: Vec<String>,

    pub contig_header: ContigHeader,
    pub reference: Reference,

    /// Alignment track data.
    /// Index always matches with AlignmentRepository index
    pub alignments: Vec<Alignment>,
    pub alignment_options: Vec<Vec<AlignmentDisplayOption>>,
    pub paired_alignments: Vec<Option<PairedAlignment>>,

    /// Variant track data.
    /// Index always matches with VariantRepository index
    pub variants: Vec<VariantTrack>,
    pub variant_loaded: Vec<bool>, // Temporary hack before proper implemetation for the indexed VCF IO

    /// Bed track data
    /// Index always matches with BedRepository index
    pub bed_intervals: Vec<BedTrack>,
    pub bed_loaded: Vec<bool>, // Temporary hack before proper implemetation for large bed file io

    pub track: Track<Gene>,

    pub sequence: Sequence,
}

impl State {
    pub fn new(
        reference: Reference,
        contigs: ContigHeader,
        //repository_file_indexes: &[RepositoryFileIndex],
    ) -> Result<Self, TGVError> {
        Ok(Self {
            reference,

            // /settings: settings.clone(),
            messages: Vec::new(),

            alignments: Vec::new(),
            alignment_options: Vec::new(),
            paired_alignments: Vec::new(),

            track: Track::<Gene>::default(),
            sequence: Sequence::default(),
            variants: Vec::new(),
            variant_loaded: Vec::new(),
            bed_intervals: Vec::new(),
            bed_loaded: Vec::new(),
            contig_header: contigs,
        })
    }

    pub fn contig_name(&self, focus: &Focus) -> Result<&String, TGVError> {
        self.contig_header
            .try_get(focus.contig_index)
            .map(|contig| &contig.name)
    }

    pub fn current_cytoband(&self, focus: &Focus) -> Result<Option<&Cytoband>, TGVError> {
        self.contig_header
            .try_get(focus.contig_index)
            .map(|contig| contig.cytoband.as_ref())
    }

    /// Maximum length of the contig.
    pub fn contig_length(&self, focus: &Focus) -> Result<Option<u64>, TGVError> {
        self.contig_header
            .try_get(focus.contig_index)
            .map(|contig| contig.length)
    }
}

impl State {
    pub async fn movement(
        &self,
        focus: Focus,
        zoom: u64,
        repository: &mut Repository,
        movement: Movement,
    ) -> Result<Focus, TGVError> {
        match movement {
            Movement::Left(n) => Ok(focus.move_left(n * zoom)),
            Movement::Right(n) => Ok(focus.move_right(n * zoom)),
            Movement::Position(position) => Ok(focus.move_to(position)),
            Movement::ContigNamePosition(contig_name, position) => Ok(Focus {
                contig_index: self
                    .contig_header
                    .try_get_index_by_str(contig_name.as_ref())?,
                position,
            }),
            Movement::NextExonsStart(n) => self.next_exons_start(focus, repository, n).await,
            Movement::NextExonsEnd(n) => self.next_exons_end(focus, repository, n).await,
            Movement::PreviousExonsStart(n) => {
                self.previous_exons_start(focus, repository, n).await
            }
            Movement::PreviousExonsEnd(n) => self.previous_exons_end(focus, repository, n).await,
            Movement::NextGenesStart(n) => self.next_genes_start(focus, repository, n).await,
            Movement::NextGenesEnd(n) => self.next_genes_end(focus, repository, n).await,
            Movement::PreviousGenesStart(n) => {
                self.previous_genes_start(focus, repository, n).await
            }
            Movement::PreviousGenesEnd(n) => self.previous_genes_end(focus, repository, n).await,

            Movement::NextContig(n) => Ok(self.next_contig(focus, n)),
            Movement::PreviousContig(n) => Ok(self.previous_contig(focus, n)),
            Movement::ContigIndex(contig_index) => Ok(Focus {
                contig_index,
                position: 1,
            }),

            Movement::Gene(name) => self.gene(repository, name.as_ref()).await,

            Movement::Default => self.default_focus(repository).await,
        }
    }

    pub fn add_message(&mut self, message: String) {
        self.messages.push(message);
    }

    pub fn add_alignment_track(&mut self) {
        self.alignments.push(Alignment::default());
        self.alignment_options.push(Vec::new());
        self.paired_alignments.push(None);
    }

    pub async fn load_alignment_data(
        &mut self,
        index: usize,
        region: &Region,
        alignment_repository: &mut AlignmentRepositoryEnum,
    ) -> Result<&mut Self, TGVError> {
        // if !self.alignment.has_complete_data(&region) {
        //     Ok(false)
        // } else {
        let started = Instant::now();
        log::debug!(
            "Loading alignment data: track={} region={:?}",
            index,
            region,
        );
        let alignment = match alignment_repository
            .read_alignment(region, &self.sequence, &self.contig_header)
            .await
        {
            Ok(alignment) => alignment,
            Err(e) => {
                log::warn!(
                    "Failed to load alignment data: track={} region={:?} elapsed_ms={} error={e}",
                    index,
                    region,
                    started.elapsed().as_millis(),
                );
                return Err(e);
            }
        };
        let read_count = alignment.reads.len();
        let depth = alignment.depth();
        self.alignments[index] = alignment;

        // Re-compute paired alignment later, if needed.
        // This is wasteful. Have it lke this for now. Fix later.
        // This might also be problematic? read positions are re-shuffled at every load.
        self.paired_alignments[index] = None;

        if let Err(e) =
            self.set_alignment_options(index, &region.focus, self.alignment_options[index].clone())
        {
            log::warn!(
                "Failed to apply alignment options after loading data: track={} region={:?} elapsed_ms={} error={e}",
                index,
                region,
                started.elapsed().as_millis(),
            );
            return Err(e);
        }

        log::info!(
            "Loaded alignment data: track={} region={:?} reads={} depth={} elapsed_ms={}",
            index,
            region,
            read_count,
            depth,
            started.elapsed().as_millis(),
        );

        Ok(self)
    }

    pub async fn load_track_data(
        &mut self,
        region: &Region,
        track_service: &mut TrackServiceEnum,
    ) -> Result<&mut Self, TGVError> {
        let started = Instant::now();
        log::debug!("Loading reference track data: region={:?}", region);
        let track = match track_service
            .query_gene_track(&self.reference, region, &self.contig_header)
            .await
        {
            Ok(track) => track,
            Err(e) => {
                log::warn!(
                    "Failed to load reference track data: region={:?} elapsed_ms={} error={e}",
                    region,
                    started.elapsed().as_millis(),
                );
                return Err(e);
            }
        };
        let feature_count = track.features.len();
        self.track = track;
        log::debug!(
            "Loaded reference track data: region={:?} features={} elapsed_ms={}",
            region,
            feature_count,
            started.elapsed().as_millis(),
        );

        Ok(self)
    }

    pub async fn load_sequence_data(
        &mut self,
        region: &Region,
        sequence_repository: &mut SequenceRepositoryEnum,
    ) -> Result<&mut Self, TGVError> {
        let started = Instant::now();
        log::debug!("Loading sequence data: region={:?}", region);
        let sequence = match sequence_repository
            .query_sequence(region, &self.contig_header)
            .await
        {
            Ok(sequence) => sequence,
            Err(e) => {
                log::warn!(
                    "Failed to load sequence data: region={:?} elapsed_ms={} error={e}",
                    region,
                    started.elapsed().as_millis(),
                );
                return Err(e);
            }
        };
        let base_count = sequence.len();
        self.sequence = sequence;
        log::debug!(
            "Loaded sequence data: region={:?} bases={} elapsed_ms={}",
            region,
            base_count,
            started.elapsed().as_millis(),
        );

        Ok(self)
    }

    pub fn add_variant_track(&mut self) {
        self.variants.push(VariantTrack::default());
        self.variant_loaded.push(false);
    }

    pub async fn load_variant_data(
        &mut self,
        index: usize,
        region: &Region,
        variant_repository: &mut VariantRepository,
    ) -> Result<&mut Self, TGVError> {
        let started = Instant::now();
        log::debug!("Loading variant data: track={} region={:?}", index, region);
        let variants = match variant_repository.read_variants(&self.contig_header) {
            Ok(variants) => variants,
            Err(e) => {
                log::warn!(
                    "Failed to load variant data: track={} region={:?} elapsed_ms={} error={e}",
                    index,
                    region,
                    started.elapsed().as_millis(),
                );
                return Err(e);
            }
        };
        let record_count = variants.intervals.len();
        let Some(variant_track) = self.variants.get_mut(index) else {
            let e = TGVError::StateError(format!("Variant index out of bounds: {index}"));
            log::warn!(
                "Failed to store variant data: track={} region={:?} elapsed_ms={} error={e}",
                index,
                region,
                started.elapsed().as_millis(),
            );
            return Err(e);
        };
        *variant_track = variants;
        let Some(variant_loaded) = self.variant_loaded.get_mut(index) else {
            let e = TGVError::StateError(format!("Variant loaded index out of bounds: {index}"));
            log::warn!(
                "Failed to mark variant data loaded: track={} region={:?} elapsed_ms={} error={e}",
                index,
                region,
                started.elapsed().as_millis(),
            );
            return Err(e);
        };
        *variant_loaded = true;
        log::debug!(
            "Loaded variant data: track={} region={:?} records={} elapsed_ms={}",
            index,
            region,
            record_count,
            started.elapsed().as_millis(),
        );
        Ok(self)
    }

    pub fn add_bed_track(&mut self) {
        self.bed_intervals.push(BedTrack::default());
        self.bed_loaded.push(false);
    }

    pub async fn load_bed_data(
        &mut self,
        index: usize,
        region: &Region,
        bed_repository: &mut BedRepository,
    ) -> Result<&mut Self, TGVError> {
        let started = Instant::now();
        log::debug!("Loading BED data: track={} region={:?}", index, region);
        let bed_intervals = match bed_repository.read_bed(&self.contig_header) {
            Ok(bed_intervals) => bed_intervals,
            Err(e) => {
                log::warn!(
                    "Failed to load BED data: track={} region={:?} elapsed_ms={} error={e}",
                    index,
                    region,
                    started.elapsed().as_millis(),
                );
                return Err(e);
            }
        };
        let record_count = bed_intervals.intervals.len();
        let Some(bed_track) = self.bed_intervals.get_mut(index) else {
            let e = TGVError::StateError(format!("BED index out of bounds: {index}"));
            log::warn!(
                "Failed to store BED data: track={} region={:?} elapsed_ms={} error={e}",
                index,
                region,
                started.elapsed().as_millis(),
            );
            return Err(e);
        };
        *bed_track = bed_intervals;
        let Some(bed_loaded) = self.bed_loaded.get_mut(index) else {
            let e = TGVError::StateError(format!("BED loaded index out of bounds: {index}"));
            log::warn!(
                "Failed to mark BED data loaded: track={} region={:?} elapsed_ms={} error={e}",
                index,
                region,
                started.elapsed().as_millis(),
            );
            return Err(e);
        };
        *bed_loaded = true;
        log::debug!(
            "Loaded BED data: track={} region={:?} records={} elapsed_ms={}",
            index,
            region,
            record_count,
            started.elapsed().as_millis(),
        );
        Ok(self)
    }

    pub async fn ensure_complete_cytoband_data(
        &mut self,
        region: &Region,
        repository: &mut Repository,
    ) -> Result<bool, TGVError> {
        if self
            .contig_header
            .cytoband_is_loaded(region.contig_index())?
        {
            Ok(false)
        } else if let Some(track_service) = repository.track_service.as_mut() {
            let cytoband = track_service
                .get_cytoband(&self.reference, region.contig_index(), &self.contig_header)
                .await?;
            self.contig_header
                .try_update_cytoband(region.contig_index(), cytoband)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl State {
    /// Main function to route state message handling.
    pub fn set_alignment_options(
        &mut self,
        index: usize,
        focus: &Focus,
        options: Vec<AlignmentDisplayOption>,
    ) -> Result<(), TGVError> {
        let options = options
            .into_iter()
            .map(|option| match option {
                AlignmentDisplayOption::Filter(AlignmentFilter::BaseAtCurrentPosition(base)) => {
                    AlignmentDisplayOption::Filter(AlignmentFilter::Base(focus.position, base))
                }

                AlignmentDisplayOption::Filter(AlignmentFilter::BaseAtCurrentPositionSoftClip) => {
                    AlignmentDisplayOption::Filter(AlignmentFilter::BaseSoftclip(focus.position))
                }

                AlignmentDisplayOption::Sort(sort) => AlignmentDisplayOption::Sort(
                    resolve_alignment_sort_current_position(sort, focus.position),
                ),

                option => option,
            })
            .collect_vec();

        self.alignment_options[index] = options.clone();

        let view_as_pairs = options.contains(&AlignmentDisplayOption::ViewAsPairs);
        let mut applied_sorts = Vec::new();

        options
            .iter()
            .cloned()
            .try_for_each(|option| match option {
                AlignmentDisplayOption::Filter(filter) => {
                    self.alignments[index].filter(filter, &self.sequence)
                }

                AlignmentDisplayOption::Sort(sort) => {
                    match self.alignments[index].sort(sort.clone()) {
                        Ok(()) => applied_sorts.push(sort),
                        Err(TGVError::AlignmentSortPositionNotLoaded { .. }) => {}
                        Err(error) => return Err(error),
                    }
                    Ok(())
                }

                AlignmentDisplayOption::ViewAsPairs => Ok(()),
            })?;

        if view_as_pairs {
            let mut paired_alignment = PairedAlignment::new(&self.alignments[index])?;
            for sort in applied_sorts {
                match paired_alignment.sort(&self.alignments[index], sort) {
                    Ok(()) => {}
                    Err(TGVError::AlignmentSortPositionNotLoaded { .. }) => {}
                    Err(error) => return Err(error),
                }
            }
            self.paired_alignments[index] = Some(paired_alignment);
        } else {
            self.paired_alignments[index] = None;
        }

        Ok(())
    }

    //Self::get_data_requirements(state, repository)
}

fn resolve_alignment_sort_current_position(sort: AlignmentSort, position: u64) -> AlignmentSort {
    match sort {
        AlignmentSort::BaseAtCurrentPosition => AlignmentSort::BaseAt(position),
        AlignmentSort::Then(first, second) => AlignmentSort::Then(
            Box::new(resolve_alignment_sort_current_position(*first, position)),
            Box::new(resolve_alignment_sort_current_position(*second, position)),
        ),
        AlignmentSort::Reverse(sort) => AlignmentSort::Reverse(Box::new(
            resolve_alignment_sort_current_position(*sort, position),
        )),
        sort => sort,
    }
}

// Movement handling
impl State {
    pub async fn next_genes_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        // The gene is in the track.
        if let Some(gene) = self.track.get_k_genes_after(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.start(),
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.start(),
        })
    }

    pub async fn next_genes_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(gene) = self.track.get_k_genes_after(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.end() + 1,
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.end() + 1,
        })
    }

    pub async fn previous_genes_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(gene) = self.track.get_k_genes_before(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.start() - 1,
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.start() - 1,
        })
    }

    pub async fn previous_genes_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(gene) = self.track.get_k_genes_before(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.end() - 1,
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.end() - 1,
        })
    }

    pub async fn next_exons_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(exon) = self.track.get_k_exons_after(focus.position, n) {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.start() + 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.start() + 1,
        })
    }

    pub async fn next_exons_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(exon) = self.track.get_k_exons_after(focus.position, n) {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.end() + 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.end() + 1,
        })
    }

    pub async fn previous_exons_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(exon) = self.track.get_k_exons_before(focus.position, n) {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.start() - 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.start() - 1,
        })
    }

    pub async fn previous_exons_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        let exon = self.track.get_k_exons_before(focus.position, n);
        if let Some(exon) = exon {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.end() - 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.end() - 1,
        })
    }

    pub async fn gene(
        &self,
        repository: &mut Repository,
        gene_name: &str,
    ) -> Result<Focus, TGVError> {
        repository
            .track_service_checked()?
            .query_gene_name(&self.reference, gene_name, &self.contig_header)
            .await
            .map(|gene| Focus {
                contig_index: gene.contig_index(),
                position: gene.start() + 1,
            })
    }

    fn next_contig(&self, focus: Focus, n: usize) -> Focus {
        Focus {
            contig_index: self.contig_header.next(focus.contig_index, n),
            position: 1,
        }
    }

    fn previous_contig(&self, focus: Focus, n: usize) -> Focus {
        Focus {
            contig_index: self.contig_header.previous(focus.contig_index, n),

            position: 1,
        }
    }

    pub async fn default_focus(&self, repository: &mut Repository) -> Result<Focus, TGVError> {
        match self.reference {
            Reference::Hg38 | Reference::Hg19 => {
                return self.gene(repository, "TP53").await;
            }

            Reference::UcscGenome(_) | Reference::UcscAccession(_) => {
                // Find the first gene on the first contig. If anything is not found, handle it later.

                let first_contig = self.contig_header.first()?;

                // Try to get the first gene in the first contig.
                // We use query_k_genes_after starting from coordinate 0 with k=1.
                match repository
                    .track_service_checked()?
                    .query_k_genes_after(&self.reference, first_contig, 0, 1, &self.contig_header)
                    .await
                {
                    Ok(gene) => {
                        // Found a gene, go to its start (using 1-based coordinates for Goto)
                        return Ok(Focus {
                            contig_index: gene.contig_index,
                            position: gene.start() + 1,
                        });
                    }
                    Err(_) => {} // Gene not found. Handle later.
                }
            }

            Reference::BYOIndexedFasta(_) | Reference::BYOTwoBit(_) | Reference::NoReference => {} // handle later
        };

        // If reaches here, go to the first contig:1
        self.contig_header
            .first()
            .map(|contig_index| Focus {
                contig_index,
                position: 1,
            })
            .map_err(|_| {
                TGVError::StateError(
            "Failed to find a default initial region. Please provide a starting region with -r."
                .to_string() )
            })
    }
}
