/// Reference chromosomes on the top of the screen.
///
use crate::models::{
    cytoband::{Cytoband, CytobandSegment, Stain},
    window::ViewingWindow,
};

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{palette::tailwind, Color, Style},
};

const HIGHLIGHT_COLOR: Color = tailwind::RED.c800;
const CYTOBAND_TEXT_LEFT_SPACING: u16 = 12;
const CYTOBAND_TEXT_RIGHT_SPACING: u16 = 7;
const MIN_AREA_WIDTH: u16 = CYTOBAND_TEXT_LEFT_SPACING + CYTOBAND_TEXT_RIGHT_SPACING + 1;
const MIN_AREA_HEIGHT: u16 = 1;
pub fn render_cytobands(
    area: &Rect,
    buf: &mut Buffer,
    cytoband: &Cytoband,
    viewing_window: &ViewingWindow,
    contig_length: Option<usize>,
) {
    if area.width <= MIN_AREA_WIDTH {
        return;
    }

    if area.height < MIN_AREA_HEIGHT {
        return;
    }

    for (x, string, style) in get_cytoband_xs_strings_and_styles(
        cytoband,
        CYTOBAND_TEXT_LEFT_SPACING,
        area.width - CYTOBAND_TEXT_RIGHT_SPACING,
    ) {
        buf.set_string(area.x + x, area.y, string, style);
    }

    // Left label: chromosome name
    let description = match (&cytoband.reference, &cytoband.contig) {
        (Some(reference), contig) => format!("{}:{}", reference, contig.full_name()),
        (None, contig) => contig.full_name().to_string(),
    }
    .chars()
    .take(CYTOBAND_TEXT_LEFT_SPACING as usize)
    .collect::<String>();

    buf.set_string(area.x, area.y, description, Style::default());

    // Right label: total length

    buf.set_string(
        area.width - CYTOBAND_TEXT_RIGHT_SPACING + 1,
        area.y,
        get_cytoband_total_length_text(cytoband.length()),
        Style::default(),
    );

    // Highlight the current viewing window

    let viewing_window_start = linear_scale(
        viewing_window.left(),
        cytoband.length(),
        CYTOBAND_TEXT_LEFT_SPACING,
        area.width - CYTOBAND_TEXT_RIGHT_SPACING,
    );
    let viewing_window_end = linear_scale(
        match contig_length {
            Some(length) => usize::min(viewing_window.right(area), length),
            None => viewing_window.right(area),
        },
        cytoband.length(),
        CYTOBAND_TEXT_LEFT_SPACING,
        area.width - CYTOBAND_TEXT_RIGHT_SPACING,
    );

    for x in viewing_window_start..viewing_window_end + 1 {
        let cell = buf.cell_mut(Position::new(area.x + x, area.y));
        if let Some(cell) = cell {
            cell.set_char(' ');
            cell.set_bg(HIGHLIGHT_COLOR);
        }
    }
}

fn get_cytoband_total_length_text(length: usize) -> String {
    let mut length = length;
    let mut power = 0;

    while length >= 1000 {
        length /= 1000;
        power += 1;
    }

    format!(
        "{}{}",
        length,
        match power {
            0 => "bp",
            1 => "kb",
            2 => "Mb",
            3 => "Gb",
            4 => "Tb",
            _ => "",
        }
    )
}

fn get_cytoband_xs_strings_and_styles(
    cytoband: &Cytoband,
    area_start: u16,
    area_end: u16,
) -> Vec<(u16, String, Style)> {
    let mut second_centromere = false;
    let mut output = Vec::new();
    for segment in cytoband.segments.iter() {
        if let Some((x, string, style)) = get_cytoband_segment_x_string_and_style(
            segment,
            cytoband.length(),
            area_start,
            area_end,
            second_centromere,
        ) {
            output.push((x, string, style));
        }

        if segment.stain == Stain::Acen {
            second_centromere = true;
        }
    }
    output
}

fn get_cytoband_segment_x_string_and_style(
    segment: &CytobandSegment,
    total_length: usize,
    area_start: u16,
    area_end: u16,
    second_centromere: bool,
) -> Option<(u16, String, Style)> {
    let onscreen_x_start = linear_scale(segment.start - 1, total_length, area_start, area_end); // 0-based, inclusive
    let onscreen_x_end = linear_scale(segment.end, total_length, area_start, area_end); // 0-based, exclusive

    if onscreen_x_end <= onscreen_x_start {
        return None;
    }

    let style = get_cytoband_segment_style(segment);

    match segment.stain {
        Stain::Acen => {
            /// Use unicode characters to draw the centromere
            let mut string = "-".repeat((onscreen_x_end - onscreen_x_start) as usize);
            if second_centromere {
                string.replace_range(0..1, "<");
            } else {
                string.replace_range(string.len() - 1..string.len(), ">");
            }
            Some((onscreen_x_start, string, style))
        }
        _ => {
            let string = "▅".repeat((onscreen_x_end - onscreen_x_start) as usize);
            Some((onscreen_x_start, string, style))
        }
    }
}

fn linear_scale(original_x: usize, original_length: usize, new_start: u16, new_end: u16) -> u16 {
    new_start + (original_x as f64 / (original_length) as f64 * (new_end - new_start) as f64) as u16
}

const DEFAULT_COLOR: Color = tailwind::GRAY.c300;
// const GNEG_COLOR: Color = tailwind::GREEN.c100;
const GPOS25_COLOR: Color = tailwind::GREEN.c200;
const GPOS50_COLOR: Color = tailwind::GREEN.c500;
const GPOS75_COLOR: Color = tailwind::GREEN.c700;
const GPOS100_COLOR: Color = tailwind::GREEN.c900;

const ACEN_COLOR: Color = tailwind::RED.c300;
const GVAR_COLOR: Color = DEFAULT_COLOR;
const STALK_COLOR: Color = DEFAULT_COLOR;
const OTHER_COLOR: Color = DEFAULT_COLOR;

fn get_cytoband_segment_style(cytoband_segment: &CytobandSegment) -> Style {
    match cytoband_segment.stain {
        Stain::Gneg => Style::default(),
        Stain::Gpos25 => Style::default().fg(GPOS25_COLOR),
        Stain::Gpos50 => Style::default().fg(GPOS50_COLOR),
        Stain::Gpos75 => Style::default().fg(GPOS75_COLOR),
        Stain::Gpos100 => Style::default().fg(GPOS100_COLOR),
        Stain::Acen => Style::default().fg(ACEN_COLOR),
        Stain::Gvar => Style::default().fg(GVAR_COLOR),
        Stain::Stalk => Style::default().fg(STALK_COLOR),
        Stain::Other => Style::default().fg(OTHER_COLOR),
    }
}
