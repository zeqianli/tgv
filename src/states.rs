use crate::models::{
    contig::Contig,
    message::{DataMessage, StateMessage},
    mode::InputMode,
    region::Region,
    register::{CommandModeRegister, NormalModeRegister},
    services::tracks::TrackService,
    track::{Feature, Track},
    window::ViewingWindow,
};
use crate::settings::Settings;
use crossterm::event::{KeyCode, KeyEvent};
use noodles_bam as bam;
use ratatui::layout::Rect;
use std::io;
use std::result::Result;

/// Genome region displayed in the window.
/// Holds states of the application.

pub struct State {
    /// Viewing window.
    pub viewing_window: ViewingWindow,

    pub input_mode: InputMode,

    pub exit: bool,

    pub debug_message: String,

    current_frame_area: Option<Rect>,

    // Handle feature movements
    feature_query_service: TrackService,
    exon_track_cache: Option<Track>,
    gene_track_cache: Option<Track>,

    // Registers
    normal_mode_register: NormalModeRegister,
    command_mode_register: CommandModeRegister,

    /// Contigs in the BAM header
    pub contigs: Vec<Contig>,

    /// Settings
    settings: Settings,
}

/// Basics
impl State {
    pub async fn new(
        initial_window: ViewingWindow,
        settings: Settings,
    ) -> Result<Self, sqlx::Error> {
        let contigs =
            load_contigs_from_bam(&settings.bam_path.clone().unwrap(), &settings.reference)
                .unwrap();

        Ok(Self {
            viewing_window: initial_window,
            input_mode: InputMode::Normal,
            exit: false,
            debug_message: String::new(),
            current_frame_area: None,
            feature_query_service: TrackService::new(settings.reference.clone()).await?,
            exon_track_cache: None,
            gene_track_cache: None,

            normal_mode_register: NormalModeRegister::new(),
            command_mode_register: CommandModeRegister::new(),

            contigs,
            settings,
        })
    }

    pub fn update_frame_area(&mut self, area: Rect) {
        self.current_frame_area = Some(area);
    }

    pub fn viewing_region(&self) -> Region {
        Region {
            contig: self.viewing_window.contig.clone(),
            start: self.viewing_window.left(),
            end: self
                .viewing_window
                .right(self.current_frame_area.as_ref().unwrap()),
        }
    }

    pub fn contig(&self) -> Contig {
        self.viewing_window.contig.clone()
    }

    /// Start coordinate of bases displayed on the screen.
    /// 1-based, inclusive.
    pub fn start(&self) -> usize {
        self.viewing_window.left()
    }

    /// End coordinate of bases displayed on the screen.
    /// 1-based, inclusive.
    pub fn end(&self) -> usize {
        self.viewing_window
            .right(self.current_frame_area.as_ref().unwrap())
    }

    /// Middle coordinate of bases displayed on the screen.
    /// 1-based, inclusive.
    pub fn middle(&self) -> usize {
        self.viewing_window
            .middle(self.current_frame_area.as_ref().unwrap())
    }

    /// Reference to the command mode register.
    pub fn command_mode_register(&self) -> &CommandModeRegister {
        &self.command_mode_register
    }
}

/// Load contigs from a BAM file header
fn load_contigs_from_bam(path: &str, reference: &str) -> io::Result<Vec<Contig>> {
    // Use the indexed_reader::Builder pattern as shown in alignment.rs
    let mut reader = bam::io::indexed_reader::Builder::default().build_from_path(path)?;
    let header = reader.read_header()?;

    // Extract contigs from the header
    let mut contigs = Vec::new();
    for (contig_name, _) in header.reference_sequences().iter() {
        if reference == "hg19" || reference == "hg38" {
            contigs.push(Contig::chrom(&contig_name.to_string()));
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unsupported reference",
            ));
        }
    }

    Ok(contigs)
}

// Message handling
impl State {
    // Translate key event to a message.
    pub fn translate_key_event(&self, key_event: KeyEvent) -> Vec<StateMessage> {
        match self.input_mode {
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
        }
    }

