use crate::{
    alignment::{
        AlignedRead, Alignment, RenderingContext, RenderingContextKind, RenderingContextModifier,
    },
    error::TGVError,
    rendering::colors::Palette,
    states::State,
    window::{OnScreenCoordinate, ViewingWindow},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

/// Render an alignment on the alignment area.
pub fn render_alignment(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    background_color: &Color,
    pallete: &Palette,
) -> Result<(), TGVError> {
    // This iterates through all cached reads and re-calculates coordinates for each movement.
    // Consider improvement.
    if let Some(alignment) = &state.alignment {
        for read in alignment.reads.iter() {
            for context in read.rendering_contexts.iter() {
                if let Some(onscreen_contexts) = get_read_rendering_info(
                    context,
                    read.y,
                    &state.window,
                    area,
                    background_color,
                    pallete,
                ) {
                    for onscreen_context in onscreen_contexts {
                        buf.set_string(
                            area.x + onscreen_context.x,
                            area.y + onscreen_context.y,
                            onscreen_context.string,
                            onscreen_context.style,
                        )
                    }
                }
            }
        }
    }
    Ok(())
}

struct OnScreenRenderingContext {
    x: u16,
    y: u16,

    string: String,

    style: Style,
}

/// Get rendering needs for an aligned read.
/// Returns: x, y,
fn get_read_rendering_info(
    context: &RenderingContext,
    y: usize,
    viewing_window: &ViewingWindow,
    area: &Rect,
    background_color: &Color,
    pallete: &Palette,
) -> Option<Vec<OnScreenRenderingContext>> {
    let onscreen_y = match viewing_window.onscreen_y_coordinate(y, area) {
        OnScreenCoordinate::OnScreen(y_start) => y_start as u16,
        _ => return None,
    };

    let (onscreen_x, length) = match OnScreenCoordinate::onscreen_start_and_length(
        &viewing_window.onscreen_x_coordinate(context.start, area),
        &viewing_window.onscreen_x_coordinate(context.end, area), // Some area for improvements
        area,
    ) {
        Some((onscreen_x, length)) => (onscreen_x, length),
        None => return None,
    };

    let mut output = Vec::new();

    match context.kind {
        RenderingContextKind::Match => output.push(OnScreenRenderingContext {
            x: onscreen_x,
            y: onscreen_y,
            string: "-".repeat(length as usize),
            style: Style::default().bg(pallete.MATCH_COLOR),
        }),

        RenderingContextKind::Deltion => output.push(OnScreenRenderingContext {
            x: onscreen_x,
            y: onscreen_y,
            string: "-".repeat(length as usize),
            style: Style::new()
                .bg(*background_color)
                .fg(pallete.DELETION_COLOR),
        }),

        RenderingContextKind::SoftClip(base) => output.push(OnScreenRenderingContext {
            x: onscreen_x,
            y: onscreen_y,
            string: base.to_string(),
            style: Style::default().bg(pallete.softclip_color(base)),
        }),
    }

    // Modifers

    for modifier in context.modifiers.iter() {
        match modifier {
            RenderingContextModifier::Forward => output.push(OnScreenRenderingContext {
                x: onscreen_x + length - 1,
                y: onscreen_y,
                string: "►".to_string(),
                style: output.last().unwrap().style.clone(),
            }),

            RenderingContextModifier::Reverse => output.push(OnScreenRenderingContext {
                x: onscreen_x,
                y: onscreen_y,
                string: "◄".to_string(),
                style: output.first().unwrap().style.clone(),
            }),

            RenderingContextModifier::Insertion(l) => output.push(OnScreenRenderingContext {
                x: onscreen_x,
                y: onscreen_y,
                string: "▌".to_string(),
                style: Style::default().fg(pallete.INSERTION_COLOR),
            }),

            RenderingContextModifier::Mismatch(coordinate, base) => {
                if let OnScreenCoordinate::OnScreen(modifier_onscreen_x) =
                    viewing_window.onscreen_x_coordinate(*coordinate, area)
                {
                    output.push(OnScreenRenderingContext {
                        x: modifier_onscreen_x as u16,
                        y: onscreen_y,
                        string: base.to_string(),
                        style: output.last().unwrap().style.clone(),
                    })
                }
            }
        }
    }

    Some(output)
}
