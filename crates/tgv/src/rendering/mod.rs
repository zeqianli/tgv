mod alignment;
mod bed;
mod colors;
mod console;
mod contig_list;
mod coordinate;
mod coverage;
pub(crate) mod cytoband;
mod intervals;
mod menu;
mod sequence;
mod sidebar;
mod status_bar;
mod track;
mod variants;
use std::collections::BTreeSet;

pub use alignment::{render_alignment, render_paired_alignment};
pub use bed::render_bed;
pub use colors::{DARK_THEME, Palette};
pub use console::render_console;
pub use contig_list::render_contig_list;
pub use coordinate::render_coordinates;
pub use coverage::render_coverage;
pub use cytoband::render_cytobands;
pub use sequence::render_sequence;
pub use status_bar::render_status_bar;
pub use track::render_track;
pub use variants::render_variants;

use crate::{
    app::{Highlight, RenderEvent},
    layout::{AlignmentView, AreaType, OnScreenCoordinate, ResolvedMainLayout, wrap_sidebar_label},
    menu::ContextMenu,
    mouse::MouseRegister,
    popup::TextPopup,
    register::{KeyRegisterType, Registers},
};

use gv_core::{message::AlignmentDisplayOption, prelude::*};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

/// Render all areas in the layout
pub fn render_main(
    buf: &mut Buffer,
    state: &mut State,
    registers: &Registers,
    layout: &ResolvedMainLayout,
    alignment_view: &AlignmentView,
    mouse_register: &MouseRegister,
    highlights: &[Highlight],
    context_menu: Option<&ContextMenu>,
    popup: Option<&TextPopup>,
    pallete: &Palette,
    render_events: &Vec<RenderEvent>,
) -> Result<(), TGVError> {
    // Expand render events
    let mut render_areas = Vec::new();
    let mut render_side_bar = false;

    // TODO: This chunk of code is is really bad.
    for event in render_events.iter() {
        match event {
            RenderEvent::All => {
                render_side_bar = true;
                for (area_type, _rect) in layout.areas.iter() {
                    if !render_areas.contains(area_type) {
                        render_areas.push(area_type.clone());
                    }
                }
            }

            RenderEvent::AllTracks => {
                for (area_type, _rect) in layout.areas.iter() {
                    if !render_areas.contains(area_type) {
                        render_areas.push(area_type.clone());
                    }
                }
            }

            RenderEvent::Sidebar => {
                render_side_bar = true;
            }

            RenderEvent::Area(area_type) => {
                if !render_areas.contains(area_type) {
                    render_areas.push(area_type.clone());
                }
            }
        }
    }

    if render_side_bar {
        let sidebar = Rect::new(
            layout.terminal_area.x,
            layout.terminal_area.y,
            layout.main_area.x.saturating_sub(layout.terminal_area.x),
            layout.terminal_area.height,
        )
        .intersection(buf.area);
        for y in sidebar.top()..sidebar.bottom() {
            for x in sidebar.left()..sidebar.right() {
                buf[(x, y)].reset();
            }
        }
        sidebar::render_sidebar(
            buf,
            state,
            registers,
            layout,
            alignment_view,
            mouse_register,
            pallete,
            render_events,
        )?;
    }

    // Render each area based on its type
    for (area_type, rect) in layout.areas.iter() {
        if rect.width == 0
            || rect.height == 0
            || rect.y >= buf.area.bottom()
            || rect.x >= buf.area.right()
        {
            continue;
        }

        // FIXME: this is bad code
        if !render_areas.contains(area_type) {
            continue;
        }

        // Invalidated areas may render less content than they did in the previous frame.
        let clear_area = rect.intersection(buf.area);
        for y in clear_area.top()..clear_area.bottom() {
            for x in clear_area.left()..clear_area.right() {
                buf[(x, y)].reset();
            }
        }

        match area_type {
            AreaType::Cytoband => render_cytobands(rect, buf, state, alignment_view, pallete)?,
            AreaType::Coordinate => {
                render_coordinates(rect, buf, alignment_view, state)?;
                render_highlights(rect, buf, alignment_view, highlights, pallete);
            }
            AreaType::Coverage(id) => {
                let index = layout.track_registry.alignment_index(*id)?;
                if alignment_view.zoom <= AlignmentView::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS
                    && let Some(alignment) = state.alignments.get(index)
                {
                    render_coverage(rect, buf, &alignment.coverage, alignment_view, pallete)?;
                }
            }
            AreaType::Alignment(id) => {
                let index = layout.track_registry.alignment_index(*id)?;
                if alignment_view.zoom <= AlignmentView::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS {
                    let top = alignment_view.top(index);
                    if state.alignment_options[index].contains(&AlignmentDisplayOption::ViewAsPairs)
                    {
                        let paired_alignment =
                            state.paired_alignments[index]
                                .as_mut()
                                .ok_or(TGVError::StateError(
                                    format!(
                                        "Paired alignment {index} not yet calculated at rendering"
                                    )
                                    .to_string(),
                                ))?;

                        render_paired_alignment(
                            top,
                            rect,
                            buf,
                            &state.alignments[index],
                            alignment_view,
                            paired_alignment,
                            pallete,
                        )?;
                    } else {
                        render_alignment(
                            top,
                            rect,
                            buf,
                            &state.alignments[index],
                            alignment_view,
                            pallete,
                        )?;
                    }
                }
            }
            AreaType::AlignmentDivider { .. } => render_alignment_divider(
                rect,
                buf,
                pallete,
                mouse_register.is_divider_highlighted(area_type),
            ),
            AreaType::Sequence => {
                if alignment_view.zoom <= AlignmentView::MAX_ZOOM_TO_DISPLAY_SEQUENCES {
                    render_sequence(rect, buf, state, alignment_view, pallete)?;
                }
            }
            AreaType::GeneTrack => {
                render_track(rect, buf, state, alignment_view, pallete)?;
            }
            AreaType::Console => {
                if matches!(
                    registers.current,
                    KeyRegisterType::Command | KeyRegisterType::Search
                ) {
                    render_console(rect, buf, registers)?;
                }
            }
            AreaType::Error => {
                render_status_bar(rect, buf, state);
            }
            AreaType::Variant(id) => {
                let index = layout.track_registry.variant_index(*id)?;
                if let Some(variants) = state.variants.get(index) {
                    render_variants(rect, buf, variants, alignment_view, pallete)?;
                }
            }
            AreaType::Bed(id) => {
                let index = layout.track_registry.bed_index(*id)?;
                if let Some(bed_intervals) = state.bed_intervals.get(index) {
                    render_bed(rect, buf, bed_intervals, alignment_view, pallete)?;
                }
            }
            AreaType::Fill => {}
        };
    }

    // Track redraws may have overwritten part of the menu, so it is drawn on every frame.
    if let Some(menu) = context_menu {
        menu::render_context_menu(buf, menu, pallete);
    }
    if let Some(popup) = popup {
        menu::render_text_popup(buf, layout.terminal_area, popup, pallete);
    }
    Ok(())
}

