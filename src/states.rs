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
    services::tracks::TrackService,
    track::{Feature, Gene, Track},
    window::ViewingWindow,
};
use crate::settings::Settings;
use crate::traits::GenomeInterval;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use rust_htslib::bam::{self, record::Cigar, Header, IndexedReader, Read, Record};
use std::collections::HashMap;
use url::Url;
/// A collection of contigs. This helps relative contig movements.
struct ContigCollection {
    contigs: Vec<Contig>,
    contig_lengths: Vec<Option<usize>>,

    contig_index: HashMap<String, usize>,
}

impl ContigCollection {
    pub fn new(contigs: Vec<Contig>, contig_lengths: Vec<Option<usize>>) -> Result<Self, TGVError> {
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
            contig_lengths,
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
        let mut contig_lengths: Vec<Option<usize>> = Vec::new();
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
                if record.contains_key("LN") {
                    contig_lengths.push(record["LN"].to_string().parse::<usize>().ok());
                } else {
                    contig_lengths.push(None);
                }
            }
        }

        Self::new(contigs, contig_lengths)
    }

    pub fn contains(&self, contig: &Contig) -> bool {
        self.contig_index.contains_key(&contig.full_name())
    }

    pub fn length(&self, contig: &Contig) -> Option<usize> {
        let index = self.contig_index.get(&contig.full_name())?;
        self.contig_lengths[*index]
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
    /// Basics
    pub input_mode: InputMode,
    pub exit: bool,

    /// Viewing window.
    window: Option<ViewingWindow>,
    area: Option<Rect>,

    // Data
    pub data: Data,

    /// Contigs in the BAM header
    contigs: Option<ContigCollection>,

    // Registers
    normal_mode_register: NormalModeRegister,
    command_mode_register: CommandModeRegister,

    /// Settings
    pub settings: Settings,

    /// Error messages for display.
    pub errors: Vec<String>,

    /// Cytobands
    cytobands: Option<Vec<Cytoband>>,
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

        let cytobands = match settings.reference.as_ref() {
            Some(reference) => Some(Cytoband::from_reference(reference)?),
            None => None,
        };

        let data = Data::new(&settings).await?;

        Ok(Self {
            window: None,
            input_mode: InputMode::Normal,
            exit: false,
            area: None,
            data,

            normal_mode_register: NormalModeRegister::new(),
            command_mode_register: CommandModeRegister::new(),

            contigs,
            settings,
            cytobands,
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
        self.data.close().await?;
        Ok(())
    }

    /// TODO: this is inefficient.
    pub fn current_cytoband_index(&self) -> Result<Option<usize>, TGVError> {
        for (i, cytoband) in self.cytobands.as_ref().unwrap().iter().enumerate() {
            if cytoband.contig == self.contig()? {
                return Ok(Some(i));
            }
        }
        Ok(None)
    }

    pub fn cytobands(&self) -> Option<&Vec<Cytoband>> {
        self.cytobands.as_ref()
    }
}

impl State {
    pub async fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<(), TGVError> {
        let messages = self.translate_key_event(key_event);
        let data_messages = self.handle_state_messages(messages).await?;
        let loaded_data = self.data.handle_data_messages(data_messages).await?;

        if loaded_data {
            self.errors.push("Data loaded".to_string());
        }
        Ok(())
    }

    pub async fn handle(&mut self, messages: Vec<StateMessage>) -> Result<(), TGVError> {
        let data_messages = self.handle_state_messages(messages).await?;
        let loaded_data = self.data.handle_data_messages(data_messages).await?;

        if loaded_data {
            self.errors.push("Data loaded".to_string());
        }
        Ok(())
    }
}

// State message handling
impl State {
    // Translate key event to a message.
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
                return vec![StateMessage::Error(TGVError::StateError(
                    "Reference is not provided".to_string(),
                ))];
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

                // match self.input_mode {
                //     InputMode::Help => {
                //         panic!("test");
                //     }
                //     _ => {}
                // }
            }
            StateMessage::Quit => self.exit = true,

            // Command mode handling
            StateMessage::AddCharToCommandModeRegisters(c) => {
                self.command_mode_register.add_char(c)
            }
            StateMessage::CommandModeRegisterError(error_message) => {
                self.add_error_message(TGVError::ParsingError(error_message))
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
                self.add_error_message(TGVError::ParsingError(error_message))
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
                data_messages.extend(self.handle_exon_movement_message(message).await?);
            }
            StateMessage::GotoNextGenesStart(_)
            | StateMessage::GotoNextGenesEnd(_)
            | StateMessage::GotoPreviousGenesStart(_)
            | StateMessage::GotoPreviousGenesEnd(_) => {
                data_messages.extend(self.handle_gene_movement_message(message).await?);
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
    const MAX_ZOOM_TO_DISPLAY_FEATURES: usize = usize::MAX;
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
                let current_frame_area = self.current_frame_area()?.clone();
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
                let current_frame_area = self.current_frame_area()?.clone();

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
                let current_frame_area = self.current_frame_area()?.clone();
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
                if let Some(contigs) = &self.contigs {
                    if !contigs.contains(&contig) {
                        self.add_error_message(TGVError::StateError(format!(
                            "Contig {} not found",
                            contig.full_name()
                        )));
                        return Ok(vec![]);
                    }
                }

                let current_frame_area = self.current_frame_area()?.clone();

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

        // 1. If can be found in the BAM header use the BAM header
        if let Some(contigs) = &self.contigs {
            if let Some(length) = contigs.length(&contig) {
                return Ok(Some(length));
            }
        }

        // 2. If the reference genome, used length in the database.
        if let Some(reference) = self.settings.reference.as_ref() {
            if let Some(length) = reference.length(&contig) {
                return Ok(Some(length));
            }
        }

        Ok(None)
    }
}

