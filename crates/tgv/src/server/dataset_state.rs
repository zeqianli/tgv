//! The loaded dataset state, dataset replacement, regional inspection, and rendering.

use super::{
    Command, Reply,
    schema::*,
    tables::{QueryRegion, TableSources},
};
use crate::{
    app::RenderEvent,
    layout::{AlignmentView, AreaType, MainLayout, ResolvedMainLayout},
    mouse::MouseRegister,
    register::Registers,
    rendering::render_main,
    settings::Settings,
    track_registry::{TrackId, TrackRegistry},
};
use gv_core::prelude::*;
use ratatui::{buffer::Buffer, layout::Rect};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// Owns the loaded dataset and the mutable state used by MCP tools.
pub(super) struct DatasetState {
    pub(super) settings: Settings,
    pub(super) state: State,
    pub(super) repository: Repository,
    pub(super) tracks: Arc<TrackRegistry>,
}

impl DatasetState {
    /// Processes commands sequentially and closes the dataset when the worker exits.
    pub(super) async fn run(
        settings: Settings,
        mut receiver: mpsc::Receiver<(Command, oneshot::Sender<Reply>)>,
    ) -> Result<(), TGVError> {
        let mut dataset: Option<Self> = None;
        while let Some((command, reply)) = receiver.recv().await {
            if reply.is_closed() && !matches!(&command, Command::Shutdown) {
                continue;
            }
            let result = match command {
                Command::Shutdown => break,
                Command::Describe => match dataset.as_ref() {
                    Some(dataset) => super::as_json(&dataset.description()),
                    None => Ok(json!({"loaded": false})),
                },
                Command::Replace(request) => Self::replace(&mut dataset, &settings, request).await,
                Command::Inspect(request) => match dataset.as_mut() {
                    Some(dataset) => dataset.inspect(request).await,
                    None => Err(TGVError::McpNoDataset {
                        operation: "inspecting",
                    }),
                },
                Command::Query(request) => match dataset.as_mut() {
                    Some(dataset) => dataset.query(request).await,
                    None => Err(TGVError::McpNoDataset {
                        operation: "querying",
                    }),
                },
                Command::Draw(request) => match dataset.as_mut() {
                    Some(dataset) => dataset.draw(request).await,
                    None => Err(TGVError::McpNoDataset {
                        operation: "drawing",
                    }),
                },
            };
            let _ = reply.send(result);
        }

        if let Some(mut dataset) = dataset {
            dataset.repository.close().await?;
        }

        Ok(())
    }

    /// Describes the current reference and tracks using their wire-format types.
    fn description(&self) -> DatasetDescription {
        DatasetDescription {
            reference: self.settings.core.reference.to_string(),
            tracks: self
                .tracks
                .entries
                .iter()
                .map(|entry| TrackDescription {
                    id: entry.id,
                    r#type: entry.repository_index.into(),
                    source: self.repository.file_path(entry.repository_index).to_owned(),
                })
                .collect(),
        }
    }

    /// Builds a replacement dataset before swapping it into the worker.
    async fn replace(
        current: &mut Option<Self>,
        defaults: &Settings,
        request: DatasetRequest,
    ) -> Reply {
        let prior_settings = current
            .as_ref()
            .map_or(defaults, |dataset| &dataset.settings);
        let new_settings = request.update_settings(prior_settings)?;
        let replacement =
            Self::new(new_settings)
                .await
                .map_err(|error| TGVError::McpInvalidInput {
                    field: "files",
                    message: error.to_string(),
                })?;
        let result = super::as_json(&replacement.description())?;
        if let Some(mut old) = current.replace(replacement) {
            if let Err(error) = old.repository.close().await {
                log::warn!("Failed to close the replaced dataset: {error}");
            }
        }
        Ok(result)
    }

    /// Loads the repositories and initializes one state slot per track.
    pub(super) async fn new(settings: Settings) -> Result<Self, TGVError> {
        let (repository, contigs, file_indexes) = Repository::new(&settings.core).await?;
        let mut state = State::new(settings.core.reference.clone(), contigs)?;
        for file in &settings.core.file_paths {
            match file {
                FilePath::AlignmentPath(_) => state.add_alignment_track(),
                FilePath::VariantPath(_) => state.add_variant_track(),
                FilePath::BedPath(_) => state.add_bed_track(),
            }
        }
        let tracks = Arc::new(TrackRegistry::new(&file_indexes));
        Ok(Self {
            state,
            repository,
            tracks,
            settings,
        })
    }

