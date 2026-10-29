use crate::layout::CellView;
use crate::rendering::colors::ToColor;
use crate::{
    layout::{AlignmentView, OnScreenCoordinate},
    rendering::colors::Palette,
};
use gv_core::{prelude::*, strand::Strand};
use gv_viewer::prepare::track::{Gene, SegmentKind};
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

const MIN_AREA_WIDTH: u16 = 5;
const MIN_AREA_HEIGHT: u16 = 2;

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
    alignment_view: &AlignmentView,
    pallete: &Palette,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let mut right_most_label_onscreen_x = 0;
    let region = alignment_view.region(f64::from(area.width));
    for gene in Gene::in_region(state, &region)? {
        for context in get_rendering_info(alignment_view, area, &gene, pallete) {
            buf.set_string(
                context.x + area.x,
                area.y,
                context.string.clone(),
                context.style,
            );

            if let Some((label_x, label)) = context.label_info
                && area.height >= 2
                && label_x > right_most_label_onscreen_x + 1
            {
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

    Ok(())
}

const MIN_GENE_ON_SCREEN_LENGTH_TO_SHOW_EXONS: usize = 10;

fn get_rendering_info(
    alignment_view: &AlignmentView,
    area: &Rect,
    gene: &Gene,
    pallete: &Palette,
) -> Vec<TrackRenderContext> {
    let gene_start_x = alignment_view.onscreen_x_coordinate(gene.start, area);
    let gene_end_x = alignment_view.onscreen_x_coordinate(gene.end, area);

    let render_whole_gene = (OnScreenCoordinate::width(&gene_start_x, &gene_end_x, area)
        <= MIN_GENE_ON_SCREEN_LENGTH_TO_SHOW_EXONS)
        | !gene.has_exons;

    if render_whole_gene {
        if let Some((x, length)) =
            OnScreenCoordinate::onscreen_start_and_length(&gene_start_x, &gene_end_x, area)
        {
            let (string, style) =
                get_gene_segment_string_and_style(length, gene.strand.clone(), pallete);

            let label = gene.name.clone();
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
        let mut exons_info: Vec<TrackRenderContext> = Vec::new();
        let mut non_cds_exons_info: Vec<TrackRenderContext> = Vec::new();
        let mut introns_info: Vec<TrackRenderContext> = Vec::new();
        let mut right_most_label_onscreen_x = 0;
        for segment in &gene.segments {
            let feature_start_x = alignment_view.onscreen_x_coordinate(segment.start, area);
            let feature_end_x = alignment_view.onscreen_x_coordinate(segment.end, area);

            if let Some((x, length)) = OnScreenCoordinate::onscreen_start_and_length(
                &feature_start_x,
                &feature_end_x,
                area,
            ) {
                let (string, style) = get_feature_segment_string_and_style(
                    length,
                    gene.strand.clone(),
                    &segment.kind,
                    pallete,
                );

                match segment.kind {
                    SegmentKind::CodingExon => {
                        let label = format!("{}:exon{}", gene.name, segment.number);

                        let label_x = x + (length.saturating_sub(label.len() as u16) / 2);
                        let label_right_coordinate = label_x + label.len() as u16 - 1; // Inclusive.

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
                    SegmentKind::NoncodingExon => {
                        let label = gene.name.clone();
                        let label_x = x + (length.saturating_sub(label.len() as u16) / 2);
                        let label_right_coordinate = label_x + label.len() as u16 - 1; // Inclusive.

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
                    SegmentKind::Intron => {
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

    let style = Style::default().bg(pallete.GENE_BACKGROUND_COLOR.color());

    (string, style)
}

fn get_feature_segment_string_and_style(
    length: u16,
    strand: Strand,
    feature_type: &SegmentKind,
    pallete: &Palette,
) -> (String, Style) {
    let string = match (strand, feature_type) {
        (Strand::Forward, SegmentKind::CodingExon) => (0..length)
            .map(|i| if i % EXON_ARROW_GAP == 0 { ">" } else { " " })
            .collect::<String>(),
        (Strand::Forward, SegmentKind::NoncodingExon) => {
            (0..length).map(|_| "▅").collect::<String>()
        }
        (Strand::Forward, SegmentKind::Intron) => (0..length)
            .map(|i| if i % INTRON_ARROW_GAP == 0 { ">" } else { "-" })
            .collect::<String>(),
        (Strand::Reverse, SegmentKind::CodingExon) => (0..length)
            .map(|i| if i % EXON_ARROW_GAP == 0 { "<" } else { "-" })
            .collect::<String>(),
        (Strand::Reverse, SegmentKind::NoncodingExon) => {
            (0..length).map(|_| "▅").collect::<String>()
        }
        (Strand::Reverse, SegmentKind::Intron) => (0..length)
            .map(|i| if i % INTRON_ARROW_GAP == 0 { "<" } else { "-" })
            .collect::<String>(),
    };

    let style = match feature_type {
        SegmentKind::CodingExon => Style::default()
            .fg(pallete.EXON_FOREGROUND_COLOR.color())
            .bg(pallete.EXON_BACKGROUND_COLOR.color()),
        SegmentKind::Intron => Style::default().fg(pallete.INTRON_FOREGROUND_COLOR.color()),
        SegmentKind::NoncodingExon => {
            Style::default().fg(pallete.NON_CDS_EXON_BACKGROUND_COLOR.color())
        }
    };

    (string, style)
}