    pub async fn handle_message(&mut self, messages: Vec<StateMessage>) -> Vec<DataMessage> {
        let mut data_messages: Vec<DataMessage> = Vec::new();

        for message in messages {
            match message {

                // Swithching modes
                StateMessage::SwitchMode(mode) => {
                    self.input_mode = mode;

                    // match self.input_mode {
                    //     InputMode::Help => {
                    //         panic!("test");
                    //     }
                    //     _ => {}
                    // }
                }
                StateMessage::Quit => self.exit = true,

                // Command mode handling
                StateMessage::AddCharToCommandModeRegisters(c) => self.command_mode_register.add_char(c),
                StateMessage::CommandModeRegisterError(error_message) => self.debug_message = error_message,
                StateMessage::ClearCommandModeRegisters => self.command_mode_register.clear(),
                StateMessage::BackspaceCommandModeRegisters => self.command_mode_register.backspace(),
                StateMessage::MoveCursorLeft(amount) => self.command_mode_register.move_cursor_left(amount),
                StateMessage::MoveCursorRight(amount) => self.command_mode_register.move_cursor_right(amount),

                // Normal mode handling
                StateMessage::AddCharToNormalModeRegisters(c) => self.normal_mode_register.add_char(c),
                StateMessage::NormalModeRegisterError(error_message) => self.debug_message = error_message,
                StateMessage::ClearNormalModeRegisters => self.normal_mode_register.clear(),

                // Movement handling
                StateMessage::MoveLeft(_) |
                StateMessage::MoveRight(_) |
                StateMessage::MoveUp(_) |
                StateMessage::MoveDown(_) |
                StateMessage::GotoCoordinate(_) |
                StateMessage::GotoContigCoordinate(_, _) => {
                    data_messages.extend(self.handle_movement_message(message));
                },

                // Zoom handling
                StateMessage::ZoomOut(r) => data_messages.extend(self.handle_zoom_out(r)),
                StateMessage::ZoomIn(r) => data_messages.extend(self.handle_zoom_in(r)),

                // Relative feature movement handling
                StateMessage::GotoNextExonsStart(_) |
                StateMessage::GotoNextExonsEnd(_) |
                StateMessage::GotoPreviousExonsStart(_) |
                StateMessage::GotoPreviousExonsEnd(_) | // TODO: this is broken
                StateMessage::GotoNextGenesStart(_) |
                StateMessage::GotoNextGenesEnd(_) |
                StateMessage::GotoPreviousGenesStart(_) |
                StateMessage::GotoPreviousGenesEnd(_) => { // TODO: this is broken
                    data_messages.extend(self.handle_feature_movement_message(message).await);
                },

                // Absolute feature handling
                StateMessage::GoToGene(_) => {
                    data_messages.extend(self.handle_goto_feature_message(message).await);
                },

                // Others
                _ => {}
            }
        }
        data_messages
    }

    const MAX_ZOOM_TO_DISPLAY_FEATURES: usize = 64;
    const MAX_ZOOM_TO_DISPLAY_ALIGNMENTS: usize = 32;

    fn get_data_requirements(&self) -> Vec<DataMessage> {
        let mut data_messages = Vec::new();

        if self.viewing_window.zoom() <= Self::MAX_ZOOM_TO_DISPLAY_FEATURES {
            data_messages.push(DataMessage::RequiresCompleteFeatures(self.viewing_region()));
        }

        if self.viewing_window.zoom() <= Self::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS {
            data_messages.push(DataMessage::RequiresCompleteAlignments(
                self.viewing_region(),
            ));
        }

        if self.viewing_window.is_basewise() {
            data_messages.push(DataMessage::RequiresCompleteSequences(
                self.viewing_region(),
            ));
        }

        data_messages
    }
}

// Movement handling
impl State {
    fn handle_movement_message(&mut self, message: StateMessage) -> Vec<DataMessage> {
        let mut data_messages = Vec::new();

        match message {
            // TODO: bound handling
            StateMessage::MoveLeft(n) => {
                self.viewing_window.set_left(
                    self.viewing_window
                        .left()
                        .saturating_sub(n * self.viewing_window.zoom()),
                );
            }
            StateMessage::MoveRight(n) => {
                self.viewing_window.set_left(
                    self.viewing_window
                        .left()
                        .saturating_add(n * self.viewing_window.zoom()),
                );
            }
            StateMessage::MoveUp(n) => {
                self.viewing_window
                    .set_top(self.viewing_window.top().saturating_sub(n));
            }
            StateMessage::MoveDown(n) => {
                self.viewing_window
                    .set_top(self.viewing_window.top().saturating_add(n));
            }
            StateMessage::GotoCoordinate(n) => {
                self.viewing_window
                    .set_middle(self.current_frame_area.as_ref().unwrap(), n);
                self.viewing_window.set_top(0);
            }
            StateMessage::GotoContigCoordinate(contig, n) => {
                self.viewing_window.contig = contig;
                self.viewing_window
                    .set_middle(self.current_frame_area.as_ref().unwrap(), n);
                self.viewing_window.set_top(0);
            }

            _ => {}
        }

        data_messages.extend(self.get_data_requirements());
        data_messages
    }
}

/// Zoom handling
impl State {
    fn handle_zoom_out(&mut self, r: usize) -> Vec<DataMessage> {
        self.viewing_window
            .zoom_out(r, self.current_frame_area.as_ref().unwrap())
            .unwrap();
        self.get_data_requirements()
    }

