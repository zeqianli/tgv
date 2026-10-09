use crate::{
    layout::{AlignmentView, AreaType, HoveringAreaType, ResolvedMainLayout},
    message::{Action, ContextMenuTarget, Movement, Scroll, UpdateLayoutAction},
};
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use gv_core::prelude::*;
use gv_core::{
    alignment::{CoverageSchema, ReadBase},
    bed::BedSchema,
    gene::{GeneSchema, GeneSegmentSchema, query_segments},
    variant::VariantSchema,
};
use itertools::Itertools;
use polars::prelude::ChunkAgg;
use ratatui::layout::Rect;

/// Columns panned per Shift+wheel notch. A wheel notch is coarser than a trackpad's
/// horizontal scroll events, which pan one column each.
const SHIFT_SCROLL_COLUMNS: u64 = 3;

/// Find the read drawn at a cell of an alignment track area, as `(alignment index, read ID)`.
fn read_at(
    state: &State,
    layout: &ResolvedMainLayout,
    alignment_view: &AlignmentView,
    track: TrackId,
    area: &Rect,
    column: u16,
    row: u16,
) -> Result<Option<(usize, usize)>, TGVError> {
    let index = layout.track_registry.alignment_index(track)?;
    let (Some((left, right)), Some(y)) = (
        alignment_view.coordinates_of_onscreen_x(column, area),
        alignment_view.coordinate_of_onscreen_y(index, row, area),
    ) else {
        return Ok(None);
    };
    let alignment = &state.alignments[index];
    let read_id = match &state.paired_alignments[index] {
        Some(paired) => paired.read_overlapping(alignment, left, right, y)?,
        None => alignment.read_overlapping(left, right, y)?,
    };
    Ok(read_id.map(|read_id| (index, read_id)))
}

/// Mouse interaction state for the currently displayed layout.
#[derive(Default)]
pub struct MouseRegister {
    mouse_down_area: Option<AreaType>,
    last_x: u16,
    last_y: u16,
    active_divider: Option<(TrackId, TrackId)>,
    sidebar_resizing: bool,
    /// Whether the pointer moved since the left button went down, which makes it a drag
    /// rather than a click.
    dragged: bool,
    pub hovered_divider: Option<(TrackId, TrackId)>,

    /// The cell of the last handled motion event. Terminals can report several motion events
    /// within one cell, which would otherwise repeat the hover lookup and redraw. Reset this
    /// whenever the state under the cursor may have changed.
    pub last_hover: Option<(u16, u16)>,
}

impl MouseRegister {
    pub fn is_divider_highlighted(&self, area_type: &AreaType) -> bool {
        match area_type {
            AreaType::AlignmentDivider { upper, lower } => {
                let pair = Some((*upper, *lower));
                self.hovered_divider == pair || self.active_divider == pair
            }
            _ => false,
        }
    }

    pub fn is_sidebar_divider_highlighted(&self) -> bool {
        self.sidebar_resizing
    }

