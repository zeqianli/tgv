use crate::{
    layout::{PixelView, Rect},
    rendering::{
        LINE_HEIGHT, TextContext,
        colors::{Palette, TEXT, ToHsla},
        fill_rect,
    },
};
use gpui::{App, Window};
use gv_core::{prelude::*, strand::Strand};
use gv_viewer::prepare::track::{Gene, SegmentKind};

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
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), gv_core::Error> {
    let middle = area.y + EXON_HEIGHT / 2.0 + 2.0;
    let label_y = area.y + EXON_HEIGHT + 4.0;
    let mut right_most_label_x = f32::MIN;
    let region = view.region(area);
    for gene in Gene::in_region(state, &region)? {
        let Some((gene_x, gene_width)) = view.span(gene.start, gene.end, area) else {
            continue;
        };
        let chevron = match gene.strand {
            Strand::Forward => "›",
            Strand::Reverse => "‹",
        };
        let mut labels = Vec::new();
        if gene_width <= MIN_GENE_ON_SCREEN_WIDTH_TO_SHOW_EXONS || !gene.has_exons {
            fill_rect(
                Rect::new(gene_x, middle - GENE_HEIGHT / 2.0, gene_width, GENE_HEIGHT),
                palette.GENE_BACKGROUND_COLOR.hsla(),
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
                                palette.INTRON_FOREGROUND_COLOR.hsla(),
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
                                palette.NON_CDS_EXON_BACKGROUND_COLOR.hsla(),
                                window,
                            );
                            labels.push((x + width / 2.0, gene.name.clone()));
                        }
                        SegmentKind::CodingExon => {
                            fill_rect(
                                Rect::new(x, middle - EXON_HEIGHT / 2.0, width, EXON_HEIGHT),
                                palette.EXON_BACKGROUND_COLOR.hsla(),
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
            if x > right_most_label_x + 6.0 && area.height >= label_y - area.y + LINE_HEIGHT {
                TextContext::default().text(&label).color(TEXT).paint(
                    x,
                    label_y,
                    LINE_HEIGHT,
                    window,
                    cx,
                )?;
                right_most_label_x = x + width;
            }
        }
    }
    Ok(())
}
