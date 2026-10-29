//! What the sidebar says about an alignment track.

use crate::viewport::{Scale, Viewport};
use gv_core::{
    message::{AlignmentDisplayOption, AlignmentFilter, AlignmentSort},
    prelude::*,
};

/// How far down an alignment track's rows the view is, such as `25% (10 / 40)`.
pub fn alignment_depth_description<Z: Scale>(
    state: &State,
    alignment_view: &Viewport<Z>,
    index: usize,
) -> Result<String, TGVError> {
    let depth = match &state.paired_alignments[index] {
        Some(paired) => paired.depth()?,
        None => state.alignments[index].depth()?,
    };
    Ok(if depth == 0 {
        "0% (0 / 0)".to_string()
    } else {
        let y = usize::min(alignment_view.top(index), depth.saturating_sub(1)) + 1;
        format!("{}% ({} / {})", y as u128 * 100 / depth as u128, y, depth)
    })
}

/// Describe an alignment display option in a few words.
pub fn describe_option(option: &AlignmentDisplayOption) -> String {
    match option {
        AlignmentDisplayOption::ViewAsPairs => "Paired".to_string(),
        AlignmentDisplayOption::Sort(sort) => match sort {
            AlignmentSort::BaseAt(position) => format!("Sorted by base at {position}"),
            AlignmentSort::StrandAt(position) => format!("Sorted by strand at {position}"),
            AlignmentSort::Start(position) => format!("Sorted by start at {position}"),
            AlignmentSort::MappingQuality(position) => format!("Sorted by MAPQ at {position}"),
            AlignmentSort::InsertSize(position) => {
                format!("Sorted by insert size at {position}")
            }
            AlignmentSort::ReadName(position) => format!("Sorted by name at {position}"),
            sort => format!("Sorted by {sort}"),
        },
        AlignmentDisplayOption::Filter(AlignmentFilter::Base(position, base)) => {
            format!("Only {base} at {position}")
        }
        AlignmentDisplayOption::Filter(AlignmentFilter::BaseSoftclip(position)) => {
            format!("Only soft clips at {position}")
        }
        option => option.to_string(),
    }
}
