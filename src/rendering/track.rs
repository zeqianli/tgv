use crate::models::{
    reference::Reference,
    strand::Strand,
    track::{Feature, Track},
    window::{OnScreenCoordinate, ViewingWindow},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{palette::tailwind, Color, Style},
};

// High zoom levels:
// - If a gene's length on screen is greater than a threshold, display underlying exons / introns.
// - Else, render a gene as one unit. Only label the gene.
// Labeling: fix in the future. Use the auto-labeling for now. In the future, re-vise the UI to avoid text overlap.
// Refactors needed:
// - Overhaul the track data structure. Change to a nested structure: track, gene, exon/introns.
// - Move track ownership to State and use one track. Otherwise, feature movement will be confusing.
//     - State saves a currently focused feature.
//     - At high zoom, w/b and W/B should do the same thing (next gene). Go to next/previous exons make no sense now.
// - Chromosome bound is needed now.
// Now it's probably a good time to move Data ownership to State.
//

/// Render the genome sequence and coordinates.
/// TODO: zoomed view
pub fn render_track(
    area: &Rect,
    buf: &mut Buffer,
    window: &ViewingWindow,
    track: &Track,
    reference: Option<&Reference>,
) {
    for feature in track.features.iter() {
        let segment = OnScreenFeatureSegment::new(feature);

        let mut onscreen_string;
        let onscreen_x;

        match (
            window.onscreen_x_coordinate(feature.start(), area),
            window.onscreen_x_coordinate(feature.end() + 1, area), // feature is 1-based inclusive. On-screen coordinates are 0-based, exclude in the end.
        ) {
            (OnScreenCoordinate::Left(x_start), OnScreenCoordinate::OnScreen(x_end)) => {
                if x_end == 0 {
                    continue;
                }
                if window.is_basewise() {
                    onscreen_string = segment.string();
                } else {
                    onscreen_string = segment.resize(x_end + x_start).string();
                }
                onscreen_string = onscreen_string.chars().skip(x_start).collect::<String>();
                onscreen_x = 0;
            }
            (OnScreenCoordinate::OnScreen(x_start), OnScreenCoordinate::OnScreen(x_end)) => {
                if x_start >= x_end {
                    continue;
                }
                if window.is_basewise() {
                    onscreen_string = segment.string();
                } else {
                    onscreen_string = segment.resize(x_end - x_start).string();
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
                        .resize(area.width as usize - x_start + x_end)
                        .string();
                }
                onscreen_string = onscreen_string
                    .chars()
                    .take(onscreen_string.len() - x_end)
                    .collect::<String>(); // TODO: handle overflow
                onscreen_x = x_start;
            }

            (OnScreenCoordinate::Left(x_start), OnScreenCoordinate::Right(x_end)) => {
                if window.is_basewise() {
                    onscreen_string = segment.string();
                } else {
                    onscreen_string = segment
                        .resize(area.width as usize + x_start + x_end)
                        .string();
                }
                onscreen_string = onscreen_string
                    .chars()
                    .skip(x_start)
                    .take(onscreen_string.len() - x_end - x_start)
                    .collect::<String>(); // TODO: handle overflow
                onscreen_x = 0;
            }

            _ => {
                continue;
            }
        }

        buf.set_string(
            onscreen_x as u16 + area.x,
            area.y,
            onscreen_string.clone(),
            segment.style(),
        );

        // Exon name

        if let Feature::Exon { name, .. } = feature {
            let name_onscreen_x;
            if name.len() >= onscreen_string.len() {
                name_onscreen_x = onscreen_x;
            } else {
                name_onscreen_x = onscreen_x + (onscreen_string.len() - name.len()) / 2;
            }

            buf.set_string(
                area.x + name_onscreen_x as u16,
                area.y + 1,
                name.clone(),
                Style::default(),
            );
        }
    }

    match reference {
        Some(reference) => {
            buf.set_string(
                area.x,
                area.y + 1,
                format!("{}:{}", reference, window.contig.full_name()),
                Style::default(),
            );
        }
        None => {
            buf.set_string(
                area.x,
                area.y + 1,
                format!("{}", window.contig.full_name()),
                Style::default(),
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OnScreenFeatureType {
    Gene,
    Exon,
    Intron,
    NonCDSExon,
    // Promoter,
    // UTR,
    // Other,
}

struct OnScreenFeatureSegment {
    pub direction: Strand,
    pub feature_type: OnScreenFeatureType,
    pub length: usize, // In bases
}

impl OnScreenFeatureSegment {
    const EXON_BACKGROUND_COLOR: Color = tailwind::BLUE.c800;
    const NON_CDS_EXON_BACKGROUND_COLOR: Color = tailwind::BLUE.c500;
    const EXON_ARROW_GAP: usize = 5;
    const INTRON_FOREGROUND_COLOR: Color = tailwind::BLUE.c300;
    const INTRON_ARROW_GAP: usize = 10;

    pub fn new(feature: &Feature) -> Self {
        OnScreenFeatureSegment {
            direction: feature.strand(),
            feature_type: match feature {
                Feature::Gene { .. } => OnScreenFeatureType::Gene,
                Feature::Exon { .. } => OnScreenFeatureType::Exon,
                Feature::Intron { .. } => OnScreenFeatureType::Intron,
                Feature::NonCDSExon { .. } => OnScreenFeatureType::NonCDSExon,
            },
            length: feature.length(),
        }
    }

    pub fn resize(&self, length: usize) -> Self {
        OnScreenFeatureSegment {
            direction: self.direction.clone(),
            feature_type: self.feature_type.clone(),
            length,
        }
    }
    pub fn string(&self) -> String {
        match self.feature_type {
            OnScreenFeatureType::Exon => (0..self.length)
                .map(|i| {
                    if (i + 1) % Self::EXON_ARROW_GAP == 0 {
                        match self.direction {
                            Strand::Forward => ">",
                            Strand::Reverse => "<",
                        }
                    } else {
                        " "
                    }
                })
                .collect::<String>(),
            OnScreenFeatureType::Intron => (0..self.length)
                .map(|i| {
                    if (i + 1) % Self::INTRON_ARROW_GAP == 0 {
                        match self.direction {
                            Strand::Forward => ">",
                            Strand::Reverse => "<",
                        }
                    } else {
                        "-"
                    }
                })
                .collect::<String>(),
            OnScreenFeatureType::NonCDSExon => (0..self.length).map(|_| "▅").collect::<String>(),
            _ => " ".to_string(),
        }
    }

    pub fn style(&self) -> Style {
        match self.feature_type {
            OnScreenFeatureType::Exon => Style::default().bg(Self::EXON_BACKGROUND_COLOR),
            OnScreenFeatureType::Intron => Style::default().fg(Self::INTRON_FOREGROUND_COLOR),
            OnScreenFeatureType::NonCDSExon => {
                Style::default().fg(Self::NON_CDS_EXON_BACKGROUND_COLOR)
            }
            _ => Style::default(),
        }
    }
}
