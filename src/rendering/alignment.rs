use crate::{
    alignment::{AlignedRead, Alignment},
    error::TGVError,
    rendering::colors::Palette,
    window::{OnScreenCoordinate, ViewingWindow},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};
use rust_htslib::bam::record::Cigar;

/// Render an alignment on the alignment area.
pub fn render_alignment(
    area: &Rect,
    buf: &mut Buffer,
    window: &ViewingWindow,
    alignment: &Alignment,
    background_color: &Color,
    pallete: &Palette,
) -> Result<(), TGVError> {
    // This iterates through all cached reads and re-calculates coordinates for each movement.
    // Consider improvement.
    for read in alignment.reads.iter() {
        if let Some(contexts) =
            get_read_rendering_info(read, window, area, background_color, pallete)
        {
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

enum Modifier {
    Forward,

    Reverse,

    Insertion(usize),
}

struct RenderingContext {
    x: u16,
    y: u16,
    string: String,
    style: Style,
}

impl RenderingContext {
    fn forward_arrow(&self) -> Self {
        return Self {
            x: self.x + self.string.len() as u16 - 1,
            y: self.y,
            string: "►".to_string(),
            style: self.style.clone(),
        };
    }

    fn reverse_arrow(&self) -> Self {
        return Self {
            x: self.x,
            y: self.y,
            string: "◄".to_string(),
            style: self.style.clone(),
        };
    }

    fn insertion(&self, background_color: &Color, pallete: &Palette) -> Self {
        return Self {
            x: self.x,
            y: self.y,
            string: "▌".to_string(),
            style: Style::default().fg(pallete.INSERTION_COLOR),
        };
    }
}

/// Get rendering needs for an aligned read.
/// Returns: x, y,
fn get_read_rendering_info(
    read: &AlignedRead,
    viewing_window: &ViewingWindow,
    area: &Rect,
    background_color: &Color,
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

    let cigars = read.read.cigar();

    if cigars.len() == 0 {
        return None;
    }

    // Scan cigars 1st pass to find the cigar index with < / > annotation.
    let cigar_index_with_direction_annotation = if read.read.is_reverse() {
        // last cigar segment
        cigars.len()
            - cigars
                .iter()
                .rev()
                .position(|op| can_be_annotated_with_arrows(op))
                .unwrap_or(0)
            - 1
    } else {
        // first eligible cigar
        read.read
            .cigar()
            .iter()
            .position(|op| can_be_annotated_with_arrows(op))
            .unwrap_or(0)
    };

    // let mut index_for_direction_annotaiton = None;

    //let mut indexes_for_insertion_annotation = 0;

    // See: https://samtools.github.io/hts-specs/SAMv1.pdf

    for (i_op, op) in read.read.cigar().iter().enumerate() {
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

        let mut new_contexts = Vec::new();
        let add_insertion = annotate_insertion_in_next_cigar;
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
                        new_contexts.push(RenderingContext {
                            x: onscreen_x,
                            y: onscreen_y,
                            string: String::from_utf8(vec![base]).unwrap_or("?".to_string()),
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
                    //println!("found deletion (readname = {:?})", read.read.cigar());
                    new_contexts.push(RenderingContext {
                        x: x,
                        y: onscreen_y,
                        string: "-".repeat(length as usize),
                        style: Style::new()
                            .bg(*background_color)
                            .fg(pallete.DELETION_COLOR),
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
                    new_contexts.push(RenderingContext {
                        x: x,
                        y: onscreen_y,
                        string: " ".repeat(length as usize),
                        style: Style::new().bg(pallete.MISMATCH_COLOR),
                    })
                }
            }

            Cigar::Match(l) | Cigar::Equal(l) => {
                // M / =
                // Full color block
                // TODO: IGV checks base with the reference here.
                if let Some((x, length)) = OnScreenCoordinate::onscreen_start_and_length(
                    &viewing_window.onscreen_x_coordinate(reference_pivot, area),
                    &viewing_window.onscreen_x_coordinate(next_reference_pivot, area),
                    area,
                ) {
                    new_contexts.push(RenderingContext {
                        x: x,
                        y: onscreen_y,
                        string: "-".repeat(length as usize),
                        style: Style::default().bg(pallete.MATCH_COLOR),
                    })
                }
            }

            Cigar::HardClip(l) | Cigar::Pad(l) => {
                // P / H
                // Don't need to do anything
                //continue;
            }
        }

        // add modifiers
        if new_contexts.is_empty() {
            continue;
        }

        if add_insertion {
            new_contexts.push(
                new_contexts
                    .first()
                    .unwrap()
                    .insertion(background_color, pallete),
            );
        };
        annotate_insertion_in_next_cigar = false;

        if i_op == cigar_index_with_direction_annotation {
            new_contexts.push(if read.read.is_reverse() {
                new_contexts.first().unwrap().forward_arrow()
            } else {
                new_contexts.last().unwrap().reverse_arrow()
            });
        }

        output.extend(new_contexts);

        reference_pivot = next_reference_pivot;
        query_pivot = next_query_pivot;
    }

    // Annotate read direction
    // If forward: Change the right most rendering context that's not a softclip / del to >
    // If reverse: Change the left most rendering context that's not a softclip / del to >

    // TODO: draw insertions
    Some(output)
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

/// Whether the cigar operation can be annotated with the < / > signs.
/// Yes: M/I/S/=/X
/// No: D/N/H/P
fn can_be_annotated_with_arrows(op: &Cigar) -> bool {
    match op {
        Cigar::Match(_l)
        | Cigar::SoftClip(_l)
        | Cigar::Equal(_l)
        | Cigar::Diff(_l)
        | Cigar::Del(_l)
        | Cigar::RefSkip(_l) => true,

        Cigar::HardClip(_l) | Cigar::Pad(_l) | Cigar::Ins(_l) => false,
    }
}
