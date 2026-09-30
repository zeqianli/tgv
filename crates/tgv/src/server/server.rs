//! The loaded server state, dataset replacement, regional inspection, and rendering.

use super::{Command, Reply, error::*, request::*};
use crate::{
    app::RenderEvent,
    layout::{AlignmentView, AreaType, MainLayout},
    mouse::MouseRegister,
    register::Registers,
    rendering::render_main,
    settings::{Settings, classify_and_build_tracks},
};
use crossterm::style::{
    Attribute, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use gv_core::{
    error::TGVError,
    intervals::{Focus, GenomeInterval, Region},
    reference::Reference,
    repository::{Repository, RepositoryFileIndex},
    settings::FilePath,
    state::State,
};
use noodles::vcf::variant::record::AlternateBases;
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};
use serde_json::{Value, json};
use std::{fmt::Write, ptr::replace};
use tokio::sync::{mpsc, oneshot};
use unicode_width::UnicodeWidthStr;

pub(super) struct Server {
    pub(super) settings: Settings,
    pub(super) state: State,
    pub(super) repository: Repository,
    pub(super) file_indexes: Vec<RepositoryFileIndex>,
    //pub(super) description: DatasetDescription,
}

impl Server {
    pub(super) async fn run(
        // mut self,
        settings: Settings,
        mut receiver: mpsc::Receiver<(Command, oneshot::Sender<Reply>)>,
    ) -> Result<(), gv_core::error::TGVError> {
        let mut server = Server::new(settings).await?;
        while let Some((command, reply)) = receiver.recv().await {
            if reply.is_closed() {
                continue;
            }
            let result = match command {
                Command::Describe => super::as_json(&server.description()),

                Command::Replace(request) => server.replace(request).await,
                Command::Inspect(request) => server.inspect(request).await,
            };
            let _ = reply.send(result);
        }

        server.repository.close().await?;

        Ok(())
    }

    fn description(&self) -> String {
        "TODO".to_string()
    }

    async fn replace(&mut self, request: DatasetRequest) -> Reply {
        let new_settings = request.update_settings(&self.settings)?;

        self.repository.close().await?;
        let replacement = Self::new(new_settings).await?;

        let result = super::as_json(&replacement.description())?;

        self.settings = replacement.settings;
        self.state = replacement.state;
        self.repository = replacement.repository;
        self.file_indexes = replacement.file_indexes;

        Ok(result)
    }

    pub(super) async fn new(
        settings: Settings,
        // request: DatasetRequest,
    ) -> Result<Self, TGVError> {
        // let reference = match &settings.core.reference {
        //     Reference::NoReference => None,
        //     reference => Some(reference.to_string()),
        // };
        let (mut repository, contigs, file_indexes) = Repository::new(&settings.core).await?;
        let mut state = State::new(settings.core.reference.clone(), contigs)?;
        for file in &settings.core.file_paths {
            match file {
                FilePath::AlignmentPath(_) => state.add_alignment_track(),
                FilePath::VariantPath(_) => state.add_variant_track(),
                FilePath::BedPath(_) => state.add_bed_track(),
            }
        }
        let focus = state.default_focus(&mut repository).await?;
        // let tracks = file_indexes
        //     .iter()
        //     .zip(request.files)
        //     .enumerate()
        //     .map(|(i, (index, source))| TrackDescription {
        //         id: TrackId(format!("t{i}")),
        //         r#type: match index {
        //             RepositoryFileIndex::Alignment(_) => "alignment",
        //             RepositoryFileIndex::Variant(_) => "variant",
        //             RepositoryFileIndex::Bed(_) => "bed",
        //         },
        //         source,
        //     })
        //     .collect();
        let mut server = Self {
            state,
            repository,
            file_indexes,
            settings, // description: DatasetDescription {
                      //     revision,
                      //     reference,
                      //     tracks,
                      // },
        };
        // VCF and BED readers are lazy; validate them before committing a replacement.

        // if let Err(error) = result {
        //     if let Err(close_error) = server.repository.close().await {
        //         log::warn!("Failed to close a rejected dataset: {close_error}");
        //     }
        //     return Err(ApiError::invalid("files", error));
        // }
        Ok(server)
    }

