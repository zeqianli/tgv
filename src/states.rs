use crate::error::TGVError;
use crate::helpers::is_url;
use crate::models::{
    contig::Contig,
    cytoband::Cytoband,
    data::Data,
    message::{DataMessage, StateMessage},
    mode::InputMode,
    reference::Reference,
    region::Region,
    register::{CommandModeRegister, NormalModeRegister},
    services::track_service::{TrackService, UcscApiTrackService, UcscDbTrackService},
    window::ViewingWindow,
};
use crate::models::{reference, track};
use crate::settings::Settings;
use crate::traits::GenomeInterval;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use rust_htslib::bam::{self, IndexedReader, Read};
use std::collections::HashMap;
use url::Url;

/// Holds states of the application.
pub struct State {
    /// Basics
    pub input_mode: InputMode,
    pub exit: bool,

    /// Viewing window.
    window: Option<ViewingWindow>,
    area: Option<Rect>,

    // Data
    pub data: Data,

    // Registers
    normal_mode_register: NormalModeRegister,
    command_mode_register: CommandModeRegister,

    /// Settings
    pub settings: Settings,

    /// Error messages for display.
    pub errors: Vec<String>,
}

/// Basics
impl State {
    pub async fn new(settings: Settings) -> Result<Self, TGVError> {
        let data = Data::new(&settings).await?;

        Ok(Self {
            window: None,
            input_mode: InputMode::Normal,
            exit: false,
            area: None,
            data,

            normal_mode_register: NormalModeRegister::new(),
            command_mode_register: CommandModeRegister::new(),

            settings,
            errors: Vec::new(),
        })
    }

    pub fn update_frame_area(&mut self, area: Rect) {
        self.area = Some(area);
    }

    pub fn self_correct_viewing_window(&mut self) {
        let area = *self.current_frame_area().unwrap();
        let contig_length = self.contig_length().unwrap();
        if let Ok(viewing_window) = self.viewing_window_mut() {
            viewing_window.self_correct(&area, contig_length);
        }
    }

    pub fn viewing_window(&self) -> Result<&ViewingWindow, TGVError> {
        if self.window.is_none() {
            return Err(TGVError::StateError(
                "Viewing window is not initialized".to_string(),
            ));
        }
        Ok(self.window.as_ref().unwrap())
    }

    pub fn viewing_window_mut(&mut self) -> Result<&mut ViewingWindow, TGVError> {
        if self.window.is_none() {
            return Err(TGVError::StateError(
                "Viewing window is not initialized".to_string(),
            ));
        }
        Ok(self.window.as_mut().unwrap())
    }

    pub fn current_frame_area(&self) -> Result<&Rect, TGVError> {
        if self.area.is_none() {
            return Err(TGVError::StateError(
                "Current frame area is not initialized".to_string(),
            ));
        }
        Ok(self.area.as_ref().unwrap())
    }

    pub fn viewing_region(&self) -> Result<Region, TGVError> {
        let viewing_window = self.viewing_window()?;

        Ok(Region {
            contig: viewing_window.contig.clone(),
            start: viewing_window.left(),
            end: viewing_window.right(self.current_frame_area()?),
        })
    }

    pub fn contig(&self) -> Result<Contig, TGVError> {
        Ok(self.viewing_window()?.contig.clone())
    }

    pub fn current_cytoband(&self) -> Result<Option<&Cytoband>, TGVError> {
        let contig = self.contig()?;
        let cytoband = self.data.contigs.cytoband(&contig);
        Ok(cytoband)
    }

    /// Start coordinate of bases displayed on the screen.
    /// 1-based, inclusive.
    pub fn start(&self) -> Result<usize, TGVError> {
        Ok(self.viewing_window()?.left())
    }

    /// End coordinate of bases displayed on the screen.
    /// 1-based, inclusive.
    pub fn end(&self) -> Result<usize, TGVError> {
        Ok(self.viewing_window()?.right(self.current_frame_area()?))
    }

    /// Middle coordinate of bases displayed on the screen.
    /// 1-based, inclusive.
    pub fn middle(&self) -> Result<usize, TGVError> {
        Ok(self.viewing_window()?.middle(self.current_frame_area()?))
    }

    /// Reference to the command mode register.
    pub fn command_mode_register(&self) -> &CommandModeRegister {
        &self.command_mode_register
    }

    pub fn initialized(&self) -> bool {
        self.window.is_some()
    }

