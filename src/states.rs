use crate::error::TGVError;
use crate::helpers::is_url;
use crate::models::{
    contig::Contig,
    message::{DataMessage, StateMessage},
    mode::InputMode,
    reference::Reference,
    region::Region,
    register::{CommandModeRegister, NormalModeRegister},
    services::tracks::TrackService,
    track::{Feature, Track},
    window::ViewingWindow,
};
use crate::settings::Settings;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use rust_htslib::bam::{self, record::Cigar, Header, IndexedReader, Read, Record};
use std::collections::HashMap;
use url::Url;
/// A collection of contigs. This helps relative contig movements.
struct ContigCollection {
    contigs: Vec<Contig>,

    contig_index: HashMap<String, usize>,
}

impl ContigCollection {
    pub fn new(contigs: Vec<Contig>) -> Result<Self, TGVError> {
        // check that contigs do not have duplicated full names
        let mut contig_index = HashMap::new();
        for (i, contig) in contigs.iter().enumerate() {
            if contig_index.contains_key(&contig.full_name()) {
                return Err(TGVError::StateError(format!(
                    "Duplicate contig names {}. Is your BAM file header correct?",
                    contig.full_name()
                )));
            }
            contig_index.insert(contig.full_name(), i);
        }

        Ok(Self {
            contigs,
            contig_index,
        })
    }

    pub fn first(&self) -> Result<&Contig, TGVError> {
        Ok(&self.contigs[0])
    }

    pub fn last(&self) -> Result<&Contig, TGVError> {
        Ok(&self.contigs[self.contigs.len() - 1])
    }

    pub fn from_bam(
        path: &String,
        bai_path: Option<&String>,
        reference: Option<&Reference>,
    ) -> Result<Self, TGVError> {
        // Use the indexed_reader::Builder pattern as shown in alignment.rs
        let is_remote_path = is_url(path);
        let bam = match bai_path {
            Some(bai_path) => {
                if is_remote_path {
                    return Err(TGVError::IOError(
                        "Custom .bai path for remote BAM files are not supported yet.".to_string(),
                    ));
                }
                IndexedReader::from_path_and_index(path, bai_path)
                    .map_err(|e| TGVError::IOError(e.to_string()))?
            }
            None => {
                if is_remote_path {
                    IndexedReader::from_url(
                        &Url::parse(path).map_err(|e| TGVError::IOError(e.to_string()))?,
                    )
                    .unwrap()
                } else {
                    IndexedReader::from_path(path).map_err(|e| TGVError::IOError(e.to_string()))?
                }
            }
        };

        let header = bam::Header::from_template(bam.header());

        let mut contigs = Vec::new();
        for (key, records) in header.to_hashmap().iter() {
            for record in records {
                if record.contains_key("SN") {
                    let contig_name = record["SN"].to_string();
                    match reference {
                        // If the reference is human, interpret contig names as chromosomes. This allows abbreviated matching (chr1 <-> 1).
                        Some(Reference::Hg19) => contigs.push(Contig::chrom(&contig_name)),
                        Some(Reference::Hg38) => contigs.push(Contig::chrom(&contig_name)),

                        // Otherwise, interpret contig names as contigs. This does not allow abbreviated matching.
                        _ => contigs.push(Contig::contig(&contig_name)),
                    }
                }
            }
        }

        Self::new(contigs)
    }

    pub fn contains(&self, contig: &Contig) -> bool {
        self.contig_index.contains_key(&contig.full_name())
    }

    pub fn next(&self, contig: &Contig, k: usize) -> Result<Contig, TGVError> {
        let index = self.contig_index[&contig.full_name()];
        let next_index = (index + k) % self.contigs.len();
        Ok(self.contigs[next_index].clone())
    }

    pub fn previous(&self, contig: &Contig, k: usize) -> Result<Contig, TGVError> {
        let index = self.contig_index[&contig.full_name()];
        let previous_index =
            (index + self.contigs.len() - k % self.contigs.len()) % self.contigs.len();
        Ok(self.contigs[previous_index].clone())
    }
}

/// Holds states of the application.
pub struct State {
    /// Viewing window.
    window: Option<ViewingWindow>,
    area: Option<Rect>,

    pub input_mode: InputMode,

    pub exit: bool,

    pub debug_message: String,

    // Handle feature movements
    feature_query_service: Option<TrackService>,
    exon_track_cache: Option<Track>,
    gene_track_cache: Option<Track>,

    // Registers
    normal_mode_register: NormalModeRegister,
    command_mode_register: CommandModeRegister,

    /// Contigs in the BAM header
    contigs: Option<ContigCollection>,

    /// Settings
    pub settings: Settings,

    /// Error messages for display.
    pub errors: Vec<String>,
}

