use crate::{
    layout::{AlignmentView, AreaType, HoveringAreaType, ResolvedMainLayout},
    message::{Action, ContextMenuTarget, Movement, Scroll, UpdateLayoutAction},
};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use gv_core::prelude::*;
use gv_core::{alignment::CoverageSchema, bed::BedSchema, variant::VariantSchema};
use itertools::Itertools;
use polars::prelude::ChunkAgg;

/// Mouse interaction state for the currently displayed layout.
#[derive(Default)]
pub struct MouseRegister {
    mouse_down_area: Option<AreaType>,
    last_x: u16,
    last_y: u16,
    active_divider: Option<(TrackId, TrackId)>,
    sidebar_resizing: bool,
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
                self.sidebar_resizing = matches!(hovered, HoveringAreaType::SidebarDivider);
                if let HoveringAreaType::Track(track_index) = hovered {
                    let area = layout.areas[track_index].0;
                    self.mouse_down_area = Some(area);
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
                self.mouse_down_area = None;
                self.active_divider = None;
                self.sidebar_resizing = false;
            }
            MouseEventKind::Moved => {
                if let HoveringAreaType::Track(track_index) = hovered {
                    let (area_type, area) = &layout.areas[track_index];
                    match area_type {
                        AreaType::Alignment(id) => {
                            let index = layout.track_registry.alignment_index(*id)?;
                            if let (Some((left_coordinate, right_coordinate)), Some(y_coordinate)) = (
                                alignment_view.coordinates_of_onscreen_x(event.column, area),
                                alignment_view.coordinate_of_onscreen_y(index, event.row, area),
                            ) && let Some(alignment) = state.alignments.get(index)
                                && let Some(read_id) =
                                    if let Some(paired) = &state.paired_alignments[index] {
                                        paired.read_overlapping(
                                            alignment,
                                            left_coordinate,
                                            right_coordinate,
                                            y_coordinate,
                                        )?
                                    } else {
                                        alignment.read_overlapping(
                                            left_coordinate,
                                            right_coordinate,
                                            y_coordinate,
                                        )?
                                    }
                            {
                                let record = &alignment.records[read_id];
                                let name = record
                                    .name()
                                    .map(|name| name.to_string())
                                    .unwrap_or_else(|| "<missing>".into());
                                let mapq = record
                                    .mapping_quality()
                                    .map(|quality| quality.get().to_string())
                                    .unwrap_or_else(|| ".".into());
                                let mut cigar = Vec::new();
                                noodles::sam::io::writer::record::write_cigar(
                                    &mut cigar,
                                    record.cigar(),
                                )?;
                                messages.push(Action::message(format!(
                                    "{}  Flags={}  MAPQ={}  Cigar={}",
                                    name,
                                    u16::from(record.flags()),
                                    mapq,
                                    String::from_utf8(cigar)?
                                )));
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