    pub fn add_error_message(&mut self, error: String) {
        self.errors.push(error);
    }

    pub async fn close(&mut self) -> Result<(), TGVError> {
        self.data.close().await?;
        Ok(())
    }
}

impl State {
    pub async fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<(), TGVError> {
        let messages = self.translate_key_event(key_event);
        self.handle(messages).await
    }

    /// Handle initial messages.
    /// This has different error handling strategy (loud) vs handle(...), which suppresses errors.
    pub async fn handle_initial_messages(
        &mut self,
        messages: Vec<StateMessage>,
    ) -> Result<(), TGVError> {
        let data_messages = self.handle_state_messages(messages).await?;
        let _loaded_data = self
            .data
            .handle_data_messages(self.settings.reference.as_ref(), data_messages)
            .await?;

        Ok(())
    }

    /// Handle messages.
    pub async fn handle(&mut self, messages: Vec<StateMessage>) -> Result<(), TGVError> {
        let debug_messages_0 = messages
            .iter()
            .map(|m| format!("{:?}", m))
            .collect::<Vec<String>>()
            .join(", ");
        let data_messages = self.handle_state_messages(messages).await?;

        let debug_message = data_messages
            .iter()
            .map(|m| format!("{:?}", m))
            .collect::<Vec<String>>()
            .join(", ");

        let loaded_data = self
            .data
            .handle_data_messages(self.settings.reference.as_ref(), data_messages)
            .await?;

        if self.settings.debug {
            if loaded_data {
                self.errors.push(format!(
                    "Data loaded: {}\n{}",
                    debug_messages_0, debug_message
                ));
            } else {
                self.errors.push(format!(
                    "Data not loaded: {}\n{}",
                    debug_messages_0, debug_message
                ));
            }
        }
        Ok(())
    }
}

/// State message handling
impl State {
    /// Translate key event to a message.
    fn translate_key_event(&self, key_event: KeyEvent) -> Vec<StateMessage> {
        let messages = match self.input_mode {
            InputMode::Normal => {
                match key_event.code {
                    // Switch mode
                    KeyCode::Char(':') => vec![
                        StateMessage::SwitchMode(InputMode::Command),
                        StateMessage::ClearNormalModeRegisters,
                    ],
                    _ => match self.normal_mode_register.translate(key_event.code) {
                        Ok(messages) => messages,
                        Err(error_message) => vec![
                            StateMessage::NormalModeRegisterError(error_message),
                            StateMessage::ClearNormalModeRegisters,
                        ],
                    },
                }
            }
            InputMode::Command => match key_event.code {
                KeyCode::Esc => vec![
                    StateMessage::ClearCommandModeRegisters,
                    StateMessage::SwitchMode(InputMode::Normal),
                ],
                KeyCode::Enter => {
                    let mut messages = vec![
                        StateMessage::ClearCommandModeRegisters,
                        StateMessage::SwitchMode(InputMode::Normal),
                    ];
                    messages.extend(match self.command_mode_register.parse() {
                        Ok(parsed_messages) => parsed_messages,
                        Err(error_message) => {
                            vec![StateMessage::CommandModeRegisterError(error_message)]
                        }
                    });
                    messages
                }
                _ => match self.command_mode_register.translate(key_event.code) {
                    Ok(messages) => messages,
                    Err(error_message) => {
                        vec![StateMessage::CommandModeRegisterError(error_message)]
                    }
                },
            },
            InputMode::Help => match key_event.code {
                KeyCode::Esc => vec![StateMessage::SwitchMode(InputMode::Normal)],
                _ => vec![],
            },
        };

        // Check that if there is a message that requires the reference genome, make sure it is provided.
        // Otherwise, pass on an error message.
        for message in messages.iter() {
            if message.requires_reference() && self.settings.reference.is_none() {
                return vec![StateMessage::Error(
                    TGVError::StateError("Reference is not provided".to_string()).to_string(),
                )];
            }
        }

        messages
    }

    /// Handle state messages.
    async fn handle_state_messages(
        &mut self,
        messages: Vec<StateMessage>,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages: Vec<DataMessage> = Vec::new();

        for message in messages {
            data_messages.extend(self.handle_state_message(message).await?);
        }

        Ok(data_messages)
    }