    async fn load_region(
        &mut self,
        region: &Region,
        indexes: &[RepositoryFileIndex],
    ) -> Result<(), gv_core::error::TGVError> {
        if let Some(sequence) = self.repository.sequence_service.as_mut() {
            self.state.load_sequence_data(region, sequence).await?;
        }
        if let Some(genes) = self.repository.track_service.as_mut() {
            self.state.load_track_data(region, genes).await?;
        }
        for &index in indexes {
            match index {
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

    pub(super) async fn inspect(&mut self, request: InspectRequest) -> Reply {
        let Interval { contig, start, end } = request.region;
        if start == 0 || end < start || end - start >= 100_000 || end > (usize::MAX / 16) as u64 {
            return Err(ApiError::invalid(
                "region",
                "Use a positive 1-based inclusive interval of at most 100000 bases within the platform coordinate range.",
            ));
        }
        if let Some(render) = &request.render {
            if !(40..=500).contains(&render.width) || !(10..=500).contains(&render.height) {
                return Err(ApiError::invalid(
                    "render",
                    "The width must be 40–500 and the height must be 10–500.",
                ));
            }
        }
        let contig_index = self
            .state
            .contig_header
            .try_get_index_by_str(&contig)
            .map_err(|error| ApiError::invalid("region.contig", error))?;
        let header = &self.state.contig_header.contigs[contig_index];
        if header.length.is_some_and(|length| end > length) {
            return Err(ApiError::invalid(
                "region.end",
                "The interval extends beyond the contig.",
            ));
        }
        let region = Interval {
            contig: header.name.clone(),
            start,
            end,
        };
        if let Some(tracks) = &request.tracks {
            if tracks.is_empty()
                || tracks.iter().enumerate().any(|(i, id)| {
                    tracks[..i].contains(id)
                        || !self.description.tracks.iter().any(|track| track.id == *id)
                })
            {
                return Err(ApiError::invalid(
                    "tracks",
                    "Select distinct track IDs from the current dataset.",
                ));
            }
        }
        let selected: Vec<_> = self
            .description
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| {
                request
                    .tracks
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&track.id))
            })
            .map(|(index, _)| index)
            .collect();
        let indexes: Vec<_> = selected
            .iter()
            .map(|&index| self.file_indexes[index])
            .collect();
        let query = Region {
            focus: Focus {
                contig_index,
                position: start + (end - start) / 2,
            },
            half_width: (end - start).div_ceil(2),
        };
        self.state.messages.clear();
        self.load_region(&query, &indexes)
            .await
            .map_err(|error| ApiError::invalid("region", error))?;

        let mut tracks = Vec::new();
        let mut coverage = Vec::new();
        for (&file_index, &index) in selected.iter().zip(&indexes) {
            let id = &self.description.tracks[file_index].id;
            let summary = match index {
                RepositoryFileIndex::Alignment(index) => {
                    let alignment = &self.state.alignments[index];
                    let count = alignment
                        .reads
                        .iter()
                        .filter(|read| read.start <= end && read.end >= start)
                        .count();
                    let positions: Vec<_> = (start..=end).map(|position| {
                        let c = alignment.coverage_at(position);
                        json!({"position": position, "A": c.A, "C": c.C, "G": c.G, "T": c.T, "N": c.N, "total": c.total, "softclip": c.softclip})
                    }).collect();
                    coverage.push(json!({"track_id": id, "positions": positions}));
                    json!({"track_id": id, "type": "alignment", "overlapping_records": count})
                }
                RepositoryFileIndex::Variant(index) => {
                    let mut records = self.state.variants[index]
                        .overlapping(contig_index, start, end)
                        .map_err(ApiError::internal)?;
                    records.sort_by_key(|record| (record.start(), record.end(), record.index));
                    let items = records.iter().take(1000).map(|record| {
                        let alternate = record.record.alternate_bases().iter()
                            .map(|allele| allele.map(str::to_owned))
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|error| ApiError::invalid("files", error))?;
                        Ok(json!({"start": record.start(), "end": record.end(), "reference": record.record.reference_bases(), "alternate": alternate}))
                    }).collect::<Result<Vec<Value>, ApiError>>()?;
                    json!({"track_id": id, "type": "variant", "overlapping_records": records.len(), "truncated": records.len() > items.len(), "items": items})
                }
                RepositoryFileIndex::Bed(index) => {
                    let mut records = self.state.bed_intervals[index]
                        .overlapping(contig_index, start, end)
                        .map_err(ApiError::internal)?;
                    records.sort_by_key(|record| (record.start(), record.end(), record.index));
                    let items: Vec<_> = records
                        .iter()
                        .take(1000)
                        .map(|record| json!({"start": record.start(), "end": record.end()}))
                        .collect();
                    json!({"track_id": id, "type": "bed", "overlapping_records": records.len(), "truncated": records.len() > items.len(), "items": items})
                }
            };
            tracks.push(summary);
        }
        let mut genes: Vec<_> = self
            .state
            .track
            .features
            .iter()
            .filter(|gene| gene.overlaps(contig_index, start, end))
            .collect();
        genes.sort_by(|a, b| (a.start(), a.end(), &a.id).cmp(&(b.start(), b.end(), &b.id)));
        let gene_items: Vec<_> = genes.iter().take(1000).map(|gene| json!({"id": gene.id, "name": gene.name, "start": gene.start(), "end": gene.end(), "strand": gene.strand.to_string()})).collect();
        let summary = json!({"tracks": tracks, "genes": {"available": self.repository.track_service.is_some(), "overlapping_records": genes.len(), "truncated": genes.len() > gene_items.len(), "items": gene_items}});
        let mut warnings = Vec::new();
        if self.repository.sequence_service.is_none() {
            warnings.push(json!({"code": "reference_unavailable", "message": "The dataset has no reference sequence."}));
        }
        if self.repository.track_service.is_none() {
            warnings.push(json!({"code": "genes_unavailable", "message": "The dataset has no gene annotation service."}));
        }
        let render = if let Some(render) = request.render {
            let mut settings = self.settings.clone();
            settings.core.file_paths = selected
                .iter()
                .map(|&index| self.settings.core.file_paths[index].clone())
                .collect();
            let layout = MainLayout::new(&settings, &indexes);
            let resolved_layout = layout.resolve(Rect::new(0, 0, render.width, render.height));
            let width = u64::from(resolved_layout.main_area.width);
            if width == 0 {
                return Err(ApiError::invalid(
                    "render.width",
                    "The layout leaves no space for tracks.",
                ));
            }
            let mut alignment_view =
                AlignmentView::new(query.focus.clone(), self.state.alignments.len());
            alignment_view.zoom = (end - start + 1).div_ceil(width).max(1);
            alignment_view.self_correct(
                &resolved_layout.main_area,
                self.state
                    .contig_length(&query.focus)
                    .map_err(ApiError::internal)?,
            );
            let displayed = alignment_view.region(&resolved_layout.main_area);
            // Summaries are captured first, so display padding cannot affect their counts.
            self.load_region(&displayed, &indexes)
                .await
                .map_err(|error| ApiError::invalid("region", error))?;
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
            for (&file_index, &index) in selected.iter().zip(&indexes) {
                let area_type = match index {
                    RepositoryFileIndex::Alignment(i) => AreaType::Alignment(i),
                    RepositoryFileIndex::Variant(i) => AreaType::Variant(i),
                    RepositoryFileIndex::Bed(i) => AreaType::Bed(i),
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
                    warnings.push(json!({"code": "render_limited", "track_id": self.description.tracks[file_index].id, "message": "The visualization omits this track or some read rows; structured results are independent of the display."}));
                }
            }
            if alignment_view.zoom > 1 {
                warnings.push(json!({"code": "render_binned", "message": "Each column spans multiple bases; zoom in for individual bases."}));
            }
            Some(
                json!({"format": render.format, "width": render.width, "height": render.height, "region": {"contig": region.contig, "start": displayed.start(), "end": displayed.end()}, "text": Self::export_buffer(&buffer, render.format), "legend": "The existing TGV palette and symbols are used. Base letters identify bases; arrows indicate orientation; coverage occupies a separate track. Read rows may be clipped. Unicode drawing characters are preserved."}),
            )
        } else {
            None
        };
        Ok(
            json!({"dataset_revision": self.description.revision, "region": region, "summary": summary, "coverage": {"method": "viewer_current", "tracks": coverage}, "render": render, "warnings": warnings}),
        )
    }
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
