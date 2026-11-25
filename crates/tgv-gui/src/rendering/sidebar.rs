use super::prelude::*;
use crate::{
    layout::{AreaType, ResolvedMainLayout},
    mouse::MouseState,
    rendering::{TrackFocus, colors::SEPARATOR},
    worker::Frame,
};
use gv_viewer::{
    message::FocusTarget,
    prepare::sidebar::{depth_description, describe_option},
    viewport::RowLayout,
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
    let fill = match frame.keyboard_state.shown_mode() {
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
            fill_rect(*section, to_hsla(fill), window);
        }
        let highlighted = area_type
            .focus_target()
            .is_some_and(|target| mouse_state.is_divider_highlighted(&AreaType::Divider(target)));
        if highlighted {
            fill_rect(
                Rect::new(section.x, section.bottom() - 2.0, section.width, 2.0),
                to_hsla(palette.HIGHLIGHT_COLOR),
                window,
            );
        } else if matches!(
            area_type,
            AreaType::Coverage(_) | AreaType::Variant(_) | AreaType::Bed(_)
        ) {
            fill_rect(
                Rect::new(section.x, section.bottom() - 1.0, section.width, 1.0),
                to_hsla(SEPARATOR),
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
                        &depth_description(
                            state,
                            &layout.track_registry,
                            &view.viewport,
                            FocusTarget::Track(id),
                        )?,
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
            let target = label
                .area_type
                .focus_target()
                .expect("sidebar labels name file tracks");
            let shows_depth = view.viewport.tracks[&target].rows == RowLayout::Expanded
                && area.height >= 2.0 * LINE_HEIGHT + PADDING;
            if shows_depth {
                render_depth(state, layout, view, target, area, width, window, cx)?;
            }
            let y = if shows_depth {
                area.y + PADDING
            } else {
                area.y + (area.height - LINE_HEIGHT) / 2.0
            };
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
                let shows_depth = view.viewport.tracks[&FocusTarget::Reference].rows
                    == RowLayout::Expanded
                    && section.height >= 2.0 * LINE_HEIGHT + PADDING;
                if shows_depth {
                    render_depth(
                        state,
                        layout,
                        view,
                        FocusTarget::Reference,
                        *section,
                        width,
                        window,
                        cx,
                    )?;
                }
                let y = if shows_depth {
                    section.y + PADDING
                } else {
                    section.y + (section.height - LINE_HEIGHT) / 2.0
                };
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
    fill_rect(layout.sidebar_divider_area, to_hsla(divider), window);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_depth(
    state: &gv_core::state::State,
    layout: &ResolvedMainLayout,
    view: &PixelView,
    target: FocusTarget,
    area: Rect,
    width: f32,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    TextContext::default()
        .text(&fit(
            &depth_description(state, &layout.track_registry, &view.viewport, target)?,
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
    Ok(())
}
