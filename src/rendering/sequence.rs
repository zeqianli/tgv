use crate::models::region::Region;
use crate::models::sequence::Sequence;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};
pub fn render_sequence(
    area: &Rect,
    buf: &mut Buffer,
    region: &Region,
    sequence: &Sequence,
) -> Result<(), ()> {
    buf.set_string(
        area.x,
        area.y,
        sequence.get_sequence(region).ok_or(())?,
        Style::default(),
    );
    Ok(())
}