    /// Translate a mouse event into app messages using the last rendered layout.
    pub fn handle_mouse_event(
        &mut self,
        state: &State,
        layout: &ResolvedMainLayout,
        alignment_view: &AlignmentView,
        event: MouseEvent,
    ) -> Result<Vec<Action>, TGVError> {
        let cell = (event.column, event.row);
        if event.kind == MouseEventKind::Moved {
            if self.last_hover == Some(cell) {
                return Ok(Vec::new());
            }
            self.last_hover = Some(cell);
        } else {
            self.last_hover = None;
        }

        let mut messages = Vec::new();
        let hovered = layout.get_area_type_at_position(event.column, event.row);
        // A drag keeps scrolling the track where it started, even if it crosses into another.
        let dragging = matches!(event.kind, MouseEventKind::Drag(MouseButton::Left));
        if !dragging
            && let HoveringAreaType::Track(track_index) = hovered
            && let AreaType::Alignment(id) | AreaType::Coverage(id) = layout.areas[track_index].0
        {
            messages.push(Action::FocusAlignment(id));
        }
        self.hovered_divider = match hovered {
            HoveringAreaType::Track(track_index) => match layout.areas[track_index].0 {
                AreaType::AlignmentDivider { upper, lower } => Some((upper, lower)),
                _ => None,
            },
            _ => None,
        };

        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.last_x = event.column;
                self.last_y = event.row;
                self.mouse_down_area = None;
                self.active_divider = None;
                self.dragged = false;
                self.sidebar_resizing = matches!(hovered, HoveringAreaType::SidebarDivider);
                if let HoveringAreaType::Track(track_index) = hovered {
                    let area = layout.areas[track_index].0;
                    self.mouse_down_area = Some(area);
                    // The cytoband spans the whole contig, so a click there goes to that part of
                    // the contig at the same zoom.
                    let rect = layout.areas[track_index].1;
                    let bar_width = rect
                        .width
                        .saturating_sub(crate::rendering::cytoband::CYTOBAND_TEXT_RIGHT_SPACING);
                    if area == AreaType::Cytoband
                        && event.column - rect.x < bar_width
                        && let Some(length) = state.contig_length(&alignment_view.focus)?
                    {
                        // Aim at the middle of the clicked column.
                        let fraction =
                            (f64::from(event.column - rect.x) + 0.5) / f64::from(bar_width);
                        let position = ((fraction * length as f64) as u64).clamp(1, length);
                        messages.push(Movement::Position(position).into());
                    }
                    if let AreaType::AlignmentDivider { upper, lower } = area {
                        self.active_divider = Some((upper, lower));
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                let target = match hovered {
                    HoveringAreaType::Sidebar(_) => Some(ContextMenuTarget::Sidebar),
                    HoveringAreaType::Track(track_index) => match &layout.areas[track_index] {
                        (AreaType::Alignment(id) | AreaType::Coverage(id), area) => {
                            Some(ContextMenuTarget::Alignment {
                                track: *id,
                                // Base actions need a single base under the cursor.
                                position: alignment_view
                                    .coordinates_of_onscreen_x(event.column, area)
                                    .and_then(|(left, right)| (left == right).then_some(left)),
                            })
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(target) = target {
                    messages.push(Action::OpenContextMenu {
                        target,
                        column: event.column,
                        row: event.row,
                    });
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if (event.column, event.row) != (self.last_x, self.last_y) {
                    self.dragged = true;
                }
                if self.sidebar_resizing {
                    messages.push(Action::UpdateLayout(UpdateLayoutAction::SetSidebarWidth(
                        event.column,
                    )));
                } else if let Some((upper, lower)) = self.active_divider {
                    let delta_rows = event.row as i32 - self.last_y as i32;
                    if delta_rows != 0 {
                        messages.push(Action::UpdateLayout(
                            UpdateLayoutAction::ResizeAlignmentPair {
                                upper,
                                lower,
                                delta_rows,
                            },
                        ));
                    }
                } else if let Some(area) = self.mouse_down_area {
                    match area {
                        AreaType::Alignment(id) | AreaType::Coverage(id) => {
                            if event.column < self.last_x {
                                messages.push(Movement::Right(1).into());
                            } else if event.column > self.last_x {
                                messages.push(Movement::Left(1).into());
                            }
                            if event.row != self.last_y {
                                messages.push(Action::FocusAlignment(id));
                                messages.push(if event.row > self.last_y {
                                    Scroll::Up(1).into()
                                } else {
                                    Scroll::Down(1).into()
                                });
                            }
                        }
                        AreaType::Bed(_) | AreaType::Variant(_) => {
                            if event.column < self.last_x {
                                messages.push(Movement::Right(1).into());
                            } else if event.column > self.last_x {
                                messages.push(Movement::Left(1).into());
                            }
                        }
                        _ => {}
                    }
                }
                self.last_x = event.column;
                self.last_y = event.row;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                // A click on a read, without dragging, shows its details.
                if !self.dragged
                    && let Some(AreaType::Alignment(track)) = self.mouse_down_area
                    && let HoveringAreaType::Track(track_index) = hovered
                    && let (AreaType::Alignment(id), area) = &layout.areas[track_index]
                    && *id == track
                    && let Some((_, read_id)) = read_at(
                        state,
                        layout,
                        alignment_view,
                        track,
                        area,
                        event.column,
                        event.row,
                    )?
                {
                    messages.push(Action::OpenReadDetails { track, read_id });
                }
                self.mouse_down_area = None;
                self.active_divider = None;
                self.sidebar_resizing = false;
            }
            MouseEventKind::Moved => {
                if let HoveringAreaType::Track(track_index) = hovered {
                    let (area_type, area) = &layout.areas[track_index];
                    match area_type {
                        AreaType::Alignment(id) => {
                            if let Some((index, read_id)) = read_at(
                                state,
                                layout,
                                alignment_view,
                                *id,
                                area,
                                event.column,
                                event.row,
                            )? && let Some((left, right)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                            {
                                let alignment = &state.alignments[index];
                                let record = &alignment.records[read_id];
                                let contig = &state.contig_header.contigs
                                    [alignment_view.focus.contig_index]
                                    .name;
                                let name = record
                                    .name()
                                    .map(|name| name.to_string())
                                    .unwrap_or_else(|| "<missing>".into());
                                let description = if left == right {
                                    let base = match alignment.read_base_at(read_id, left)? {
                                        // Show the quality as the file stores it.
                                        Some(ReadBase::Base { base, quality }) => format!(
                                            "Base: {}  Qual: {}",
                                            base as char,
                                            quality.map_or_else(
                                                || ".".into(),
                                                |q| alignment
                                                    .quality_encoding
                                                    .stored(q)
                                                    .to_string()
                                            )
                                        ),
                                        Some(ReadBase::Deletion) => "Base: -".to_string(),
                                        None => "Base: .".to_string(),
                                    };
                                    format!("{contig}:{left}  {name}  {base}")
                                } else {
                                    format!("{contig}:{left}-{right}  {name}")
                                };
                                let mut cigar = Vec::new();
                                noodles::sam::io::writer::record::write_cigar(
                                    &mut cigar,
                                    record.cigar(),
                                )?;
                                let mapq = record
                                    .mapping_quality()
                                    .map(|quality| quality.get().to_string())
                                    .unwrap_or_else(|| ".".into());
                                messages.push(Action::message(format!(
                                    "{description}  Cigar: {}  MAPQ: {mapq}",
                                    String::from_utf8(cigar)?
                                )));
                            }
                        }
                        AreaType::GeneTrack => {
                            if let Some((left, right)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                            {
                                let genes = state.track.query(
                                    alignment_view.focus.contig_index,
                                    left,
                                    right,
                                )?;
                                let segments = query_segments(genes.clone(), left, right)?;
                                let contig_index = alignment_view.focus.contig_index;
                                let contig = &state.contig_header.contigs[contig_index].name;
                                let locus = if left == right {
                                    format!("{contig}:{left}")
                                } else {
                                    format!("{contig}:{left}-{right}")
                                };
                                let segment_genes =
                                    segments.column(GeneSegmentSchema::GENE_ROW_ID)?.u64()?;
                                let segment_kinds =
                                    segments.column(GeneSegmentSchema::KIND)?.str()?;
                                let segment_numbers =
                                    segments.column(GeneSegmentSchema::FEATURE_INDEX)?.u64()?;
                                let row_ids = genes.column(GeneSchema::ROW_ID)?.u64()?;
                                let names = genes.column(GeneSchema::NAME)?.str()?;
                                let ids = genes.column(GeneSchema::ID)?.str()?;
                                let strands = genes.column(GeneSchema::STRAND)?.str()?;
                                let exon_starts = genes.column(GeneSchema::EXON_STARTS)?.list()?;
                                for row in 0..genes.height() {
                                    let row_id =
                                        row_ids.get(row).expect("gene row IDs are non-null");
                                    let label = [names.get(row), ids.get(row)]
                                        .into_iter()
                                        .flatten()
                                        .dedup()
                                        .join(" ");
                                    let strand = strands.get(row).unwrap_or(".");
                                    let exons = exon_starts
                                        .get_as_series(row)
                                        .map_or(0, |starts| starts.len());
                                    let part = (0..segments.height())
                                        .find(|&segment| segment_genes.get(segment) == Some(row_id))
                                        .map(|segment| {
                                            let number = segment_numbers
                                                .get(segment)
                                                .expect("segment numbers are non-null");
                                            match segment_kinds.get(segment) {
                                                Some("intron") => format!(
                                                    ": intron {number} of {}",
                                                    exons.saturating_sub(1)
                                                ),
                                                Some("noncoding_exon") => format!(
                                                    ": exon {number} of {exons}, untranslated"
                                                ),
                                                _ => format!(": exon {number} of {exons}"),
                                            }
                                        })
                                        .unwrap_or_default();
                                    messages.push(Action::message(format!(
                                        "{locus}  {label} ({strand}){part}"
                                    )));
                                }
                            }
                        }
                        AreaType::Sequence => {
                            if let Some((left, right)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                            {
                                let description = (left..=right)
                                    .filter_map(|coordinate| {
                                        state
                                            .sequence
                                            .base_at(coordinate)
                                            .map(|base| format!("{}: {}", coordinate, base as char))
                                    })
                                    .join(", ");
                                messages.push(Action::message(description));
                            }
                        }
                        AreaType::Coverage(id) => {
                            let index = layout.track_registry.alignment_index(*id)?;
                            if let Some((left, right)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                                && let Some(alignment) = state.alignments.get(index)
                            {
                                let coverage = alignment.coverage.query(left, right)?;
                                let counts = format!(
                                    "A:{}, T:{}, C:{}, G:{}, N:{}, total:{}",
                                    coverage
                                        .column(CoverageSchema::A)?
                                        .u64()?
                                        .sum()
                                        .unwrap_or(0),
                                    coverage
                                        .column(CoverageSchema::T)?
                                        .u64()?
                                        .sum()
                                        .unwrap_or(0),
                                    coverage
                                        .column(CoverageSchema::C)?
                                        .u64()?
                                        .sum()
                                        .unwrap_or(0),
                                    coverage
                                        .column(CoverageSchema::G)?
                                        .u64()?
                                        .sum()
                                        .unwrap_or(0),
                                    coverage
                                        .column(CoverageSchema::N)?
                                        .u64()?
                                        .sum()
                                        .unwrap_or(0),
                                    coverage
                                        .column(CoverageSchema::TOTAL)?
                                        .u64()?
                                        .sum()
                                        .unwrap_or(0)
                                );
                                let description = if left == right {
                                    format!("{}: {}", left, counts)
                                } else {
                                    format!("{} - {}: {}", left, right, counts)
                                };
                                messages.push(Action::message(description));
                            }
                        }
                        AreaType::Variant(id) => {
                            let index = layout.track_registry.variant_index(*id)?;
                            if let Some((left, right)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                                && let Some(variants) = state.variants.get(index)
                            {
                                let rows = variants.query(
                                    alignment_view.focus.contig_index,
                                    left,
                                    right,
                                )?;
                                let starts = rows.column(VariantSchema::START)?.u64()?;
                                let references = rows.column(VariantSchema::REFERENCE)?.str()?;
                                let alternates = rows.column(VariantSchema::ALTERNATE)?.list()?;
                                let qualities = rows.column(VariantSchema::QUALITY_SCORE)?.f32()?;
                                let contig = &state.contig_header.contigs
                                    [alignment_view.focus.contig_index]
                                    .name;
                                for row in 0..rows.height() {
                                    let alleles = alternates.get_as_series(row);
                                    let alternate = match alleles {
                                        Some(alleles) => alleles
                                            .str()?
                                            .iter()
                                            .map(|allele| {
                                                allele.expect("alternate alleles are non-null")
                                            })
                                            .join(","),
                                        None => String::new(),
                                    };
                                    let quality = qualities
                                        .get(row)
                                        .map(|q| q.to_string())
                                        .unwrap_or_else(|| "?".into());
                                    messages.push(Action::message(format!(
                                        "Variant: {}:{} {}>{} QUAL={}",
                                        contig,
                                        starts.get(row).expect("variant starts are non-null"),
                                        references.get(row).expect("reference bases are non-null"),
                                        alternate,
                                        quality
                                    )));
                                }
                            }
                        }
                        AreaType::Bed(id) => {
                            let index = layout.track_registry.bed_index(*id)?;
                            if let Some((left, right)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                                && let Some(intervals) = state.bed_intervals.get(index)
                            {
                                let rows = intervals.query(
                                    alignment_view.focus.contig_index,
                                    left,
                                    right,
                                )?;
                                let starts = rows.column(BedSchema::START)?.u64()?;
                                let ends = rows.column(BedSchema::END)?.u64()?;
                                let names = rows.column(BedSchema::NAME)?.str()?;
                                let contig = &state.contig_header.contigs
                                    [alignment_view.focus.contig_index]
                                    .name;
                                for row in 0..rows.height() {
                                    let name = names
                                        .get(row)
                                        .map_or_else(String::new, |name| format!(" {name}"));
                                    messages.push(Action::message(format!(
                                        "BED interval: {}:{}-{}{}",
                                        contig,
                                        starts.get(row).expect("BED starts are non-null"),
                                        ends.get(row).expect("BED ends are non-null"),
                                        name
                                    )));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            // Shift turns the vertical wheel into horizontal panning, as in browsers.
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
                if event.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                if matches!(hovered, HoveringAreaType::Track(_)) {
                    let movement = if event.kind == MouseEventKind::ScrollDown {
                        Movement::Right(SHIFT_SCROLL_COLUMNS)
                    } else {
                        Movement::Left(SHIFT_SCROLL_COLUMNS)
                    };
                    messages.push(movement.into());
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                // The focus action pushed above targets the hovered alignment.
                if let HoveringAreaType::Track(track_index) = hovered
                    && let AreaType::Alignment(_) | AreaType::Coverage(_) =
                        layout.areas[track_index].0
                {
                    let scroll = if matches!(event.kind, MouseEventKind::ScrollDown) {
                        Scroll::Down(1)
                    } else {
                        Scroll::Up(1)
                    };
                    messages.push(scroll.into());
                }
            }
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
                if matches!(hovered, HoveringAreaType::Track(_)) {
                    let movement = if matches!(event.kind, MouseEventKind::ScrollLeft) {
                        Movement::Left(1)
                    } else {
                        Movement::Right(1)
                    };
                    messages.push(movement.into());
                }
            }
            _ => {}
        }
        Ok(messages)
    }
}
