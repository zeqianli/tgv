//! Shared interval painting, density summaries, overflow markers, and feature picking.

use crate::layout::{AlignmentView, IntervalMode, OnScreenCoordinate};
use gv_core::intervals::IntervalSchema;
use gv_core::prelude::*;
use polars::prelude::DataFrame;

use ratatui::{buffer::Buffer, layout::Rect, style::Color, style::Style};

/// Says that the track isn't loaded for this view, because the view is zoomed out too far.
pub fn render_zoom_in_notice(area: &Rect, buf: &mut Buffer, features: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    buf.set_stringn(
        area.x,
        area.y,
        format!("zoom in to view {features}"),
        area.width as usize,
        Style::default().fg(Color::DarkGray),
    );
}

/// The format-specific labels and strand glyphs of a packed interval track.
#[derive(Clone, Copy)]
pub enum IntervalLabels {
    Bed,
    Variant,
}

/// The appearance of a packed track, shared across interval file formats.
pub struct IntervalStyle {
    pub mode: gv_core::intervals::IntervalDisplayMode,
    pub colors: [Color; 2],
    pub first_lane: u32,
    pub labels: IntervalLabels,
}

/// The hidden-lane marker for the actual allocated track height.
pub fn overflow_marker(
    mode: gv_core::intervals::IntervalDisplayMode,
    lanes: u32,
    height: u16,
) -> Option<String> {
    let hidden = lanes.saturating_sub(IntervalMode::from(mode).visible_lanes(height));
    (hidden > 0).then(|| format!("+{hidden}"))
}

/// Draws genomic lanes at the selected density, with a bounded terminal footprint.
pub fn render_packed_intervals(
    area: &Rect,
    buf: &mut Buffer,
    rows: &DataFrame,
    lanes: u32,
    view: &AlignmentView,
    style: IntervalStyle,
) -> Result<(), TGVError> {
    use gv_core::{bed::BedSchema, variant::VariantSchema};
    if area.width == 0 || area.height == 0 {
        return Ok(());
    }
    let starts = rows.column(IntervalSchema::START)?.u64()?;
    let ends = rows.column(IntervalSchema::END)?.u64()?;
    let row_lanes = rows.column(IntervalSchema::LANE)?.u32()?;
    let mode = IntervalMode::from(style.mode);
    let visible = mode.visible_lanes(area.height);
    let width = usize::from(area.width);
    let mut counts = vec![0usize; width];
    for row in 0..rows.height() {
        let lane = row_lanes.get(row).expect("interval lanes are non-null");
        if lane < style.first_lane || lane - style.first_lane >= visible {
            continue;
        }
        let display_lane = lane - style.first_lane;
        let start = view
            .onscreen_x_coordinate(starts.get(row).expect("interval starts are non-null"), area);
        let end =
            view.onscreen_x_coordinate(ends.get(row).expect("interval ends are non-null"), area);
        let Some((x, length)) = OnScreenCoordinate::onscreen_start_and_length(&start, &end, area)
        else {
            continue;
        };
        let color = style.colors[row % style.colors.len()];
        match mode {
            IntervalMode::Expanded => {
                let (fill, name) = match style.labels {
                    IntervalLabels::Bed => {
                        let fill = match rows.column(BedSchema::STRAND)?.str()?.get(row) {
                            Some("+") => '›',
                            Some("-") => '‹',
                            _ => ' ',
                        };
                        (
                            fill,
                            rows.column(BedSchema::NAME)?
                                .str()?
                                .get(row)
                                .map(str::to_owned),
                        )
                    }
                    IntervalLabels::Variant => {
                        let ids = rows.column(VariantSchema::IDS)?.list()?.get_as_series(row);
                        let name = ids
                            .map(|ids| -> Result<String, TGVError> {
                                Ok(ids.str()?.iter().flatten().collect::<Vec<_>>().join(";"))
                            })
                            .transpose()?;
                        (' ', name)
                    }
                };
                let mut text = vec![fill; usize::from(length)];
                // Restrict labels to ASCII so their character count equals their terminal width.
                if let Some(name) = name
                    && name.is_ascii()
                    && !name.chars().any(char::is_control)
                    && !name.is_empty()
                    && name.len() + 2 <= text.len()
                {
                    let offset = (text.len() - name.len()) / 2;
                    text.splice(offset..offset + name.len(), name.chars());
                }
                buf.set_string(
                    area.x + x,
                    area.y + display_lane as u16,
                    text.into_iter().collect::<String>(),
                    Style::default().bg(color).fg(Color::White),
                );
            }
            IntervalMode::Collapsed => {
                for column in x..x + length {
                    counts[usize::from(column)] += 1;
                    buf[(area.x + column, area.y)].set_char(' ').set_bg(color);
                }
            }
        }
    }
    match mode {
        IntervalMode::Collapsed => {
            for (column, count) in counts.into_iter().enumerate() {
                if count >= 2 {
                    let cell = &mut buf[(area.x + column as u16, area.y)];
                    let color = match cell.bg {
                        Color::Rgb(r, g, b) => Color::Rgb(r / 2, g / 2, b / 2),
                        _ => Color::DarkGray,
                    };
                    cell.set_bg(color).set_fg(Color::White);
                    if count >= 3 {
                        cell.set_char(if count < 10 {
                            char::from_digit(count as u32, 10).expect("single digit count")
                        } else {
                            '+'
                        });
                    }
                }
            }
        }
        IntervalMode::Expanded => {}
    }
    if let Some(marker) = overflow_marker(style.mode, lanes, area.height) {
        let length = marker.len().min(width);
        buf.set_stringn(
            area.right() - length as u16,
            area.bottom() - 1,
            marker,
            length,
            Style::default().fg(Color::White).bg(Color::DarkGray),
        );
    }
    Ok(())
}

/// Picks the last painted feature in the terminal cell, excluding overflow marker cells.
pub fn pick_interval(
    rows: DataFrame,
    mode: gv_core::intervals::IntervalDisplayMode,
    lanes: u32,
    area: &Rect,
    column: u16,
    row: u16,
    first_lane: u32,
) -> Result<DataFrame, TGVError> {
    if !area.contains((column, row).into())
        || overflow_marker(mode, lanes, area.height).is_some_and(|marker| {
            row == area.bottom() - 1
                && column
                    >= area
                        .right()
                        .saturating_sub(marker.len().min(area.width as usize) as u16)
        })
    {
        return Ok(rows.clear());
    }
    let mode = IntervalMode::from(mode);
    let row_lanes = rows.column(IntervalSchema::LANE)?.u32()?;
    let selected = row_lanes
        .into_no_null_iter()
        .enumerate()
        .rev()
        .find_map(|(index, lane)| {
            (lane >= first_lane
                && lane - first_lane < mode.visible_lanes(area.height)
                && mode.contains_lane(row - area.y, lane - first_lane))
            .then_some(index)
        });
    Ok(selected.map_or_else(|| rows.clear(), |index| rows.slice(index as i64, 1)))
}
