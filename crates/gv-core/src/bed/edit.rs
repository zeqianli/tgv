//! Edits to plain BED tracks: adding and removing bases, the undo history, and writing files.
//!
//! `PlainBed::whole_file` stays the source of truth. An edit replaces it, and the history keeps
//! the frames before each step. Polars frames share their column buffers, so the snapshots are
//! cheap.

use super::{bed::BedSchema, repository::PlainBed};
use crate::error::TGVError;
use noodles::bgzf;
use polars::prelude::*;
use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

/// Whether an edit covers bases with features or clears them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BedEditKind {
    Add,
    Remove,
}

/// A change to a BED track, on the file's own contig name. 1-based, inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BedEdit {
    pub kind: BedEditKind,
    pub contig: String,
    pub start: u64,
    pub end: u64,
}

/// The undo and redo steps of one track, and which revision its file holds.
#[derive(Debug)]
pub struct BedHistory {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    revision: u64,
    /// Revisions are never reused, so a saved revision only matches the state that was saved.
    next_revision: u64,
    /// `None` until a new track is first written.
    saved_revision: Option<u64>,
    /// Set by `begin_step`. The first edit after it saves a snapshot, so one insert session is
    /// one undo step.
    step_pending: bool,
}

#[derive(Debug)]
struct Snapshot {
    frame: DataFrame,
    revision: u64,
}

impl BedHistory {
    /// Starts the history of a track. `saved` tells whether a file holds the current features.
    pub(super) fn new(saved: bool) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            next_revision: 1,
            saved_revision: saved.then_some(0),
            step_pending: true,
        }
    }

    pub fn is_modified(&self) -> bool {
        self.saved_revision != Some(self.revision)
    }

    /// Makes the next edit start a new undo step.
    pub fn begin_step(&mut self) {
        self.step_pending = true;
    }

    /// Records that the features are now in a file.
    pub fn mark_saved(&mut self) {
        self.saved_revision = Some(self.revision);
        // Later edits must not join the saved step, or undo couldn't return to it.
        self.step_pending = true;
    }

    /// Records the features from before an edit.
    fn record(&mut self, before: DataFrame) {
        if self.step_pending {
            self.undo.push(Snapshot {
                frame: before,
                revision: self.revision,
            });
            self.revision = self.next_revision;
            self.next_revision += 1;
            self.step_pending = false;
        }
        self.redo.clear();
    }

    /// Moves one step from `from` to `to`, and returns the features to show.
    fn step(
        from: &mut Vec<Snapshot>,
        to: &mut Vec<Snapshot>,
        revision: &mut u64,
        current: &DataFrame,
    ) -> Option<DataFrame> {
        let snapshot = from.pop()?;
        to.push(Snapshot {
            frame: current.clone(),
            revision: *revision,
        });
        *revision = snapshot.revision;
        Some(snapshot.frame)
    }
}

impl PlainBed {
    /// A temporary column with each row's position among the file's contigs.
    const CONTIG_RANK: &'static str = "contig_rank";

    /// Applies an edit and returns whether any feature changed.
    pub fn apply(&mut self, edit: &BedEdit) -> Result<bool, TGVError> {
        let edited = match edit.kind {
            BedEditKind::Add => self.added(edit)?,
            BedEditKind::Remove => self.removed(edit)?,
        };
        let Some(edited) = edited else {
            return Ok(false);
        };
        if !self.contigs.contains(&edit.contig) {
            self.contigs.push(edit.contig.clone());
        }
        let edited = self.renumbered(edited)?;
        let before = std::mem::replace(&mut self.whole_file, edited);
        self.history.record(before);
        Ok(true)
    }

    /// Returns to the features before the last step, or `false` if there is none.
    pub fn undo(&mut self) -> bool {
        let history = &mut self.history;
        let frame = BedHistory::step(
            &mut history.undo,
            &mut history.redo,
            &mut history.revision,
            &self.whole_file,
        );
        self.restore(frame)
    }

