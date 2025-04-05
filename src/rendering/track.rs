use crate::models::{
    reference::Reference,
    strand::Strand,
    track::{FeatureType, Gene, Track},
    window::{OnScreenCoordinate, ViewingWindow},
};
use crate::traits::GenomeInterval;
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
    for feature in track.genes.iter() {
        for (track_x, track_string, track_style, label_x, track_label) in
            get_rendering_info(window, area, feature)
        {
            buf.set_string(
                track_x as u16 + area.x,
                area.y,
                track_string.clone(),
                track_style,
            );

            buf.set_string(
                label_x as u16 + area.x,
                area.y + 1,
                track_label.clone(),
                Style::default(),
            );
        }
    }
}

const MIN_GENE_ON_SCREEN_LENGTH_TO_SHOW_EXONS: usize = 10;

fn get_rendering_info(
    window: &ViewingWindow,
    area: &Rect,
    gene: &Gene,
) -> Vec<(usize, String, Style, usize, String)> {
    let mut rendering_info: Vec<(usize, String, Style, usize, String)> = Vec::new();

    // First, check if the gene should be rendered as a single segment or multiple segments.

    let gene_start_x = window.onscreen_x_coordinate(gene.start(), area);
    let gene_end_x = window.onscreen_x_coordinate(gene.end() + 1, area);

    let render_whole_gene =
        gene_start_x.width(&gene_end_x, area) <= MIN_GENE_ON_SCREEN_LENGTH_TO_SHOW_EXONS;

    if render_whole_gene {
        if let Some((x, length)) =
            OnScreenCoordinate::onscreen_start_and_length(&gene_start_x, &gene_end_x, area)
        {
            let (string, style) = get_gene_segment_string_and_style(length, gene.strand.clone());

            // label x and text
            let label = format!("{}", gene.name);
            let label_x = x + (length.saturating_sub(label.len()) / 2);

            rendering_info.push((x, string, style, label_x, label));
        }
    } else {
        // Render each exon as a separate segment.

        for (feature_start, feature_end, feature_type, feature_index) in gene.features() {
            let feature_start_x = window.onscreen_x_coordinate(feature_start, area);
            let feature_end_x = window.onscreen_x_coordinate(feature_end, area);

            if let Some((x, length)) = OnScreenCoordinate::onscreen_start_and_length(
                &feature_start_x,
                &feature_end_x,
                area,
            ) {
                let (string, style) = get_feature_segment_string_and_style(
                    length,
                    gene.strand.clone(),
                    &feature_type,
                );

                let label = match feature_type {
                    FeatureType::Exon => format!("{}:exon{}", gene.name, feature_index),
                    FeatureType::Intron => format!("{}:intron{}", gene.name, feature_index),
                    FeatureType::NonCDSExon => "".to_string(),
                };

                let label_x = x + (length.saturating_sub(label.len()) / 2);

                rendering_info.push((x, string, style, label_x, label));
            }
        }
    }

    rendering_info
}

const EXON_ARROW_GAP: usize = 5;
const INTRON_ARROW_GAP: usize = 10;
const GENE_ARROW_GAP: usize = 5;

const EXON_BACKGROUND_COLOR: Color = tailwind::BLUE.c800;
const GENE_BACKGROUND_COLOR: Color = tailwind::BLUE.c700;
const NON_CDS_EXON_BACKGROUND_COLOR: Color = tailwind::BLUE.c500;
const INTRON_FOREGROUND_COLOR: Color = tailwind::BLUE.c300;

fn get_gene_segment_string_and_style(length: usize, strand: Strand) -> (String, Style) {
    let string = match strand {
        Strand::Forward => (0..length)
            .map(|i| {
                if (i + 1) % GENE_ARROW_GAP == 0 {
                    ">"
                } else {
                    " "
                }
            })
            .collect::<String>(),
        Strand::Reverse => (0..length)
            .map(|i| {
                if (i + 1) % GENE_ARROW_GAP == 0 {
                    "<"
                } else {
                    " "
                }
            })
            .collect::<String>(),
    };

    let style = Style::default().bg(GENE_BACKGROUND_COLOR);

    (string, style)
}

