use crate::error::TGVError;
use crate::intervals::GenomeInterval;
use crate::repository::Repository;
use crate::repository::{AlignmentRepository, AlignmentRepositoryEnum};
use crate::settings::Settings;
use crate::tracks::{TrackCache, TrackService};
use crate::{
    alignment::Alignment,
    contig_header::ContigHeader,
    cytoband::Cytoband,
    display_mode::DisplayMode,
    feature::Gene,
    message::{DataMessage, StateMessage},
    reference::Reference,
    region::Region,
    rendering::MainLayout,
    sequence::{Sequence, SequenceCache, SequenceRepository},
    track::Track,
    window::ViewingWindow,
};
use ratatui::layout::Rect;

/// Holds states of the application.
pub struct State {
    /// Basics
    pub exit: bool,

    /// Viewing window.
    pub window: ViewingWindow,
    pub area: Rect,

    pub reference: Option<Reference>,

    /// Settings
    ///pub settings: Settings,

    /// Error messages for display.
    pub errors: Vec<String>,

    /// Alignment segments.
    pub alignment: Option<Alignment>,

    /// Tracks.
    pub track: Option<Track<Gene>>,
    pub track_cache: TrackCache,

    /// Sequences.
    pub sequence: Option<Sequence>,
    pub sequence_cache: SequenceCache,

    // TODO: in the first implementation, refresh all data when the viewing window is near the boundary.
    /// Contigs in the BAM header
    pub contig_header: ContigHeader,

    /// Display mode
    pub display_mode: DisplayMode,
    pub layout: MainLayout,
}

/// Getters
impl State {
    pub fn new(
        settings: &Settings,
        // initial_window: ViewingWindow,
        initial_area: Rect,
        sequence_cache: SequenceCache,
        track_cache: TrackCache,
        contigs: ContigHeader,
    ) -> Result<Self, TGVError> {
        Ok(Self {
            window: ViewingWindow::default(),
            exit: false,
            area: initial_area,

            reference: settings.reference.clone(),

            // /settings: settings.clone(),
            errors: Vec::new(),

            alignment: None,
            track: None,
            track_cache,
            sequence: None,
            sequence_cache,
            contig_header: contigs,

            display_mode: DisplayMode::Main,
            layout: MainLayout::initialize(settings)?,
        })
    }

    pub fn viewing_region(&self) -> Region {
        Region {
            contig_index: self.window.contig_index,
            start: self.window.left(),
            end: self.window.right(&self.area),
        }
    }

    /// Start coordinate of bases displayed on the screen.
    /// 1-based, inclusive.
    pub fn start(&self) -> usize {
        self.window.left()
    }

    /// End coordinate of bases displayed on the screen.
    /// 1-based, inclusive.
    pub fn end(&self) -> usize {
        self.window.right(&self.area)
    }

    /// Middle coordinate of bases displayed on the screen.
    /// 1-based, inclusive.
    pub fn middle(&self) -> usize {
        self.window.middle(&self.area)
    }

    pub fn contig_index(&self) -> usize {
        self.window.contig_index
    }

    pub fn current_cytoband(&self) -> Option<&Cytoband> {
        self.contig_header.cytoband(self.contig_index())
    }

    pub fn add_error_message(&mut self, error: String) {
        self.errors.push(error);
    }

    /// Maximum length of the contig.
    pub fn contig_length(&self) -> Result<Option<usize>, TGVError> {
        Ok(self.contig_header.get(self.contig_index())?.length)
    }

    /// Get the reference if set.
    pub fn reference_checked(&self) -> Result<&Reference, TGVError> {
        match self.reference {
            Some(ref reference) => Ok(reference),
            None => Err(TGVError::StateError("Reference is not set".to_string())),
        }
    }

    pub fn track_checked(&self) -> Result<&Track<Gene>, TGVError> {
        self.track
            .as_ref()
            .ok_or(TGVError::StateError("Track is not initialized".to_string()))
    }
}

// mutating methods
impl State {
    pub fn self_correct_viewing_window(&mut self) {
        let contig_length = self.contig_length().unwrap();
        self.window.self_correct(&self.area, contig_length);
    }

    pub fn alignment_renderable(&self) -> bool {
        self.alignment.is_some()
            && self.window.zoom() <= StateHandler::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS
    }

    pub fn sequence_renderable(&self) -> bool {
        self.reference.is_some()
            && self.sequence.is_some()
            && self.window.zoom() <= StateHandler::MAX_ZOOM_TO_DISPLAY_SEQUENCES
    }

