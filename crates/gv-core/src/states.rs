use crate::settings::Settings;
use crate::tracks::TrackService;
use crate::{
    alignment::Alignment,
    contig_header::ContigHeader,
    cytoband::Cytoband,
    error::TGVError,
    feature::Gene,
    intervals::{Focus, GenomeInterval, Region},
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

/// Getters
impl State {
    pub fn new(settings: &Settings, contigs: ContigHeader) -> Result<Self, TGVError> {
        Ok(Self {
            reference: settings.reference.clone(),

            // /settings: settings.clone(),
            messages: Vec::new(),

            alignment: Alignment::default(),
            alignment_options: Vec::new(),
            track: Track::<Gene>::default(),
            sequence: Sequence::default(),
            contig_header: contigs,
        })
    }

    pub fn contig_name(&self, region: &Region) -> Result<&String, TGVError> {
        self.contig_header
            .try_get(region.contig_index)
            .map(|contig| &contig.name)
    }

    pub fn current_cytoband(&self, region: &Region) -> Option<&Cytoband> {
        self.contig_header
            .try_get(region.contig_index)
            .unwrap()
            .cytoband
            .as_ref()
    }

    /// Maximum length of the contig.
    pub fn contig_length(&self, region: &Region) -> Result<Option<usize>, TGVError> {
        Ok(self.contig_header.try_get(region.contig_index)?.length)
    }

    const ALIGNMENT_CACHE_RATIO: usize = 3;

    pub fn alignment_cache_region(state: &State, region: &Region) -> Result<Region, TGVError> {
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

    pub fn sequence_cache_region(state: &State, region: &Region) -> Result<Region, TGVError> {
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

    pub fn track_cache_region(state: &State, region: &Region) -> Result<Region, TGVError> {
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

impl State {
    fn add_message(&mut self, message: String) -> Result<(), TGVError> {
        self.messages.push(message);
        Ok(())
    }

    fn get_data_requirements(
        &self,
        region: &Region,
        repository: &mut Repository, // settings: &Settings,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages = Vec::new();

        // It's important to load sequence first!
        // Alignment IO requires calculating mismatches with the reference sequence.

        if repository.sequence_service.is_some()
            && self.sequence_renderable()
            && !self.sequence.has_complete_data(&region)
        {
            let sequence_cache_region = self.sequence_cache_region(region)?;
            data_messages.push(DataMessage::RequiresCompleteSequences(
                sequence_cache_region,
            ));
        }
        if repository.alignment_repository.is_some()
            && self.alignment_renderable()
            && !self.alignment.has_complete_data(&region)
        {
            let alignment_cache_region = self.alignment_cache_region(region)?;
            data_messages.push(DataMessage::RequiresCompleteAlignments(
                alignment_cache_region,
            ));
        }

        if repository.track_service.is_some() {
            if !self.track.has_complete_data(&region) {
                // viewing_window.zoom <= Self::MAX_ZOOM_TO_DISPLAY_FEATURES is always true
                let track_cache_region = self.track_cache_region(&region)?;
                data_messages.push(DataMessage::RequiresCompleteFeatures(track_cache_region));
            }

            // Cytobands
            data_messages.push(DataMessage::RequiresCytobands(region.contig_index));
        }

        Ok(data_messages)
    }

    pub async fn handle_data_message(
        &mut self,
        repository: &mut Repository,
        data_message: DataMessage,
    ) -> Result<bool, TGVError> {
        let mut loaded_data = false;

        match data_message {
            DataMessage::RequiresCompleteAlignments(region) => {
                if !self.alignment.has_complete_data(&region) {
                    self.alignment = {
                        let mut alignment = repository
                            .alignment_repository
                            .as_mut()
                            .unwrap()
                            .read_alignment(&region, &self.sequence, &self.contig_header)
                            .await?;

                        alignment.apply_options(&self.alignment_options, &self.sequence)?;

                        alignment
                    };

                    // apply sorting and filtering

                    loaded_data = true;
                }
            }
            DataMessage::RequiresCompleteFeatures(region) => {
                let has_complete_track = self.track.has_complete_data(&region);
                if let Some(track_service) = repository.track_service.as_mut() {
                    if !has_complete_track {
                        if let Ok(track) = track_service
                            .query_gene_track(&self.reference, &region, &self.contig_header)
                            .await
                        {
                            self.track = track;
                            loaded_data = true;
                        } else {
                            // Do nothing (track not found). TODO: fix this shit properly.
                        }
                    }
                } else {
                    loaded_data = match self.reference {
                        // FIXME: this is duplicate code as Settings.
                        Reference::BYOIndexedFasta(_) => false,
                        _ => true,
                    };
                }
            }
            DataMessage::RequiresCompleteSequences(region) => {
                let sequence_service = repository.sequence_service_checked()?;

                if !self.sequence.has_complete_data(&region) {
                    let sequence = sequence_service
                        .query_sequence(&region, &self.contig_header)
                        .await?;

                    self.sequence = sequence;
                    loaded_data = true;
                }
            }

            DataMessage::RequiresCytobands(contig_index) => {
                if self.contig_header.cytoband_is_loaded(contig_index)? {
                    return Ok(false);
                }

                if let Some(track_service) = repository.track_service.as_mut() {
                    let cytoband = track_service
                        .get_cytoband(&self.reference, contig_index, &self.contig_header)
                        .await?;
                    self.contig_header
                        .try_update_cytoband(contig_index, cytoband)?;
                    loaded_data = true;
                }
            }
        }

        Ok(loaded_data)
    }
}

impl StateHandler {
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

// Movement handling
impl State {
    async fn next_genes_start(
        &self,
        region: Region,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Region, TGVError> {
        if n == 0 {
            return Ok(region);
        }

        let middle = region.middle();

        // The gene is in the track.
        if let Some(target_gene) = self.track.get_k_genes_after(middle, n) {
            return Ok(region.move_to(target_gene.start()));
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_after(
                &self.reference,
                region.contig_index,
                middle,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(region.move_to(gene.start()))
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
        &self,
        repository: &mut Repository,
        gene_name: String,
    ) -> Result<Focus, TGVError> {
        repository
            .track_service_checked()?
            .query_gene_name(&self.reference, &gene_name, &self.contig_header)
            .await
            .map(|gene| Focus {
                contig_index: gene.contig_index(),
                position: gene.start() + 1,
            })
    }

    async fn go_to_next_contig(&self, region: Region, n: usize) -> Focus {
        Focus {
            contig_index: self.contig_header.next(&region.contig_index(), n),
            position: 1,
        }
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

    async fn default_region(&self, repository: &mut Repository) -> Result<(), TGVError> {
        match self.reference {
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
