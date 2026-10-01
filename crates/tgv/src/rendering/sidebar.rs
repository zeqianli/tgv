use crate::{
    app::RenderEvent,
    layout::{AlignmentView, AreaType, ResolvedMainLayout, wrap_sidebar_label},
    mouse::MouseRegister,
    register::Registers,
    rendering::colors::Palette,
};

use gv_core::{message::AlignmentDisplayOption, prelude::*};
use itertools::Itertools;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

fn alignment_depth_description(
    state: &State,
    alignment_view: &AlignmentView,
    index: usize,
) -> String {
    let depth = state.alignments[index].depth();
    let mut description = if depth == 0 {
        "0% (0 / 0)".to_string()
    } else {
        let y = usize::min(alignment_view.top(index), depth.saturating_sub(1)) + 1;
        format!("{}% ({} / {})", y as u128 * 100 / depth as u128, y, depth)
    };
    if !state.alignment_options[index].is_empty() {
        let options = state.alignment_options[index]
            .iter()
            .map(|option| format!("{option}"))
            .join(",");
        description.push_str(&format!(" ({options})"));
    }
    description
}

pub fn render_sidebar(
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
        for (area, id) in &layout.sidebar_alignment_depths {
            let index = layout.track_registry.alignment_index(*id)?;
            buf.set_stringn(
                area.x,
                area.y,
                alignment_depth_description(state, alignment_view, index),
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
    Ok(())
}