/// Basics
impl State {
    pub async fn new(settings: Settings) -> Result<Self, TGVError> {
        let contigs = match settings.bam_path.clone() {
            Some(bam_path) => Some(ContigCollection::from_bam(
                &bam_path,
                settings.bai_path.as_ref(),
                settings.reference.as_ref(),
            )?),
            None => None,
        };

        Ok(Self {
            window: None,
            input_mode: InputMode::Normal,
            exit: false,
            debug_message: String::new(),
            area: None,
            feature_query_service: match settings.reference.as_ref() {
                Some(reference) => match TrackService::new(reference.clone()).await {
                    Ok(service) => Some(service),
                    Err(_) => {
                        return Err(TGVError::IOError(format!(
                            "Failed to create track service for reference {}",
                            reference
                        )));
                    }
                },
                None => None,
            },
            exon_track_cache: None,
            gene_track_cache: None,

            normal_mode_register: NormalModeRegister::new(),
            command_mode_register: CommandModeRegister::new(),

            contigs,
            settings,

            errors: Vec::new(),
        })
    }

    pub fn update_frame_area(&mut self, area: Rect) {
        self.area = Some(area);
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

    pub fn add_error_message(&mut self, error: TGVError) {
        self.errors.push(format!("{}", error));
    }

    pub async fn close(&mut self) -> Result<(), TGVError> {
        if self.feature_query_service.is_some() {
            self.feature_query_service.as_ref().unwrap().close().await?;
        }
        Ok(())
    }
}

// Message handling
impl State {
    // Translate key event to a message.
    pub fn translate_key_event(&self, key_event: KeyEvent) -> Vec<StateMessage> {
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
                return vec![StateMessage::Error(TGVError::StateError(
                    "Reference is not provided".to_string(),
                ))];
            }
        }

