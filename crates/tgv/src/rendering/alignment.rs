use gv_core::{
    alignment::{RenderingContext, RenderingContextKind, RenderingContextModifier},
    error::TGVError,
    message::AlignmentDisplayOption,
    rendering::colors::Palette,
    states::State,
    window::{OnScreenCoordinate, ViewingWindow},
};
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

/// Render an alignment on the alignment area.
pub fn render_alignment(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    pallete: &Palette,
) -> Result<(), TGVError> {
    if area.height < 1 {
        return Ok(());
    }

    let display_as_pairs = state
        .alignment_options
        .iter()
        .any(|option| *option == AlignmentDisplayOption::ViewAsPairs);
    if display_as_pairs && state.alignment.read_pairs.is_none() {
        return Err(TGVError::StateError(
            "Read pairs are not calculated before rendering.".to_string(),
        ));
    }

    if display_as_pairs {
        state
            .alignment
            .read_pairs
            .as_ref()
            .unwrap()
            .iter()
            .zip(state.alignment.show_pairs.as_ref().unwrap().iter())
            .try_for_each(|(read_pair, show_pair)| {
                if *show_pair {
                    let y = state.alignment.ys[read_pair.read_1_index];
                    read_pair.rendering_contexts.iter().try_for_each(|context| {
                        render_contexts(context, y, buf, state, area, pallete)
                    })
                } else {
                    Ok(())
                }
            })?;
    } else {
        state
            .alignment
            .ys_index
            .iter()
            .enumerate()
            .try_for_each(|(y, read_indexes)| {
                read_indexes.iter().try_for_each(|read_index| {
                    state.alignment.reads[*read_index]
                        .rendering_contexts
                        .iter()
                        .try_for_each(|context| {
                            render_contexts(context, y, buf, state, area, pallete)
                        })
                })
            })?
    };
    Ok(())
}

fn render_contexts(
    context: &RenderingContext,
    y: usize,
    buf: &mut Buffer,
    state: &State,
    area: &Rect,
    pallete: &Palette,
) -> Result<(), TGVError> {
    if let Some(onscreen_contexts) =
        get_read_rendering_info(context, y, &state.window, area, pallete)?
    {
        for onscreen_context in onscreen_contexts {
            buf.set_string(
                area.x + onscreen_context.x,
                area.y + onscreen_context.y,
                onscreen_context.string,
                onscreen_context.style,
            );
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
    pallete: &Palette,
) -> Result<Option<Vec<OnScreenRenderingContext>>, TGVError> {
    let onscreen_y = match viewing_window.onscreen_y_coordinate(y, area) {
        OnScreenCoordinate::OnScreen(y_start) => y_start as u16,
        _ => return Ok(None),
    };

    let start_onscreen_coordinate = viewing_window.onscreen_x_coordinate(context.start, area);
    let end_onscreen_coordinate = viewing_window.onscreen_x_coordinate(context.end, area);

    let (onscreen_x, length) = match OnScreenCoordinate::onscreen_start_and_length(
        &start_onscreen_coordinate,
        &end_onscreen_coordinate,
        area,
    ) {
        Some((onscreen_x, length)) => (onscreen_x, length),
        None => return Ok(None),
    };

    let mut output = Vec::new();

    match context.kind {
        RenderingContextKind::Match => output.push(OnScreenRenderingContext {
            x: onscreen_x,
            y: onscreen_y,
            string: "-".repeat(length as usize),
            style: Style::default()
                .bg(pallete.MATCH_COLOR)
                .fg(pallete.MATCH_FG_COLOR),
        }),

        RenderingContextKind::Deletion => output.push(OnScreenRenderingContext {
            x: onscreen_x,
            y: onscreen_y,
            string: "-".repeat(length as usize),
            style: Style::new()
                .bg(pallete.background)
                .fg(pallete.DELETION_COLOR),
        }),

        RenderingContextKind::SoftClip(base) => output.push(OnScreenRenderingContext {
            x: onscreen_x,
            y: onscreen_y,
            string: String::from_utf8(vec![base])?,
            style: Style::default().bg(pallete.softclip_color(base)),
        }),

        RenderingContextKind::PairGap => output.push(OnScreenRenderingContext {
            x: onscreen_x,
            y: onscreen_y,
            string: "-".repeat(length as usize),
            style: Style::new()
                .bg(pallete.background)
                .fg(pallete.PAIRGAP_COLOR),
        }),

        RenderingContextKind::PairOverlap => output.push(OnScreenRenderingContext {
            x: onscreen_x,
            y: onscreen_y,
            string: "-".repeat(length as usize),
            style: Style::new()
                .bg(pallete.background)
                .fg(pallete.PAIR_OVERLAP_COLOR),
        }),
    }

    // Modifers
    for modifier in context.modifiers.iter() {
        match modifier {
            RenderingContextModifier::Forward => {
                if let OnScreenCoordinate::OnScreen(x) = end_onscreen_coordinate {
                    output.push(OnScreenRenderingContext {
                        x: x as u16,
                        y: onscreen_y,
                        string: "►".to_string(),
                        style: output.first().unwrap().style,
                    })
                }
            }

            RenderingContextModifier::Reverse => {
                if let OnScreenCoordinate::OnScreen(x) = start_onscreen_coordinate {
                    output.push(OnScreenRenderingContext {
                        x: x as u16,
                        y: onscreen_y,
                        string: "◄".to_string(),
                        style: output.first().unwrap().style,
                    })
                }
            }

            RenderingContextModifier::Insertion(l) => {
                if let OnScreenCoordinate::OnScreen(x) = start_onscreen_coordinate {
                    output.push(OnScreenRenderingContext {
                        x: x as u16,
                        y: onscreen_y,
                        string: "▌".to_string(),
                        style: Style::default().fg(pallete.INSERTION_COLOR),
                    })
                }
            }

            RenderingContextModifier::Mismatch(coordinate, base) => {
                if let OnScreenCoordinate::OnScreen(modifier_onscreen_x) =
                    viewing_window.onscreen_x_coordinate(*coordinate, area)
                {
                    output.push(OnScreenRenderingContext {
                        x: modifier_onscreen_x as u16,
                        y: onscreen_y,
                        string: String::from_utf8(vec![*base])?,
                        style: output
                            .first()
                            .unwrap()
                            .style
                            .fg(pallete.mismatch_color(*base)),
                    })
                }
            }

            RenderingContextModifier::PairConflict(coordinate) => {
                if let OnScreenCoordinate::OnScreen(modifier_onscreen_x) =
                    viewing_window.onscreen_x_coordinate(*coordinate, area)
                {
                    output.push(OnScreenRenderingContext {
                        x: modifier_onscreen_x as u16,
                        y: onscreen_y,
                        string: "?".to_string(),
                        style: output.first().unwrap().style,
                    })
                }
            }
        }
    }

    Ok(Some(output))
}