fn get_feature_segment_string_and_style(
    length: usize,
    strand: Strand,
    feature_type: &FeatureType,
) -> (String, Style) {
    let string = match (strand, feature_type) {
        (Strand::Forward, FeatureType::Exon) => (0..length)
            .map(|i| {
                if (i + 1) % EXON_ARROW_GAP == 0 {
                    ">"
                } else {
                    " "
                }
            })
            .collect::<String>(),
        (Strand::Forward, FeatureType::NonCDSExon) => (0..length).map(|_| "▅").collect::<String>(),
        (Strand::Forward, FeatureType::Intron) => (0..length)
            .map(|i| {
                if (i + 1) % INTRON_ARROW_GAP == 0 {
                    ">"
                } else {
                    " "
                }
            })
            .collect::<String>(),
        (Strand::Reverse, FeatureType::Exon) => (0..length)
            .map(|i| {
                if (i + 1) % EXON_ARROW_GAP == 0 {
                    "<"
                } else {
                    " "
                }
            })
            .collect::<String>(),
        (Strand::Reverse, FeatureType::NonCDSExon) => (0..length).map(|_| "▅").collect::<String>(),
        (Strand::Reverse, FeatureType::Intron) => (0..length)
            .map(|i| {
                if (i + 1) % INTRON_ARROW_GAP == 0 {
                    "<"
                } else {
                    " "
                }
            })
            .collect::<String>(),
    };

    let style = match feature_type {
        FeatureType::Exon => Style::default().bg(EXON_BACKGROUND_COLOR),
        FeatureType::Intron => Style::default().fg(INTRON_FOREGROUND_COLOR),
        FeatureType::NonCDSExon => Style::default().fg(NON_CDS_EXON_BACKGROUND_COLOR),
    };

    (string, style)
}

// #[derive(Debug, Clone, PartialEq)]
// pub enum OnScreenFeatureType {
//     Gene,
//     Exon,
//     Intron,
//     NonCDSExon,
//     // Promoter,
//     // UTR,
//     // Other,
// }

// struct OnScreenFeatureSegment {
//     pub direction: Strand,
//     pub feature_type: OnScreenFeatureType,
//     pub length: usize, // In bases
// }

// impl OnScreenFeatureSegment {
//     const EXON_BACKGROUND_COLOR: Color = tailwind::BLUE.c800;
//     const NON_CDS_EXON_BACKGROUND_COLOR: Color = tailwind::BLUE.c500;
//     const EXON_ARROW_GAP: usize = 5;
//     const INTRON_FOREGROUND_COLOR: Color = tailwind::BLUE.c300;
//     const INTRON_ARROW_GAP: usize = 10;

//     pub fn new(feature: &Feature) -> Self {
//         OnScreenFeatureSegment {
//             direction: feature.strand(),
//             feature_type: match feature {
//                 Feature::Gene { .. } => OnScreenFeatureType::Gene,
//                 Feature::Exon { .. } => OnScreenFeatureType::Exon,
//                 Feature::Intron { .. } => OnScreenFeatureType::Intron,
//                 Feature::NonCDSExon { .. } => OnScreenFeatureType::NonCDSExon,
//             },
//             length: feature.length(),
//         }
//     }

//     pub fn resize(&self, length: usize) -> Self {
//         OnScreenFeatureSegment {
//             direction: self.direction.clone(),
//             feature_type: self.feature_type.clone(),
//             length,
//         }
//     }
//     pub fn string(&self) -> String {
//         match self.feature_type {
//             OnScreenFeatureType::Exon => (0..self.length)
//                 .map(|i| {
//                     if (i + 1) % Self::EXON_ARROW_GAP == 0 {
//                         match self.direction {
//                             Strand::Forward => ">",
//                             Strand::Reverse => "<",
//                         }
//                     } else {
//                         " "
//                     }
//                 })
//                 .collect::<String>(),
//             OnScreenFeatureType::Intron => (0..self.length)
//                 .map(|i| {
//                     if (i + 1) % Self::INTRON_ARROW_GAP == 0 {
//                         match self.direction {
//                             Strand::Forward => ">",
//                             Strand::Reverse => "<",
//                         }
//                     } else {
//                         "-"
//                     }
//                 })
//                 .collect::<String>(),
//             OnScreenFeatureType::NonCDSExon => (0..self.length).map(|_| "▅").collect::<String>(),
//             _ => " ".to_string(),
//         }
//     }

//     pub fn style(&self) -> Style {
//         match self.feature_type {
//             OnScreenFeatureType::Exon => Style::default().bg(Self::EXON_BACKGROUND_COLOR),
//             OnScreenFeatureType::Intron => Style::default().fg(Self::INTRON_FOREGROUND_COLOR),
//             OnScreenFeatureType::NonCDSExon => {
//                 Style::default().fg(Self::NON_CDS_EXON_BACKGROUND_COLOR)
//             }
//             _ => Style::default(),
//         }
//     }
// }
