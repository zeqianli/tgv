mod alignment;
mod bed;
mod colors;
mod console;
mod contig_list;
mod coordinate;
mod coverage;
mod cytoband;
mod help;
mod intervals;
mod sequence;
mod status_bar;
mod track;
mod variants;
pub use alignment::{render_alignment, render_paired_alignment};
pub use bed::render_bed;
pub use colors::{DARK_THEME, Palette};
pub use console::render_console;
pub use contig_list::render_contig_list;
pub use coordinate::render_coordinates;
pub use coverage::render_coverage;
pub use cytoband::render_cytobands;
pub use help::render_help;
pub use sequence::render_sequence;
pub use status_bar::render_status_bar;
pub use track::render_track;
pub use variants::render_variants;

use crate::{
    app::RenderEvent,
    layout::{AlignmentView, AreaType, ResolvedMainLayout, wrap_sidebar_label},
    mouse::MouseRegister,
    register::{KeyRegisterType, Registers},
};

use gv_core::{error::TGVError, message::AlignmentDisplayOption, state::State};
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
    pallete: &Palette,
    render_events: &Vec<RenderEvent>,
) -> Result<(), TGVError> {
    let sidebar_style = Style::default().fg(Color::Gray);
    if layout.sidebar_width > 0 {
        let separator_style = Style::default().fg(Color::DarkGray);
        for area in &layout.sidebar_section_dividers {
            buf.set_stringn(
                area.x,
                area.y,
                "_".repeat(area.width as usize),
                area.width as usize,
                separator_style,
            );
        }
        for (area, label) in &layout.sidebar_labels {
            let lines = wrap_sidebar_label(label, area.width);
            let visible_lines = lines.len().min(area.height as usize);
            let first_row = area.y + (area.height as usize - visible_lines) as u16 / 2;
            for (offset, line) in lines.iter().take(visible_lines).enumerate() {
                buf.set_stringn(
                    area.x,
                    first_row + offset as u16,
                    line,
                    area.width as usize,
                    sidebar_style,
                );
            }
        }
        for (index, (area_type, _)) in layout.areas.iter().enumerate() {
            let area = layout.sidebar_areas[index];
            if area.width == 0 || area.height == 0 {
                continue;
            }
            match area_type {
                AreaType::Cytoband => {
                    buf.set_stringn(
                        area.x,
                        area.y,
                        state.reference.to_string(),
                        area.width as usize,
                        sidebar_style,
                    );
                }
                AreaType::Coordinate => {
                    buf.set_stringn(
                        area.x,
                        area.y,
                        format!(
                            "{}:{}",
                            state.contig_name(&alignment_view.focus)?,
                            alignment_view.focus.position
                        ),
                        area.width as usize,
                        sidebar_style,
                    );
                }
                _ => {}
            }
        }
        for (area, index) in &layout.sidebar_alignment_depths {
            buf.set_stringn(
                area.x,
                area.y,
                status_bar::alignment_depth_description(state, alignment_view, *index),
                area.width as usize,
                sidebar_style,
            );
        }
        let divider_style = if mouse_register.is_sidebar_divider_highlighted() {
            Style::default().fg(pallete.HIGHLIGHT_COLOR)
        } else {
            sidebar_style
        };
        for y in layout.sidebar_divider_area.top()..layout.sidebar_divider_area.bottom() {
            buf.set_string(layout.sidebar_divider_area.x, y, "│", divider_style);
        }
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

        match area_type {
            AreaType::Cytoband => render_cytobands(rect, buf, state, alignment_view, pallete)?,
            AreaType::Coordinate => render_coordinates(rect, buf, alignment_view, state)?,
            AreaType::Coverage(index) => {
                if alignment_view.zoom <= AlignmentView::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS
                    && let Some(alignment) = state.alignments.get(*index)
                {
                    render_coverage(rect, buf, alignment, alignment_view, pallete)?;
                }
            }
            AreaType::Alignment(index) => {
                if alignment_view.zoom <= AlignmentView::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS {
                    if state.alignment_options[*index]
                        .contains(&AlignmentDisplayOption::ViewAsPairs)
                    {
                        let paired_alignment = state.paired_alignments[*index].as_mut().ok_or(
                            TGVError::StateError(
                                format!("Paired alignment {index} not yet calculated at rendering")
                                    .to_string(),
                            ),
                        )?;

                        render_paired_alignment(
                            *index,
                            rect,
                            buf,
                            &mut state.alignments[*index],
                            alignment_view,
                            paired_alignment,
                            &state.sequence,
                            pallete,
                        )?;
                    } else {
                        render_alignment(
                            *index,
                            rect,
                            buf,
                            &mut state.alignments[*index],
                            alignment_view,
                            &state.sequence,
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
                if registers.current == KeyRegisterType::Command {
                    render_console(rect, buf, registers)?;
                }
            }
            AreaType::Error => {
                render_status_bar(rect, buf, state);
            }
            AreaType::Variant(index) => {
                if let Some(variants) = state.variants.get(*index) {
                    render_variants(rect, buf, variants, alignment_view, pallete)?;
                }
            }
            AreaType::Bed(index) => {
                if let Some(bed_intervals) = state.bed_intervals.get(*index) {
                    render_bed(rect, buf, bed_intervals, alignment_view, pallete)?;
                }
            }
            AreaType::Fill => {}
        };
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