    /// Repeats the last undone step, or returns `false` if there is none.
    pub fn redo(&mut self) -> bool {
        let history = &mut self.history;
        let frame = BedHistory::step(
            &mut history.redo,
            &mut history.undo,
            &mut history.revision,
            &self.whole_file,
        );
        self.restore(frame)
    }

    fn restore(&mut self, frame: Option<DataFrame>) -> bool {
        let Some(frame) = frame else {
            return false;
        };
        self.whole_file = frame;
        self.history.begin_step();
        true
    }

    /// Covers the uncovered bases of the edit's range. Returns `None` if they are all covered.
    ///
    /// An uncovered run extends the feature that ends just before it, or else the feature that
    /// starts just after it. Otherwise it becomes a new feature. Features never merge, so names
    /// and extra columns aren't lost.
    fn added(&self, edit: &BedEdit) -> Result<Option<DataFrame>, TGVError> {
        let nearby = self
            .whole_file
            .clone()
            .lazy()
            .filter(
                col(Self::CONTIG)
                    .eq(lit(edit.contig.as_str()))
                    .and(col(BedSchema::START).lt_eq(lit(edit.end + 1)))
                    .and((col(BedSchema::END) + lit(1u64)).gt_eq(lit(edit.start))),
            )
            .sort(
                [BedSchema::START, BedSchema::ROW_ID],
                SortMultipleOptions::default(),
            )
            .collect()?;
        let features: Vec<(u64, u64, u64)> = nearby
            .column(BedSchema::ROW_ID)?
            .u64()?
            .into_no_null_iter()
            .zip(nearby.column(BedSchema::START)?.u64()?.into_no_null_iter())
            .zip(nearby.column(BedSchema::END)?.u64()?.into_no_null_iter())
            .map(|((row_id, start), end)| (row_id, start, end))
            .collect();

        let mut runs = Vec::new();
        let mut next = edit.start;
        for &(_, start, end) in &features {
            if start > next && next <= edit.end {
                runs.push((next, (start - 1).min(edit.end)));
            }
            next = next.max(end + 1);
        }
        if next <= edit.end {
            runs.push((next, edit.end));
        }
        if runs.is_empty() {
            return Ok(None);
        }

        let mut ends = col(BedSchema::END);
        let mut starts = col(BedSchema::START);
        let mut new_features = Vec::new();
        for (run_start, run_end) in runs {
            // Rows are sorted by start, then row ID, so `find` prefers the earliest row.
            if let Some(&(row_id, _, _)) = features.iter().find(|(_, _, end)| *end + 1 == run_start)
            {
                ends = when(col(BedSchema::ROW_ID).eq(lit(row_id)))
                    .then(lit(run_end))
                    .otherwise(ends);
            } else if let Some(&(row_id, _, _)) =
                features.iter().find(|(_, start, _)| *start == run_end + 1)
            {
                starts = when(col(BedSchema::ROW_ID).eq(lit(row_id)))
                    .then(lit(run_start))
                    .otherwise(starts);
            } else {
                new_features.push((run_start, run_end));
            }
        }
        let mut edited = self
            .whole_file
            .clone()
            .lazy()
            .with_columns([starts.alias(BedSchema::START), ends.alias(BedSchema::END)])
            .collect()?;
        if !new_features.is_empty() {
            edited.vstack_mut(&self.new_features(&edit.contig, &new_features)?)?;
        }
        Ok(Some(edited))
    }