    pub fn track_renderable(&self) -> bool {
        self.reference.is_some()
            && self.track.is_some()
            && self.window.zoom() <= StateHandler::MAX_ZOOM_TO_DISPLAY_FEATURES
    }

    pub fn cytoband_renderable(&self) -> bool {
        self.current_cytoband().is_some()
    }
}

pub struct StateHandler {}

impl StateHandler {
    /// Handle initial messages.
    /// This has different error handling strategy (loud) vs handle(...), which suppresses errors.
    pub async fn handle_initial_messages(
        state: &mut State,
        repository: &Repository,
        settings: &Settings,
        messages: Vec<StateMessage>,
    ) -> Result<(), TGVError> {
        let mut data_messages = Vec::new();

        for message in messages {
            data_messages.extend(
                StateHandler::handle_state_message(state, repository, settings, message).await?,
            );
        }

        let mut loaded_data = false;
        for data_message in data_messages {
            loaded_data = Self::handle_data_message(state, repository, data_message).await?;
        }

        Ok(())
    }

    /// Handle messages after initialization. This blocks any error messages instead of propagating them.
    pub async fn handle(
        state: &mut State,
        repository: &Repository,
        settings: &Settings,
        messages: Vec<StateMessage>,
    ) -> Result<(), TGVError> {
        StateHandler::clear_messages(state)?;

        let mut data_messages: Vec<DataMessage> = Vec::new();

        for message in messages {
            match StateHandler::handle_state_message(state, repository, settings, message).await {
                Ok(messages) => data_messages.extend(messages),
                Err(e) => return StateHandler::add_message(state, e.to_string()),
            }
        }

        let data_messages = StateHandler::get_data_requirements(state, settings)?;

        for data_message in data_messages {
            match Self::handle_data_message(state, repository, data_message).await {
                Ok(_) => {}
                Err(e) => return StateHandler::add_message(state, e.to_string()),
            }
        }

        Ok(())
    }

    /// Main function to route state message handling.
    async fn handle_state_message(
        state: &mut State,
        repository: &Repository,
        settings: &Settings,
        message: StateMessage,
    ) -> Result<Vec<DataMessage>, TGVError> {
        match message {
            // Swithching modes
            StateMessage::Quit => StateHandler::quit(state)?,

            // Movement handling
            StateMessage::MoveLeft(n) => StateHandler::move_left(state, n)?,
            StateMessage::MoveRight(n) => StateHandler::move_right(state, n)?,
            StateMessage::MoveUp(n) => StateHandler::move_up(state, n)?,
            StateMessage::MoveDown(n) => StateHandler::move_down(state, n)?,
            StateMessage::GotoCoordinate(n) => StateHandler::go_to_coordinate(state, n)?,
            StateMessage::GotoContigNameCoordinate(contig_str, n) => {
                StateHandler::go_to_contig_coordinate(
                    state,
                    state.contig_header.get_index_by_str(&contig_str)?,
                    n,
                )?
            }

            // Zoom handling
            StateMessage::ZoomOut(r) => StateHandler::handle_zoom_out(state, r)?,
            StateMessage::ZoomIn(r) => StateHandler::handle_zoom_in(state, r)?,

            // Relative feature movement handling
            StateMessage::GotoNextExonsStart(n) => {
                StateHandler::go_to_next_exons_start(state, repository, n).await?
            }
            StateMessage::GotoNextExonsEnd(n) => {
                StateHandler::go_to_next_exons_end(state, repository, n).await?
            }
            StateMessage::GotoPreviousExonsStart(n) => {
                StateHandler::go_to_previous_exons_start(state, repository, n).await?
            }
            StateMessage::GotoPreviousExonsEnd(n) => {
                StateHandler::go_to_previous_exons_end(state, repository, n).await?
            }
            StateMessage::GotoNextGenesStart(n) => {
                StateHandler::go_to_next_genes_start(state, repository, n).await?
            }
            StateMessage::GotoNextGenesEnd(n) => {
                StateHandler::go_to_next_genes_end(state, repository, n).await?
            }
            StateMessage::GotoPreviousGenesStart(n) => {
                StateHandler::go_to_previous_genes_start(state, repository, n).await?
            }
            StateMessage::GotoPreviousGenesEnd(n) => {
                StateHandler::go_to_previous_genes_end(state, repository, n).await?
            }
            StateMessage::GotoNextContig(n) => StateHandler::go_to_next_contig(state, n).await?,
            StateMessage::GotoPreviousContig(n) => {
                StateHandler::go_to_previous_contig(state, n).await?
            }
            StateMessage::GotoContigIndex(index) => {
                StateHandler::go_to_contig_index(state, index).await?
            }

            // Absolute feature handling
            StateMessage::GoToGene(gene_id) => {
                StateHandler::go_to_gene(state, repository, gene_id).await?
            }

            // Find the default region
            StateMessage::GoToDefault => StateHandler::go_to_default(state, repository).await?,

            // Error messages
            StateMessage::Message(message) => StateHandler::add_message(state, message)?,

            StateMessage::SetDisplayMode(display_mode) => {
                state.display_mode = display_mode;
            }

            _ => {
                return Err(TGVError::StateError(format!(
                    "Unhandled state message: {:?}",
                    message
                )));
            }
        };

        Self::get_data_requirements(state, settings)
    }
}

