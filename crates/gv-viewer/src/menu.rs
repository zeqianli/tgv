//! The items of right-click context menus.
//!
//! A menu is built once when it opens, from the clicked target and the current state. Each
//! front end draws the items and turns clicks on them into their actions.

use crate::{
    message::{
        Action, AlignmentOptionUpdate, FocusTarget, TrackAction, UiAction, UpdateLayoutAction,
    },
    viewport::RowLayout,
};
use gv_core::{
    alignment::{Alignment, BaseCoverage},
    message::{AlignmentDisplayOption, AlignmentFilter, AlignmentSort},
    prelude::*,
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuLevel {
    pub items: Vec<MenuItem>,
    pub hovered: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuHit {
    Item { level: usize, index: usize },
    Outside,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub levels: Vec<MenuLevel>,
}

impl Menu {
    pub fn new(items: Vec<MenuItem>) -> Self {
        Self {
            levels: vec![MenuLevel {
                items,
                hovered: None,
            }],
        }
    }

    pub fn hover(&mut self, hit: MenuHit) -> bool {
        let before: Vec<Option<usize>> = self.levels.iter().map(|level| level.hovered).collect();
        match hit {
            MenuHit::Item { level, index } => {
                if self.levels[level].hovered != Some(index) {
                    self.levels.truncate(level + 1);
                    self.levels[level].hovered = Some(index);
                    if let MenuItem::Submenu { items, .. } = &self.levels[level].items[index] {
                        let items = items.clone();
                        self.levels.push(MenuLevel {
                            items,
                            hovered: None,
                        });
                    }
                }
            }
            MenuHit::Outside => {
                if self.levels.len() > 1
                    && let Some(deepest) = self.levels.last_mut()
                {
                    deepest.hovered = None;
                }
            }
        }
        self.levels.iter().map(|level| level.hovered).ne(before)
    }

    pub fn click(&self, hit: MenuHit) -> Vec<Action> {
        let MenuHit::Item { level, index } = hit else {
            return vec![UiAction::CloseContextMenu.into()];
        };
        match &self.levels[level].items[index] {
            MenuItem::Entry {
                action: Some(action),
                ..
            } => vec![action.clone(), UiAction::CloseContextMenu.into()],
            _ => Vec::new(),
        }
    }
}

/// Items for an alignment track's reads or coverage, clicked at `position`.
pub fn alignment_items(
    track: TrackId,
    position: Option<u64>,
    alignment: &Alignment,
    options: &[AlignmentDisplayOption],
) -> Result<Vec<MenuItem>, crate::Error> {
    let update = |update| Action::Track(TrackAction::UpdateAlignmentOptions { track, update });
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
            let bases = alignment.coverage.bases(position, position)?;
            let mut counts = Vec::new();
            for (count, label, filter) in [
                (
                    (|base| base.a) as fn(&BaseCoverage) -> u64,
                    "A",
                    AlignmentFilter::Base(position, 'A'),
                ),
                (|base| base.c, "C", AlignmentFilter::Base(position, 'C')),
                (|base| base.g, "G", AlignmentFilter::Base(position, 'G')),
                (|base| base.t, "T", AlignmentFilter::Base(position, 'T')),
                (|base| base.n, "N", AlignmentFilter::Base(position, 'N')),
                (
                    |base| base.softclip,
                    "Soft clip",
                    AlignmentFilter::BaseSoftclip(position),
                ),
            ] {
                let count = bases.iter().map(count).sum::<u64>();
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

pub fn row_layout_item(target: FocusTarget, rows: RowLayout) -> MenuItem {
    let (label, next) = match rows {
        RowLayout::Collapsed => ("Expand overlapping features", RowLayout::Expanded),
        RowLayout::Expanded => ("✓ Expand overlapping features", RowLayout::Collapsed),
    };
    MenuItem::entry(
        label,
        Some(Action::Track(TrackAction::SetRowLayout {
            target,
            rows: next,
        })),
    )
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
            Some(Action::Track(TrackAction::CopyTrack(track))),
        ));
    }
    if let Some(copied) = copied {
        items.push(MenuItem::entry(
            format!("Paste {copied} below"),
            Some(Action::Track(TrackAction::PasteTrackBelow(track))),
        ));
    }
    items.push(MenuItem::entry(
        if modified {
            format!("Remove {file_name} (discard changes)")
        } else {
            format!("Remove {file_name}")
        },
        Some(Action::Track(TrackAction::RemoveTrack {
            track,
            force: modified,
        })),
    ));
    items
}

/// Items for the sidebar. A track's section has the track's items and, for a file track,
/// items to insert a track next to it, then the sidebar's own.
pub fn sidebar_items(track_items: Vec<MenuItem>, insert_beside: Option<TrackId>) -> Vec<MenuItem> {
    let mut items = track_items;
    if let Some(track) = insert_beside {
        items.push(MenuItem::entry(
            "Insert track above",
            Some(Action::Track(TrackAction::NewTrackBeside {
                track,
                below: false,
            })),
        ));
        items.push(MenuItem::entry(
            "Insert track below",
            Some(Action::Track(TrackAction::NewTrackBeside {
                track,
                below: true,
            })),
        ));
    }
    items.push(MenuItem::entry(
        "Hide sidebar (s)",
        Some(UpdateLayoutAction::ToggleSidebar.into()),
    ));
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn width(pixels: u16) -> Action {
        UpdateLayoutAction::SetSidebarWidth(pixels).into()
    }

    fn close() -> Action {
        UiAction::CloseContextMenu.into()
    }

    fn sort_items() -> Vec<MenuItem> {
        vec![
            MenuItem::entry("By name", Some(width(2))),
            MenuItem::Submenu {
                label: "More".to_string(),
                items: more_items(),
            },
        ]
    }

    fn more_items() -> Vec<MenuItem> {
        vec![MenuItem::entry("Deep", Some(width(3)))]
    }

    fn menu() -> Menu {
        Menu::new(vec![
            MenuItem::entry("Copy", Some(width(1))),
            MenuItem::entry("Disabled", None),
            MenuItem::Submenu {
                label: "Sort".to_string(),
                items: sort_items(),
            },
            MenuItem::Submenu {
                label: "Color".to_string(),
                items: vec![MenuItem::entry("Red", Some(width(4)))],
            },
        ])
    }

    fn at(level: usize, index: usize) -> MenuHit {
        MenuHit::Item { level, index }
    }

    fn hovered(menu: &Menu) -> Vec<Option<usize>> {
        menu.levels.iter().map(|level| level.hovered).collect()
    }

    #[test]
    fn new_opens_only_the_main_level_without_hover() {
        let menu = menu();
        assert_eq!(menu.levels.len(), 1);
        assert_eq!(hovered(&menu), vec![None]);
    }

    #[rstest]
    #[case::entry(vec![at(0, 0)], vec![true], vec![Some(0)])]
    #[case::same_item_twice(vec![at(0, 0), at(0, 0)], vec![true, false], vec![Some(0)])]
    #[case::disabled_item(vec![at(0, 1)], vec![true], vec![Some(1)])]
    #[case::submenu_opens(vec![at(0, 2)], vec![true], vec![Some(2), None])]
    #[case::into_submenu(vec![at(0, 2), at(1, 0)], vec![true, true], vec![Some(2), Some(0)])]
    #[case::nested_submenu_opens(
        vec![at(0, 2), at(1, 1)],
        vec![true, true],
        vec![Some(2), Some(1), None]
    )]
    #[case::into_nested_submenu(
        vec![at(0, 2), at(1, 1), at(2, 0)],
        vec![true, true, true],
        vec![Some(2), Some(1), Some(0)]
    )]
    #[case::back_to_parent_keeps_submenu(
        vec![at(0, 2), at(1, 0), at(0, 2)],
        vec![true, true, false],
        vec![Some(2), Some(0)]
    )]
    #[case::other_submenu_replaces_it(
        vec![at(0, 2), at(1, 1), at(0, 3)],
        vec![true, true, true],
        vec![Some(3), None]
    )]
    #[case::entry_closes_submenu(vec![at(0, 2), at(0, 0)], vec![true, true], vec![Some(0)])]
    #[case::sibling_closes_nested_submenu(
        vec![at(0, 2), at(1, 1), at(1, 0)],
        vec![true, true, true],
        vec![Some(2), Some(0)]
    )]
    #[case::outside_keeps_main_hover(vec![at(0, 0), MenuHit::Outside], vec![true, false], vec![Some(0)])]
    #[case::outside_clears_deepest_hover(
        vec![at(0, 2), at(1, 0), MenuHit::Outside],
        vec![true, true, true],
        vec![Some(2), None]
    )]
    #[case::outside_twice(
        vec![at(0, 2), at(1, 0), MenuHit::Outside, MenuHit::Outside],
        vec![true, true, true, false],
        vec![Some(2), None]
    )]
    fn hover(
        #[case] hits: Vec<MenuHit>,
        #[case] changes: Vec<bool>,
        #[case] expected: Vec<Option<usize>>,
    ) {
        let mut menu = menu();
        let changed: Vec<bool> = hits.into_iter().map(|hit| menu.hover(hit)).collect();
        assert_eq!(changed, changes);
        assert_eq!(hovered(&menu), expected);
    }

    #[test]
    fn hover_opens_the_items_of_each_submenu() {
        let mut menu = menu();
        menu.hover(at(0, 2));
        menu.hover(at(1, 1));
        assert_eq!(menu.levels[1].items, sort_items());
        assert_eq!(menu.levels[2].items, more_items());
    }

    #[rstest]
    #[case::entry(vec![], at(0, 0), vec![width(1), close()])]
    #[case::disabled_item(vec![], at(0, 1), vec![])]
    #[case::submenu_item(vec![], at(0, 2), vec![])]
    #[case::submenu_entry(vec![at(0, 2)], at(1, 0), vec![width(2), close()])]
    #[case::nested_submenu_entry(vec![at(0, 2), at(1, 1)], at(2, 0), vec![width(3), close()])]
    #[case::outside(vec![at(0, 2)], MenuHit::Outside, vec![close()])]
    fn click(#[case] hovers: Vec<MenuHit>, #[case] hit: MenuHit, #[case] expected: Vec<Action>) {
        let mut menu = menu();
        for hover in hovers {
            menu.hover(hover);
        }
        assert_eq!(menu.click(hit), expected);
    }
}