    fn handle_zoom_in(&mut self, r: usize) -> Vec<DataMessage> {
        self.viewing_window
            .zoom_in(r, self.current_frame_area.as_ref().unwrap())
            .unwrap();
        self.get_data_requirements()
    }
}

/// Feature movement handling
impl State {
    const DEFAULT_CACHE_N_GENES: usize = 5;

    async fn get_exon_and_gene_cache(
        &self,
        contig: &Contig,
        position: usize,
        n_genes: usize,
    ) -> Result<(Track, Track), String> {
        let this_gene = self
            .feature_query_service
            .query_gene_covering(contig, position)
            .await;
        let next_genes = self
            .feature_query_service
            .query_genes_after(contig, position, n_genes)
            .await;
        let previous_genes = self
            .feature_query_service
            .query_genes_before(contig, position, n_genes)
            .await;

        let all_genes: Vec<Feature> = match (this_gene, next_genes, previous_genes) {
            (Ok(this_gene), Ok(next_genes), Ok(previous_genes)) => {
                let mut all_genes = Vec::new();
                if let Some(this_gene) = this_gene {
                    all_genes.push(this_gene);
                }
                all_genes.extend(next_genes);
                all_genes.extend(previous_genes);

                all_genes
            }
            _ => return Err("Failed to get exon and gene cache".to_string()),
        };

        let mut exons = Vec::new();

        for gene in all_genes.iter() {
            exons.extend(gene.exons().unwrap());
        }

        let gene_track = Track::from(all_genes, contig.clone()).unwrap();
        let exon_track = Track::from(exons, contig.clone()).unwrap();

        Ok((exon_track, gene_track))
    }