/// Feature movement handling
impl State {
    async fn handle_gene_movement_message(
        &mut self,
        message: StateMessage,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut state_messages = Vec::new();

        let track = match self.data.track.as_ref() {
            Some(track) => track,
            None => return Err(TGVError::StateError("Track not initialized".to_string())),
        };

        match message {
            StateMessage::GotoNextGenesStart(n_movements) => {
                if n_movements == 0 {
                    return Ok(self.get_data_requirements()?);
                }

                let target_gene = track.get_k_genes_after(self.middle()?, n_movements);
                if let Some(target_gene) = target_gene {
                    state_messages.push(StateMessage::GotoCoordinate(target_gene.start() + 1));
                } else {
                    // Query for the target gene
                    let track_service = self.data.track_service.as_ref().unwrap();
                    let gene = track_service
                        .query_k_genes_after(&self.contig()?, self.middle()?, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(gene.start() + 1));
                }
            }
            StateMessage::GotoNextGenesEnd(n_movements) => {
                if n_movements == 0 {
                    return Ok(self.get_data_requirements()?);
                }

                let target_gene = track.get_k_genes_after(self.middle()?, n_movements);
                if let Some(target_gene) = target_gene {
                    state_messages.push(StateMessage::GotoCoordinate(target_gene.start() + 1));
                } else {
                    // Query for the target gene
                    let track_service = self.data.track_service.as_ref().unwrap();
                    let gene = track_service
                        .query_k_genes_after(&self.contig()?, self.middle()?, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(gene.start() + 1));
                }
            }
            StateMessage::GotoPreviousGenesEnd(n_movements) => {
                if n_movements == 0 {
                    return Ok(self.get_data_requirements()?);
                }

                let target_gene = track.get_k_genes_before(self.middle()?, n_movements);
                if let Some(target_gene) = target_gene {
                    state_messages.push(StateMessage::GotoCoordinate(target_gene.start() + 1));
                } else {
                    // Query for the target gene
                    let track_service = self.data.track_service.as_ref().unwrap();
                    let gene = track_service
                        .query_k_genes_before(&self.contig()?, self.middle()?, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(gene.start() + 1));
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

    async fn handle_exon_movement_message(
        &mut self,
        message: StateMessage,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut state_messages = Vec::new();

        let track = match self.data.track.as_ref() {
            Some(track) => track,
            None => return Err(TGVError::StateError("Track not initialized".to_string())),
        };

        match message {
            StateMessage::GotoNextExonsStart(n_movements) => {
                if n_movements == 0 {
                    return Ok(self.get_data_requirements()?);
                }

                let target_exon = track.get_k_exons_after(self.middle()?, n_movements);
                if let Some(target_exon) = target_exon {
                    state_messages.push(StateMessage::GotoCoordinate(target_exon.start() + 1));
                } else {
                    // Query for the target exon
                    let track_service = self.data.track_service.as_ref().unwrap();
                    let exon = track_service
                        .query_k_exons_after(&self.contig()?, self.middle()?, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(exon.start() + 1));
                }
            }
            StateMessage::GotoNextExonsEnd(n_movements) => {
                if n_movements == 0 {
                    return Ok(self.get_data_requirements()?);
                }

                let target_exon = track.get_k_exons_after(self.middle()?, n_movements);
                if let Some(target_exon) = target_exon {
                    state_messages.push(StateMessage::GotoCoordinate(target_exon.start() + 1));
                } else {
                    // Query for the target exon
                    let track_service = self.data.track_service.as_ref().unwrap();
                    let exon = track_service
                        .query_k_exons_after(&self.contig()?, self.middle()?, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(exon.start() + 1));
                }
            }
            StateMessage::GotoPreviousExonsStart(n_movements) => {
                if n_movements == 0 {
                    return Ok(self.get_data_requirements()?);
                }

                let target_exon = track.get_k_exons_before(self.middle()?, n_movements);
                if let Some(target_exon) = target_exon {
                    state_messages.push(StateMessage::GotoCoordinate(target_exon.start() + 1));
                } else {
                    // Query for the target exon
                    let track_service = self.data.track_service.as_ref().unwrap();
                    let exon = track_service
                        .query_k_exons_before(&self.contig()?, self.middle()?, n_movements)
                        .await?;

                    state_messages.push(StateMessage::GotoCoordinate(exon.start() + 1));
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
        let track_service = self.data.track_service.as_ref().unwrap();

        if let StateMessage::GoToGene(gene_id) = message {
            let gene = track_service.query_gene_name(&gene_id).await?;
            state_messages.push(StateMessage::GotoContigCoordinate(
                gene.contig().full_name(),
                gene.start(),
            ));
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