        messages
    }

    /// Handle state messages.
    pub async fn handle_messages(
        &mut self,
        messages: Vec<StateMessage>,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages: Vec<DataMessage> = Vec::new();

        for message in messages {
            data_messages.extend(self.handle_message(message).await?);
        }

        Ok(data_messages)
    }

    /// Main function to route state message handling.
    pub async fn handle_message(
        &mut self,
        message: StateMessage,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages: Vec<DataMessage> = Vec::new();

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
            StateMessage::CommandModeRegisterError(error_message) => self.add_error_message(TGVError::ParsingError(error_message)),
            StateMessage::ClearCommandModeRegisters => self.command_mode_register.clear(),
            StateMessage::BackspaceCommandModeRegisters => self.command_mode_register.backspace(),
            StateMessage::MoveCursorLeft(amount) => self.command_mode_register.move_cursor_left(amount),
            StateMessage::MoveCursorRight(amount) => self.command_mode_register.move_cursor_right(amount),

            // Normal mode handling
            StateMessage::AddCharToNormalModeRegisters(c) => self.normal_mode_register.add_char(c),
            StateMessage::NormalModeRegisterError(error_message) => self.add_error_message(TGVError::ParsingError(error_message)),
            StateMessage::ClearNormalModeRegisters => self.normal_mode_register.clear(),

            // Movement handling
            StateMessage::MoveLeft(_) |
            StateMessage::MoveRight(_) |
            StateMessage::MoveUp(_) |
            StateMessage::MoveDown(_) |
            StateMessage::GotoCoordinate(_) |
            StateMessage::GotoContigCoordinate(_, _) => {
                data_messages.extend(self.handle_movement_message(message)?);
            },

            // Zoom handling
            StateMessage::ZoomOut(r) => data_messages.extend(self.handle_zoom_out(r)?),
            StateMessage::ZoomIn(r) => data_messages.extend(self.handle_zoom_in(r)?),

            // Relative feature movement handling
            StateMessage::GotoNextExonsStart(_) |
            StateMessage::GotoNextExonsEnd(_) |
            StateMessage::GotoPreviousExonsStart(_) |
            StateMessage::GotoPreviousExonsEnd(_) | // TODO: this is broken
            StateMessage::GotoNextGenesStart(_) |
            StateMessage::GotoNextGenesEnd(_) |
            StateMessage::GotoPreviousGenesStart(_) |
            StateMessage::GotoPreviousGenesEnd(_) => { // TODO: this is broken
                data_messages.extend(self.handle_feature_movement_message(message).await?);
            },

            // Absolute feature handling
            StateMessage::GoToGene(_) => {
                data_messages.extend(self.handle_goto_feature_message(message).await?);
            },

            // Find the default region
            StateMessage::GoToDefault => {
                data_messages.extend(self.handle_goto_default_message().await?);
            },

            // Error messages
            StateMessage::Error(e) => self.add_error_message(e),

            // Others
            _ => {}
        }

        Ok(data_messages)
    }

    const MAX_ZOOM_TO_DISPLAY_FEATURES: usize = 64;
    const MAX_ZOOM_TO_DISPLAY_ALIGNMENTS: usize = 32;
    const MAX_ZOOM_TO_DISPLAY_SEQUENCES: usize = 2;
    fn get_data_requirements(&self) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages = Vec::new();

        let viewing_window = self.viewing_window()?;
        let viewing_region = self.viewing_region()?;

        if self.settings.bam_path.is_some()
            && viewing_window.zoom() <= Self::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS
        {
            data_messages.push(DataMessage::RequiresCompleteAlignments(
                viewing_region.clone(),
            ));
        }

        if self.settings.reference.is_some() {
            if viewing_window.zoom() <= Self::MAX_ZOOM_TO_DISPLAY_FEATURES {
                data_messages.push(DataMessage::RequiresCompleteFeatures(
                    viewing_region.clone(),
                ));
            }

            if viewing_window.zoom() <= Self::MAX_ZOOM_TO_DISPLAY_SEQUENCES {
                data_messages.push(DataMessage::RequiresCompleteSequences(
                    viewing_region.clone(),
                ));
            }
        }

        Ok(data_messages)
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
                let viewing_window = self.viewing_window_mut()?;

                viewing_window.set_left(
                    viewing_window
                        .left()
                        .saturating_sub(n * viewing_window.zoom()),
                );
            }
            StateMessage::MoveRight(n) => {
                let viewing_window = self.viewing_window_mut()?;

                viewing_window.set_left(
                    viewing_window
                        .left()
                        .saturating_add(n * viewing_window.zoom()),
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
                let current_frame_area: Rect = *self.current_frame_area()?;
                let viewing_window = self.viewing_window_mut()?;

                viewing_window.set_middle(&current_frame_area, n);
            }
            StateMessage::GotoContigCoordinate(contig, n) => {
                // If bam_path is provided, check that the contig is valid.
                let contig = match self.settings.reference {
                    Some(Reference::Hg38) | Some(Reference::Hg19) => Contig::chrom(&contig),
                    _ => Contig::contig(&contig),
                };
                if let Some(contigs) = &self.contigs {
                    if !contigs.contains(&contig) {
                        self.add_error_message(TGVError::StateError(format!(
                            "Contig {} not found",
                            contig.full_name()
                        )));
                        return Ok(vec![]);
                    }
                }

                let current_frame_area: Rect = *self.current_frame_area()?;

                match self.window {
                    Some(ref mut window) => {
                        window.contig = contig;
                        window.set_middle(&current_frame_area, n);
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
        let current_frame_area = *self.current_frame_area()?;
        let viewing_window = self.viewing_window_mut()?;

        viewing_window.zoom_out(r, &current_frame_area).unwrap();
        self.get_data_requirements()
    }

    fn handle_zoom_in(&mut self, r: usize) -> Result<Vec<DataMessage>, TGVError> {
        let current_frame_area: Rect = *self.current_frame_area()?;
        let viewing_window = self.viewing_window_mut()?;

        viewing_window.zoom_in(r, &current_frame_area).unwrap();
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
        if self.feature_query_service.is_none() {
            return Err("Feature query service not initialized".to_string());
        }
        let feature_query_service = self.feature_query_service.as_ref().unwrap();

        let this_gene = feature_query_service
            .query_gene_covering(contig, position)
            .await;
        let next_genes = feature_query_service
            .query_genes_after(contig, position, n_genes)
            .await;
        let previous_genes = feature_query_service
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

    async fn handle_feature_movement_message(
        &mut self,
        message: StateMessage,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut state_messages = Vec::new();

        match message {
            StateMessage::GotoNextExonsStart(n_movements)
            | StateMessage::GotoNextExonsEnd(n_movements)
            | StateMessage::GotoPreviousExonsStart(n_movements)
            | StateMessage::GotoPreviousExonsEnd(n_movements) => {
                let n_query = usize::max(n_movements, State::DEFAULT_CACHE_N_GENES);
                let mut need_cache_update = true;
                let contig = self.contig()?;
                let position = self.middle()?;

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
                let contig = self.contig()?;
                let position = self.middle()?;

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

        let position = self.middle()?;

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

        if self.feature_query_service.is_none() {
            return Err(TGVError::StateError(
                "Feature query service not initialized".to_string(),
            ));
        }
        let feature_query_service = self.feature_query_service.as_ref().unwrap();

        if let StateMessage::GoToGene(gene_id) = message {
            let query_result: Result<Feature, sqlx::Error> =
                feature_query_service.query_gene_name(&gene_id).await;
            match query_result {
                Ok(gene) => {
                    state_messages.push(StateMessage::GotoContigCoordinate(
                        gene.contig().full_name(),
                        gene.start(),
                    ));
                }
                _ => {
                    self.add_error_message(TGVError::IOError(format!(
                        "Failed to query gene {}",
                        gene_id
                    )));
                }
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
                    self.contigs.as_ref().unwrap().first()?.full_name(),
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
