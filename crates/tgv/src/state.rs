use gv_core::settings::Settings;
use gv_core::tracks::TrackService;
use gv_core::{
    alignment::Alignment,
    contig_header::ContigHeader,
    cytoband::Cytoband,
    error::TGVError,
    feature::Gene,
    intervals::{GenomeInterval, Region},
    message::{AlignmentDisplayOption, AlignmentFilter, DataMessage, Message},
    reference::Reference,
    //register::Registers,
    //rendering::{MainLayout, layout::resize_node},
    repository::Repository,
    sequence::{Sequence, SequenceRepository},
    track::Track,
    window::{Rect, ViewingWindow},
};
use itertools::Itertools;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Scene {
    Main,
    Help,
    ContigList,
}

/// Holds states of the application.
pub struct State {
    pub messages: Vec<String>,

    pub contig_header: ContigHeader,
    pub reference: Reference,
    pub alignment: Alignment,
    pub alignment_options: Vec<AlignmentDisplayOption>,
    pub track: Track<Gene>,
    pub sequence: Sequence,
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
        state.messages.clear();

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
            // Movement handling
            Message::MoveLeft(n) => StateHandler::move_left(state, n)?,
            Message::MoveRight(n) => StateHandler::move_right(state, n)?,
            Message::MoveUp(n) => StateHandler::move_up(state, n)?,
            Message::MoveDown(n) => StateHandler::move_down(state, n)?,
            Message::GotoCoordinate(n) => StateHandler::go_to_coordinate(state, n)?,
            Message::GotoContigNameCoordinate(contig_str, n) => {
                StateHandler::go_to_contig_coordinate(
                    state,
                    state.contig_header.try_get_index_by_str(&contig_str)?,
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
            Message::GotoPreviousContig(n) => StateHandler::go_to_previous_contig(state, n).await?,
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

            // Message::SwitchScene(display_mode) => {
            //     state.scene = display_mode;
            // }

            // Message::ResizeTrack {
            //     mouse_down_x,
            //     mouse_down_y,
            //     mouse_released_x,
            //     mouse_released_y,
            // } => {
            //     let mut new_node = state.layout.root.clone();

            //     resize_node(
            //         &mut new_node,
            //         *state.area(),
            //         mouse_down_x,
            //         mouse_down_y,
            //         mouse_released_x,
            //         mouse_released_y,
            //     )?;

            //     state.layout.root = new_node;
            // }
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
        }

        Self::get_data_requirements(state, repository)
    }
}

// Data message handling

impl StateHandler {
    fn get_data_requirements(
        state: &State,
        repository: &mut Repository, // settings: &Settings,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages = Vec::new();

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
}

// Movement handling
impl StateHandler {
    fn move_up(state: &mut State, n: usize) -> Result<(), TGVError> {
        state.window.set_top(
            state.window.top().saturating_sub(n),
            &state.main_area,
            state.alignment.depth(),
        );
        Ok(())
    }
    fn move_down(state: &mut State, n: usize) -> Result<(), TGVError> {
        state.window.set_top(
            state.window.top().saturating_add(n),
            &state.main_area,
            state.alignment.depth(),
        );
        Ok(())
    }
    fn go_to_coordinate(state: &mut State, n: usize) -> Result<(), TGVError> {
        let contig_length = state.contig_length()?;

        state.window.set_middle(&state.main_area, n, contig_length);
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
            .set_middle(&state.main_area, n, state.contig_length()?);
        state
            .window
            .set_top(0, &state.main_area, state.alignment.depth());

        Ok(())
    }

    fn go_to_y(state: &mut State, y: usize) -> Result<(), TGVError> {
        state
            .window
            .set_top(y, &state.main_area, state.alignment.depth());

        Ok(())
    }

    fn handle_zoom_out(state: &mut State, r: usize) -> Result<(), TGVError> {
        state
            .window
            .zoom_out(r, &state.main_area, state.contig_length()?)
            .unwrap();
        Ok(())
    }

    fn handle_zoom_in(state: &mut State, r: usize) -> Result<(), TGVError> {
        state
            .window
            .zoom_in(r, &state.main_area, state.contig_length()?)
            .unwrap();
        Ok(())
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
                            .as_mut()
                            .unwrap()
                            .read_alignment(&region, &state.sequence, &state.contig_header)
                            .await?;

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
                        .try_update_cytoband(contig_index, cytoband)?;
                    loaded_data = true;
                }
            }
        }

        Ok(loaded_data)
    }
}
