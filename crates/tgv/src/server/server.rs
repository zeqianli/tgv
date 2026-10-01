//! The loaded server state, dataset replacement, regional inspection, and rendering.

use super::{Command, Reply, error::*, schema::*};
use crate::{
    app::RenderEvent,
    layout::{AlignmentView, AreaType, MainLayout},
    mouse::MouseRegister,
    register::Registers,
    rendering::render_main,
    settings::Settings,
    track_registry::{TrackId, TrackRegistry},
};
use crossterm::style::{
    Attribute, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use gv_core::prelude::*;
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};
use serde_json::json;
use std::{fmt::Write, sync::Arc};
use tokio::sync::{mpsc, oneshot};
use unicode_width::UnicodeWidthStr;

/// Owns the loaded dataset and the mutable state used by HTTP requests.
pub(super) struct Server {
    pub(super) settings: Settings,
    pub(super) state: State,
    pub(super) repository: Repository,
    pub(super) tracks: Arc<TrackRegistry>,
}

impl Server {
    /// Processes commands sequentially and closes the dataset when the worker exits.
    pub(super) async fn run(
        settings: Settings,
        mut receiver: mpsc::Receiver<(Command, oneshot::Sender<Reply>)>,
    ) -> Result<(), gv_core::error::TGVError> {
        let mut server: Option<Self> = None;
        while let Some((command, reply)) = receiver.recv().await {
            if reply.is_closed() {
                continue;
            }
            let result = match command {
                Command::Describe => match server.as_ref() {
                    Some(server) => super::as_json(&server.description()),
                    None => Ok(json!({"loaded": false})),
                },
                Command::Replace(request) => Self::replace(&mut server, &settings, request).await,
                Command::Inspect(request) => match server.as_mut() {
                    Some(server) => server.inspect(request).await,
                    None => Err(ApiError::conflict(
                        "no_dataset",
                        "Load a dataset before inspecting.",
                    )),
                },
                Command::Draw(request) => match server.as_mut() {
                    Some(server) => server.draw(request).await,
                    None => Err(ApiError::conflict(
                        "no_dataset",
                        "Load a dataset before drawing.",
                    )),
                },
            };
            let _ = reply.send(result);
        }

        if let Some(mut server) = server {
            server.repository.close().await?;
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
        let prior_settings = current.as_ref().map_or(defaults, |server| &server.settings);
        let new_settings = request.update_settings(prior_settings)?;
        let replacement = Self::new(new_settings)
            .await
            .map_err(|error| ApiError::invalid("files", error))?;
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
    fn selected_tracks(&self, tracks: Option<&[TrackId]>) -> Result<Vec<TrackId>, ApiError> {
        if let Some(tracks) = tracks {
            if tracks.is_empty()
                || tracks
                    .iter()
                    .enumerate()
                    .any(|(i, id)| tracks[..i].contains(id) || *id >= self.tracks.entries.len())
            {
                return Err(ApiError::invalid(
                    "tracks",
                    "Select distinct track IDs from the current dataset.",
                ));
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

    /// Summarizes an inclusive region, clamping its end to a known contig length.
    pub(super) async fn inspect(&mut self, request: InspectRequest) -> Reply {
        let selected = self.selected_tracks(request.tracks.as_deref())?;
        let query = Region::try_from_contig_names_and_bounds(
            &request.region.contig,
            request.region.start,
            request.region.end,
            &self.state.contig_header,
            Some(InspectInterval::MAX_QUERY_WIDTH),
        )?;
        self.load_region(&query, &selected).await?;
        let contig_index = query.contig_index();
        let header = &self.state.contig_header.contigs[contig_index];
        let region = InspectInterval {
            contig: header.name.clone(),
            start: request.region.start,
            end: header
                .length
                .map_or(request.region.end, |length| request.region.end.min(length)),
        };

        let mut track_summaries = Vec::new();
        let mut coverage_summaries = Vec::new();
        for &id in &selected {
            match self.tracks.get(id).repository_index {
                RepositoryFileIndex::Alignment(index) => {
                    let (track_summary, track_coverage) =
                        TrackSummary::from_alignment(id, &self.state.alignments[index], &region);
                    coverage_summaries.push(track_coverage);
                    track_summaries.push(track_summary);
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
                &self.state.track.features,
                self.repository.track_service.is_some(),
                contig_index,
                &region,
            ),
        };
        let mut warnings = Vec::new();
        if self.repository.sequence_service.is_none() {
            warnings.push(ResponseWarning::ReferenceUnavailable {
                message: "The dataset has no reference sequence.".to_owned(),
            });
        }
        if self.repository.track_service.is_none() {
            warnings.push(ResponseWarning::GenesUnavailable {
                message: "The dataset has no gene annotation service.".to_owned(),
            });
        }
        super::as_json(&InspectResponse {
            region,
            summary,
            coverage: CoverageSummary {
                method: CoverageMethod::ViewerCurrent,
                tracks: coverage_summaries,
            },
            warnings,
        })
    }

    /// Renders the requested viewport through the existing TUI renderer.
    pub(super) async fn draw(&mut self, request: DrawRequest) -> Reply {
        let DrawRequest {
            center: DrawCenter { contig, position },
            zoom,
            half_width,
            tracks,
            format,
            width,
            height,
        } = request;
        if position == 0
            || half_width > 49_999
            || position
                .checked_add(half_width)
                .is_none_or(|end| end > (usize::MAX / 16) as u64)
        {
            return Err(ApiError::invalid(
                "center",
                "Use a positive 1-based center and a half-width of at most 49999 bases within the platform coordinate range.",
            ));
        }
        if zoom == 0 {
            return Err(ApiError::invalid("zoom", "The zoom must be positive."));
        }
        if !(40..=500).contains(&width) || !(10..=500).contains(&height) {
            return Err(ApiError::invalid(
                "draw",
                "The width must be 40–500 and the height must be 10–500.",
            ));
        }
        let contig_index = self
            .state
            .contig_header
            .try_get_index_by_str(&contig)
            .map_err(|error| ApiError::invalid("center.contig", error))?;
        let header = &self.state.contig_header.contigs[contig_index];
        if header.length.is_some_and(|length| position > length) {
            return Err(ApiError::invalid(
                "center.position",
                "The center is beyond the contig.",
            ));
        }
        let contig = header.name.clone();
        let selected = self.selected_tracks(tracks.as_deref())?;
        let query = Region {
            focus: Focus {
                contig_index,
                position,
            },
            half_width,
        };
        let layout = MainLayout::new(&self.settings, Arc::clone(&self.tracks), &selected);
        let resolved_layout = layout.resolve(Rect::new(0, 0, width, height), &self.repository);
        let main_width = u64::from(resolved_layout.main_area.width);
        if main_width == 0 {
            return Err(ApiError::invalid(
                "width",
                "The layout leaves no space for tracks.",
            ));
        }
        if main_width
            .checked_mul(zoom)
            .is_none_or(|span| span > 100_000)
        {
            return Err(ApiError::invalid(
                "zoom",
                "The displayed span must be at most 100000 bases; reduce the zoom or width.",
            ));
        }
        let mut alignment_view =
            AlignmentView::new(query.focus.clone(), self.state.alignments.len());
        alignment_view.zoom = zoom;
        alignment_view.self_correct(
            &resolved_layout.main_area,
            self.state
                .contig_length(&query.focus)
                .map_err(ApiError::internal)?,
        );
        let displayed = alignment_view.region(&resolved_layout.main_area);
        self.state.messages.clear();
        self.load_region(&query, &selected)
            .await
            .map_err(|error| ApiError::invalid("region", error))?;
        if displayed != query {
            self.load_region(&displayed, &selected)
                .await
                .map_err(|error| ApiError::invalid("region", error))?;
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
        )
        .map_err(ApiError::internal)?;
        let mut warnings = Vec::new();
        if self.repository.sequence_service.is_none() {
            warnings.push(ResponseWarning::ReferenceUnavailable {
                message: "The dataset has no reference sequence.".to_owned(),
            });
        }
        if self.repository.track_service.is_none() {
            warnings.push(ResponseWarning::GenesUnavailable {
                message: "The dataset has no gene annotation service.".to_owned(),
            });
        }
        for &id in &selected {
            let index = self.tracks.get(id).repository_index;
            let area_type = match index {
                RepositoryFileIndex::Alignment(_) => AreaType::Alignment(id),
                RepositoryFileIndex::Variant(_) => AreaType::Variant(id),
                RepositoryFileIndex::Bed(_) => AreaType::Bed(id),
            };
            let area = resolved_layout
                .areas
                .iter()
                .find(|(kind, _)| *kind == area_type)
                .map(|(_, area)| area);
            let hidden = area.is_none_or(|area| area.height == 0);
            let clipped = match index {
                RepositoryFileIndex::Alignment(i) => {
                    area.is_some_and(|area| {
                        self.state.alignments[i].depth() > usize::from(area.height)
                    }) || alignment_view.zoom > AlignmentView::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS
                }
                _ => false,
            };
            if hidden || clipped {
                warnings.push(ResponseWarning::RenderLimited {
                    track_id: id,
                    message: "The visualization omits this track or some read rows; structured results are independent of the display.".to_owned(),
                });
            }
        }
        if alignment_view.zoom > 1 {
            warnings.push(ResponseWarning::RenderBinned {
                message: "Each column spans multiple bases; zoom in for individual bases."
                    .to_owned(),
            });
        }
        super::as_json(&DrawResponse {
            center: DrawCenter {
                contig: contig.clone(),
                position: alignment_view.focus.position,
            },
            zoom: alignment_view.zoom,
            half_width,
            format,
            width,
            height,
            region: DrawInterval {
                contig,
                start: displayed.start(),
                end: displayed.end(),
            },
            text: Self::export_buffer(&buffer, format),
            legend: "The existing TGV palette and symbols are used. Base letters identify bases; arrows indicate orientation; coverage occupies a separate track. Read rows may be clipped. Unicode drawing characters are preserved.".to_owned(),
            warnings,
        })
    }

    /// Exports terminal cells as plain text or ANSI-colored text.
    fn export_buffer(buffer: &Buffer, format: RenderFormat) -> String {
        let mut output = String::new();
        for y in buffer.area.top()..buffer.area.bottom() {
            let mut x = buffer.area.left();
            let mut previous_style = None;
            while x < buffer.area.right() {
                let cell = &buffer[(x, y)];
                let symbol: String = cell
                    .symbol()
                    .chars()
                    .map(|c| if c.is_control() { '�' } else { c })
                    .collect();
                if format == RenderFormat::Ansi && previous_style != Some(cell.style()) {
                    let _ = write!(
                        output,
                        "{}{}{}",
                        SetAttribute(Attribute::Reset),
                        SetForegroundColor(cell.fg.into()),
                        SetBackgroundColor(cell.bg.into())
                    );
                    for (modifier, attribute) in [
                        (Modifier::BOLD, Attribute::Bold),
                        (Modifier::DIM, Attribute::Dim),
                        (Modifier::ITALIC, Attribute::Italic),
                        (Modifier::UNDERLINED, Attribute::Underlined),
                        (Modifier::REVERSED, Attribute::Reverse),
                        (Modifier::CROSSED_OUT, Attribute::CrossedOut),
                    ] {
                        if cell.modifier.contains(modifier) {
                            let _ = write!(output, "{}", SetAttribute(attribute));
                        }
                    }
                    previous_style = Some(cell.style());
                }
                output.push_str(&symbol);
                x += UnicodeWidthStr::width(symbol.as_str()).max(1) as u16;
            }
            if format == RenderFormat::Ansi {
                let _ = write!(output, "{}{}", SetAttribute(Attribute::Reset), ResetColor);
            }
            output.push('\n');
        }
        output
    }
}
