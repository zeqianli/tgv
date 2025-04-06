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
    let mut right_most_label_onscreen_x = 0;
    for feature in track.genes.iter() {
        for (track_x, track_string, track_style, label_info) in
            get_rendering_info(window, area, feature)
        {
            buf.set_string(
                track_x as u16 + area.x,
                area.y,
                track_string.clone(),
                track_style,
            );

            if let Some((label_x, label)) = label_info {
                if label_x > right_most_label_onscreen_x + 1 {
                    right_most_label_onscreen_x = label_x + label.len() - 1;

                    buf.set_string(
                        label_x as u16 + area.x,
                        area.y + 1,
                        label.clone(),
                        Style::default(),
                    );
                }
            }
        }
    }
}

const MIN_GENE_ON_SCREEN_LENGTH_TO_SHOW_EXONS: usize = 10;

fn get_rendering_info(
    window: &ViewingWindow,
    area: &Rect,
    gene: &Gene,
) -> Vec<(usize, String, Style, Option<(usize, String)>)> {
    // First, check if the gene should be rendered as a single segment or multiple segments.

    let gene_start_x = window.onscreen_x_coordinate(gene.start(), area);
    let gene_end_x = window.onscreen_x_coordinate(gene.end(), area);

    let render_whole_gene = OnScreenCoordinate::width(&gene_start_x, &gene_end_x, area)
        <= MIN_GENE_ON_SCREEN_LENGTH_TO_SHOW_EXONS;

    if render_whole_gene {
        if let Some((x, length)) =
            OnScreenCoordinate::onscreen_start_and_length(&gene_start_x, &gene_end_x, area)
        {
            let (string, style) = get_gene_segment_string_and_style(length, gene.strand.clone());

            // label x and text
            let label = gene.name.to_string();
            let label_x = x + (length.saturating_sub(label.len()) / 2);

            vec![(x, string, style, Some((label_x, label)))]
        } else {
            vec![]
        }
    } else {
        // Render each exon as a separate segment.
        let mut exons_info: Vec<(usize, String, Style, Option<(usize, String)>)> = Vec::new();
        let mut non_cds_exons_info: Vec<(usize, String, Style, Option<(usize, String)>)> =
            Vec::new();
        let mut introns_info: Vec<(usize, String, Style, Option<(usize, String)>)> = Vec::new();
        let mut right_most_label_onscreen_x = 0;
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

                match feature_type {
                    FeatureType::Exon => {
                        let label = format!("{}:exon{}", gene.name, feature_index);

                        let label_x = x + (length.saturating_sub(label.len()) / 2);
                        let label_right_coordinate = label_x + label.len() - 1; // inclusive

                        exons_info.push((
                            x,
                            string,
                            style,
                            if label_x > right_most_label_onscreen_x + 1 {
                                right_most_label_onscreen_x = label_right_coordinate;

                                Some((label_x, label))
                            } else {
                                None
                            },
                        ));
                    }
                    FeatureType::NonCDSExon => {
                        let label = gene.name.to_string();
                        let label_x = x + (length.saturating_sub(label.len()) / 2);
                        let label_right_coordinate = label_x + label.len() - 1; // inclusive

                        non_cds_exons_info.push((
                            x,
                            string,
                            style,
                            if label_x > right_most_label_onscreen_x + 1 {
                                right_most_label_onscreen_x = label_right_coordinate;

                                Some((label_x, label))
                            } else {
                                None
                            },
                        ));
                    }
                    FeatureType::Intron => {
                        introns_info.push((x, string, style, None));
                    }
                }
            }
        }

        // The order decides rendering order.
        // Exons are on top of non-CDS exons, on top of introns.

        [introns_info, non_cds_exons_info, exons_info].concat()
    }
}

const EXON_ARROW_GAP: usize = 5;
const INTRON_ARROW_GAP: usize = 10;
const GENE_ARROW_GAP: usize = 5;

const EXON_BACKGROUND_COLOR: Color = tailwind::BLUE.c800;
const EXON_FOREGROUND_COLOR: Color = tailwind::WHITE;
const GENE_BACKGROUND_COLOR: Color = tailwind::BLUE.c700;
const NON_CDS_EXON_BACKGROUND_COLOR: Color = tailwind::BLUE.c500;
const INTRON_FOREGROUND_COLOR: Color = tailwind::BLUE.c300;

fn get_gene_segment_string_and_style(length: usize, strand: Strand) -> (String, Style) {
    let string = match strand {
        Strand::Forward => (0..length)
            .map(|i| if i % GENE_ARROW_GAP == 0 { ">" } else { " " })
            .collect::<String>(),
        Strand::Reverse => (0..length)
            .map(|i| if i % GENE_ARROW_GAP == 0 { "<" } else { " " })
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
            .map(|i| if i % EXON_ARROW_GAP == 0 { ">" } else { " " })
            .collect::<String>(),
        (Strand::Forward, FeatureType::NonCDSExon) => (0..length).map(|_| "▅").collect::<String>(),
        (Strand::Forward, FeatureType::Intron) => (0..length)
            .map(|i| if i % INTRON_ARROW_GAP == 0 { ">" } else { "-" })
            .collect::<String>(),
        (Strand::Reverse, FeatureType::Exon) => (0..length)
            .map(|i| if i % EXON_ARROW_GAP == 0 { "<" } else { "-" })
            .collect::<String>(),
        (Strand::Reverse, FeatureType::NonCDSExon) => (0..length).map(|_| "▅").collect::<String>(),
        (Strand::Reverse, FeatureType::Intron) => (0..length)
            .map(|i| if i % INTRON_ARROW_GAP == 0 { "<" } else { "-" })
            .collect::<String>(),
    };

    let style = match feature_type {
        FeatureType::Exon => Style::default()
            .fg(EXON_FOREGROUND_COLOR)
            .bg(EXON_BACKGROUND_COLOR),
        FeatureType::Intron => Style::default().fg(INTRON_FOREGROUND_COLOR),
        FeatureType::NonCDSExon => Style::default().fg(NON_CDS_EXON_BACKGROUND_COLOR),
    };

    (string, style)
}
