use crate::{
    layout::{AlignmentView, MainLayout, OnScreenCoordinate},
    rendering::colors::Palette,
};
use gv_core::{
    alignment::{
        BaseModification, ModificationType, RenderingContext, RenderingContextKind,
        RenderingContextModifier,
    },
    error::TGVError,
    message::AlignmentDisplayOption,
    state::State,
};
use ratatui::{buffer::Buffer, layout::Rect, style::{Color, Style}};
use std::collections::HashMap;

/// Render an alignment on the alignment area.
pub fn render_alignment(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    alignment_view: &AlignmentView,
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

    let show_modifications = state
        .alignment_options
        .iter()
        .any(|option| *option == AlignmentDisplayOption::ShowBaseModifications);

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
                        // Paired mode: modifications not supported yet; pass None.
                        render_contexts(context, y, buf, alignment_view, area, pallete, None)
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
                    let read = &state.alignment.reads[*read_index];
                    let mods: Option<&HashMap<u64, Vec<BaseModification>>> =
                        if show_modifications && !read.base_modifications.is_empty() {
                            Some(&read.base_modifications)
                        } else {
                            None
                        };
                    read.rendering_contexts.iter().try_for_each(|context| {
                        render_contexts(context, y, buf, alignment_view, area, pallete, mods)
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
    alignment_view: &AlignmentView,
    area: &Rect,
    pallete: &Palette,
    base_modifications: Option<&HashMap<u64, Vec<BaseModification>>>,
) -> Result<(), TGVError> {
    if let Some(onscreen_contexts) =
        get_read_rendering_info(context, y, alignment_view, area, pallete, base_modifications)?
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

/// Return the background color for a reference position from the modification map.
/// Returns `None` when the position has no modification data.
fn mod_bg_at(
    pos: u64,
    mods: &HashMap<u64, Vec<BaseModification>>,
    pallete: &Palette,
) -> Option<Color> {
    mods.get(&pos).and_then(|mod_list| {
        // Prefer 5mC, then 5hmC, then 6mA.
        mod_list
            .iter()
            .find(|m| matches!(m.modification_type, ModificationType::FiveMC))
            .or_else(|| {
                mod_list
                    .iter()
                    .find(|m| matches!(m.modification_type, ModificationType::FiveHMC))
            })
            .or_else(|| mod_list.first())
            .map(|m| pallete.modification_color(&m.modification_type, m.probability))
    })
}

/// Get rendering info for an aligned read context.
fn get_read_rendering_info(
    context: &RenderingContext,
    y: usize,
    alignment_view: &AlignmentView,
    area: &Rect,
    pallete: &Palette,
    base_modifications: Option<&HashMap<u64, Vec<BaseModification>>>,
) -> Result<Option<Vec<OnScreenRenderingContext>>, TGVError> {
    let onscreen_y = match alignment_view.onscreen_y_coordinate(y, area) {
        OnScreenCoordinate::OnScreen(y_start) => y_start as u16,
        _ => return Ok(None),
    };

    let start_onscreen_coordinate = alignment_view.onscreen_x_coordinate(context.start, area);
    let end_onscreen_coordinate = alignment_view.onscreen_x_coordinate(context.end, area);

    let (onscreen_x, length) = match OnScreenCoordinate::onscreen_start_and_length(
        &start_onscreen_coordinate,
        &end_onscreen_coordinate,
        area,
    ) {
        Some((onscreen_x, length)) => (onscreen_x, length),
        None => return Ok(None),
    };

    let mut output = Vec::new();

    // ── Base context rendering ─────────────────────────────────────────────
    match context.kind {
        RenderingContextKind::Match => {
            if let Some(mods) = base_modifications {
                // Per-position rendering so each cell can have its own
                // modification background colour.
                for pos in context.start..=context.end {
                    let bg = mod_bg_at(pos, mods, pallete).unwrap_or(pallete.MATCH_COLOR);
                    if let OnScreenCoordinate::OnScreen(cell_x) =
                        alignment_view.onscreen_x_coordinate(pos, area)
                    {
                        output.push(OnScreenRenderingContext {
                            x: cell_x as u16,
                            y: onscreen_y,
                            string: "-".to_string(),
                            style: Style::default().bg(bg).fg(pallete.MATCH_FG_COLOR),
                        });
                    }
                }
            } else {
                output.push(OnScreenRenderingContext {
                    x: onscreen_x,
                    y: onscreen_y,
                    string: "-".repeat(length as usize),
                    style: Style::default()
                        .bg(pallete.MATCH_COLOR)
                        .fg(pallete.MATCH_FG_COLOR),
                });
            }
        }

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

    // ── Modifiers ─────────────────────────────────────────────────────────
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

            RenderingContextModifier::Insertion(_l) => {
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
                    alignment_view.onscreen_x_coordinate(*coordinate, area)
                {
                    // When showing modifications, preserve the modification
                    // background at this position while changing only the fg.
                    let base_style = if let Some(mods) = base_modifications {
                        let bg =
                            mod_bg_at(*coordinate, mods, pallete).unwrap_or(pallete.MATCH_COLOR);
                        Style::default()
                            .bg(bg)
                            .fg(pallete.mismatch_color(*base))
                    } else {
                        output
                            .first()
                            .unwrap()
                            .style
                            .fg(pallete.mismatch_color(*base))
                    };
                    output.push(OnScreenRenderingContext {
                        x: modifier_onscreen_x as u16,
                        y: onscreen_y,
                        string: String::from_utf8(vec![*base])?,
                        style: base_style,
                    })
                }
            }

            RenderingContextModifier::PairConflict(coordinate) => {
                if let OnScreenCoordinate::OnScreen(modifier_onscreen_x) =
                    alignment_view.onscreen_x_coordinate(*coordinate, area)
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
