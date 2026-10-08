use crate::{
    app::RenderEvent,
    layout::{AlignmentView, AreaType, ResolvedMainLayout, wrap_sidebar_label},
    mouse::MouseRegister,
    register::Registers,
    rendering::colors::Palette,
};

use gv_core::{
    message::{AlignmentDisplayOption, AlignmentFilter, AlignmentSort},
    prelude::*,
};
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
) -> Result<String, TGVError> {
    let depth = match &state.paired_alignments[index] {
        Some(paired) => paired.depth()?,
        None => state.alignments[index].depth()?,
    };
    Ok(if depth == 0 {
        "0% (0 / 0)".to_string()
    } else {
        let y = usize::min(alignment_view.top(index), depth.saturating_sub(1)) + 1;
        format!("{}% ({} / {})", y as u128 * 100 / depth as u128, y, depth)
    })
}

/// Describe an alignment display option in a few words.
fn describe_option(option: &AlignmentDisplayOption) -> String {
    match option {
        AlignmentDisplayOption::ViewAsPairs => "Paired".to_string(),
        AlignmentDisplayOption::Sort(sort) => match sort {
            AlignmentSort::BaseAt(position) => format!("Sorted by base at {position}"),
            AlignmentSort::StrandAt(position) => format!("Sorted by strand at {position}"),
            AlignmentSort::Start(position) => format!("Sorted by start at {position}"),
            AlignmentSort::MappingQuality(position) => format!("Sorted by MAPQ at {position}"),
            AlignmentSort::InsertSize(position) => {
                format!("Sorted by insert size at {position}")
            }
            AlignmentSort::ReadName(position) => format!("Sorted by name at {position}"),
            sort => format!("Sorted by {sort}"),
        },
        AlignmentDisplayOption::Filter(AlignmentFilter::Base(position, base)) => {
            format!("Only {base} at {position}")
        }
        AlignmentDisplayOption::Filter(AlignmentFilter::BaseSoftclip(position)) => {
            format!("Only soft clips at {position}")
        }
        option => option.to_string(),
    }
}

/// Wrap text at spaces to fit `width`, breaking words that are longer than a line.
fn wrap_words(text: &str, width: u16) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width as usize => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.extend(wrap_sidebar_label(word, width)),
        }
    }
    lines
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
        let option_style = Style::default().fg(pallete.SIDEBAR_OPTION_COLOR);
        for label in &layout.sidebar_labels {
            let mut area = label.area;
            // An alignment's display options sit at the bottom of its label area, just above the
            // depth line. The file name keeps at least one line.
            if let AreaType::Coverage(id) = label.area_type {
                let index = layout.track_registry.alignment_index(id)?;
                let mut option_lines = state.alignment_options[index]
                    .iter()
                    .flat_map(|option| wrap_words(&describe_option(option), area.width))
                    .collect_vec();
                let rows = option_lines
                    .len()
                    .min(area.height.saturating_sub(1) as usize);
                if rows < option_lines.len() && rows > 0 {
                    option_lines.truncate(rows);
                    let last = &mut option_lines[rows - 1];
                    if last.chars().count() >= area.width as usize {
                        last.pop();
                    }
                    last.push('…');
                }
                area.height -= rows as u16;
                for (offset, line) in option_lines.iter().take(rows).enumerate() {
                    buf.set_stringn(
                        area.x,
                        area.bottom() + offset as u16,
                        line,
                        area.width as usize,
                        option_style,
                    );
                }
            }
            let lines = wrap_sidebar_label(&label.name, area.width);
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
                alignment_depth_description(state, alignment_view, index)?,
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