    /// Clears the edit's range: deletes the features inside it, trims the ones that cross one
    /// end, and splits the ones that contain it. Returns `None` if no feature overlaps it.
    fn removed(&self, edit: &BedEdit) -> Result<Option<DataFrame>, TGVError> {
        let (start, end) = (edit.start, edit.end);
        let overlapping = col(Self::CONTIG)
            .eq(lit(edit.contig.as_str()))
            .and(col(BedSchema::START).lt_eq(lit(end)))
            .and(col(BedSchema::END).gt_eq(lit(start)));
        let begins_before = col(BedSchema::START).lt(lit(start));
        let ends_after = col(BedSchema::END).gt(lit(end));

        let affected = self
            .whole_file
            .clone()
            .lazy()
            .filter(overlapping.clone())
            .collect()?;
        if affected.height() == 0 {
            return Ok(None);
        }
        // The right parts of split features copy the other columns, and are numbered after the
        // existing rows until `renumbered` puts them in file order.
        let mut right_parts = affected
            .lazy()
            .filter(begins_before.clone().and(ends_after.clone()))
            .with_columns([lit(end + 1).cast(DataType::UInt64).alias(BedSchema::START)])
            .collect()?;
        let first_new_row = self.whole_file.height() as u64;
        right_parts.with_column(Column::new(
            BedSchema::ROW_ID.into(),
            (first_new_row..first_new_row + right_parts.height() as u64).collect::<Vec<_>>(),
        ))?;

        let mut edited = self
            .whole_file
            .clone()
            .lazy()
            .filter(
                overlapping
                    .clone()
                    .and(begins_before.clone().not())
                    .and(ends_after.clone().not())
                    .not(),
            )
            .with_columns([
                when(overlapping.clone().and(begins_before.clone()))
                    .then(lit(start - 1))
                    .otherwise(col(BedSchema::END))
                    .alias(BedSchema::END),
                when(overlapping.and(begins_before.not()).and(ends_after))
                    .then(lit(end + 1))
                    .otherwise(col(BedSchema::START))
                    .alias(BedSchema::START),
            ])
            .collect()?;
        edited.vstack_mut(&right_parts)?;
        Ok(Some(edited))
    }

    /// Rows for new features with no name, score, strand, or extra columns.
    fn new_features(&self, contig: &str, features: &[(u64, u64)]) -> Result<DataFrame, TGVError> {
        let first_row = self.whole_file.height() as u64;
        let count = features.len();
        Ok(DataFrame::new(
            count,
            vec![
                Column::new(
                    BedSchema::ROW_ID.into(),
                    (first_row..first_row + count as u64).collect::<Vec<_>>(),
                ),
                Column::new(BedSchema::CONTIG_INDEX.into(), vec![0u64; count]),
                Column::new(
                    BedSchema::START.into(),
                    features.iter().map(|&(start, _)| start).collect::<Vec<_>>(),
                ),
                Column::new(
                    BedSchema::END.into(),
                    features.iter().map(|&(_, end)| end).collect::<Vec<_>>(),
                ),
                Column::new(BedSchema::NAME.into(), vec![None::<String>; count]),
                Column::new(BedSchema::SCORE.into(), vec![None::<u16>; count]),
                Column::new(BedSchema::STRAND.into(), vec![None::<String>; count]),
                Column::new(BedSchema::EXTRA.into(), vec![None::<String>; count]),
                Column::new(Self::CONTIG.into(), vec![contig.to_owned(); count]),
            ],
        )?)
    }

    /// Sorts rows into the order a write uses. Files that were sorted stay sorted, and others
    /// keep their row order, with new rows at the end.
    fn in_file_order(&self, frame: DataFrame) -> Result<DataFrame, TGVError> {
        if !self.sorted_on_load {
            return Ok(frame.sort([BedSchema::ROW_ID], SortMultipleOptions::default())?);
        }
        let ranks: HashMap<&str, u32> = self
            .contigs
            .iter()
            .enumerate()
            .map(|(rank, contig)| (contig.as_str(), rank as u32))
            .collect();
        let contig_ranks = frame
            .column(Self::CONTIG)?
            .str()?
            .iter()
            .map(|contig| ranks[contig.expect("BED contigs are non-null")])
            .collect::<Vec<_>>();
        Ok(frame
            .lazy()
            .with_columns([lit(Series::new(Self::CONTIG_RANK.into(), contig_ranks))])
            .sort(
                [
                    Self::CONTIG_RANK,
                    BedSchema::START,
                    BedSchema::END,
                    BedSchema::ROW_ID,
                ],
                SortMultipleOptions::default(),
            )
            .drop(cols([Self::CONTIG_RANK]))
            .collect()?)
    }

