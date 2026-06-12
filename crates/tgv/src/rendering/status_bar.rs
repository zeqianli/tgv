use gv_core::{error::TGVError, state::State};

use itertools::Itertools;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

use crate::layout::AlignmentView;

pub fn render_status_bar(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    alignment_view: &AlignmentView,
) -> Result<(), TGVError> {
    if area.width < 1 || area.height < 2 {
        return Ok(());
    }

    // Messages
    let index_start = state.messages.len().saturating_sub(area.height as usize);
    let index_end = state.messages.len();

    if index_start < index_end {
        for (i, error) in state.messages[index_start..index_end].iter().enumerate() {
            if i >= area.height as usize {
                break;
            }
            buf.set_string(area.x, area.y + i as u16, error.clone(), Style::default());
        }
    }

    // X and y coordinates

    let x_coordinate_string = format!(
        "{}: {}",
        state.contig_name(&alignment_view.focus)?,
        alignment_view.focus.position
    );
    let first_alignment = state.alignments.first();
    let first_alignment_options = state.alignment_options.first();

    let mut y_coordinate_string = if first_alignment.is_none_or(|alignment| alignment.depth() == 0)
    {
        "".to_string()
    } else {
        let y = alignment_view.top() + 1; // Change to 1-base
        let depth = first_alignment.unwrap().depth();
        let percent = y * 100 / depth;
        format!("{}% ({} / {})", percent, y, depth)
    };

    // Alignment options

    if let Some(alignment_options) = first_alignment_options
        && !alignment_options.is_empty()
    {
        let alignment_option_string = alignment_options
            .iter()
            .map(|option| format!("{}", option))
            .join(",");

        y_coordinate_string = y_coordinate_string + " (" + &alignment_option_string + ")";
    }

    if area.height == 1 {
        let string = x_coordinate_string + "  " + &y_coordinate_string;
        buf.set_string(
            area.x + area.width.saturating_sub(string.len() as u16),
            area.y,
            string,
            Style::default(),
        );
    } else if area.height > 1 {
        buf.set_string(
            area.x + area.width.saturating_sub(x_coordinate_string.len() as u16),
            area.y,
            x_coordinate_string,
            Style::default(),
        );

        buf.set_string(
            area.x + area.width.saturating_sub(y_coordinate_string.len() as u16),
            area.y + 1,
            y_coordinate_string,
            Style::default(),
        );
    }

    Ok(())
}
