use gv_core::prelude::*;
use itertools::Itertools;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

use crate::layout::AlignmentView;

pub fn render_status_bar(area: &Rect, buf: &mut Buffer, state: &State) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // Messages
    let index_start = state.messages.len().saturating_sub(area.height as usize);
    let index_end = state.messages.len();

    if index_start < index_end {
        for (i, error) in state.messages[index_start..index_end].iter().enumerate() {
            if i >= area.height as usize {
                break;
            }
            buf.set_stringn(
                area.x,
                area.y + i as u16,
                error,
                area.width as usize,
                Style::default(),
            );
        }
    }
}