    /// Main function to route state message handling.
    async fn handle_state_message(
        &mut self,
        message: StateMessage,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages: Vec<DataMessage> = Vec::new();

        match message {
            // Swithching modes
            StateMessage::SwitchMode(mode) => {
                self.input_mode = mode;
            }
            StateMessage::Quit => self.exit = true,

            // Command mode handling
            StateMessage::AddCharToCommandModeRegisters(c) => {
                self.command_mode_register.add_char(c)
            }
            StateMessage::CommandModeRegisterError(error_message) => {
                self.add_error_message(error_message)
            }
            StateMessage::ClearCommandModeRegisters => self.command_mode_register.clear(),
            StateMessage::BackspaceCommandModeRegisters => self.command_mode_register.backspace(),
            StateMessage::MoveCursorLeft(amount) => {
                self.command_mode_register.move_cursor_left(amount)
            }
            StateMessage::MoveCursorRight(amount) => {
                self.command_mode_register.move_cursor_right(amount)
            }

            // Normal mode handling
            StateMessage::AddCharToNormalModeRegisters(c) => self.normal_mode_register.add_char(c),
            StateMessage::NormalModeRegisterError(error_message) => {
                self.add_error_message(error_message)
            }
            StateMessage::ClearNormalModeRegisters => self.normal_mode_register.clear(),

            // Movement handling
            StateMessage::MoveLeft(_)
            | StateMessage::MoveRight(_)
            | StateMessage::MoveUp(_)
            | StateMessage::MoveDown(_)
            | StateMessage::GotoCoordinate(_)
            | StateMessage::GotoContigCoordinate(_, _) => {
                data_messages.extend(self.handle_movement_message(message)?);
            }

            // Zoom handling
            StateMessage::ZoomOut(r) => data_messages.extend(self.handle_zoom_out(r)?),
            StateMessage::ZoomIn(r) => data_messages.extend(self.handle_zoom_in(r)?),

            // Relative feature movement handling
            StateMessage::GotoNextExonsStart(_)
            | StateMessage::GotoNextExonsEnd(_)
            | StateMessage::GotoPreviousExonsStart(_)
            | StateMessage::GotoPreviousExonsEnd(_) => {
                data_messages.extend(self.handle_feature_movement_message(message).await?);
            }
            StateMessage::GotoNextGenesStart(_)
            | StateMessage::GotoNextGenesEnd(_)
            | StateMessage::GotoPreviousGenesStart(_)
            | StateMessage::GotoPreviousGenesEnd(_) => {
                data_messages.extend(self.handle_feature_movement_message(message).await?);
            }

            // Absolute feature handling
            StateMessage::GoToGene(_) => {
                data_messages.extend(self.handle_goto_feature_message(message).await?);
            }

            // Find the default region
            StateMessage::GoToDefault => {
                data_messages.extend(self.handle_goto_default_message().await?);
            }

            // Error messages
            StateMessage::Error(e) => self.add_error_message(e),

            // Others
            _ => {}
        }

        Ok(data_messages)
    }
}

// Data message handling

impl State {
    pub const MAX_ZOOM_TO_DISPLAY_FEATURES: usize = usize::MAX;
    pub const MAX_ZOOM_TO_DISPLAY_ALIGNMENTS: usize = 32;
    pub const MAX_ZOOM_TO_DISPLAY_SEQUENCES: usize = 2;
    fn get_data_requirements(&self) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages = Vec::new();

        let viewing_window = self.viewing_window()?;
        let viewing_region = self.viewing_region()?;

        if self.settings.bam_path.is_some()
            && viewing_window.zoom() <= Self::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS
            && !self.data.has_complete_alignment(&viewing_region)
        {
            let alignment_cache_region = self.alignment_cache_region(&viewing_region)?;
            data_messages.push(DataMessage::RequiresCompleteAlignments(
                alignment_cache_region,
            ));
        }

        if self.settings.reference.is_some() {
            if !self.data.has_complete_track(&viewing_region) {
                // viewing_window.zoom() <= Self::MAX_ZOOM_TO_DISPLAY_FEATURES is always true
                let track_cache_region = self.track_cache_region(&viewing_region)?;
                data_messages.push(DataMessage::RequiresCompleteFeatures(track_cache_region));
            }

            if (viewing_window.zoom() <= Self::MAX_ZOOM_TO_DISPLAY_SEQUENCES)
                && !self.data.has_complete_sequence(&viewing_region)
            {
                let sequence_cache_region = self.sequence_cache_region(&viewing_region)?;
                data_messages.push(DataMessage::RequiresCompleteSequences(
                    sequence_cache_region,
                ));
            }

            // Cytobands
            data_messages.push(DataMessage::RequiresCytobands(self.contig()?));
        }

