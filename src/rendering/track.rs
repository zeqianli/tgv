use crate::models::{
    strand::Strand,
    track::{Feature, Track},
    window::{OnScreenCoordinate, ViewingWindow},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{palette::tailwind, Color, Style},
};

/// Render the genome sequence and coordinates.
/// TODO: zoomed view
pub fn render_track(area: &Rect, buf: &mut Buffer, window: &ViewingWindow, track: &Track) {
    // coordinates
    let left_coord_str = format!("{}:{}", window.contig.full_name(), window.left());
    let right_coord_str = format!(
        "{}:{}",
        window.contig.full_name(),
        window.left() + area.width as usize
    );
    buf.set_string(area.x, area.y + 1, left_coord_str, Style::default());
    buf.set_string(
        area.x + area.width - right_coord_str.len() as u16,
        area.y + 1,
        right_coord_str,
        Style::default(),
    );

    for feature in track.features.iter() {
        let segment = OnScreenFeatureSegment::new(feature);

        let mut onscreen_string;
        let onscreen_x;

        match (
            window.onscreen_x_coordinate(feature.start(), area),
            window.onscreen_x_coordinate(feature.end(), area),
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
                onscreen_string = onscreen_string[..onscreen_string.len() - x_end].to_string(); // TODO: handle overflow
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
                onscreen_string =
                    onscreen_string[x_start..onscreen_string.len() - x_end].to_string(); // TODO: handle overflow
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum OnScreenFeatureType {
    Gene,
    Exon,
    Intron,
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
            _ => " ".to_string(),
        }
    }

    pub fn style(&self) -> Style {
        match self.feature_type {
            OnScreenFeatureType::Exon => Style::default().bg(Self::EXON_BACKGROUND_COLOR),
            OnScreenFeatureType::Intron => Style::default().fg(Self::INTRON_FOREGROUND_COLOR),
            _ => Style::default(),
        }
    }
}
