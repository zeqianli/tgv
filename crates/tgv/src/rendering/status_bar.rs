use gv_core::state::State;
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

pub(crate) fn alignment_depth_description(
    state: &State,
    alignment_view: &AlignmentView,
    index: usize,
) -> String {
    let depth = state.alignments[index].depth();
    let mut description = if depth == 0 {
        "0% (0 / 0)".to_string()
    } else {
        let y = usize::min(alignment_view.top(index), depth.saturating_sub(1)) + 1;
        format!("{}% ({} / {})", y as u128 * 100 / depth as u128, y, depth)
    };
    if !state.alignment_options[index].is_empty() {
        let options = state.alignment_options[index]
            .iter()
            .map(|option| format!("{option}"))
            .join(",");
        description.push_str(&format!(" ({options})"));
    }
    description
}
