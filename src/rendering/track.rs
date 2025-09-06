use crate::intervals::GenomeInterval;
use crate::{
    error::TGVError,
    feature::{Gene, SubGeneFeatureType},
    rendering::colors::Palette,
    states::State,
    strand::Strand,
    window::{OnScreenCoordinate, ViewingWindow},
};
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

const MIN_AREA_WIDTH: u16 = 5;
const MIN_AREA_HEIGHT: u16 = 2;

// Type alias for the complex return type
struct TrackRenderContext {
    x: u16,
    string: String,
    style: Style,

    // Gene label below the gene segment.
    label_info: Option<(u16, String)>,
}

/// Render the genome features.
pub fn render_track(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    pallete: &Palette,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let track = state.track_checked()?;

    let mut right_most_label_onscreen_x = 0;
    for feature in track.genes().iter() {
        for context in get_rendering_info(&state.window, area, feature, pallete) {
            buf.set_string(
                context.x + area.x,
                area.y,
                context.string.clone(),
                context.style,
            );

            if let Some((label_x, label)) = context.label_info {
                if area.height >= 2 && label_x > right_most_label_onscreen_x + 1 {
                    right_most_label_onscreen_x = label_x + label.len() as u16 - 1;

                    buf.set_string(
                        label_x + area.x,
                        area.y + 1,
                        label.clone(),
                        Style::default(),
                    );
                }
            }
        }
    }

    Ok(())
}

const MIN_GENE_ON_SCREEN_LENGTH_TO_SHOW_EXONS: usize = 10;

fn get_rendering_info(
    window: &ViewingWindow,
    area: &Rect,
    gene: &Gene,
    pallete: &Palette,
) -> Vec<TrackRenderContext> {
    // First, check if the gene should be rendered as a single segment or multiple segments.

    let gene_start_x = window.onscreen_x_coordinate(gene.start(), area);
    let gene_end_x = window.onscreen_x_coordinate(gene.end(), area);

    let render_whole_gene = (OnScreenCoordinate::width(&gene_start_x, &gene_end_x, area)
        <= MIN_GENE_ON_SCREEN_LENGTH_TO_SHOW_EXONS)
        | !gene.has_exons;

    if render_whole_gene {
        if let Some((x, length)) =
            OnScreenCoordinate::onscreen_start_and_length(&gene_start_x, &gene_end_x, area)
        {
            let (string, style) =
                get_gene_segment_string_and_style(length, gene.strand.clone(), pallete);

            // label x and text
            let label = gene.name.to_string();
            let label_x = x + (length.saturating_sub(label.len() as u16) / 2);

            vec![TrackRenderContext {
                x,
                string,
                style,
                label_info: Some((label_x, label)),
            }]
        } else {
            vec![]
        }
    } else {
        // Render each exon as a separate segment.
        let mut exons_info: Vec<TrackRenderContext> = Vec::new();
        let mut non_cds_exons_info: Vec<TrackRenderContext> = Vec::new();
        let mut introns_info: Vec<TrackRenderContext> = Vec::new();
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
                    pallete,
                );

                match feature_type {
                    SubGeneFeatureType::Exon => {
                        let label = format!("{}:exon{}", gene.name, feature_index);

                        let label_x = x + (length.saturating_sub(label.len() as u16) / 2);
                        let label_right_coordinate = label_x + label.len() as u16 - 1; // inclusive

                        exons_info.push(TrackRenderContext {
                            x,
                            string,
                            style,
                            label_info: if label_x > right_most_label_onscreen_x + 1 {
                                right_most_label_onscreen_x = label_right_coordinate;

                                Some((label_x, label))
                            } else {
                                None
                            },
                        });
                    }
                    SubGeneFeatureType::NonCDSExon => {
                        let label = gene.name.to_string();
                        let label_x = x + (length.saturating_sub(label.len() as u16) / 2);
                        let label_right_coordinate = label_x + label.len() as u16 - 1; // inclusive

                        non_cds_exons_info.push(TrackRenderContext {
                            x,
                            string,
                            style,
                            label_info: if label_x > right_most_label_onscreen_x + 1 {
                                right_most_label_onscreen_x = label_right_coordinate;

                                Some((label_x, label))
                            } else {
                                None
                            },
                        });
                    }
                    SubGeneFeatureType::Intron => {
                        introns_info.push(TrackRenderContext {
                            x,
                            string,
                            style,
                            label_info: None,
                        });
                    }
                }
            }
        }

        // The order decides rendering order.
        // Exons are on top of non-CDS exons, on top of introns.

        introns_info
            .into_iter()
            .chain(non_cds_exons_info)
            .chain(exons_info)
            .collect()
    }
}

const EXON_ARROW_GAP: u16 = 5;
const INTRON_ARROW_GAP: u16 = 10;
const GENE_ARROW_GAP: u16 = 5;

fn get_gene_segment_string_and_style(
    length: u16,
    strand: Strand,
    pallete: &Palette,
) -> (String, Style) {
    let string = match strand {
        Strand::Forward => (0..length)
            .map(|i| if i % GENE_ARROW_GAP == 0 { ">" } else { " " })
            .collect::<String>(),
        Strand::Reverse => (0..length)
            .map(|i| if i % GENE_ARROW_GAP == 0 { "<" } else { " " })
            .collect::<String>(),
    };

    let style = Style::default().bg(pallete.GENE_BACKGROUND_COLOR);

    (string, style)
}

fn get_feature_segment_string_and_style(
    length: u16,
    strand: Strand,
    feature_type: &SubGeneFeatureType,
    pallete: &Palette,
) -> (String, Style) {
    let string = match (strand, feature_type) {
        (Strand::Forward, SubGeneFeatureType::Exon) => (0..length)
            .map(|i| if i % EXON_ARROW_GAP == 0 { ">" } else { " " })
            .collect::<String>(),
        (Strand::Forward, SubGeneFeatureType::NonCDSExon) => {
            (0..length).map(|_| "▅").collect::<String>()
        }
        (Strand::Forward, SubGeneFeatureType::Intron) => (0..length)
            .map(|i| if i % INTRON_ARROW_GAP == 0 { ">" } else { "-" })
            .collect::<String>(),
        (Strand::Reverse, SubGeneFeatureType::Exon) => (0..length)
            .map(|i| if i % EXON_ARROW_GAP == 0 { "<" } else { "-" })
            .collect::<String>(),
        (Strand::Reverse, SubGeneFeatureType::NonCDSExon) => {
            (0..length).map(|_| "▅").collect::<String>()
        }
        (Strand::Reverse, SubGeneFeatureType::Intron) => (0..length)
            .map(|i| if i % INTRON_ARROW_GAP == 0 { "<" } else { "-" })
            .collect::<String>(),
    };

    let style = match feature_type {
        SubGeneFeatureType::Exon => Style::default()
            .fg(pallete.EXON_FOREGROUND_COLOR)
            .bg(pallete.EXON_BACKGROUND_COLOR),
        SubGeneFeatureType::Intron => Style::default().fg(pallete.INTRON_FOREGROUND_COLOR),
        SubGeneFeatureType::NonCDSExon => {
            Style::default().fg(pallete.NON_CDS_EXON_BACKGROUND_COLOR)
        }
    };

    (string, style)
}
