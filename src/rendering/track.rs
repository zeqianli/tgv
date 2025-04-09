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

const MIN_AREA_WIDTH: u16 = 5;
const MIN_AREA_HEIGHT: u16 = 2;

// Type alias for the complex return type
type TrackRenderInfo = (usize, String, Style, Option<(usize, String)>);

/// Render the genome features.
pub fn render_track(
    area: &Rect,
    buf: &mut Buffer,
    window: &ViewingWindow,
    track: &Track,
    _reference: Option<&Reference>,
) {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return;
    }

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
                if area.height >= 2 && label_x > right_most_label_onscreen_x + 1 {
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

fn get_rendering_info(window: &ViewingWindow, area: &Rect, gene: &Gene) -> Vec<TrackRenderInfo> {
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
        let mut exons_info: Vec<TrackRenderInfo> = Vec::new();
        let mut non_cds_exons_info: Vec<TrackRenderInfo> = Vec::new();
        let mut introns_info: Vec<TrackRenderInfo> = Vec::new();
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
