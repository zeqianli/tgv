use crate::{
    layout::{AreaType, PixelView, Rect, ResolvedMainLayout},
    mouse::MouseRegister,
    rendering::{
        CHAR_WIDTH, LINE_HEIGHT, Painter, TrackFocus,
        colors::{MUTED_TEXT, Palette, SEPARATOR, TEXT, ToHsla},
    },
    worker::Frame,
};
use gv_core::prelude::*;
use gv_viewer::{
    message::FocusTarget,
    prepare::sidebar::{alignment_depth_description, describe_option},
    register::KeyRegisterType,
};

const PADDING: f32 = 6.0;

/// Cuts text to fit a width, ending it with `…`.
fn fit(text: &str, width: f32) -> String {
    let characters = (width / CHAR_WIDTH).floor().max(0.0) as usize;
    if text.chars().count() <= characters {
        text.to_string()
    } else {
        let mut text: String = text.chars().take(characters.saturating_sub(1)).collect();
        text.push('…');
        text
    }
}

/// Draws the track labels, the focus, the locus, and each alignment's depth and options.
pub fn render_sidebar(
    painter: &mut Painter,
    frame: &Frame,
    layout: &ResolvedMainLayout,
    view: &PixelView,
    mouse_register: &MouseRegister,
    focus: &TrackFocus,
    palette: &Palette,
) -> Result<(), TGVError> {
    if layout.sidebar_width == 0.0 {
        return Ok(());
    }
    let state = &frame.state;
    let width = layout.sidebar_width - 2.0 * PADDING;

    // The focused target's whole section is filled, colored by mode.
    let fill = match frame.registers.shown_mode() {
        KeyRegisterType::Insert { .. } => palette.FOCUS_INSERT,
        _ => palette.FOCUS_NORMAL,
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
            painter.fill(*section, fill.hsla());
        }
        if matches!(
            area_type,
            AreaType::Coverage(_) | AreaType::Variant(_) | AreaType::Bed(_)
        ) {
            painter.fill(
                Rect::new(section.x, section.bottom() - 1.0, section.width, 1.0),
                SEPARATOR.hsla(),
            );
        }
    }

    for label in &layout.sidebar_labels {
        let area = label.area;
        let focused = match label.area_type {
            AreaType::Coverage(id) | AreaType::Variant(id) | AreaType::Bed(id) => {
                focus.target == FocusTarget::Track(id)
            }
            _ => false,
        };
        let name = fit(&label.name, width);
        if let AreaType::Coverage(id) = label.area_type {
            // An alignment's name goes at the top, its display options below, and its depth at
            // the bottom.
            let index = layout.track_registry.alignment_index(id)?;
            let mut y = area.y + PADDING;
            if focused {
                painter.bold_text(area.x + PADDING, y, &name, TEXT);
            } else {
                painter.text(area.x + PADDING, y, &name, TEXT);
            }
            y += LINE_HEIGHT;
            for option in &state.alignment_options[index] {
                if y + 2.0 * LINE_HEIGHT > area.bottom() {
                    break;
                }
                painter.text(
                    area.x + PADDING,
                    y,
                    &fit(&describe_option(option), width),
                    palette.SIDEBAR_OPTION_COLOR,
                );
                y += LINE_HEIGHT;
            }
            if area.height >= 2.0 * LINE_HEIGHT + PADDING {
                painter.text(
                    area.x + PADDING,
                    area.bottom() - LINE_HEIGHT - PADDING,
                    &fit(
                        &alignment_depth_description(state, &view.viewport, index)?,
                        width,
                    ),
                    MUTED_TEXT,
                );
            }
        } else {
            let y = area.y + (area.height - LINE_HEIGHT) / 2.0;
            if focused {
                painter.bold_text(area.x + PADDING, y, &name, TEXT);
            } else {
                painter.text(area.x + PADDING, y, &name, TEXT);
            }
        }
    }

    for (area_type, section) in &layout.sidebar_sections {
        match area_type {
            AreaType::Cytoband | AreaType::Coordinate => {
                let locus = format!(
                    "{}:{}",
                    state.contig_name(&view.viewport.focus)?,
                    view.viewport.focus.position
                );
                painter.text(
                    section.x + PADDING,
                    section.y + (section.height - LINE_HEIGHT) / 2.0,
                    &fit(&locus, width),
                    TEXT,
                );
            }
            // The reference's section names the genome beside the sequence and the genes.
            AreaType::Sequence | AreaType::GeneTrack => {
                let y = section.y + (section.height - LINE_HEIGHT) / 2.0;
                let name = fit(&state.reference.to_string(), width);
                if focus.target == FocusTarget::Reference {
                    painter.bold_text(section.x + PADDING, y, &name, TEXT);
                } else {
                    painter.text(section.x + PADDING, y, &name, TEXT);
                }
            }
            _ => {}
        }
    }

    let divider = if mouse_register.is_sidebar_divider_highlighted() {
        palette.HIGHLIGHT_COLOR
    } else {
        SEPARATOR
    };
    painter.fill(layout.sidebar_divider_area, divider.hsla());
    Ok(())
}