        Ok(data_messages)
    }

    const ALIGNMENT_CACHE_RATIO: usize = 3;

    fn alignment_cache_region(&self, region: &Region) -> Result<Region, TGVError> {
        let left = region
            .start
            .saturating_sub(Self::ALIGNMENT_CACHE_RATIO * region.width() / 2)
            .max(1);
        let right = region
            .end
            .saturating_add(Self::ALIGNMENT_CACHE_RATIO * region.width() / 2)
            .min(if let Some(contig_length) = self.contig_length()? {
                contig_length
            } else {
                usize::MAX
            });
        Ok(Region {
            contig: region.contig.clone(),
            start: left,
            end: right,
        })
    }

    const SEQUENCE_CACHE_RATIO: usize = 3;

    fn sequence_cache_region(&self, region: &Region) -> Result<Region, TGVError> {
        let left = region
            .start
            .saturating_sub(Self::SEQUENCE_CACHE_RATIO * region.width() / 2)
            .max(1);
        let right = region
            .end
            .saturating_add(Self::SEQUENCE_CACHE_RATIO * region.width() / 2)
            .min(if let Some(contig_length) = self.contig_length()? {
                contig_length
            } else {
                usize::MAX
            });
        Ok(Region {
            contig: region.contig.clone(),
            start: left,
            end: right,
        })
    }

    const TRACK_CACHE_RATIO: usize = 10;

    fn track_cache_region(&self, region: &Region) -> Result<Region, TGVError> {
        let left = region
            .start
            .saturating_sub(Self::TRACK_CACHE_RATIO * region.width() / 2)
            .max(1);
        let right = region
            .end
            .saturating_add(Self::TRACK_CACHE_RATIO * region.width() / 2)
            .min(if let Some(contig_length) = self.contig_length()? {
                contig_length
            } else {
                usize::MAX
            });
        Ok(Region {
            contig: region.contig.clone(),
            start: left,
            end: right,
        })
    }
}

// Movement handling
impl State {
    fn handle_movement_message(
        &mut self,
        message: StateMessage,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages = Vec::new();

        match message {
            // TODO: bound handling
            StateMessage::MoveLeft(n) => {
                let current_frame_area = *self.current_frame_area()?;
                let contig_length = self.contig_length()?;
                let viewing_window = self.viewing_window_mut()?;

                viewing_window.set_left(
                    viewing_window
                        .left()
                        .saturating_sub(n * viewing_window.zoom()),
                    &current_frame_area,
                    contig_length,
                );
            }
            StateMessage::MoveRight(n) => {
                let current_frame_area = *self.current_frame_area()?;

                let contig_length: Option<usize> = self.contig_length()?;
                let viewing_window = self.viewing_window_mut()?;

                viewing_window.set_left(
                    viewing_window
                        .left()
                        .saturating_add(n * viewing_window.zoom()),
                    &current_frame_area,
                    contig_length,
                );
            }
            StateMessage::MoveUp(n) => {
                let viewing_window = self.viewing_window_mut()?;

                viewing_window.set_top(viewing_window.top().saturating_sub(n));
            }
            StateMessage::MoveDown(n) => {
                let viewing_window = self.viewing_window_mut()?;

                viewing_window.set_top(viewing_window.top().saturating_add(n));
            }

            StateMessage::GotoCoordinate(n) => {
                let current_frame_area = *self.current_frame_area()?;
                let contig_length = self.contig_length()?;
                let viewing_window = self.viewing_window_mut()?;

                viewing_window.set_middle(&current_frame_area, n, contig_length);
            }
            StateMessage::GotoContigCoordinate(contig, n) => {
                // If bam_path is provided, check that the contig is valid.
                let contig = match self.settings.reference {
                    Some(Reference::Hg38) | Some(Reference::Hg19) => Contig::chrom(&contig),
                    _ => Contig::contig(&contig),
                };
                if !self.data.contigs.contains(&contig) {
                    return Err(TGVError::StateError(format!(
                        "Contig {} not found for reference {}",
                        contig.full_name(),
                        self.settings.reference.as_ref().unwrap()
                    )));
                }

                let current_frame_area = *self.current_frame_area()?;

                match self.window {
                    Some(ref mut window) => {
                        window.contig = contig;
                        window.set_middle(&current_frame_area, n, None); // Don't know contig length yet.
                        window.set_top(0);
                    }
                    None => {
                        self.window = Some(ViewingWindow::new_basewise_window(contig, n, 0));
                    }
                }
            }

            _ => {} // TOOD: GotoContig, GotoPreviousContig, GotoNextContig
        }

        data_messages.extend(self.get_data_requirements()?);
        Ok(data_messages)
    }
}

