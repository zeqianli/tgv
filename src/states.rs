use crate::error::TGVError;
use crate::intervals::GenomeInterval;
use crate::message::AlignmentDisplayOption;
use crate::message::AlignmentFilter;
use crate::repository::AlignmentRepository;
use crate::repository::Repository;
use crate::settings::Settings;
use crate::tracks::TrackService;
use crate::{
    alignment::Alignment,
    contig_header::ContigHeader,
    cytoband::Cytoband,
    feature::Gene,
    intervals::Region,
    message::{DataMessage, Message},
    reference::Reference,
    register::{KeyRegisterType, Registers},
    rendering::{layout::resize_node, MainLayout, Scene},
    sequence::{Sequence, SequenceRepository},
    track::Track,
    window::ViewingWindow,
};
use itertools::Itertools;
use ratatui::layout::Rect;

/// Holds states of the application.
pub struct State {
    pub exit: bool,

    pub window: ViewingWindow,
    pub scene: Scene,
    pub layout: MainLayout,
    pub messages: Vec<String>,

    pub contig_header: ContigHeader,
    pub reference: Reference,
    pub alignment: Alignment,
    pub alignment_options: Vec<AlignmentDisplayOption>,
    pub track: Track<Gene>,
    pub sequence: Sequence,
}

/// Getters
impl State {
    pub fn new(
        settings: &Settings,
        initial_area: Rect,
        contigs: ContigHeader,
    ) -> Result<Self, TGVError> {
        Ok(Self {
            window: ViewingWindow::default(),
            exit: false,

            reference: settings.reference.clone(),

            // /settings: settings.clone(),
            messages: Vec::new(),

            alignment: Alignment::default(),
            alignment_options: Vec::new(),
            track: Track::<Gene>::default(),
            sequence: Sequence::default(),
            contig_header: contigs,

            scene: Scene::Main,
            layout: MainLayout::initialize(settings, initial_area)?,
        })
    }

    pub fn area(&self) -> &Rect {
        &self.layout.main_area
    }

    pub fn set_area(&mut self, area: Rect) -> Result<(), TGVError> {
        self.layout.set_area(area).map(|_| ())
    }

    pub fn viewing_region(&self) -> Region {
        Region {
            contig_index: self.window.contig_index,
            start: self.window.left(),
            end: self.window.right(self.area()),
        }
    }

    /// Middle coordinate of bases displayed on the screen.
    /// 1-based, inclusive.
    pub fn middle(&self) -> usize {
        self.window.middle(self.area())
    }

    pub fn contig_index(&self) -> usize {
        self.window.contig_index
    }

    pub fn contig_name(&self) -> Result<&String, TGVError> {
        self.contig_header.get_name(self.contig_index())
    }

    pub fn current_cytoband(&self) -> Option<&Cytoband> {
        self.contig_header.cytoband(self.contig_index())
    }

    /// Maximum length of the contig.
    pub fn contig_length(&self) -> Result<Option<usize>, TGVError> {
        Ok(self.contig_header.get(self.contig_index())?.length)
    }

    pub fn self_correct_viewing_window(&mut self) {
        self.window
            .self_correct(&self.layout.main_area, self.contig_length().unwrap());
    }
}

pub struct StateHandler {}