// Data message handling

impl StateHandler {
    pub const MAX_ZOOM_TO_DISPLAY_FEATURES: usize = usize::MAX;
    pub const MAX_ZOOM_TO_DISPLAY_ALIGNMENTS: usize = 32;
    pub const MAX_ZOOM_TO_DISPLAY_SEQUENCES: usize = 2;

    fn quit(state: &mut State) -> Result<(), TGVError> {
        state.exit = true;
        Ok(())
    }

    fn add_message(state: &mut State, message: String) -> Result<(), TGVError> {
        state.errors.push(message);
        Ok(())
    }

    fn clear_messages(state: &mut State) -> Result<(), TGVError> {
        state.errors.clear();
        Ok(())
    }

    fn get_data_requirements(
        state: &State,
        settings: &Settings,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages = Vec::new();

        let viewing_region = state.viewing_region();

        // It's important to load sequence first!
        // Alignment IO requires calculating mismatches with the reference sequence.

        if settings.needs_sequence()
            && (state.window.zoom() <= Self::MAX_ZOOM_TO_DISPLAY_SEQUENCES)
            && !Self::has_complete_sequence(state, &viewing_region)
        {
            let sequence_cache_region = Self::sequence_cache_region(state, &viewing_region)?;
            data_messages.push(DataMessage::RequiresCompleteSequences(
                sequence_cache_region,
            ));
        }
        if settings.needs_alignment()
            && state.window.zoom() <= Self::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS
            && !Self::has_complete_alignment(state, &viewing_region)
        {
            let alignment_cache_region = Self::alignment_cache_region(state, &viewing_region)?;
            data_messages.push(DataMessage::RequiresCompleteAlignments(
                alignment_cache_region,
            ));
        }

        if settings.needs_track() {
            if !Self::has_complete_track(state, &viewing_region) {
                // viewing_window.zoom() <= Self::MAX_ZOOM_TO_DISPLAY_FEATURES is always true
                let track_cache_region = Self::track_cache_region(state, &viewing_region)?;
                data_messages.push(DataMessage::RequiresCompleteFeatures(track_cache_region));
            }

            // Cytobands
            data_messages.push(DataMessage::RequiresCytobands(state.contig_index()));
        }

        Ok(data_messages)
    }

    const ALIGNMENT_CACHE_RATIO: usize = 3;

    fn alignment_cache_region(state: &State, region: &Region) -> Result<Region, TGVError> {
        let left = region
            .start
            .saturating_sub(Self::ALIGNMENT_CACHE_RATIO * region.width() / 2)
            .max(1);
        let right = region
            .end
            .saturating_add(Self::ALIGNMENT_CACHE_RATIO * region.width() / 2)
            .min(if let Some(contig_length) = state.contig_length()? {
                contig_length
            } else {
                usize::MAX
            });
        Ok(Region {
            contig_index: region.contig_index,
            start: left,
            end: right,
        })
    }

    const SEQUENCE_CACHE_RATIO: usize = 3;

    fn sequence_cache_region(state: &State, region: &Region) -> Result<Region, TGVError> {
        let left = region
            .start
            .saturating_sub(Self::SEQUENCE_CACHE_RATIO * region.width() / 2)
            .max(1);
        let right = region
            .end
            .saturating_add(Self::SEQUENCE_CACHE_RATIO * region.width() / 2)
            .min(if let Some(contig_length) = state.contig_length()? {
                contig_length
            } else {
                usize::MAX
            });
        Ok(Region {
            contig_index: region.contig_index,
            start: left,
            end: right,
        })
    }