/// Zoom handling
impl State {
    fn handle_zoom_out(&mut self, r: usize) -> Result<Vec<DataMessage>, TGVError> {
        let contig_length = self.contig_length()?;
        let current_frame_area = *self.current_frame_area()?;
        let viewing_window = self.viewing_window_mut()?;

        viewing_window
            .zoom_out(r, &current_frame_area, contig_length)
            .unwrap();
        self.get_data_requirements()
    }

    fn handle_zoom_in(&mut self, r: usize) -> Result<Vec<DataMessage>, TGVError> {
        let contig_length = self.contig_length()?;
        let current_frame_area: Rect = *self.current_frame_area()?;
        let viewing_window = self.viewing_window_mut()?;

        viewing_window
            .zoom_in(r, &current_frame_area, contig_length)
            .unwrap();
        self.get_data_requirements()
    }

    /// Maximum length of the contig.
    pub fn contig_length(&self) -> Result<Option<usize>, TGVError> {
        let contig = self.contig()?;

        if let Some(length) = self.data.contigs.length(&contig) {
            return Ok(Some(length));
        }
        Ok(None)
    }
}

/// Feature movement handling
impl State {
    async fn handle_feature_movement_message(
        &mut self,
        message: StateMessage,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut state_messages = Vec::new();
        let contig = self.contig()?;
        let middle = self.middle()?;

        if self.data.track_service.is_none() {
            return Err(TGVError::StateError(
                "Track service not initialized".to_string(),
            ));
        }

        let track_service = self.data.track_service.as_mut().unwrap();
        if let Some(reference) = self.settings.reference.as_ref() {
            track_service
                .check_or_load_contig(reference, &contig)
                .await?;
        } else {
            return Err(TGVError::StateError(
                "No reference is provided. Cannot handle feature movement.".to_string(),
            )); // TODO: this breaks the app. handle this gracefully.
        }

        let track = match self.data.track.as_ref() {
            Some(track) => track,
            None => return Err(TGVError::StateError("Track not initialized".to_string())),
        };

        match message {
            StateMessage::GotoNextGenesStart(n_movements) => {
                if n_movements == 0 {
                    return self.get_data_requirements();
                }

                let target_gene = track.get_k_genes_after(middle, n_movements);
                if let Some(target_gene) = target_gene {
                    state_messages.push(StateMessage::GotoCoordinate(target_gene.start() + 1));
                } else {
                    // Query for the target gene
                    let gene = track_service
                        .query_k_genes_after(&contig, middle, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(gene.start() + 1));
                }
            }
            StateMessage::GotoNextGenesEnd(n_movements) => {
                if n_movements == 0 {
                    return self.get_data_requirements();
                }

                let target_gene = track.get_k_genes_after(self.middle()?, n_movements);
                if let Some(target_gene) = target_gene {
                    state_messages.push(StateMessage::GotoCoordinate(target_gene.end() + 1));
                } else {
                    // Query for the target gene
                    let track_service = self.data.track_service.as_ref().unwrap();
                    let gene = track_service
                        .query_k_genes_after(&self.contig()?, self.middle()?, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(gene.end() + 1));
                }
            }
            StateMessage::GotoPreviousGenesStart(n_movements) => {
                if n_movements == 0 {
                    return self.get_data_requirements();
                }

                let target_gene = track.get_k_genes_before(middle, n_movements);
                if let Some(target_gene) = target_gene {
                    state_messages.push(StateMessage::GotoCoordinate(target_gene.start() - 1));
                } else {
                    // Query for the target gene
                    let gene = track_service
                        .query_k_genes_before(&contig, middle, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(gene.start() - 1));
                }
            }

            StateMessage::GotoPreviousGenesEnd(n_movements) => {
                if n_movements == 0 {
                    return self.get_data_requirements();
                }

                let target_gene = track.get_k_genes_before(middle, n_movements);
                if let Some(target_gene) = target_gene {
                    state_messages.push(StateMessage::GotoCoordinate(target_gene.end() - 1));
                } else {
                    // Query for the target gene
                    let gene = track_service
                        .query_k_genes_before(&contig, middle, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(gene.end() - 1));
                }
            }

            StateMessage::GotoNextExonsStart(n_movements) => {
                if n_movements == 0 {
                    return self.get_data_requirements();
                }

                let target_exon = track.get_k_exons_after(middle, n_movements);
                if let Some(target_exon) = target_exon {
                    state_messages.push(StateMessage::GotoCoordinate(target_exon.start() + 1));
                } else {
                    // Query for the target exon
                    let exon = track_service
                        .query_k_exons_after(&contig, middle, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(exon.start() + 1));
                }
            }
            StateMessage::GotoNextExonsEnd(n_movements) => {
                if n_movements == 0 {
                    return self.get_data_requirements();
                }

                let target_exon = track.get_k_exons_after(middle, n_movements);
                if let Some(target_exon) = target_exon {
                    state_messages.push(StateMessage::GotoCoordinate(target_exon.end() + 1));
                    // this prevents continuous movements getting stuck
                } else {
                    // Query for the target exon
                    let exon = track_service
                        .query_k_exons_after(&contig, middle, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(exon.end() + 1));
                }
            }

            StateMessage::GotoPreviousExonsStart(n_movements) => {
                if n_movements == 0 {
                    return self.get_data_requirements();
                }

                let target_exon = track.get_k_exons_before(middle, n_movements);
                if let Some(target_exon) = target_exon {
                    state_messages.push(StateMessage::GotoCoordinate(target_exon.end() - 1));
                } else {
                    // Query for the target exon
                    let exon = track_service
                        .query_k_exons_before(&contig, middle, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(exon.end() - 1));
                }
            }

            StateMessage::GotoPreviousExonsEnd(n_movements) => {
                if n_movements == 0 {
                    return self.get_data_requirements();
                }

                let target_exon = track.get_k_exons_before(middle, n_movements);
                if let Some(target_exon) = target_exon {
                    state_messages.push(StateMessage::GotoCoordinate(target_exon.end() - 1));
                } else {
                    // Query for the target exon
                    let exon = track_service
                        .query_k_exons_before(&contig, middle, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(exon.end() - 1));
                }
            }
            _ => {}
        }

        let mut data_messages = Vec::new();
        for state_message in state_messages {
            data_messages.extend(self.handle_movement_message(state_message)?);
        }

        Ok(data_messages)
    }
}

