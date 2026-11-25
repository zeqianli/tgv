use super::prelude::*;
use crate::layout::GENE_ROW_HEIGHT;
use gv_core::{prelude::*, strand::Strand};
use gv_viewer::{
    prepare::track::{Gene, SegmentKind},
    viewport::TrackView,
};

/// Genes narrower than this show as one bar, without exons.
const MIN_GENE_ON_SCREEN_WIDTH_TO_SHOW_EXONS: f32 = 30.0;
/// The space between strand chevrons along a gene.
const CHEVRON_SPACING: f32 = 30.0;
const EXON_HEIGHT: f32 = 14.0;
const NONCODING_EXON_HEIGHT: f32 = 8.0;
const GENE_HEIGHT: f32 = 10.0;

/// Render the genome features: genes with their exons and introns, and labels below.
pub fn render_track(
    area: &Rect,
    state: &State,
    view: &PixelView,
    track_view: TrackView,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let rows = track_view.visible_rows((area.height / GENE_ROW_HEIGHT).ceil() as usize);
    let mut right_most_label_x = vec![f32::MIN; (rows.end - rows.start) as usize];
    let region = view.region(area);
    for gene in Gene::in_region(state, &region, rows.clone())? {
        let Some((gene_x, gene_width)) = view.span(gene.start, gene.end, area) else {
            continue;
        };
        let row = (gene.y - rows.start) as usize;
        let row_top = area.y + row as f32 * GENE_ROW_HEIGHT;
        let middle = row_top + EXON_HEIGHT / 2.0 + 2.0;
        let label_y = row_top + EXON_HEIGHT + 4.0;
        let chevron = match gene.strand {
            Strand::Forward => "›",
            Strand::Reverse => "‹",
        };
        let mut labels = Vec::new();
        if gene_width <= MIN_GENE_ON_SCREEN_WIDTH_TO_SHOW_EXONS || !gene.has_exons {
            fill_rect(
                Rect::new(gene_x, middle - GENE_HEIGHT / 2.0, gene_width, GENE_HEIGHT),
                to_hsla(palette.GENE_BACKGROUND_COLOR),
                window,
            );
            labels.push((gene_x + gene_width / 2.0, gene.name.clone()));
        } else {
            // Introns go under noncoding exons, which go under coding exons.
            for kind in [
                SegmentKind::Intron,
                SegmentKind::NoncodingExon,
                SegmentKind::CodingExon,
            ] {
                for segment in gene.segments.iter().filter(|segment| segment.kind == kind) {
                    let Some((x, width)) = view.span(segment.start, segment.end, area) else {
                        continue;
                    };
                    match kind {
                        SegmentKind::Intron => {
                            fill_rect(
                                Rect::new(x, middle - 0.5, width, 1.0),
                                to_hsla(palette.INTRON_FOREGROUND_COLOR),
                                window,
                            );
                            let mut chevron_x = x + CHEVRON_SPACING / 2.0;
                            while chevron_x + 4.0 < x + width {
                                TextContext::default()
                                    .text(chevron)
                                    .color(palette.INTRON_FOREGROUND_COLOR)
                                    .paint(
                                        chevron_x - 2.0,
                                        middle - LINE_HEIGHT / 2.0,
                                        LINE_HEIGHT,
                                        window,
                                        cx,
                                    )?;
                                chevron_x += CHEVRON_SPACING;
                            }
                        }
                        SegmentKind::NoncodingExon => {
                            fill_rect(
                                Rect::new(
                                    x,
                                    middle - NONCODING_EXON_HEIGHT / 2.0,
                                    width,
                                    NONCODING_EXON_HEIGHT,
                                ),
                                to_hsla(palette.NON_CDS_EXON_BACKGROUND_COLOR),
                                window,
                            );
                            labels.push((x + width / 2.0, gene.name.clone()));
                        }
                        SegmentKind::CodingExon => {
                            fill_rect(
                                Rect::new(x, middle - EXON_HEIGHT / 2.0, width, EXON_HEIGHT),
                                to_hsla(palette.EXON_BACKGROUND_COLOR),
                                window,
                            );
                            labels.push((
                                x + width / 2.0,
                                format!("{}:exon{}", gene.name, segment.number),
                            ));
                        }
                    }
                }
            }
        }
        // Labels go left to right, and a label that would overlap the previous one is left out.
        labels.sort_by(|(a, _), (b, _)| a.total_cmp(b));
        for (center, label) in labels {
            let width = TextContext::default().text(&label).width(window);
            let x = (center - width / 2.0).max(area.x);
            if x > right_most_label_x[row] + 6.0 && label_y + LINE_HEIGHT <= area.bottom() {
                TextContext::default().text(&label).color(TEXT).paint(
                    x,
                    label_y,
                    LINE_HEIGHT,
                    window,
                    cx,
                )?;
                right_most_label_x[row] = x + width;
            }
        }
    }
    Ok(())
}