    /// Numbers rows from 0 in file order, and sorts them as reads expect.
    fn renumbered(&self, frame: DataFrame) -> Result<DataFrame, TGVError> {
        let mut frame = self.in_file_order(frame)?;
        frame.with_column(Column::new(
            BedSchema::ROW_ID.into(),
            (0..frame.height() as u64).collect::<Vec<_>>(),
        ))?;
        Ok(frame.sort(
            [BedSchema::START, BedSchema::END, BedSchema::ROW_ID],
            SortMultipleOptions::default(),
        )?)
    }

    /// Writes the features to `path` and returns how many were written. The history is not
    /// changed; the caller marks the revision saved when `path` is the track's own file.
    ///
    /// The file goes to a temporary file in the same directory first and is renamed over the
    /// target, so a crash can't leave half a file. Paths ending in `.gz` are written with bgzf.
    pub fn write(&self, path: &str) -> Result<usize, TGVError> {
        let failed = |operation: &'static str| {
            move |source: io::Error| TGVError::FileOperationError {
                operation,
                path: PathBuf::from(path),
                source,
            }
        };
        let text = self.to_text()?;
        let target = Path::new(path);
        let directory = target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(failed("write"))?;
        if path.to_lowercase().ends_with(".gz") {
            let mut writer = bgzf::io::Writer::new(temporary.as_file_mut());
            writer.write_all(text.as_bytes()).map_err(failed("write"))?;
            writer.finish().map_err(failed("write"))?;
        } else {
            temporary
                .write_all(text.as_bytes())
                .map_err(failed("write"))?;
        }
        // Temporary files are private, so give the file the permissions it had, or the usual
        // ones for a new file.
        let permissions = match fs::metadata(target) {
            Ok(metadata) => metadata.permissions(),
            Err(_) => default_permissions(temporary.path())?,
        };
        fs::set_permissions(temporary.path(), permissions).map_err(failed("write"))?;
        temporary
            .persist(target)
            .map_err(|error| failed("replace")(error.error))?;
        Ok(self.whole_file.height())
    }

    /// The file's text: the header lines, then every feature with the file's column count.
    fn to_text(&self) -> Result<String, TGVError> {
        let frame = self.in_file_order(self.whole_file.clone())?;
        let contigs = frame.column(Self::CONTIG)?.str()?;
        let starts = frame.column(BedSchema::START)?.u64()?;
        let ends = frame.column(BedSchema::END)?.u64()?;
        let names = frame.column(BedSchema::NAME)?.str()?;
        let scores = frame.column(BedSchema::SCORE)?.u16()?;
        let strands = frame.column(BedSchema::STRAND)?.str()?;
        let extras = frame.column(BedSchema::EXTRA)?.str()?;

        let mut text = String::new();
        for line in &self.header_lines {
            text.push_str(line);
            text.push('\n');
        }
        for row in 0..frame.height() {
            let mut fields = vec![
                contigs
                    .get(row)
                    .expect("BED contigs are non-null")
                    .to_owned(),
                // BED starts are 0-based. Ends are exclusive, so they match inclusive 1-based ends.
                (starts.get(row).expect("BED starts are non-null") - 1).to_string(),
                ends.get(row).expect("BED ends are non-null").to_string(),
            ];
            let optional = [
                names.get(row).map(str::to_owned),
                scores.get(row).map(|score| score.to_string()),
                strands.get(row).map(str::to_owned),
            ];
            fields.extend(
                optional
                    .into_iter()
                    .take(self.column_count - 3)
                    .map(|field| field.unwrap_or_else(|| ".".to_owned())),
            );
            fields.extend(extras.get(row).map(str::to_owned));
            text.push_str(&fields.join("\t"));
            text.push('\n');
        }
        Ok(text)
    }
}

/// The permissions a new file usually gets.
fn default_permissions(path: &Path) -> Result<fs::Permissions, TGVError> {
    let mut permissions = fs::metadata(path)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o644);
    }
    Ok(permissions)
}
