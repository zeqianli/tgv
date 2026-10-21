//! The items of right-click context menus.
//!
//! A menu is built once when it opens, from the clicked target and the current state. Each
//! front end draws the items and turns clicks on them into their actions.

use crate::message::{Action, AlignmentOptionUpdate, UpdateLayoutAction};
use gv_core::{
    alignment::{Alignment, CoverageSchema},
    message::{AlignmentDisplayOption, AlignmentFilter, AlignmentSort},
    prelude::*,
};
use polars::prelude::ChunkAgg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    /// An item that sends `action` when clicked. `None` shows the item as disabled.
    Entry {
        label: String,
        action: Option<Action>,
    },
    Submenu {
        label: String,
        items: Vec<MenuItem>,
    },
}

impl MenuItem {
    pub fn entry(label: impl Into<String>, action: Option<Action>) -> Self {
        Self::Entry {
            label: label.into(),
            action,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Entry { label, .. } | Self::Submenu { label, .. } => label,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::Entry { action, .. } => action.is_some(),
            Self::Submenu { .. } => true,
        }
    }
}

/// Items for an alignment track's reads or coverage, clicked at `position`.
pub fn alignment_items(
    track: TrackId,
    position: Option<u64>,
    alignment: &Alignment,
    options: &[AlignmentDisplayOption],
) -> Result<Vec<MenuItem>, TGVError> {
    let update = |update| Action::UpdateAlignmentOptions { track, update };
    // Without a single clicked base, the base items have no position to name.
    let at = position.map_or_else(String::new, |position| format!(" at {position}"));
    let sort_label = format!("Sort{at}");
    let mut items = vec![match position {
        Some(position) => MenuItem::Submenu {
            label: sort_label,
            items: [
                ("Base", AlignmentSort::BaseAt(position)),
                ("Strand", AlignmentSort::StrandAt(position)),
                ("Start", AlignmentSort::Start(position)),
                ("Mapping quality", AlignmentSort::MappingQuality(position)),
                ("Insert size", AlignmentSort::InsertSize(position)),
                ("Read name", AlignmentSort::ReadName(position)),
            ]
            .into_iter()
            .map(|(label, sort)| {
                MenuItem::entry(label, Some(update(AlignmentOptionUpdate::Sort(sort))))
            })
            .collect(),
        },
        None => MenuItem::entry(sort_label, None),
    }];

    let filter_label = format!("Filter by base{at}");
    items.push(match position {
        Some(position) => {
            let coverage = alignment.coverage.query(position, position)?;
            let mut counts = Vec::new();
            for (column, label, filter) in [
                (CoverageSchema::A, "A", AlignmentFilter::Base(position, 'A')),
                (CoverageSchema::C, "C", AlignmentFilter::Base(position, 'C')),
                (CoverageSchema::G, "G", AlignmentFilter::Base(position, 'G')),
                (CoverageSchema::T, "T", AlignmentFilter::Base(position, 'T')),
                (CoverageSchema::N, "N", AlignmentFilter::Base(position, 'N')),
                (
                    CoverageSchema::SOFTCLIP,
                    "Soft clip",
                    AlignmentFilter::BaseSoftclip(position),
                ),
            ] {
                let count = coverage.column(column)?.u64()?.sum().unwrap_or(0);
                if count > 0 {
                    counts.push((count, label, filter));
                }
            }
            // The most common choices come first.
            counts.sort_by_key(|(count, _, _)| std::cmp::Reverse(*count));
            if counts.is_empty() {
                MenuItem::entry(filter_label, None)
            } else {
                MenuItem::Submenu {
                    label: filter_label,
                    items: counts
                        .into_iter()
                        .map(|(count, label, filter)| {
                            MenuItem::entry(
                                format!("{label} ({count})"),
                                Some(update(AlignmentOptionUpdate::Filter(filter))),
                            )
                        })
                        .collect(),
                }
            }
        }
        None => MenuItem::entry(filter_label, None),
    });

    let paired = options.contains(&AlignmentDisplayOption::ViewAsPairs);
    items.push(MenuItem::entry(
        if paired {
            "✓ View as pairs"
        } else {
            "View as pairs"
        },
        Some(update(AlignmentOptionUpdate::TogglePaired)),
    ));
    items.push(MenuItem::entry(
        "Reset display options",
        (!options.is_empty()).then(|| update(AlignmentOptionUpdate::Reset)),
    ));
    Ok(items)
}

/// Items that act on a file track: copying it, pasting the copied track below it, and
/// removing it. `copied` is the copied track's name. A modified track is removed without
/// writing, and its item says so.
pub fn track_items(
    track: TrackId,
    file_name: &str,
    copyable: bool,
    modified: bool,
    copied: Option<&str>,
) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if copyable {
        items.push(MenuItem::entry(
            format!("Copy {file_name}"),
            Some(Action::CopyTrack(track)),
        ));
    }
    if let Some(copied) = copied {
        items.push(MenuItem::entry(
            format!("Paste {copied} below"),
            Some(Action::PasteTrackBelow(track)),
        ));
    }
    items.push(MenuItem::entry(
        if modified {
            format!("Remove {file_name} (discard changes)")
        } else {
            format!("Remove {file_name}")
        },
        Some(Action::RemoveTrack {
            track,
            force: modified,
        }),
    ));
    items
}

/// Items for the sidebar. A file track's section has the track's items and items to insert
/// a track next to it, then the sidebar's own.
pub fn sidebar_items(track: Option<(TrackId, Vec<MenuItem>)>) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if let Some((track, track_items)) = track {
        items.extend(track_items);
        items.push(MenuItem::entry(
            "Insert track above",
            Some(Action::NewTrackBeside {
                track,
                below: false,
            }),
        ));
        items.push(MenuItem::entry(
            "Insert track below",
            Some(Action::NewTrackBeside { track, below: true }),
        ));
    }
    items.push(MenuItem::entry(
        "Hide sidebar (s)",
        Some(UpdateLayoutAction::ToggleSidebar.into()),
    ));
    items
}
