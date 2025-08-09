use crate::{
    alignment::{AlignedRead, Alignment},
    error::TGVError,
    rendering::colors::DARK_THEME,
    window::{OnScreenCoordinate, ViewingWindow},
};
use ratatui::{buffer::Buffer, layout::Rect, style::Style};
use rust_htslib::bam::record::Cigar;

/// Render an alignment on the alignment area.
pub fn render_alignment(
    area: &Rect,
    buf: &mut Buffer,
    window: &ViewingWindow,
    alignment: &Alignment,
) -> Result<(), TGVError> {
    // This iterates through all cached reads and re-calculates coordinates for each movement.
    // Consider improvement.
    for read in alignment.reads.iter() {
        if let Some(contexts) = get_read_rendering_info(read, window, area) {
            for context in contexts {
                buf.set_string(
                    area.x + context.x,
                    area.y + context.y,
                    context.string,
                    context.style,
                )
            }
        }
    }
    Ok(())
}

struct RenderingContext {
    x: u16,
    y: u16,
    string: String,
    style: Style,
}

/// Get rendering needs for an aligned read.
/// Returns: x, y,
fn get_read_rendering_info(
    read: &AlignedRead,
    viewing_window: &ViewingWindow,
    area: &Rect,
) -> Option<Vec<RenderingContext>> {
    let mut output: Vec<RenderingContext> = Vec::new();
    let cigar_coordinates_and_styles = Vec::new();

    let mut reference_pivot: usize = read.start; // used in the output
    let mut query_pivot: usize = 0; // # bases relative to the softclip start.

    let onscreen_y = match viewing_window.onscreen_y_coordinate(read.y, area) {
        OnScreenCoordinate::OnScreen(y_start) => y_start as u16,
        _ => return None,
    };

    let mut annotate_insertion_in_next_cigar = false;

    for op in read.read.cigar().iter() {
        match op {
            Cigar::SoftClip(l) => {
                for i_base in query_pivot..query_pivot + *l as usize {
                    // Prevent cases when a soft clip is at the very starting of the reference genome:
                    //    ----------- (ref)
                    //  ssss======>   (read)
                    //    ^           edge of screen
                    //  ^^            these softcliped bases are not displayed
                    if reference_pivot + i_base < 1 + read.leading_softclips {
                        continue;
                    }

                    let abs_start = reference_pivot + i_base - read.leading_softclips;

                    if let Some((onscreen_x, _length)) =
                        OnScreenCoordinate::onscreen_start_and_length(
                            &viewing_window.onscreen_x_coordinate(abs_start, area),
                            &viewing_window.onscreen_x_coordinate(abs_start, area),
                            area,
                        )
                    {
                        let base = read.read.seq()[i_base];
                        let style = Style::default().bg(DARK_THEME.softclip_color(base));
                        output.push(RenderingContext {
                            x: onscreen_x,
                            y: onscreen_y,
                            string: base.to_string(),
                            style,
                        });
                    } else {
                        continue;
                    }
                }
            }

            Cigar::Ins(l) => {
                annotate_insertion_in_next_cigar = true;
            }

            Cigar::Del() => {

            }

            Cigar::Diff() => {

            }

            Cigar::Match() => {

            }

            Cigar::Pad() => 
        }

        if consumes_reference(op) {
            reference_pivot += op.len() as usize;
            // Note that softclip does not consume query and is handled above.
        }

        if consumes_query(op) {
            query_pivot += op.len() as usize;
        }
    }

    // Annotate read direction

    let n_cigar_segments = cigar_coordinates_and_styles.len();

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
/// Returns: Vec<
fn get_cigar_coordinates_and_styles(read: &AlignedRead) -> Vec<(usize, usize, Style)> {}

/// Whether the cigar operation consumes reference.
/// Yes: M/D/N/=/X
/// No: I/S/H/P
/// See: https://samtools.github.io/hts-specs/SAMv1.pdf
fn consumes_reference(op: &Cigar) -> bool {
    match op {
        Cigar::Match(_l)
        | Cigar::Del(_l)
        | Cigar::RefSkip(_l)
        | Cigar::Equal(_l)
        | Cigar::Diff(_l) => true,

        Cigar::SoftClip(_l) | Cigar::Ins(_l) | Cigar::HardClip(_l) | Cigar::Pad(_l) => false,
    }
}

/// Whether the cigar operation consumes query.
/// Yes: M/I/S/=/X
/// No: D/N/H/P
fn consumes_query(op: &Cigar) -> bool {
    match op {
        Cigar::Match(_l)
        | Cigar::Ins(_l)
        | Cigar::SoftClip(_l)
        | Cigar::Equal(_l)
        | Cigar::Diff(_l) => true,

        Cigar::Del(_l) | Cigar::RefSkip(_l) | Cigar::HardClip(_l) | Cigar::Pad(_l) => false,
    }
}

/// Only labels that consumes reference are display onscreen.
fn get_cigar_style(op: &Cigar) -> Style {
    match op {
        Cigar::Match(_l) | Cigar::Equal(_l) => Style::default().bg(colors::MATCH_COLOR),
        // By SAM spec, M can also be mismatch. TODO: think about this in the future.
        Cigar::Diff(_l) => Style::default().bg(colors::MISMATCH_COLOR),

        Cigar::Del(_l) | Cigar::RefSkip(_l) => Style::default(),

        _ => Style::default(),
    }
}
