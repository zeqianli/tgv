use crate::{
    layout::{AlignmentView, OnScreenCoordinate},
    rendering::colors::Palette,
};
use gv_core::{gene::query_segments, prelude::*, strand::Strand};
use polars::prelude::DataFrame;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};
use std::{collections::HashMap, ops::Range};

enum SegmentKind {
    CodingExon,
    NoncodingExon,
    Intron,
}

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
    let region = alignment_view.region(area);
    let rows = state
        .track
        .query(region.contig_index(), region.start(), region.end())?;
    let segments = query_segments(rows.clone(), region.start(), region.end())?;
    let segment_gene_ids = segments.column("gene_row_id")?.u64()?;
    let mut segment_ranges: HashMap<u64, Range<usize>> = HashMap::new();
    for (row, id) in segment_gene_ids.into_no_null_iter().enumerate() {
        segment_ranges
            .entry(id)
            .and_modify(|range| range.end = row + 1)
            .or_insert(row..row + 1);
    }
    let gene_ids = rows.column("row_id")?.u64()?;
    for (row, id) in gene_ids.into_no_null_iter().enumerate() {
        let segment_rows = segment_ranges.get(&id).cloned().unwrap_or(0..0);
        for context in get_rendering_info(
            alignment_view,
            area,
            &rows,
            row,
            &segments,
            segment_rows,
            pallete,
        )? {
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
    genes: &DataFrame,
    row: usize,
    segments: &DataFrame,
    segment_rows: Range<usize>,
    pallete: &Palette,
) -> Result<Vec<TrackRenderContext>, TGVError> {
    let start = genes
        .column("start")?
        .u64()?
        .get(row)
        .expect("gene starts are non-null");
    let end = genes
        .column("end")?
        .u64()?
        .get(row)
        .expect("gene ends are non-null");
    let name = genes
        .column("name")?
        .str()?
        .get(row)
        .expect("gene names are non-null");
    let strand = Strand::from_str(
        genes
            .column("strand")?
            .str()?
            .get(row)
            .expect("gene strands are non-null")
            .to_owned(),
    )?;
    let has_exons = genes
        .column("has_exons")?
        .bool()?
        .get(row)
        .expect("exon availability is non-null");

    let gene_start_x = alignment_view.onscreen_x_coordinate(start, area);
    let gene_end_x = alignment_view.onscreen_x_coordinate(end, area);

    let render_whole_gene = (OnScreenCoordinate::width(&gene_start_x, &gene_end_x, area)
        <= MIN_GENE_ON_SCREEN_LENGTH_TO_SHOW_EXONS)
        | !has_exons;

    if render_whole_gene {
        if let Some((x, length)) =
            OnScreenCoordinate::onscreen_start_and_length(&gene_start_x, &gene_end_x, area)
        {
            let (string, style) =
                get_gene_segment_string_and_style(length, strand.clone(), pallete);

            let label = name.to_owned();
            let label_x = x + (length.saturating_sub(label.len() as u16) / 2);

            Ok(vec![TrackRenderContext {
                x,
                string,
                style,
                label_info: Some((label_x, label)),
            }])
        } else {
            Ok(vec![])
        }
    } else {
        let mut exons_info: Vec<TrackRenderContext> = Vec::new();
        let mut non_cds_exons_info: Vec<TrackRenderContext> = Vec::new();
        let mut introns_info: Vec<TrackRenderContext> = Vec::new();
        let mut right_most_label_onscreen_x = 0;
        let starts = segments.column("start")?.u64()?;
        let ends = segments.column("end")?.u64()?;
        let kinds = segments.column("kind")?.str()?;
        let indexes = segments.column("feature_index")?.u64()?;
        for row in segment_rows {
            let feature_start = starts.get(row).expect("segment starts are non-null");
            let feature_end = ends.get(row).expect("segment ends are non-null");
            let feature_index = indexes.get(row).expect("segment indexes are non-null");
            let feature_type = match kinds.get(row).expect("segment kinds are non-null") {
                "coding_exon" => SegmentKind::CodingExon,
                "noncoding_exon" => SegmentKind::NoncodingExon,
                "intron" => SegmentKind::Intron,
                kind => {
                    return Err(TGVError::ValueError(format!(
                        "Unknown gene segment kind: {kind}."
                    )));
                }
            };
            let feature_start_x = alignment_view.onscreen_x_coordinate(feature_start, area);
            let feature_end_x = alignment_view.onscreen_x_coordinate(feature_end, area);

            if let Some((x, length)) = OnScreenCoordinate::onscreen_start_and_length(
                &feature_start_x,
                &feature_end_x,
                area,
            ) {
                let (string, style) = get_feature_segment_string_and_style(
                    length,
                    strand.clone(),
                    &feature_type,
                    pallete,
                );

                match feature_type {
                    SegmentKind::CodingExon => {
                        let label = format!("{}:exon{}", name, feature_index);

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
                        let label = name.to_owned();
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

        Ok(introns_info
            .into_iter()
            .chain(non_cds_exons_info)
            .chain(exons_info)
            .collect())
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
            .fg(pallete.EXON_FOREGROUND_COLOR)
            .bg(pallete.EXON_BACKGROUND_COLOR),
        SegmentKind::Intron => Style::default().fg(pallete.INTRON_FOREGROUND_COLOR),
        SegmentKind::NoncodingExon => Style::default().fg(pallete.NON_CDS_EXON_BACKGROUND_COLOR),
    };

    (string, style)
}