/// Absolute feature handling
impl State {
    async fn handle_goto_feature_message(
        &mut self,
        message: StateMessage,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut state_messages = Vec::new();

        if self.data.track_service.is_none() {
            return Err(TGVError::StateError(
                "Feature query service not initialized".to_string(),
            ));
        }
        let track_service = self.data.track_service.as_mut().unwrap();

        if let StateMessage::GoToGene(gene_id) = message {
            if let Some(reference) = self.settings.reference.as_ref() {
                track_service
                    .check_or_load_gene(reference, &gene_id)
                    .await?; // TODO: verbose on whether data was loaded
                let gene = track_service.query_gene_name(&gene_id).await?;
                state_messages.push(StateMessage::GotoContigCoordinate(
                    gene.contig().full_name(),
                    gene.start(),
                ));
            } else {
                return Err(TGVError::StateError(format!(
                    "No reference is provided. Cannot goto a gene {}",
                    gene_id
                )));
            }
        }

        let mut data_messages = Vec::new();
        for state_message in state_messages {
            data_messages.extend(self.handle_movement_message(state_message)?);
        }

        Ok(data_messages)
    }
}

/// Looking for the default region
impl State {
    const DEFAULT_GENE: &str = "KRAS";
    async fn handle_goto_default_message(&mut self) -> Result<Vec<DataMessage>, TGVError> {
        match (
            self.settings.reference.as_ref(),
            self.settings.bam_path.as_ref(),
        ) {
            (Some(_), Some(_)) | (None, Some(_)) => {
                self.handle_movement_message(StateMessage::GotoContigCoordinate(
                    self.data.contigs.first()?.full_name(),
                    1,
                ))
            }
            (Some(_), None) => {
                self.handle_goto_feature_message(StateMessage::GoToGene(
                    Self::DEFAULT_GENE.to_string(),
                ))
                .await
            }
            (None, None) => {
                Err(TGVError::StateError(
                    "Neither a reference nor a BAM file is provided. Cannot identify the default region.".to_string(),
                ))
            }
        }
    }
}
