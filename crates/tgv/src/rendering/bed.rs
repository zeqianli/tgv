use crate::layout::CellView;
use crate::rendering::colors::to_color;
use crate::{
    layout::{AlignmentView, OnScreenCoordinate},
    rendering::{colors::Palette, render_zoom_in_notice},
};
use gv_core::{bed::BedTable, strand::Strand};
use gv_viewer::viewport::TrackView;
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
    track_view: TrackView,
    palette: &Palette,
) -> Result<(), crate::Error> {
    let region = alignment_view.region(f64::from(area.width));
    if !bed.has_complete_data(&region) {
        render_zoom_in_notice(area, buf, "BED features");
        return Ok(());
    }
    let rows = track_view.visible_rows(area.height as usize);
    let style = Style::default().bg(to_color(palette.BED)).fg(Color::White);
    for feature in bed.features(
        region.contig_index(),
        region.start(),
        region.end(),
        rows.clone(),
    )? {
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
        buf.set_string(
            area.x + x,
            area.y + (feature.y - rows.start) as u16,
            text.into_iter().collect::<String>(),
            style,
        );
    }
    Ok(())
}
