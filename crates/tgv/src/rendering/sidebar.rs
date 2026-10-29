use crate::rendering::colors::ToColor;
use crate::{
    layout::{AlignmentView, AreaType, ResolvedMainLayout, wrap_sidebar_label},
    mouse::MouseRegister,
    rendering::{TrackFocus, colors::Palette},
};
use gv_viewer::prepare::sidebar::{alignment_depth_description, describe_option};
use gv_viewer::{layout::RenderEvent, message::FocusTarget, register::Registers};

use gv_core::prelude::*;
use itertools::Itertools;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

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
    focus: &TrackFocus,
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
        let option_style = Style::default().fg(pallete.SIDEBAR_OPTION_COLOR.color());
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
            let label_style = match label.area_type {
                AreaType::Coverage(id) | AreaType::Variant(id) | AreaType::Bed(id)
                    if focus.target == FocusTarget::Track(id) =>
                {
                    sidebar_style.add_modifier(Modifier::BOLD)
                }
                _ => sidebar_style,
            };
            let lines = wrap_sidebar_label(&label.name, area.width);
            let visible_lines = lines.len().min(area.height as usize);
            let first_row = area.y + (area.height as usize - visible_lines) as u16 / 2;
            for (offset, line) in lines.iter().take(visible_lines).enumerate() {
                buf.set_stringn(
                    area.x,
                    first_row + offset as u16,
                    line,
                    area.width as usize,
                    label_style,
                );
            }
        }
        for (index, (area_type, _)) in layout.areas.iter().enumerate() {
            let area = layout.sidebar_areas[index];
            if area.width == 0 || area.height == 0 {
                continue;
            }
            match area_type {
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
            Style::default().fg(pallete.HIGHLIGHT_COLOR.color())
        } else {
            sidebar_style
        };
        for y in layout.sidebar_divider_area.top()..layout.sidebar_divider_area.bottom() {
            buf.set_string(layout.sidebar_divider_area.x, y, "│", divider_style);
        }
        // The reference's section names the genome beside the sequence and the genes.
        if let Some((_, section)) = layout
            .sidebar_sections
            .iter()
            .find(|(area_type, _)| matches!(area_type, AreaType::Sequence | AreaType::GeneTrack))
            && section.height > 0
        {
            let style = match focus.target {
                FocusTarget::Reference => sidebar_style.add_modifier(Modifier::BOLD),
                FocusTarget::Track(_) => sidebar_style,
            };
            buf.set_stringn(
                section.x,
                section.y + (section.height - 1) / 2,
                state.reference.to_string(),
                section.width as usize,
                style,
            );
        }
        // The focused target's whole section is filled, colored by mode.
        let fill = match registers.shown_mode() {
            gv_viewer::keys::Mode::Insert(_) => pallete.FOCUS_INSERT.color(),
            _ => pallete.FOCUS_NORMAL.color(),
        };
        for (area_type, section) in &layout.sidebar_sections {
            let focused = match (area_type, focus.target) {
                (AreaType::Sequence | AreaType::GeneTrack, FocusTarget::Reference) => true,
                (
                    AreaType::Coverage(id) | AreaType::Variant(id) | AreaType::Bed(id),
                    FocusTarget::Track(focused),
                ) => *id == focused,
                _ => false,
            };
            if focused {
                buf.set_style(*section, Style::default().bg(fill));
            }
        }
    }
    Ok(())
}