fn render_alignment_divider(area: &Rect, buf: &mut Buffer, palette: &Palette, highlighted: bool) {
    let style = if highlighted {
        Style::default().bg(palette.HIGHLIGHT_COLOR)
    } else {
        Style::default()
    };

    for y in area.top()..area.bottom() {
        buf.set_string(area.x, y, "-".repeat(area.width as usize), style);
    }
}

pub fn get_abbreviated_length_string(length: u64) -> String {
    let mut length = length;
    let mut power = 0;

    while length >= 1000 {
        length /= 1000;
        power += 1;
    }

    format!(
        "{}{}",
        length,
        match power {
            0 => "bp",
            1 => "kb",
            2 => "Mb",
            3 => "Gb",
            4 => "Tb",
            _ => "",
        }
    )
}

/// Tints the columns of highlighted intervals on the focused contig, keeping the ruler text.
fn render_highlights(
    area: &Rect,
    buf: &mut Buffer,
    alignment_view: &AlignmentView,
    highlights: &[Highlight],
    pallete: &Palette,
) {
    for highlight in highlights
        .iter()
        .filter(|highlight| highlight.contig_index == alignment_view.focus.contig_index)
    {
        let start = alignment_view.onscreen_x_coordinate(highlight.start, area);
        let end = alignment_view.onscreen_x_coordinate(highlight.end, area);
        if let Some((x, width)) = OnScreenCoordinate::onscreen_start_and_length(&start, &end, area)
        {
            buf.set_style(
                Rect::new(area.x + x, area.y, width, area.height),
                Style::default().bg(pallete.HIGHLIGHT),
            );
        }
    }
}