    async fn handle_feature_movement_message(&mut self, message: StateMessage) -> Vec<DataMessage> {
        let mut state_messages = Vec::new();

        match message {
            StateMessage::GotoNextExonsStart(n_movements)
            | StateMessage::GotoNextExonsEnd(n_movements)
            | StateMessage::GotoPreviousExonsStart(n_movements)
            | StateMessage::GotoPreviousExonsEnd(n_movements) => {
                let n_query = usize::max(n_movements, State::DEFAULT_CACHE_N_GENES);
                let mut need_cache_update = true;
                let contig = self.contig();
                let position = self.middle();

                need_cache_update = self.exon_track_cache.is_none()
                    || self.exon_track_cache.as_ref().unwrap().contig != contig
                    || self
                        .exon_track_cache
                        .as_ref()
                        .unwrap()
                        .get_k_features_after(position, n_query)
                        .is_none()
                    || self
                        .exon_track_cache
                        .as_ref()
                        .unwrap()
                        .get_k_features_before(position, n_query)
                        .is_none();
                if need_cache_update {
                    let (exon_track, gene_track) = self
                        .get_exon_and_gene_cache(&contig, position, n_movements)
                        .await
                        .unwrap();
                    self.exon_track_cache = Some(exon_track);
                    self.gene_track_cache = Some(gene_track);
                } // TODO: this is assuming that n_query genes must have at least n_query exons. Mgiht be false sometimes?
            }

            StateMessage::GotoNextGenesStart(n_movements)
            | StateMessage::GotoNextGenesEnd(n_movements)
            | StateMessage::GotoPreviousGenesStart(n_movements)
            | StateMessage::GotoPreviousGenesEnd(n_movements) => {
                let n_query = usize::max(n_movements, State::DEFAULT_CACHE_N_GENES);
                let mut need_cache_update = true;
                let contig = self.contig();
                let position = self.middle();

                need_cache_update = self.gene_track_cache.is_none()
                    || self.gene_track_cache.as_ref().unwrap().contig != contig
                    || self
                        .gene_track_cache
                        .as_ref()
                        .unwrap()
                        .get_k_features_after(position, n_query)
                        .is_none()
                    || self
                        .gene_track_cache
                        .as_ref()
                        .unwrap()
                        .get_k_features_before(position, n_query + 1)
                        .is_none();
                if need_cache_update {
                    let (exon_track, gene_track) = self
                        .get_exon_and_gene_cache(&contig, position, n_movements)
                        .await
                        .unwrap();
                    self.exon_track_cache = Some(exon_track);
                    self.gene_track_cache = Some(gene_track);
                }
            }

            _ => {}
        };

        let position = self.middle();

        match message {
            StateMessage::GotoNextExonsStart(n_movements) => {
                match self
                    .exon_track_cache
                    .as_ref()
                    .unwrap()
                    .get_saturating_k_features_after(position, n_movements)
                {
                    Some((_, feature)) => {
                        state_messages.push(StateMessage::GotoCoordinate(feature.start()));
                    }
                    _ => {
                        state_messages.push(StateMessage::NormalModeRegisterError(
                            "Feature parsing error".to_string(),
                        ));
                    }
                }
            }

            StateMessage::GotoNextExonsEnd(n_movements) => {
                match self
                    .exon_track_cache
                    .as_ref()
                    .unwrap()
                    .get_saturating_k_features_after(position, n_movements)
                {
                    Some((_, feature)) => {
                        state_messages.push(StateMessage::GotoCoordinate(feature.end()));
                    }
                    _ => {
                        state_messages.push(StateMessage::NormalModeRegisterError(
                            "Feature parsing error".to_string(),
                        ));
                    }
                }
            }

            StateMessage::GotoPreviousExonsStart(n_movements) => {
                match self
                    .exon_track_cache
                    .as_ref()
                    .unwrap()
                    .get_saturating_k_features_before(position, n_movements)
                {
                    Some((_, feature)) => {
                        state_messages.push(StateMessage::GotoCoordinate(feature.start()));
                    }
                    _ => {
                        state_messages.push(StateMessage::NormalModeRegisterError(
                            "Feature parsing error".to_string(),
                        ));
                    }
                }
            }

            StateMessage::GotoPreviousExonsEnd(n_movements) => {
                match self
                    .exon_track_cache
                    .as_ref()
                    .unwrap()
                    .get_saturating_k_features_before(position, n_movements)
                {
                    Some((_, feature)) => {
                        state_messages.push(StateMessage::GotoCoordinate(feature.end()));
                    }
                    _ => {
                        state_messages.push(StateMessage::NormalModeRegisterError(
                            "Feature parsing error".to_string(),
                        ));
                    }
                }
            }

            StateMessage::GotoNextGenesStart(n_movements) => {
                match self
                    .gene_track_cache
                    .as_ref()
                    .unwrap()
                    .get_saturating_k_features_after(position, n_movements)
                {
                    Some((_, feature)) => {
                        state_messages.push(StateMessage::GotoCoordinate(feature.start()));
                    }
                    _ => {
                        state_messages.push(StateMessage::NormalModeRegisterError(
                            "Feature parsing error".to_string(),
                        ));
                    }
                }
            }

            StateMessage::GotoNextGenesEnd(n_movements) => {
                match self
                    .gene_track_cache
                    .as_ref()
                    .unwrap()
                    .get_saturating_k_features_after(position, n_movements)
                {
                    Some((_, feature)) => {
                        state_messages.push(StateMessage::GotoCoordinate(feature.end()));
                    }
                    _ => {
                        state_messages.push(StateMessage::NormalModeRegisterError(
                            "Feature parsing error".to_string(),
                        ));
                    }
                }
            }

            StateMessage::GotoPreviousGenesStart(n_movements) => {
                match self
                    .gene_track_cache
                    .as_ref()
                    .unwrap()
                    .get_saturating_k_features_before(position, n_movements)
                {
                    Some((_, feature)) => {
                        state_messages.push(StateMessage::GotoCoordinate(feature.start()));
                    }
                    _ => {
                        state_messages.push(StateMessage::NormalModeRegisterError(
                            "Feature parsing error".to_string(),
                        ));
                    }
                }
            }

            StateMessage::GotoPreviousGenesEnd(n_movements) => {
                match self
                    .gene_track_cache
                    .as_ref()
                    .unwrap()
                    .get_saturating_k_features_before(position, n_movements + 1)
                {
                    // TODO: fix this.
                    Some((_, feature)) => {
                        state_messages.push(StateMessage::GotoCoordinate(feature.end() - 1));
                    }
                    _ => {
                        state_messages.push(StateMessage::NormalModeRegisterError(
                            "Feature parsing error".to_string(),
                        ));
                    }
                }
            }

            _ => {}
        }

        let mut data_messages = Vec::new();
        for state_message in state_messages {
            data_messages.extend(self.handle_movement_message(state_message));
        }

        data_messages
    }
}

/// Absolute feature handling
impl State {
    async fn handle_goto_feature_message(&mut self, message: StateMessage) -> Vec<DataMessage> {
        let mut state_messages = Vec::new();

        if let StateMessage::GoToGene(gene_id) = message {
            let query_result: Result<Feature, sqlx::Error> =
                self.feature_query_service.query_gene_name(&gene_id).await;
            match query_result {
                Ok(gene) => {
                    state_messages.push(StateMessage::GotoContigCoordinate(
                        gene.contig(),
                        gene.start(),
                    ));
                }
                _ => {
                    state_messages.push(StateMessage::NormalModeRegisterError(
                        "Feature parsing error".to_string(),
                    ));
                }
            }
        }

        let mut data_messages = Vec::new();
        for state_message in state_messages {
            data_messages.extend(self.handle_movement_message(state_message));
        }

        data_messages
    }
}
