use crate::{
    alignment::{AlignedRead, Alignment},
    error::TGVError,
    rendering::colors::Palette,
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
    pallete: &Palette,
) -> Result<(), TGVError> {
    // This iterates through all cached reads and re-calculates coordinates for each movement.
    // Consider improvement.
    for read in alignment.reads.iter() {
        if let Some(contexts) = get_read_rendering_info(read, window, area, pallete) {
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
    pallete: &Palette,
) -> Option<Vec<RenderingContext>> {
    let mut output: Vec<RenderingContext> = Vec::new();

    let mut reference_pivot: usize = read.start; // used in the output
    let mut query_pivot: usize = 0; // # bases relative to the softclip start.

    let onscreen_y = match viewing_window.onscreen_y_coordinate(read.y, area) {
        OnScreenCoordinate::OnScreen(y_start) => y_start as u16,
        _ => return None,
    };

    let mut annotate_insertion_in_next_cigar = false;

    // let mut index_for_direction_annotaiton = None;

    //let mut indexes_for_insertion_annotation = 0;

    // See: https://samtools.github.io/hts-specs/SAMv1.pdf

    for op in read.read.cigar().iter() {
        let next_reference_pivot = if consumes_reference(op) {
            reference_pivot + op.len() as usize
            // Note that softclip does not consume query and is handled above.
        } else {
            reference_pivot
        };

        let next_query_pivot = if consumes_query(op) {
            query_pivot + op.len() as usize
        } else {
            query_pivot
        };
        match op {
            Cigar::SoftClip(l) => {
                // S
                // TODO:
                // 1x zoom: display color
                // 2x zoom: half-block rendering
                // higher zoom: whole block color? half-block to the best ability? Think about this.
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
                        let style = Style::default().bg(pallete.softclip_color(base));
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
                // TODO: draw the insertion.
            }

            Cigar::Del(l) | Cigar::RefSkip(l) => {
                // D / N
                // ---------------- ref
                // ===----===       read (lines with no bckground colors)
                if let Some((x, length)) = OnScreenCoordinate::onscreen_start_and_length(
                    &viewing_window.onscreen_x_coordinate(reference_pivot, area),
                    &viewing_window.onscreen_x_coordinate(next_reference_pivot, area),
                    area,
                ) {
                    output.push(RenderingContext {
                        x: x,
                        y: onscreen_y,
                        string: "-".repeat(length as usize),
                        style: Style::default().fg(pallete.DELETION_COLOR),
                    })
                }
            }

            Cigar::Diff(l) => {
                // X
                // TODO:
                // 1x zoom: Display base letter + color
                // 2x zoom: (?) Half-base rendering with mismatch color
                //

                //
                if let Some((x, length)) = OnScreenCoordinate::onscreen_start_and_length(
                    &viewing_window.onscreen_x_coordinate(reference_pivot, area),
                    &viewing_window.onscreen_x_coordinate(next_reference_pivot, area),
                    area,
                ) {
                    output.push(RenderingContext {
                        x: x,
                        y: onscreen_y,
                        string: " ".repeat(length as usize),
                        style: Style::default().bg(pallete.MISMATCH_COLOR),
                    })
                }
            }

            Cigar::Match(l) | Cigar::Equal(l) => {
                // M / =
                // Full color block
                if let Some((x, length)) = OnScreenCoordinate::onscreen_start_and_length(
                    &viewing_window.onscreen_x_coordinate(reference_pivot, area),
                    &viewing_window.onscreen_x_coordinate(next_reference_pivot, area),
                    area,
                ) {
                    output.push(RenderingContext {
                        x: x,
                        y: onscreen_y,
                        string: " ".repeat(length as usize),
                        style: Style::default().bg(pallete.MATCH_COLOR),
                    })
                }
            }

            Cigar::HardClip(l) | Cigar::Pad(l) => {
                // P / H
                // Don't need to do anything
            }
        }

        reference_pivot = next_reference_pivot;
        query_pivot = next_query_pivot;
    }

    // Annotate read direction
    // If forward: Change the right most rendering context that's not a softclip / del to >
    // If reverse: Change the left most rendering context that's not a softclip / del to >

    if read.read.is_reverse() {
        if let Some(context) = output.last_mut() {
            if context.string.len() > 0 {
                context.string.pop();
                context.string.push('>');
            }
        }
    } else {
        if let Some(context) = output.first_mut() {
            if context.string.len() > 0 {
                context.string =
                    ">".to_string() + &context.string.chars().skip(1).collect::<String>();
            }
        }
    }

    // TODO: draw insertions
    Some(output)
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
