use crate::{
    layout::{AreaType, PixelView, Rect, ResolvedMainLayout},
    mouse::MouseState,
    rendering::{
        CHAR_WIDTH, LINE_HEIGHT, TextContext, TrackFocus,
        colors::{MUTED_TEXT, Palette, SEPARATOR, TEXT, ToHsla},
        fill_rect,
    },
    worker::Frame,
};
use gpui::{App, Window};
use gv_viewer::{
    message::FocusTarget,
    prepare::sidebar::{alignment_depth_description, describe_option},
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
    frame: &Frame,
    layout: &ResolvedMainLayout,
    view: &PixelView,
    mouse_state: &MouseState,
    focus: &TrackFocus,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    if layout.sidebar_width == 0.0 {
        return Ok(());
    }
    let state = &frame.state;
    let width = layout.sidebar_width - 2.0 * PADDING;

    // The focused target's whole section is filled, colored by mode.
    let fill = match frame.registers.shown_mode() {
        gv_viewer::keys::Mode::Insert(_) => palette.FOCUS_INSERT,
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
            fill_rect(*section, fill.hsla(), window);
        }
        if matches!(
            area_type,
            AreaType::Coverage(_) | AreaType::Variant(_) | AreaType::Bed(_)
        ) {
            fill_rect(
                Rect::new(section.x, section.bottom() - 1.0, section.width, 1.0),
                SEPARATOR.hsla(),
                window,
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
                TextContext::default()
                    .text(&name)
                    .color(TEXT)
                    .bold()
                    .paint(area.x + PADDING, y, LINE_HEIGHT, window, cx)?;
            } else {
                TextContext::default().text(&name).color(TEXT).paint(
                    area.x + PADDING,
                    y,
                    LINE_HEIGHT,
                    window,
                    cx,
                )?;
            }
            y += LINE_HEIGHT;
            for option in &state.alignment_options[index] {
                if y + 2.0 * LINE_HEIGHT > area.bottom() {
                    break;
                }
                TextContext::default()
                    .text(&fit(&describe_option(option), width))
                    .color(palette.SIDEBAR_OPTION_COLOR)
                    .paint(area.x + PADDING, y, LINE_HEIGHT, window, cx)?;
                y += LINE_HEIGHT;
            }
            if area.height >= 2.0 * LINE_HEIGHT + PADDING {
                TextContext::default()
                    .text(&fit(
                        &alignment_depth_description(state, &view.viewport, index)?,
                        width,
                    ))
                    .color(MUTED_TEXT)
                    .paint(
                        area.x + PADDING,
                        area.bottom() - LINE_HEIGHT - PADDING,
                        LINE_HEIGHT,
                        window,
                        cx,
                    )?;
            }
        } else {
            let y = area.y + (area.height - LINE_HEIGHT) / 2.0;
            if focused {
                TextContext::default()
                    .text(&name)
                    .color(TEXT)
                    .bold()
                    .paint(area.x + PADDING, y, LINE_HEIGHT, window, cx)?;
            } else {
                TextContext::default().text(&name).color(TEXT).paint(
                    area.x + PADDING,
                    y,
                    LINE_HEIGHT,
                    window,
                    cx,
                )?;
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
                TextContext::default()
                    .text(&fit(&locus, width))
                    .color(TEXT)
                    .paint(
                        section.x + PADDING,
                        section.y + (section.height - LINE_HEIGHT) / 2.0,
                        LINE_HEIGHT,
                        window,
                        cx,
                    )?;
            }
            // The reference's section names the genome beside the sequence and the genes.
            AreaType::Sequence | AreaType::GeneTrack => {
                let y = section.y + (section.height - LINE_HEIGHT) / 2.0;
                let name = fit(&state.reference.to_string(), width);
                if focus.target == FocusTarget::Reference {
                    TextContext::default()
                        .text(&name)
                        .color(TEXT)
                        .bold()
                        .paint(section.x + PADDING, y, LINE_HEIGHT, window, cx)?;
                } else {
                    TextContext::default().text(&name).color(TEXT).paint(
                        section.x + PADDING,
                        y,
                        LINE_HEIGHT,
                        window,
                        cx,
                    )?;
                }
            }
            _ => {}
        }
    }

    let divider = if mouse_state.is_sidebar_divider_highlighted() {
        palette.HIGHLIGHT_COLOR
    } else {
        SEPARATOR
    };
    fill_rect(layout.sidebar_divider_area, divider.hsla(), window);
    Ok(())
}