    const TRACK_CACHE_RATIO: usize = 10;

    fn track_cache_region(state: &State, region: &Region) -> Result<Region, TGVError> {
        let left = region
            .start
            .saturating_sub(Self::TRACK_CACHE_RATIO * region.width() / 2)
            .max(1);
        let right = region
            .end
            .saturating_add(Self::TRACK_CACHE_RATIO * region.width() / 2)
            .min(if let Some(contig_length) = state.contig_length()? {
                contig_length
            } else {
                usize::MAX
            });
        Ok(Region {
            contig_index: region.contig_index,
            start: left,
            end: right,
        })
    }
}

// Movement handling
impl StateHandler {
    fn move_left(state: &mut State, n: usize) -> Result<(), TGVError> {
        let contig_length = state.contig_length()?;

        state.window.set_left(
            state.window.left().saturating_sub(n * state.window.zoom()),
            &state.area,
            contig_length,
        );
        Ok(())
    }
    fn move_right(state: &mut State, n: usize) -> Result<(), TGVError> {
        let contig_length: Option<usize> = state.contig_length()?;

        state.window.set_left(
            state.window.left().saturating_add(n * state.window.zoom()),
            &state.area,
            contig_length,
        );
        Ok(())
    }
    fn move_up(state: &mut State, n: usize) -> Result<(), TGVError> {
        state.window.set_top(state.window.top().saturating_sub(n));
        Ok(())
    }
    fn move_down(state: &mut State, n: usize) -> Result<(), TGVError> {
        state.window.set_top(state.window.top().saturating_add(n));
        Ok(())
    }
    fn go_to_coordinate(state: &mut State, n: usize) -> Result<(), TGVError> {
        let contig_length = state.contig_length()?;

        state.window.set_middle(&state.area, n, contig_length);
        Ok(())
    }
    fn go_to_contig_coordinate(
        state: &mut State,
        contig_index: usize,
        n: usize,
    ) -> Result<(), TGVError> {
        // If bam_path is provided, check that the contig is valid.

        state.window.contig_index = contig_index;
        state.window.set_middle(&state.area, n, None); // Don't know contig length yet.
        state.window.set_top(0);

        Ok(())
    }

    fn handle_zoom_out(state: &mut State, r: usize) -> Result<(), TGVError> {
        let contig_length = state.contig_length()?;

        state
            .window
            .zoom_out(r, &state.area, contig_length)
            .unwrap();
        Ok(())
    }

    fn handle_zoom_in(state: &mut State, r: usize) -> Result<(), TGVError> {
        state
            .window
            .zoom_in(r, &state.area, state.contig_length()?)
            .unwrap();
        Ok(())
    }

    async fn go_to_next_genes_start(
        state: &mut State,
        repository: &Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let track = state.track_checked()?;
        let middle = state.middle();

        // The gene is in the track.
        if let Some(target_gene) = track.get_k_genes_after(middle, n) {
            return Self::go_to_coordinate(state, target_gene.start() + 1);
        }

        // Query for the target gene
        let gene = repository
            .track_service
            .as_ref()
            .ok_or(TGVError::StateError(
                "Track service not initialized".to_string(),
            ))?
            .query_k_genes_after(
                &state.reference_checked()?.clone(),
                state.contig_index(),
                middle,
                n,
                &mut state.track_cache,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, gene.start() + 1)
    }