impl StateHandler {
    /// Handle initial messages.
    /// This has different error handling strategy (loud) vs handle(...), which suppresses errors.
    pub async fn handle_initial_messages(
        state: &mut State,
        repository: &mut Repository,
        registers: &mut Registers,
        settings: &Settings,
        messages: Vec<Message>,
    ) -> Result<(), TGVError> {
        let mut data_messages = Vec::new();

        for message in messages {
            data_messages.extend(
                StateHandler::handle_state_message(state, repository, registers, settings, message)
                    .await?,
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
        repository: &mut Repository,
        registers: &mut Registers,
        settings: &Settings,
        messages: Vec<Message>,
    ) -> Result<(), TGVError> {
        StateHandler::clear_messages(state)?;

        let mut data_messages: Vec<DataMessage> = Vec::new();

        for message in messages {
            match StateHandler::handle_state_message(
                state, repository, registers, settings, message,
            )
            .await
            {
                Ok(messages) => data_messages.extend(messages),
                Err(e) => return StateHandler::add_message(state, e.to_string()),
            }
        }

        let data_messages = StateHandler::get_data_requirements(state, repository)?;

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
        repository: &mut Repository,
        registers: &mut Registers,
        settings: &Settings,
        message: Message,
    ) -> Result<Vec<DataMessage>, TGVError> {
        match message {
            // Swithching modes
            Message::Quit => StateHandler::quit(state)?,

            // Movement handling
            Message::MoveLeft(n) => StateHandler::move_left(state, n)?,
            Message::MoveRight(n) => StateHandler::move_right(state, n)?,
            Message::MoveUp(n) => StateHandler::move_up(state, n)?,
            Message::MoveDown(n) => StateHandler::move_down(state, n)?,
            Message::GotoCoordinate(n) => StateHandler::go_to_coordinate(state, n)?,
            Message::GotoContigNameCoordinate(contig_str, n) => {
                StateHandler::go_to_contig_coordinate(
                    state,
                    state.contig_header.get_index_by_str(&contig_str)?,
                    n,
                )?
            }

            Message::GotoY(y) => StateHandler::go_to_y(state, y)?,
            Message::GotoYBottom => StateHandler::go_to_y(state, state.alignment.depth())?,

            // Zoom handling
            Message::ZoomOut(r) => StateHandler::handle_zoom_out(state, r)?,
            Message::ZoomIn(r) => StateHandler::handle_zoom_in(state, r)?,

            // Relative feature movement handling
            Message::GotoNextExonsStart(n) => {
                StateHandler::go_to_next_exons_start(state, repository, n).await?
            }
            Message::GotoNextExonsEnd(n) => {
                StateHandler::go_to_next_exons_end(state, repository, n).await?
            }
            Message::GotoPreviousExonsStart(n) => {
                StateHandler::go_to_previous_exons_start(state, repository, n).await?
            }
            Message::GotoPreviousExonsEnd(n) => {
                StateHandler::go_to_previous_exons_end(state, repository, n).await?
            }
            Message::GotoNextGenesStart(n) => {
                StateHandler::go_to_next_genes_start(state, repository, n).await?
            }
            Message::GotoNextGenesEnd(n) => {
                StateHandler::go_to_next_genes_end(state, repository, n).await?
            }
            Message::GotoPreviousGenesStart(n) => {
                StateHandler::go_to_previous_genes_start(state, repository, n).await?
            }
            Message::GotoPreviousGenesEnd(n) => {
                StateHandler::go_to_previous_genes_end(state, repository, n).await?
            }
            Message::GotoNextContig(n) => StateHandler::go_to_next_contig(state, n).await?,
            Message::GotoPreviousContig(n) => {
                StateHandler::go_to_previous_contig(state, n).await?
            }
            Message::GotoContigIndex(index) => {
                StateHandler::go_to_contig_index(state, index).await?
            }

            // Absolute feature handling
            Message::GoToGene(gene_id) => {
                StateHandler::go_to_gene(state, repository, gene_id).await?
            }

            // Find the default region
            Message::GoToDefault => StateHandler::go_to_default(state, repository).await?,

            // Error messages
            Message::Message(message) => StateHandler::add_message(state, message)?,

            Message::SwitchScene(display_mode) => {
                state.scene = display_mode;
            }

            Message::ResizeTrack {
                mouse_down_x,
                mouse_down_y,
                mouse_released_x,
                mouse_released_y,
            } => {
                let mut new_node = state.layout.root.clone();

                resize_node(
                    &mut new_node,
                    *state.area(),
                    mouse_down_x,
                    mouse_down_y,
                    mouse_released_x,
                    mouse_released_y,
                )?;

                state.layout.root = new_node;
            }

            Message::SetAlignmentChange(options) => {
                let middle = state.middle();

                state.alignment.reset(&state.sequence)?;

                let options = options
                    .into_iter()
                    .map(|option| match option {
                        AlignmentDisplayOption::Filter(AlignmentFilter::BaseAtCurrentPosition(
                            base,
                        )) => AlignmentDisplayOption::Filter(AlignmentFilter::Base(middle, base)),

                        AlignmentDisplayOption::Filter(
                            AlignmentFilter::BaseAtCurrentPositionSoftClip,
                        ) => AlignmentDisplayOption::Filter(AlignmentFilter::BaseSoftclip(middle)),

                        _ => option,
                    })
                    .collect_vec();
                state.alignment_options = options;
                let _ = state
                    .alignment
                    .apply_options(&state.alignment_options, &state.sequence)?;
            }

            Message::AddAlignmentChange(options) => {}

            Message::ClearAllKeyRegisters => registers.clear(),

            Message::ClearKeyRegister(register_type) => {
                todo!()
            }

            Message::SwitchKeyRegister(register_type) => registers.current = register_type,
        }

        Self::get_data_requirements(state, repository)
    }
}

// Data message handling

impl StateHandler {
    fn quit(state: &mut State) -> Result<(), TGVError> {
        state.exit = true;
        Ok(())
    }

    fn add_message(state: &mut State, message: String) -> Result<(), TGVError> {
        state.messages.push(message);
        Ok(())
    }

    fn clear_messages(state: &mut State) -> Result<(), TGVError> {
        state.messages.clear();
        Ok(())
    }

    fn get_data_requirements(
        state: &State,
        repository: &mut Repository, // settings: &Settings,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages = Vec::new();

        let viewing_region = state.viewing_region();

        // It's important to load sequence first!
        // Alignment IO requires calculating mismatches with the reference sequence.

        if repository.sequence_service.is_some()
            && state.window.sequence_renderable()
            && !state.sequence.has_complete_data(&viewing_region)
        {
            let sequence_cache_region = Self::sequence_cache_region(state, &viewing_region)?;
            data_messages.push(DataMessage::RequiresCompleteSequences(
                sequence_cache_region,
            ));
        }
        if repository.alignment_repository.is_some()
            && state.window.alignment_renderable()
            && !state.alignment.has_complete_data(&viewing_region)
        {
            let alignment_cache_region = Self::alignment_cache_region(state, &viewing_region)?;
            data_messages.push(DataMessage::RequiresCompleteAlignments(
                alignment_cache_region,
            ));
        }

        if repository.track_service.is_some() {
            if !state.track.has_complete_data(&viewing_region) {
                // viewing_window.zoom <= Self::MAX_ZOOM_TO_DISPLAY_FEATURES is always true
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

    const SEQUENCE_CACHE_RATIO: usize = 6;

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
        let area = &state.layout.main_area;

        state.window.set_left(
            state.window.left().saturating_sub(n * state.window.zoom),
            area,
            contig_length,
        );
        Ok(())
    }
    fn move_right(state: &mut State, n: usize) -> Result<(), TGVError> {
        let contig_length: Option<usize> = state.contig_length()?;

        state.window.set_left(
            state.window.left().saturating_add(n * state.window.zoom),
            &state.layout.main_area,
            contig_length,
        );
        Ok(())
    }
    fn move_up(state: &mut State, n: usize) -> Result<(), TGVError> {
        state.window.set_top(
            state.window.top().saturating_sub(n),
            &state.layout.main_area,
            state.alignment.depth(),
        );
        Ok(())
    }
    fn move_down(state: &mut State, n: usize) -> Result<(), TGVError> {
        state.window.set_top(
            state.window.top().saturating_add(n),
            &state.layout.main_area,
            state.alignment.depth(),
        );
        Ok(())
    }
    fn go_to_coordinate(state: &mut State, n: usize) -> Result<(), TGVError> {
        let contig_length = state.contig_length()?;

        state
            .window
            .set_middle(&state.layout.main_area, n, contig_length);
        Ok(())
    }
    fn go_to_contig_coordinate(
        state: &mut State,
        contig_index: usize,
        n: usize,
    ) -> Result<(), TGVError> {
        state.window.contig_index = contig_index;
        state
            .window
            .set_middle(&state.layout.main_area, n, state.contig_length()?);
        state
            .window
            .set_top(0, &state.layout.main_area, state.alignment.depth());

        Ok(())
    }

    fn go_to_y(state: &mut State, y: usize) -> Result<(), TGVError> {
        state
            .window
            .set_top(y, &state.layout.main_area, state.alignment.depth());

        Ok(())
    }

    fn handle_zoom_out(state: &mut State, r: usize) -> Result<(), TGVError> {
        state
            .window
            .zoom_out(r, &state.layout.main_area, state.contig_length()?)
            .unwrap();
        Ok(())
    }

    fn handle_zoom_in(state: &mut State, r: usize) -> Result<(), TGVError> {
        state
            .window
            .zoom_in(r, &state.layout.main_area, state.contig_length()?)
            .unwrap();
        Ok(())
    }

    async fn go_to_next_genes_start(
        state: &mut State,
        repository: &mut Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();

        // The gene is in the track.
        if let Some(target_gene) = state.track.get_k_genes_after(middle, n) {
            return Self::go_to_coordinate(state, target_gene.start() + 1);
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_after(
                &state.reference,
                state.contig_index(),
                middle,
                n,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, gene.start() + 1)
    }

    async fn go_to_next_genes_end(
        state: &mut State,
        repository: &mut Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();

        if let Some(target_gene) = state.track.get_k_genes_after(middle, n) {
            return Self::go_to_coordinate(state, target_gene.end() + 1);
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_after(
                &state.reference,
                state.contig_index(),
                middle,
                n,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, gene.end() + 1)
    }

    async fn go_to_previous_genes_start(
        state: &mut State,
        repository: &mut Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();

        if let Some(target_gene) = state.track.get_k_genes_before(middle, n) {
            return Self::go_to_coordinate(state, target_gene.start() - 1);
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_before(
                &state.reference,
                state.contig_index(),
                middle,
                n,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, gene.start() - 1)
    }

    async fn go_to_previous_genes_end(
        state: &mut State,
        repository: &mut Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();

        if let Some(target_gene) = state.track.get_k_genes_before(middle, n) {
            return Self::go_to_coordinate(state, target_gene.end() - 1);
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_before(
                &state.reference,
                state.contig_index(),
                middle,
                n,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, gene.end() - 1)
    }

    async fn go_to_next_exons_start(
        state: &mut State,
        repository: &mut Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();

        if let Some(target_exon) = state.track.get_k_exons_after(middle, n) {
            return Self::go_to_coordinate(state, target_exon.start() + 1);
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_after(
                &state.reference,
                state.contig_index(),
                middle,
                n,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, exon.start() + 1)
    }

    async fn go_to_next_exons_end(
        state: &mut State,
        repository: &mut Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();

        if let Some(target_exon) = state.track.get_k_exons_after(middle, n) {
            return Self::go_to_coordinate(state, target_exon.end() + 1);
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_after(
                &state.reference,
                state.contig_index(),
                middle,
                n,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, exon.end() + 1)
    }

    async fn go_to_previous_exons_start(
        state: &mut State,
        repository: &mut Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();

        if let Some(target_exon) = state.track.get_k_exons_before(middle, n) {
            return Self::go_to_coordinate(state, target_exon.start() - 1);
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_before(
                &state.reference,
                state.contig_index(),
                middle,
                n,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, exon.start() - 1)
    }

    async fn go_to_previous_exons_end(
        state: &mut State,
        repository: &mut Repository,
        n: usize,
    ) -> Result<(), TGVError> {
        if n == 0 {
            return Ok(());
        }

        let middle = state.middle();

        let target_exon = state.track.get_k_exons_before(middle, n);
        if let Some(target_exon) = target_exon {
            return Self::go_to_coordinate(state, target_exon.end() - 1);
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_before(
                &state.reference,
                state.contig_index(),
                middle,
                n,
                &state.contig_header,
            )
            .await?;

        Self::go_to_coordinate(state, exon.end() - 1)
    }

    async fn go_to_gene(
        state: &mut State,
        repository: &mut Repository,
        gene_name: String,
    ) -> Result<(), TGVError> {
        let gene = repository
            .track_service_checked()?
            .query_gene_name(&state.reference, &gene_name, &state.contig_header)
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

    async fn go_to_default(state: &mut State, repository: &mut Repository) -> Result<(), TGVError> {
        match state.reference {
            Reference::Hg38 | Reference::Hg19 => {
                return Self::go_to_gene(state, repository, "TP53".to_string()).await;
            }

            Reference::UcscGenome(_) | Reference::UcscAccession(_) => {
                // Find the first gene on the first contig. If anything is not found, handle it later.

                let first_contig = state.contig_header.first()?;

                // Try to get the first gene in the first contig.
                // We use query_k_genes_after starting from coordinate 0 with k=1.
                match repository
                    .track_service_checked()?
                    .query_k_genes_after(&state.reference, first_contig, 0, 1, &state.contig_header)
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

            Reference::BYOIndexedFasta(_) | Reference::BYOTwoBit(_) | Reference::NoReference => {} // handle later
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
        repository: &mut Repository,
        data_message: DataMessage,
    ) -> Result<bool, TGVError> {
        let mut loaded_data = false;

        match data_message {
            DataMessage::RequiresCompleteAlignments(region) => {
                if !state.alignment.has_complete_data(&region) {
                    state.alignment = {
                        let mut alignment = repository
                            .alignment_repository
                            .as_ref()
                            .unwrap()
                            .read_alignment(&region, &state.sequence, &state.contig_header)?;

                        alignment.apply_options(&state.alignment_options, &state.sequence)?;

                        alignment
                    };

                    // apply sorting and filtering

                    loaded_data = true;
                }
            }
            DataMessage::RequiresCompleteFeatures(region) => {
                let has_complete_track = state.track.has_complete_data(&region);
                if let Some(track_service) = repository.track_service.as_mut() {
                    if !has_complete_track {
                        if let Ok(track) = track_service
                            .query_gene_track(&state.reference, &region, &state.contig_header)
                            .await
                        {
                            state.track = track;
                            loaded_data = true;
                        } else {
                            // Do nothing (track not found). TODO: fix this shit properly.
                        }
                    }
                } else {
                    loaded_data = match state.reference {
                        // FIXME: this is duplicate code as Settings.
                        Reference::BYOIndexedFasta(_) => false,
                        _ => true,
                    };
                }
            }
            DataMessage::RequiresCompleteSequences(region) => {
                let sequence_service = repository.sequence_service_checked()?;

                if !state.sequence.has_complete_data(&region) {
                    let sequence = sequence_service
                        .query_sequence(&region, &state.contig_header)
                        .await?;

                    state.sequence = sequence;
                    loaded_data = true;
                }
            }

            DataMessage::RequiresCytobands(contig_index) => {
                if state.contig_header.cytoband_is_loaded(contig_index)? {
                    return Ok(false);
                }

                if let Some(track_service) = repository.track_service.as_mut() {
                    let cytoband = track_service
                        .get_cytoband(&state.reference, contig_index, &state.contig_header)
                        .await?;
                    state
                        .contig_header
                        .update_cytoband(contig_index, cytoband)?;
                    loaded_data = true;
                }
            }
        }

        Ok(loaded_data)
    }
}
