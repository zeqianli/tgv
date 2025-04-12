use crate::models::{
    alignment::{AlignedRead, Alignment},
    strand::Strand,
    window::{OnScreenCoordinate, ViewingWindow},
};
use crate::rendering::colors;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};
use rust_htslib::bam::record::Cigar;

/// Render an alignment on the alignment area.
pub fn render_alignment(
    area: &Rect,
    buf: &mut Buffer,
    window: &ViewingWindow,
    alignment: &Alignment,
) {
    // This iterates through all cached reads and re-calculates coordinates for each movement.
    // Consider improvement.
    for read in alignment.reads.iter() {
        for (x, y, onscreen_string, style) in get_read_rendering_info(read, window, area) {
            buf.set_string(x as u16 + area.x, y as u16 + area.y, onscreen_string, style);
        }
    }
}

fn get_read_rendering_info(
    read: &AlignedRead,
    viewing_window: &ViewingWindow,
    area: &Rect,
) -> Vec<(usize, usize, String, Style)> {
    let mut output = Vec::new();
    let cigar_segments = get_cigar_segments(read);
    let n_cigar_segments = cigar_segments.len();

    let onscreen_y = match viewing_window.onscreen_y_coordinate(read.y, area) {
        OnScreenCoordinate::OnScreen(y_start) => y_start,
        _ => return vec![],
    };

    for (i_cigar_segment, (start_coord, end_coord, style)) in cigar_segments.iter().enumerate() {
        if let Some((x, length)) = OnScreenCoordinate::onscreen_start_and_length(
            &viewing_window.onscreen_x_coordinate(*start_coord, area),
            &viewing_window.onscreen_x_coordinate(*end_coord, area),
            area,
        ) {
            output.push((
                x,
                onscreen_y,
                get_segment_string(length, {
                    if i_cigar_segment == 0 {
                        Some(true)
                    } else if i_cigar_segment == n_cigar_segments - 1 {
                        Some(false)
                    } else {
                        None
                    }
                }),
                *style,
            ));
        }
    }
    output
}

fn get_segment_string(length: usize, is_reverse: Option<bool>) -> String {
    match is_reverse {
        Some(true) => (0..length)
            .map(|i| if i == 0 { "<" } else { "-" })
            .collect::<String>(),
        Some(false) => (0..length)
            .map(|i| if i == length - 1 { ">" } else { "-" })
            .collect::<String>(),
        None => "-".repeat(length),
    }
}

/// Render a read as sections of styled texts
/// See: https://samtools.github.io/hts-specs/SAMv1.pdf
fn get_cigar_segments(read: &AlignedRead) -> Vec<(usize, usize, Style)> {
    let mut output = Vec::new();
    let mut pivot: usize = 0; // position relative to the softclip-start

    for op in read.read.cigar().iter() {
        match op {
            Cigar::Ins(_l) | Cigar::HardClip(_l) | Cigar::Pad(_l) => continue,
            Cigar::Del(l) | Cigar::RefSkip(l) => {
                pivot += *l as usize;
                continue;
            }
            Cigar::Match(l) | Cigar::Equal(l) | Cigar::Diff(l) => {
                let start = read.start + pivot - read.leading_softclips; // Should not overflow
                let end = start + *l as usize - 1;
                pivot += *l as usize;
                output.push((start, end, Style::default().bg(colors::MATCH_COLOR)));
            }
            Cigar::SoftClip(l) => {
                // If this is a leading softclip, check subtraction overflow.
                for i_base in pivot..pivot + *l as usize {
                    let base_coord_is_valid = read.start + i_base >= 1 + read.leading_softclips;
                    if base_coord_is_valid {
                        let abs_start = read.start + i_base - read.leading_softclips;

                        let base = read.read.seq()[i_base];
                        let base_color = match base {
                            b'A' => colors::SOFTCLIP_A,
                            b'C' => colors::SOFTCLIP_C,
                            b'G' => colors::SOFTCLIP_G,
                            b'T' => colors::SOFTCLIP_T,
                            _ => colors::SOFTCLIP_N,
                        };
                        output.push((abs_start, abs_start, Style::default().bg(base_color)));
                    }
                }

                pivot += *l as usize;
            }
        }
    }
    output
}
