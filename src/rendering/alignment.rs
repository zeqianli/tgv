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
        render_read(area, buf, window, read);
    }
}

fn render_read(area: &Rect, buf: &mut Buffer, window: &ViewingWindow, read: &AlignedRead) {
    for segment in get_cigar_segments(read) {
        let mut onscreen_string;
        let onscreen_x;

        match (
            window.onscreen_x_coordinate(read.start + segment.pivot as usize, area),
            window.onscreen_x_coordinate(
                read.start + segment.pivot as usize + segment.length as usize,
                area,
            ),
        ) {
            (OnScreenCoordinate::Left(x_start), OnScreenCoordinate::OnScreen(x_end)) => {
                if x_end == 0 {
                    continue;
                }
                if window.is_basewise() {
                    onscreen_string = segment.string();
                } else {
                    onscreen_string = segment.resize(x_end as u16 + x_start as u16).string();
                }
                onscreen_string = onscreen_string[x_start..].to_string();
                onscreen_x = 0;
            }
            (OnScreenCoordinate::OnScreen(x_start), OnScreenCoordinate::OnScreen(x_end)) => {
                if x_start >= x_end {
                    continue;
                }
                if window.is_basewise() {
                    onscreen_string = segment.string();
                } else {
                    onscreen_string = segment.resize((x_end - x_start) as u16).string();
                }
                onscreen_x = x_start;
            }
            (OnScreenCoordinate::OnScreen(x_start), OnScreenCoordinate::Right(x_end)) => {
                if x_start >= area.width as usize {
                    continue;
                }
                if window.is_basewise() {
                    onscreen_string = segment.string();
                } else {
                    onscreen_string = segment
                        .resize(area.width - x_start as u16 + x_end as u16)
                        .string();
                }
                onscreen_string = onscreen_string[..onscreen_string.len() - x_end].to_string(); // TODO: handle overflow
                onscreen_x = x_start;
            }

            (OnScreenCoordinate::Left(x_start), OnScreenCoordinate::Right(x_end)) => {
                if window.is_basewise() {
                    onscreen_string = segment.string();
                } else {
                    onscreen_string = segment
                        .resize(area.width + x_start as u16 + x_end as u16)
                        .string();
                }
                onscreen_string =
                    onscreen_string[x_start..onscreen_string.len() - x_end].to_string(); // TODO: handle overflow
                onscreen_x = 0;
            }

            _ => {
                continue;
            }
        }

        let onscreen_y = match window.onscreen_y_coordinate(read.y, area) {
            OnScreenCoordinate::OnScreen(y_start) => y_start,
            _ => continue,
        };

        buf.set_stringn(
            onscreen_x as u16 + area.x,
            onscreen_y as u16 + area.y,
            onscreen_string,
            area.width as usize - onscreen_x,
            segment.style(),
        );
    }
}

struct OnScreenCigarSegment {
    pivot: u16,   // number of bases from the start of the read
    length: u16,  // number of bases in the segment
    style: Style, // style of the segment
    direction: Option<Strand>,
}

impl OnScreenCigarSegment {
    pub fn string(&self) -> String {
        let mut string = "-".repeat(self.length as usize);
        match self.direction {
            Some(Strand::Reverse) => {
                string.replace_range(0..1, "<");
            }
            Some(Strand::Forward) => {
                string.replace_range(string.len() - 1..string.len(), ">");
            }
            None => {}
        }
        string
    }

    pub fn style(&self) -> Style {
        self.style
    }

    pub fn resize(&self, length: u16) -> Self {
        OnScreenCigarSegment {
            pivot: self.pivot,
            length,
            style: self.style,
            direction: self.direction.clone(),
        }
    }
}

/// Render a read as sections of styled texts
/// See: https://samtools.github.io/hts-specs/SAMv1.pdf
///
///
fn get_cigar_segments(read: &AlignedRead) -> Vec<OnScreenCigarSegment> {
    let mut output = Vec::new();
    let mut pivot: usize = 0;

    for op in read.read.cigar().iter() {
        match op {
            Cigar::Ins(_l) | Cigar::HardClip(_l) | Cigar::Pad(_l) => continue,
            Cigar::Del(l) | Cigar::RefSkip(l) => {
                pivot += *l as usize;
                continue;
            }
            Cigar::Match(l) | Cigar::Equal(l) | Cigar::Diff(l) => {
                output.push(OnScreenCigarSegment {
                    pivot: pivot as u16,
                    length: *l as u16,
                    style: Style::default().bg(colors::MATCH_COLOR),
                    direction: None,
                });
                pivot += *l as usize;
            }
            Cigar::SoftClip(l) => {
                for i_softclipped_base in pivot..pivot + *l as usize {
                    let base = read.read.seq()[i_softclipped_base];
                    let base_color = match base {
                        b'A' => colors::SOFTCLIP_A,
                        b'C' => colors::SOFTCLIP_C,
                        b'G' => colors::SOFTCLIP_G,
                        b'T' => colors::SOFTCLIP_T,
                        _ => colors::SOFTCLIP_N,
                    };
                    output.push(OnScreenCigarSegment {
                        pivot: i_softclipped_base as u16,
                        length: 1,
                        style: Style::default().bg(base_color),
                        direction: None,
                    });
                }
                pivot += *l as usize;
            }
        }
    }

    // direction
    if !output.is_empty() {
        match read.read.is_reverse() {
            true => {
                output.first_mut().unwrap().direction = Some(Strand::Reverse);
            }
            false => {
                output.last_mut().unwrap().direction = Some(Strand::Forward);
            }
        }
    }

    output
}
