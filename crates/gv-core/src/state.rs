use crate::sequence::SequenceRepositoryEnum;
use crate::tracks::{TrackService, TrackServiceEnum};
use crate::variant::VariantRepositoryEnum;
use crate::{
    alignment::{Alignment, AlignmentRepository, PairedAlignment, tables},
    bed::{BedRepositoryEnum, BedTable},
    contig_header::ContigHeader,
    cytoband::CytobandTable,
    error::TGVError,
    gene::{GeneSchema, GeneTable},
    intervals::{Focus, Region},
    message::{AlignmentDisplayOption, AlignmentFilter, AlignmentSort, Movement},
    reference::Reference,
    //register::Registers,
    //rendering::{MainLayout, layout::resize_node},
    repository::{Repository, RepositoryFileIndex},
    sequence::Sequence,
    settings::FilePath,
    variant::VariantTable,
};
use itertools::Itertools;
use std::time::Instant;

/// Selects the data that [`State::ensure_loaded`] covers for a region.
pub struct LoadRequest<'a> {
    /// Loads the reference sequence.
    pub sequence: bool,
    /// Loads the gene annotations.
    pub genes: bool,
    /// Loads the reference's cytobands, once for the whole session.
    pub cytobands: bool,
    /// The file tracks to load.
    pub files: &'a [RepositoryFileIndex],
    /// How far loads extend beyond the region on a cache miss.
    pub cache: CachePolicy,
}

/// Extends loads on a cache miss so that nearby requests hit the cache.
///
/// Each ratio multiplies the requested half-width around the same focus.
#[derive(Clone, Copy, Debug)]
pub struct CachePolicy {
    pub alignment_ratio: u64,
    pub sequence_ratio: u64,
    pub track_ratio: u64,
}

impl CachePolicy {
    /// Pads small viewer regions generously, so panning and zooming rarely reload.
    pub const VIEWER: Self = Self {
        alignment_ratio: 3,
        sequence_ratio: 6,
        track_ratio: 10,
    };

    /// Loads exactly the requested region.
    pub const EXACT: Self = Self {
        alignment_ratio: 1,
        sequence_ratio: 1,
        track_ratio: 1,
    };

    fn padded(region: &Region, ratio: u64) -> Region {
        Region {
            focus: region.focus.clone(),
            half_width: region.half_width * ratio,
        }
    }

    pub fn alignment_region(&self, region: &Region) -> Region {
        Self::padded(region, self.alignment_ratio)
    }

    pub fn sequence_region(&self, region: &Region) -> Region {
        Self::padded(region, self.sequence_ratio)
    }

