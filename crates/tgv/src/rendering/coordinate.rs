use crate::layout::CellView;
use crate::layout::{AlignmentView, OnScreenCoordinate};
use gv_core::prelude::*;
use gv_viewer::prepare::coordinate::{
    calculate_intermarker_distance, get_abbreviated_coordinate_text,
};

use itertools::izip;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;

pub fn render_coordinates(
    area: &Rect,
    buf: &mut Buffer,
    alignment_view: &AlignmentView,
    state: &State,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let contig_length = state.contig_length(&alignment_view.focus)?;

    let (coordinate_texts, coordinate_texts_xs, markers_onscreen_x) =
        calculate_coordinates(alignment_view, area, contig_length);

    for (text, text_x, marker_x) in izip!(
        coordinate_texts.iter(),
        coordinate_texts_xs.iter(),
        markers_onscreen_x.iter()
    ) {
        buf.set_stringn(
            area.x + *text_x,
            area.y,
            text,
            area.width as usize - *text_x as usize,
            Style::default(),
        );
        if area.height >= 2 {
            buf.set_stringn(
                area.x + *marker_x,
                area.y + 1,
                "|",
                area.width as usize - *marker_x as usize,
                Style::default(),
            );
        }
    }

    Ok(())
}

const MIN_SPACING_BETWEEN_MARKERS: u16 = 15;

/// Calculate coordinate markers.
/// left and right are 1-based, inclusive.
fn calculate_coordinates(
    alignment_view: &AlignmentView,
    area: &Rect,
    contig_length: Option<u64>,
) -> (Vec<String>, Vec<u16>, Vec<u16>) {
    let (intermarker_distance, power) =
        calculate_intermarker_distance(alignment_view.zoom.0 * MIN_SPACING_BETWEEN_MARKERS as u64);

    let mut pivot = (alignment_view.left(area) / intermarker_distance + 1) * intermarker_distance; // First marker
    let mut markers_onscreen_x: Vec<u16> = Vec::new();
    let mut coordinate_texts: Vec<String> = Vec::new();
    let mut coordinate_texts_xs: Vec<u16> = Vec::new();

    let render_bound = match contig_length {
        Some(length) => u64::min(alignment_view.right(area), length),
        None => alignment_view.right(area),
    };

    while pivot < render_bound {
        let marker_text = get_abbreviated_coordinate_text(pivot, power);

        let onscreen_marker_coordinate = alignment_view.onscreen_x_coordinate(pivot, area);

        match onscreen_marker_coordinate {
            OnScreenCoordinate::OnScreen(x) => {
                if x >= marker_text.len() / 2 {
                    markers_onscreen_x.push(x as u16);
                    coordinate_texts_xs.push((x - marker_text.len() / 2) as u16);
                    coordinate_texts.push(marker_text);
                } else {
                    markers_onscreen_x.push(x as u16);
                    coordinate_texts_xs.push(0_u16);
                    coordinate_texts.push(marker_text[(marker_text.len() / 2 - x)..].to_string());
                }
            }
            _ => {
                continue;
            }
        }

        pivot += intermarker_distance;
    }

    (coordinate_texts, coordinate_texts_xs, markers_onscreen_x)
}