    /// Loads regional data for the selected tracks into the current state.
    async fn load_region(
        &mut self,
        region: &Region,
        track_ids: &[TrackId],
    ) -> Result<(), gv_core::error::TGVError> {
        if let Some(sequence) = self.repository.sequence_service.as_mut() {
            self.state.load_sequence_data(region, sequence).await?;
        }
        if let Some(genes) = self.repository.track_service.as_mut() {
            self.state.load_track_data(region, genes).await?;
        }
        for &id in track_ids {
            match self.tracks.get(id).repository_index {
                RepositoryFileIndex::Alignment(i) => {
                    self.state
                        .load_alignment_data(
                            i,
                            region,
                            &mut self.repository.alignment_repositories[i],
                        )
                        .await?;
                }
                RepositoryFileIndex::Variant(i) if !self.state.variant_loaded[i] => {
                    self.state
                        .load_variant_data(i, region, &mut self.repository.variant_repositories[i])
                        .await?;
                }
                RepositoryFileIndex::Bed(i) if !self.state.bed_loaded[i] => {
                    self.state
                        .load_bed_data(i, region, &mut self.repository.bed_repositories[i])
                        .await?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Validates requested track IDs and returns them in dataset order.
    fn selected_tracks(&self, tracks: Option<&[TrackId]>) -> Result<Vec<TrackId>, TGVError> {
        if let Some(tracks) = tracks {
            if tracks.is_empty()
                || tracks
                    .iter()
                    .enumerate()
                    .any(|(i, id)| tracks[..i].contains(id) || *id >= self.tracks.entries.len())
            {
                return Err(TGVError::McpInvalidInput {
                    field: "tracks",
                    message: "Select distinct track IDs from the current dataset.".to_owned(),
                });
            }
        }

        Ok(self
            .tracks
            .entries
            .iter()
            .filter(|entry| tracks.is_none_or(|ids| ids.contains(&entry.id)))
            .map(|entry| entry.id)
            .collect())
    }

    /// Reports reference and gene services that are unavailable to this dataset.
    fn availability_warnings(&self) -> Vec<InspectWarning> {
        let mut warnings = Vec::new();
        if self.repository.sequence_service.is_none() {
            warnings.push(InspectWarning::ReferenceUnavailable {
                message: "The dataset has no reference sequence.".to_owned(),
            });
        }
        if self.repository.track_service.is_none() {
            warnings.push(InspectWarning::GenesUnavailable {
                message: "The dataset has no gene annotation service.".to_owned(),
            });
        }
        warnings
    }

    /// Summarizes an inclusive region, clamping its end to a known contig length.
    pub(super) async fn inspect(&mut self, request: InspectRequest) -> Reply {
        let selected = self.selected_tracks(request.tracks.as_deref())?;
        let (query, region) = request.region.resolve(&self.state.contig_header)?;
        self.load_region(&query, &selected).await?;
        let contig_index = query.contig_index();

        let mut track_summaries = Vec::new();
        for &id in &selected {
            match self.tracks.get(id).repository_index {
                RepositoryFileIndex::Alignment(index) => {
                    track_summaries.push(TrackSummary::from_alignment(
                        id,
                        &self.state.alignments[index],
                        contig_index,
                        &region,
                    )?);
                }
                RepositoryFileIndex::Variant(index) => {
                    let track_summary = TrackSummary::from_variants(
                        id,
                        &self.state.variants[index],
                        contig_index,
                        &region,
                    )?;
                    track_summaries.push(track_summary)
                }
                RepositoryFileIndex::Bed(index) => {
                    let track_summary = TrackSummary::from_bed(
                        id,
                        &self.state.bed_intervals[index],
                        contig_index,
                        &region,
                    )?;
                    track_summaries.push(track_summary)
                }
            };
        }
        let summary = InspectSummary {
            tracks: track_summaries,
            genes: GeneSummary::from_genes(
                &self.state.track,
                self.repository.track_service.is_some(),
                contig_index,
                &region,
            )?,
        };
        super::as_json(&InspectResponse {
            region,
            summary,
            warnings: self.availability_warnings(),
        })
    }

    /// Loads every variant and BED track that is not loaded yet.
    ///
    /// These repositories read whole files, so the region only labels the load in logs.
    async fn load_whole_file_tracks(&mut self, region: &Region) -> Result<(), TGVError> {
        for entry in &self.tracks.entries {
            match entry.repository_index {
                RepositoryFileIndex::Variant(i) if !self.state.variant_loaded[i] => {
                    self.state
                        .load_variant_data(i, region, &mut self.repository.variant_repositories[i])
                        .await?;
                }
                RepositoryFileIndex::Bed(i) if !self.state.bed_loaded[i] => {
                    self.state
                        .load_bed_data(i, region, &mut self.repository.bed_repositories[i])
                        .await?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Runs a read-only SQL query over the curated tables.
    pub(super) async fn query(&mut self, request: QueryRequest) -> Reply {
        let limit = request.limit()?;
        let resolved = request
            .region
            .as_ref()
            .map(|region| region.resolve(&self.state.contig_header))
            .transpose()?;
        match &resolved {
            Some((query, _)) => {
                let all: Vec<TrackId> = self.tracks.entries.iter().map(|entry| entry.id).collect();
                self.load_region(query, &all).await?;
            }
            None => {
                let anywhere = Region {
                    focus: Focus {
                        contig_index: 0,
                        position: 1,
                    },
                    half_width: 0,
                };
                self.load_whole_file_tracks(&anywhere).await?;
            }
        }

        let region = resolved.as_ref().map(|(query, interval)| QueryRegion {
            contig_index: query.contig_index(),
            contig: &interval.contig,
            start: interval.start,
            end: interval.end,
        });
        let sources = TableSources {
            state: &self.state,
            tracks: &self.tracks,
            sources: self
                .tracks
                .entries
                .iter()
                .map(|entry| self.repository.file_path(entry.repository_index))
                .collect(),
        };
        let tables = sources.build(region.as_ref())?;
        let (frame, truncated) = request.execute(tables, limit)?;
        let warnings = if resolved.is_some() {
            self.availability_warnings()
        } else {
            Vec::new()
        };
        super::as_json(&QueryResponse::from_frame(
            resolved.map(|(_, interval)| interval),
            &frame,
            truncated,
            warnings,
        )?)
    }

    /// Reports dataset and layout limitations visible in a drawing.
    fn draw_warnings(
        &self,
        selected: &[TrackId],
        layout: &ResolvedMainLayout,
        alignment_view: &AlignmentView,
    ) -> Result<Vec<DrawWarning>, TGVError> {
        let mut warnings: Vec<_> = self
            .availability_warnings()
            .into_iter()
            .map(DrawWarning::from)
            .collect();
        for &id in selected {
            let index = self.tracks.get(id).repository_index;
            let area_type = match index {
                RepositoryFileIndex::Alignment(_) => AreaType::Alignment(id),
                RepositoryFileIndex::Variant(_) => AreaType::Variant(id),
                RepositoryFileIndex::Bed(_) => AreaType::Bed(id),
            };
            let area = layout
                .areas
                .iter()
                .find(|(kind, _)| *kind == area_type)
                .map(|(_, area)| area);
            let hidden = area.is_none_or(|area| area.height == 0);
            let clipped = match index {
                RepositoryFileIndex::Alignment(i) => {
                    let depth = match &self.state.paired_alignments[i] {
                        Some(paired) => paired.depth()?,
                        None => self.state.alignments[i].depth()?,
                    };
                    area.is_some_and(|area| depth > usize::from(area.height))
                        || alignment_view.zoom > AlignmentView::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS
                }
                _ => false,
            };
            if hidden || clipped {
                warnings.push(DrawWarning::RenderLimited {
                    track_id: id,
                    message: "The visualization omits this track or some read rows; structured results are independent of the display.".to_owned(),
                });
            }
        }
        if alignment_view.zoom > 1 {
            warnings.push(DrawWarning::RenderBinned {
                message: "Each column spans multiple bases; zoom in for individual bases."
                    .to_owned(),
            });
        }
        Ok(warnings)
    }

    /// Renders the requested viewport through the existing TUI renderer.
    pub(super) async fn draw(&mut self, request: DrawRequest) -> Reply {
        let query = request.try_to_region(&self.state.contig_header)?;

        let contig = self.state.contig_header.contigs[query.contig_index()]
            .name
            .clone();
        let selected = self.selected_tracks(request.tracks.as_deref())?;
        let layout = MainLayout::new(&self.settings, Arc::clone(&self.tracks), &selected);
        let resolved_layout = layout.resolve(
            Rect::new(0, 0, request.canvas_width, request.canvas_height),
            &self.repository,
        );
        let main_width = u64::from(resolved_layout.main_area.width);
        if main_width == 0 {
            return Err(TGVError::McpInvalidInput {
                field: "canvas_width",
                message: "The layout leaves no space for tracks.".to_owned(),
            });
        }
        if main_width
            .checked_mul(request.zoom)
            .is_none_or(|span| span > 100_000)
        {
            return Err(TGVError::McpInvalidInput {
                field: "zoom",
                message:
                    "The displayed span must be at most 100000 bases; reduce the zoom or width."
                        .to_owned(),
            });
        }

        let mut alignment_view = AlignmentView::new_with_zoom(
            query.focus.clone(),
            request.zoom,
            self.state.alignments.len(),
        );
        alignment_view.self_correct(
            &resolved_layout.main_area,
            self.state.contig_length(&query.focus)?,
        );
        let displayed = alignment_view.region(&resolved_layout.main_area);
        if displayed.end() > (usize::MAX / 16) as u64 {
            return Err(TGVError::McpInvalidInput {
                field: "center.position",
                message: "The displayed viewport extends beyond the platform coordinate range."
                    .to_owned(),
            });
        }

        self.load_region(&query, &selected).await?;
        if displayed != query {
            self.load_region(&displayed, &selected).await?;
        }

        let mut buffer = Buffer::empty(resolved_layout.terminal_area);
        render_main(
            &mut buffer,
            &mut self.state,
            &Registers::default(),
            &resolved_layout,
            &alignment_view,
            &MouseRegister::default(),
            &self.settings.palette,
            &vec![RenderEvent::All],
        )?;
        super::as_json(&DrawResponse::from_buffer(
            contig,
            &displayed,
            request.format,
            &buffer,
            self.draw_warnings(&selected, &resolved_layout, &alignment_view)?,
        ))
    }
}