    pub fn track_region(&self, region: &Region) -> Region {
        Self::padded(region, self.track_ratio)
    }
}

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

    /// Variant track data for the loaded region.
    /// Index always matches with the variant repository index.
    pub variants: Vec<VariantTable>,

    /// BED track data for the loaded region.
    /// Index always matches with the BED repository index.
    pub bed_intervals: Vec<BedTable>,

    pub track: GeneTable,

    pub cytobands: CytobandTable,

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

            track: GeneTable::default(),
            cytobands: CytobandTable::default(),
            sequence: Sequence::default(),
            variants: Vec::new(),
            bed_intervals: Vec::new(),
            contig_header: contigs,
        })
    }

    pub fn contig_name(&self, focus: &Focus) -> Result<&String, TGVError> {
        self.contig_header
            .try_get(focus.contig_index)
            .map(|contig| &contig.name)
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

    /// Appends an empty track slot for a file, matching [`Repository::open_file`].
    pub fn add_track(&mut self, file_path: &FilePath) {
        match file_path {
            FilePath::AlignmentPath(_) => self.add_alignment_track(),
            FilePath::VariantPath(_) => self.add_variant_track(),
            FilePath::BedPath(_) => self.add_bed_track(),
        }
    }

    /// Removes a track slot and the contig names its file used, matching
    /// [`Repository::remove`].
    pub fn remove_track(&mut self, index: RepositoryFileIndex) {
        self.contig_header.remove_file(index);
        match index {
            RepositoryFileIndex::Alignment(index) => {
                self.alignments.remove(index);
                self.alignment_options.remove(index);
                self.paired_alignments.remove(index);
            }
            RepositoryFileIndex::Variant(index) => {
                self.variants.remove(index);
            }
            RepositoryFileIndex::Bed(index) => {
                self.bed_intervals.remove(index);
            }
        }
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
        alignment_repository: &mut AlignmentRepository,
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
        let name = self
            .contig_header
            .try_get(region.contig_index())?
            .file_name(RepositoryFileIndex::Alignment(index));
        let alignment = match alignment_repository
            .read_alignment(region, name, &self.sequence)
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
        let read_count = alignment.records.len();
        let depth = alignment.depth()?;
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
        let feature_count = track.data.height();
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
        let mismatch_tables = self
            .alignments
            .iter()
            .map(|alignment| {
                tables::reference_mismatches(
                    &alignment.tables.cigar_runs,
                    &sequence,
                    alignment.contig_index,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (alignment, table) in self.alignments.iter_mut().zip(mismatch_tables) {
            alignment.tables.reference_mismatches = table;
        }
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
        self.variants.push(VariantTable::default());
    }

    pub async fn load_variant_data(
        &mut self,
        index: usize,
        region: &Region,
        variant_repository: &mut VariantRepositoryEnum,
    ) -> Result<&mut Self, TGVError> {
        let started = Instant::now();
        log::debug!("Loading variant data: track={} region={:?}", index, region);
        let name = self
            .contig_header
            .try_get(region.contig_index())?
            .file_name(RepositoryFileIndex::Variant(index));
        let variants = match variant_repository.read_variants(region, name) {
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
        log::debug!(
            "Loaded variant data: track={} region={:?} records={} elapsed_ms={}",
            index,
            region,
            variants.data.height(),
            started.elapsed().as_millis(),
        );
        self.variants[index] = variants;
        Ok(self)
    }

    pub fn add_bed_track(&mut self) {
        self.bed_intervals.push(BedTable::default());
    }

    pub async fn load_bed_data(
        &mut self,
        index: usize,
        region: &Region,
        bed_repository: &mut BedRepositoryEnum,
    ) -> Result<&mut Self, TGVError> {
        let started = Instant::now();
        log::debug!("Loading BED data: track={} region={:?}", index, region);
        let name = self
            .contig_header
            .try_get(region.contig_index())?
            .file_name(RepositoryFileIndex::Bed(index));
        let bed_intervals = match bed_repository.read_bed(region, name) {
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
        log::debug!(
            "Loaded BED data: track={} region={:?} records={} elapsed_ms={}",
            index,
            region,
            bed_intervals.data.height(),
            started.elapsed().as_millis(),
        );
        self.bed_intervals[index] = bed_intervals;
        Ok(self)
    }

    /// Loads the requested data for a region, skipping data that is already complete there.
    ///
    /// On a cache miss, loads extend beyond the region according to the request's cache policy.
    pub async fn ensure_loaded(
        &mut self,
        region: &Region,
        request: &LoadRequest<'_>,
        repository: &mut Repository,
    ) -> Result<(), TGVError> {
        // Load the sequence first: alignment loads compute mismatches against it.
        if request.sequence
            && let Some(sequence_service) = repository.sequence_service.as_mut()
            && !self.sequence.has_complete_data(region)
        {
            let cache_region = request.cache.sequence_region(region);
            log::trace!(
                "Sequence cache miss; requesting data load: region={:?} cache_region={:?}",
                region,
                cache_region,
            );
            self.load_sequence_data(&cache_region, sequence_service)
                .await?;
        }

        for &file in request.files {
            match file {
                RepositoryFileIndex::Alignment(index) => {
                    if self.alignments[index].has_complete_data(region) {
                        log::trace!(
                            "Skipping alignment data load because cached data is complete: track={} region={:?}",
                            index,
                            region,
                        );
                        continue;
                    }
                    let cache_region = request.cache.alignment_region(region);
                    log::trace!(
                        "Alignment cache miss; requesting data load: track={} region={:?} cache_region={:?}",
                        index,
                        region,
                        cache_region,
                    );
                    self.load_alignment_data(
                        index,
                        &cache_region,
                        &mut repository.alignment_repositories[index],
                    )
                    .await?;
                }
                RepositoryFileIndex::Variant(index)
                    if !self.variants[index].has_complete_data(region) =>
                {
                    let cache_region = request.cache.track_region(region);
                    self.load_variant_data(
                        index,
                        &cache_region,
                        &mut repository.variant_repositories[index],
                    )
                    .await?;
                }
                RepositoryFileIndex::Bed(index)
                    if !self.bed_intervals[index].has_complete_data(region) =>
                {
                    let cache_region = request.cache.track_region(region);
                    self.load_bed_data(
                        index,
                        &cache_region,
                        &mut repository.bed_repositories[index],
                    )
                    .await?;
                }
                _ => {}
            }
        }

        if request.cytobands
            && !self.cytobands.loaded
            && let Some(track_service) = repository.track_service.as_mut()
        {
            // Cytobands only decorate the view, so a failed query leaves them empty instead of
            // failing the load. The empty table counts as loaded, which stops repeated queries.
            self.cytobands = track_service
                .query_cytobands(&self.reference, &self.contig_header)
                .await
                .unwrap_or_else(|e| {
                    log::warn!("Failed to load the cytobands: error={e}");
                    CytobandTable::loaded_empty()
                });
        }

        if request.genes
            && let Some(track_service) = repository.track_service.as_mut()
            && !self.track.has_complete_data(region)
        {
            let cache_region = request.cache.track_region(region);
            log::trace!(
                "Reference track cache miss; requesting data load: region={:?} cache_region={:?}",
                region,
                cache_region,
            );
            self.load_track_data(&cache_region, track_service).await?;
        }

        Ok(())
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

        // Filters and sorts rewrite read visibility and rows in place. When they change, start
        // again from the loaded reads so that removed options no longer apply.
        let previous = &self.alignment_options[index];
        if *previous != options
            && previous.iter().any(|option| {
                matches!(
                    option,
                    AlignmentDisplayOption::Filter(_) | AlignmentDisplayOption::Sort(_)
                )
            })
        {
            self.alignments[index].filter(AlignmentFilter::Default, &self.sequence)?;
        }

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

        let mut gene = self.track.get_k_genes_after(focus.position, n)?;
        if gene.height() == 0 {
            gene = repository
                .track_service_checked()?
                .query_k_genes_after(
                    &self.reference,
                    focus.contig_index,
                    focus.position,
                    n,
                    &self.contig_header,
                )
                .await?;
        }

        Ok(Focus {
            contig_index: gene
                .column(GeneSchema::CONTIG_INDEX)?
                .u64()?
                .get(0)
                .expect("selected contigs are non-null") as usize,
            position: gene
                .column(GeneSchema::START)?
                .u64()?
                .get(0)
                .expect("selected starts are non-null"),
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

        let mut gene = self.track.get_k_genes_after(focus.position, n)?;
        if gene.height() == 0 {
            gene = repository
                .track_service_checked()?
                .query_k_genes_after(
                    &self.reference,
                    focus.contig_index,
                    focus.position,
                    n,
                    &self.contig_header,
                )
                .await?;
        }

        Ok(Focus {
            contig_index: gene
                .column(GeneSchema::CONTIG_INDEX)?
                .u64()?
                .get(0)
                .expect("selected contigs are non-null") as usize,
            position: gene
                .column(GeneSchema::END)?
                .u64()?
                .get(0)
                .expect("selected ends are non-null")
                + 1,
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

        let mut gene = self.track.get_k_genes_before(focus.position, n)?;
        if gene.height() == 0 {
            gene = repository
                .track_service_checked()?
                .query_k_genes_before(
                    &self.reference,
                    focus.contig_index,
                    focus.position,
                    n,
                    &self.contig_header,
                )
                .await?;
        }

        Ok(Focus {
            contig_index: gene
                .column(GeneSchema::CONTIG_INDEX)?
                .u64()?
                .get(0)
                .expect("selected contigs are non-null") as usize,
            position: gene
                .column(GeneSchema::START)?
                .u64()?
                .get(0)
                .expect("selected starts are non-null")
                - 1,
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

        let mut gene = self.track.get_k_genes_before(focus.position, n)?;
        if gene.height() == 0 {
            gene = repository
                .track_service_checked()?
                .query_k_genes_before(
                    &self.reference,
                    focus.contig_index,
                    focus.position,
                    n,
                    &self.contig_header,
                )
                .await?;
        }

        Ok(Focus {
            contig_index: gene
                .column(GeneSchema::CONTIG_INDEX)?
                .u64()?
                .get(0)
                .expect("selected contigs are non-null") as usize,
            position: gene
                .column(GeneSchema::END)?
                .u64()?
                .get(0)
                .expect("selected ends are non-null")
                - 1,
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

        let mut exon = self.track.get_k_exons_after(focus.position, n)?;
        if exon.height() == 0 {
            exon = repository
                .track_service_checked()?
                .query_k_exons_after(
                    &self.reference,
                    focus.contig_index,
                    focus.position,
                    n,
                    &self.contig_header,
                )
                .await?;
        }

        Ok(Focus {
            contig_index: exon
                .column(GeneSchema::CONTIG_INDEX)?
                .u64()?
                .get(0)
                .expect("selected contigs are non-null") as usize,
            position: exon
                .column(GeneSchema::START)?
                .u64()?
                .get(0)
                .expect("selected starts are non-null")
                + 1,
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

        let mut exon = self.track.get_k_exons_after(focus.position, n)?;
        if exon.height() == 0 {
            exon = repository
                .track_service_checked()?
                .query_k_exons_after(
                    &self.reference,
                    focus.contig_index,
                    focus.position,
                    n,
                    &self.contig_header,
                )
                .await?;
        }

        Ok(Focus {
            contig_index: exon
                .column(GeneSchema::CONTIG_INDEX)?
                .u64()?
                .get(0)
                .expect("selected contigs are non-null") as usize,
            position: exon
                .column(GeneSchema::END)?
                .u64()?
                .get(0)
                .expect("selected ends are non-null")
                + 1,
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

        let mut exon = self.track.get_k_exons_before(focus.position, n)?;
        if exon.height() == 0 {
            exon = repository
                .track_service_checked()?
                .query_k_exons_before(
                    &self.reference,
                    focus.contig_index,
                    focus.position,
                    n,
                    &self.contig_header,
                )
                .await?;
        }

        Ok(Focus {
            contig_index: exon
                .column(GeneSchema::CONTIG_INDEX)?
                .u64()?
                .get(0)
                .expect("selected contigs are non-null") as usize,
            position: exon
                .column(GeneSchema::START)?
                .u64()?
                .get(0)
                .expect("selected starts are non-null")
                - 1,
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

        let mut exon = self.track.get_k_exons_before(focus.position, n)?;
        if exon.height() == 0 {
            exon = repository
                .track_service_checked()?
                .query_k_exons_before(
                    &self.reference,
                    focus.contig_index,
                    focus.position,
                    n,
                    &self.contig_header,
                )
                .await?;
        }

        Ok(Focus {
            contig_index: exon
                .column(GeneSchema::CONTIG_INDEX)?
                .u64()?
                .get(0)
                .expect("selected contigs are non-null") as usize,
            position: exon
                .column(GeneSchema::END)?
                .u64()?
                .get(0)
                .expect("selected ends are non-null")
                - 1,
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
            .and_then(|gene| {
                Ok(Focus {
                    contig_index: gene
                        .column(GeneSchema::CONTIG_INDEX)?
                        .u64()?
                        .get(0)
                        .expect("selected contigs are non-null")
                        as usize,
                    position: gene
                        .column(GeneSchema::START)?
                        .u64()?
                        .get(0)
                        .expect("selected starts are non-null")
                        + 1,
                })
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
                            contig_index: gene
                                .column(GeneSchema::CONTIG_INDEX)?
                                .u64()?
                                .get(0)
                                .expect("selected contigs are non-null")
                                as usize,
                            position: gene
                                .column(GeneSchema::START)?
                                .u64()?
                                .get(0)
                                .expect("selected starts are non-null")
                                + 1,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contig_header::ContigHeader;
    use noodles::sam::{
        self,
        alignment::{
            record::{
                Flags,
                cigar::{Op, op::Kind},
            },
            record_buf::Cigar,
        },
    };

    fn read(
        name: &str,
        start: u64,
        cigar_ops: impl IntoIterator<Item = (Kind, usize)>,
        sequence: &[u8],
    ) -> sam::alignment::RecordBuf {
        let cigar: Cigar = cigar_ops
            .into_iter()
            .map(|(kind, len)| Op::new(kind, len))
            .collect();

        let record = sam::alignment::RecordBuf::builder()
            .set_name(name)
            .set_flags(Flags::default())
            .set_alignment_start(noodles::core::Position::try_from(start as usize).unwrap())
            .set_cigar(cigar)
            .set_sequence(sam::alignment::record_buf::Sequence::from(sequence))
            .build();

        record
    }

    fn test_sequence() -> Sequence {
        Sequence {
            start: 1,
            sequence: vec![b'A'; 100],
            contig_index: 0,
        }
    }

    fn alignment_from_reads(
        reads: Vec<sam::alignment::RecordBuf>,
        data_complete_bound: (u64, u64),
    ) -> Alignment {
        Alignment::from_records(
            reads,
            crate::alignment::QualityEncoding::Phred33,
            0,
            data_complete_bound,
            &test_sequence(),
        )
        .unwrap()
    }

    fn state_with_alignment(alignment: Alignment) -> State {
        let mut state = State::new(
            Reference::NoReference,
            ContigHeader::new(Reference::NoReference),
        )
        .unwrap();
        state.sequence = test_sequence();
        state.add_alignment_track();
        state.alignments[0] = alignment;
        state
    }

    #[test]
    fn set_alignment_options_resolves_base_sort_at_current_position() {
        let alignment = alignment_from_reads(
            vec![
                read("c", 12, [(Kind::Match, 1)], b"C"),
                read("a", 12, [(Kind::Match, 1)], b"A"),
            ],
            (1, 100),
        );
        let mut state = state_with_alignment(alignment);
        let focus = Focus {
            contig_index: 0,
            position: 12,
        };

        state
            .set_alignment_options(
                0,
                &focus,
                vec![AlignmentDisplayOption::Sort(
                    AlignmentSort::BaseAtCurrentPosition,
                )],
            )
            .unwrap();

        assert_eq!(
            state.alignment_options[0],
            vec![AlignmentDisplayOption::Sort(AlignmentSort::BaseAt(12))]
        );
        assert_eq!(
            state.alignments[0]
                .tables
                .reads
                .column(tables::ReadSchema::Y)
                .unwrap()
                .u64()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![1, 0]
        );
    }

    #[test]
    fn set_alignment_options_treats_unloaded_base_sort_as_noop() {
        let alignment = alignment_from_reads(
            vec![
                read("c", 12, [(Kind::Match, 1)], b"C"),
                read("a", 12, [(Kind::Match, 1)], b"A"),
            ],
            (10, 20),
        );
        let original_y = alignment
            .tables
            .reads
            .column(tables::ReadSchema::Y)
            .unwrap()
            .u64()
            .unwrap()
            .into_no_null_iter()
            .collect::<Vec<_>>();
        let mut state = state_with_alignment(alignment);
        let focus = Focus {
            contig_index: 0,
            position: 30,
        };

        state
            .set_alignment_options(
                0,
                &focus,
                vec![AlignmentDisplayOption::Sort(
                    AlignmentSort::BaseAtCurrentPosition,
                )],
            )
            .unwrap();

        assert_eq!(
            state.alignment_options[0],
            vec![AlignmentDisplayOption::Sort(AlignmentSort::BaseAt(30))]
        );
        assert_eq!(
            state.alignments[0]
                .tables
                .reads
                .column(tables::ReadSchema::Y)
                .unwrap()
                .u64()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            original_y
        );
    }

    #[test]
    fn set_alignment_options_still_propagates_unsupported_sort_errors() {
        let alignment =
            alignment_from_reads(vec![read("a", 12, [(Kind::Match, 1)], b"A")], (1, 100));
        let mut state = state_with_alignment(alignment);
        let focus = Focus {
            contig_index: 0,
            position: 12,
        };

        let error = state
            .set_alignment_options(
                0,
                &focus,
                vec![AlignmentDisplayOption::Sort(AlignmentSort::Sample)],
            )
            .unwrap_err();

        assert!(matches!(error, TGVError::ValueError(_)));
    }
}