    async fn go_to_next_genes_end(
        state: &mut State,
        repository: &Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();
        let track = state.track_checked()?;

        if let Some(target_gene) = track.get_k_genes_after(middle, n) {
            return Self::go_to_coordinate(state, target_gene.end() + 1);
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_after(
                &state.reference_checked()?.clone(),
                state.contig_index(),
                middle,
                n,
                &mut state.track_cache,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, gene.end() + 1)
    }

    async fn go_to_previous_genes_start(
        state: &mut State,
        repository: &Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();
        let track = state.track_checked()?;

        if let Some(target_gene) = track.get_k_genes_before(middle, n) {
            return Self::go_to_coordinate(state, target_gene.start() - 1);
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_before(
                &state.reference_checked()?.clone(),
                state.contig_index(),
                middle,
                n,
                &mut state.track_cache,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, gene.start() - 1)
    }

    async fn go_to_previous_genes_end(
        state: &mut State,
        repository: &Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();
        let track = state.track_checked()?;

        if let Some(target_gene) = track.get_k_genes_before(middle, n) {
            return Self::go_to_coordinate(state, target_gene.end() - 1);
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_before(
                &state.reference_checked()?.clone(),
                state.contig_index(),
                middle,
                n,
                &mut state.track_cache,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, gene.end() - 1)
    }

    async fn go_to_next_exons_start(
        state: &mut State,
        repository: &Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();
        let track = state.track_checked()?;

        if let Some(target_exon) = track.get_k_exons_after(middle, n) {
            return Self::go_to_coordinate(state, target_exon.start() + 1);
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_after(
                &state.reference_checked()?.clone(),
                state.contig_index(),
                middle,
                n,
                &mut state.track_cache,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, exon.start() + 1)
    }

    async fn go_to_next_exons_end(
        state: &mut State,
        repository: &Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();
        let track = state.track_checked()?;

        if let Some(target_exon) = track.get_k_exons_after(middle, n) {
            return Self::go_to_coordinate(state, target_exon.end() + 1);
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_after(
                &state.reference_checked()?.clone(),
                state.contig_index(),
                middle,
                n,
                &mut state.track_cache,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, exon.end() + 1)
    }

    async fn go_to_previous_exons_start(
        state: &mut State,
        repository: &Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();
        let track = state.track_checked()?;

        if let Some(target_exon) = track.get_k_exons_before(middle, n) {
            return Self::go_to_coordinate(state, target_exon.start() - 1);
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_before(
                &state.reference_checked()?.clone(),
                state.contig_index(),
                middle,
                n,
                &mut state.track_cache,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, exon.start() - 1)
    }

    async fn go_to_previous_exons_end(
        state: &mut State,
        repository: &Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();
        let track = state.track_checked()?;

        let target_exon = track.get_k_exons_before(middle, n);
        if let Some(target_exon) = target_exon {
            return Self::go_to_coordinate(state, target_exon.end() - 1);
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_before(
                &state.reference_checked()?.clone(),
                state.contig_index(),
                middle,
                n,
                &mut state.track_cache,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, exon.end() - 1)
    }

    async fn go_to_gene(
        state: &mut State,
        repository: &Repository,
        gene_name: String,
    ) -> Result<(), TGVError> {
        let gene = repository
            .track_service_checked()?
            .query_gene_name(
                &state.reference_checked()?.clone(),
                &gene_name,
                &mut state.track_cache,
                &state.contig_header,
            )
            .await?;

        Self::go_to_contig_coordinate(state, gene.contig_index(), gene.start() + 1)
    }

    async fn go_to_next_contig(state: &mut State, n: usize) -> Result<(), TGVError> {
        Self::go_to_contig_coordinate(state, state.contig_header.next(&state.contig_index(), n), 1)
    }

    async fn go_to_previous_contig(state: &mut State, n: usize) -> Result<(), TGVError> {
        Self::go_to_contig_coordinate(
            state,
            state.contig_header.previous(&state.contig_index(), n),
            1,
        )
    }

    async fn go_to_contig_index(state: &mut State, contig_index: usize) -> Result<(), TGVError> {
        Self::go_to_contig_coordinate(state, contig_index, 1)
    }

    async fn go_to_default(state: &mut State, repository: &Repository) -> Result<(), TGVError> {
        let reference = state.reference.as_ref();
        match reference {
            Some(Reference::Hg38) | Some(Reference::Hg19) => {
                return Self::go_to_gene(state, repository, "TP53".to_string()).await;
            }

            Some(Reference::UcscGenome { .. }) | Some(Reference::UcscAccession { .. }) => {
                // Find the first gene on the first contig. If anything is not found, handle it later.

                let first_contig = state.contig_header.first()?;

                // Try to get the first gene in the first contig.
                // We use query_k_genes_after starting from coordinate 0 with k=1.
                match repository
                    .track_service_checked()?
                    .query_k_genes_after(
                        &state.reference_checked()?.clone(),
                        first_contig,
                        0,
                        1,
                        &mut state.track_cache,
                        &state.contig_header,
                    )
                    .await
                {
                    Ok(gene) => {
                        // Found a gene, go to its start (using 1-based coordinates for Goto)
                        return Self::go_to_contig_coordinate(
                            state,
                            gene.contig_index(),
                            gene.start() + 1,
                        );
                    }
                    Err(_) => {} // Gene not found. Handle later.
                }
            }
            None => {} // handle later
        };

        // If reaches here, go to the first contig:1
        if let Ok(_) = Self::go_to_contig_coordinate(state, state.contig_header.first()?, 1) {
            return Ok(());
        }

        Err(TGVError::StateError(
            "Failed to find a default initial region. Please provide a starting region with -r."
                .to_string(),
        ))
    }
}

impl StateHandler {
    // TODO: async
    pub async fn handle_data_message(
        state: &mut State,
        repository: &Repository,
        data_message: DataMessage,
    ) -> Result<bool, TGVError> {
        let mut loaded_data = false;

        match data_message {
            DataMessage::RequiresCompleteAlignments(region) => {
                if !Self::has_complete_alignment(state, &region) {
                    state.alignment = Some(
                        repository
                            .alignment_repository
                            .as_ref()
                            .unwrap()
                            .read_alignment(
                                &region,
                                state.sequence.as_ref(),
                                &state.contig_header,
                            )?,
                    ); // TODO: unwrap is weird.
                    loaded_data = true;
                }
            }
            DataMessage::RequiresCompleteFeatures(region) => {
                let has_complete_track = Self::has_complete_track(state, &region);
                if let (Some(ref reference), Some(track_service)) =
                    (state.reference.clone(), repository.track_service.as_ref())
                {
                    if !has_complete_track {
                        if let Ok(track) = track_service
                            .query_gene_track(
                                reference,
                                &region,
                                &mut state.track_cache,
                                &state.contig_header,
                            )
                            .await
                        {
                            state.track = Some(track);
                            loaded_data = true;
                        } else {
                            // Do nothing (track not found). TODO: fix this shit properly.
                        }
                    }
                } else if state.reference.is_none() {
                    // No reference provided, cannot load features
                } else {
                    return Err(TGVError::StateError(
                        "Track service not initialized".to_string(),
                    ));
                }
            }
            DataMessage::RequiresCompleteSequences(region) => {
                let sequence_service = repository.sequence_service_checked()?;

                if !Self::has_complete_sequence(state, &region) {
                    let sequence = sequence_service
                        .query_sequence(&region, &mut state.sequence_cache, &state.contig_header)
                        .await?;

                    state.sequence = Some(sequence);
                    loaded_data = true;
                }
            }

            DataMessage::RequiresCytobands(contig_index) => {
                if state.contig_header.cytoband_is_loaded(contig_index)? {
                    return Ok(false);
                }

                if let (Some(ref reference), Some(track_service)) =
                    (state.reference.clone(), repository.track_service.as_ref())
                {
                    let cytoband = track_service
                        .get_cytoband(
                            reference,
                            contig_index,
                            &mut state.track_cache,
                            &state.contig_header,
                        )
                        .await?;
                    state
                        .contig_header
                        .update_cytoband(contig_index, cytoband)?;
                    loaded_data = true;
                } else if state.reference.is_none() {
                    // Cannot load cytobands without reference
                } else {
                    // track service not available
                }
            }
        }

        Ok(loaded_data)
    }

    pub async fn load_all_data(
        state: &mut State,
        repository: &Repository,
        region: Region,
    ) -> Result<bool, TGVError> {
        let loaded_alignment = Self::handle_data_message(
            state,
            repository,
            DataMessage::RequiresCompleteAlignments(region.clone()),
        )
        .await?;
        let loaded_track = Self::handle_data_message(
            state,
            repository,
            DataMessage::RequiresCompleteFeatures(region.clone()),
        )
        .await?;
        let loaded_sequence = Self::handle_data_message(
            state,
            repository,
            DataMessage::RequiresCompleteSequences(region.clone()),
        )
        .await?;
        Ok(loaded_alignment || loaded_track || loaded_sequence)
    }

    pub fn has_complete_alignment(state: &State, region: &Region) -> bool {
        state.alignment.is_some() && state.alignment.as_ref().unwrap().has_complete_data(region)
    }

    pub fn has_complete_track(state: &State, region: &Region) -> bool {
        // self.track_cache.get_track(region.contig()) == Some(None)
        state.track.is_some() && state.track.as_ref().unwrap().has_complete_data(region)
    }

    pub fn has_complete_sequence(state: &State, region: &Region) -> bool {
        state.sequence.is_some() && state.sequence.as_ref().unwrap().has_complete_data(region)
    }
}
