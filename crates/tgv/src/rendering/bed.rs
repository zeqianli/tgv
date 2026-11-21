use crate::layout::CellView;
use crate::rendering::colors::ToColor;
use crate::{
    layout::{AlignmentView, OnScreenCoordinate},
    rendering::{colors::Palette, intervals::render_zoom_in_notice},
};
use gv_core::{bed::BedTable, strand::Strand};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

pub fn render_bed(
    area: &Rect,
    buf: &mut Buffer,
    bed: &BedTable,
    alignment_view: &AlignmentView,
    palette: &Palette,
) -> Result<(), crate::Error> {
    let region = alignment_view.region(f64::from(area.width));
    if !bed.has_complete_data(&region) {
        render_zoom_in_notice(area, buf, "BED features");
        return Ok(());
    }
    let colors = [palette.BED1.color(), palette.BED2.color()];
    for (row, feature) in bed
        .features(region.contig_index(), region.start(), region.end())?
        .iter()
        .enumerate()
    {
        let start = alignment_view.onscreen_x_coordinate(feature.start, area);
        let end = alignment_view.onscreen_x_coordinate(feature.end, area);
        let Some((x, width)) = OnScreenCoordinate::onscreen_start_and_length(&start, &end, area)
        else {
            continue;
        };
        let width = usize::from(width);
        let fill = match feature.strand {
            Some(Strand::Forward) => '›',
            Some(Strand::Reverse) => '‹',
            None => ' ',
        };
        let mut text: Vec<char> = vec![fill; width];
        // Names are placed by character count, so only ASCII names are drawn.
        if let Some(name) = feature.name.as_deref()
            && name.is_ascii()
            && name.len() + 2 <= width
        {
            let offset = (width - name.len()) / 2;
            text.splice(offset..offset + name.len(), name.chars());
        }
        // Colors alternate by on-screen order, because row IDs of indexed files change between
        // loads.
        buf.set_string(
            area.x + x,
            area.y,
            text.into_iter().collect::<String>(),
            Style::default().bg(colors[row % 2]).fg(Color::White),
        );
    }
    Ok(())
}
