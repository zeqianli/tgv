# BED editing with track focus, track mode, and insert mode

## Context

tgv is a read-only viewer today. The goal is to edit and save BED tracks from the TUI, with
vim-like keys. The model is the VS Code Jupyter vim plugin. There, a cell layer sits on top of
vim's modes: cell command mode moves between cells, and cell edit mode is vim inside one cell.
In tgv, tracks play the role of cells:

- **Normal mode** (existing) moves along the genome. It applies to the focused track.
- **Track mode** (new, reached with `Esc`) moves the focus between tracks, and creates,
  removes, and undoes edits on tracks.
- **Insert mode** (new, reached with `i`/`a`) edits the focused BED track at the cursor.

Today `focused_alignment: usize` (on `App`, `crates/tgv/src/app.rs:73`) is an alignment
repository index. Only mouse hover sets it, and nothing draws it. It becomes a general track
focus.

Decisions already made with the user:
- `:w` writes tracks. Session saving moves to a new `:ws`.
- `u`/`Ctrl-r` undo and redo edits in track mode. In normal mode they keep moving through the
  jump history.
- Saving keeps the extra BED columns (7 and up) and the header lines.
- The sidebar marks modified tracks until they are saved. New tracks are always marked until
  they are first written.

---

## 1. UX

### 1.1 Modes and transitions

```
             Esc                    i / a  (focused track is an editable BED)
  Track ◄──────────── Normal ───────────────────────────► Insert
    │  ────────────►    ▲  ◄──────────────────────────────  │
    │      Enter        │               Esc                 │
    └───────────────────┼─── i / a ─────────────────────────┘
                        └─── o / O (new track) ──► Insert
```

- Normal → Track: `Esc`. If a count or `g` is pending in `registers.normal`, the first `Esc`
  only clears it, as in vim. Today `Esc` in normal mode shows an error.
- Track → Normal: `Enter`. `Esc` in track mode does nothing, since track mode is the outermost
  layer (as in Jupyter). It does clear a pending count.
- Normal or Track → Insert: `i` or `a` on an editable BED track.
- Insert → Normal: `Esc`, as in vim. The insert session becomes one undo step.
- `:` and `/` work from Normal and Track mode. After `Enter` or `Esc` they return to the mode
  they came from, so `registers` remembers that mode.
- Popups, the context menu, and the contig list keep their current behaviour, and so does
  `Esc` in them.

### 1.2 Mode indicator

- In normal mode the console row is blank today. It now shows the mode, as vim does:
  `-- TRACK --` or `-- INSERT --`, followed by the focused track's name. Normal mode shows
  nothing, as in vim.
- If the focused track is modified, the line ends with `[+]`, for example
  `-- INSERT -- peaks.bed [+]`.
- With the sidebar hidden, this line is the only place the focus is named.

### 1.3 Track focus

- Only file tracks (the `TrackRegistry` entries) can take focus: alignment, variant, and BED.
  The reference rows (cytoband, ruler, sequence, and genes) can't.
- At startup the first file track in display order has focus. With no file tracks, the focus
  is `None`. Track mode then shows `-- TRACK -- (no tracks)` and only `o`/`O` work.
- Drawing the focus:
  - The sidebar divider beside the focused track's rows becomes a bar `┃`. The divider is
    already a column of its own, so the labels don't shift or rewrap.
  - The bar's colour shows the mode, as Jupyter's cell border does: dim in normal mode,
    blue in track mode, and green in insert mode. Command and search input keep the colour
    of the mode they return to.
  - In track mode the focused track's label is also drawn bold.
- **Mouse:** hovering no longer moves the focus. That would undo keyboard selection as soon
  as the pointer moved.
  - Left-click on a track, or on its sidebar section, focuses it.
  - The mouse wheel and vertical drags scroll the alignment under the pointer, without
    changing the focus.
- **Removing the focused track** (with `dd`, `:bd`, or the sidebar menu) moves the focus to
  the next track in display order. If there is none, it moves to the previous track, and
  otherwise to `None`. Today the focus resets to alignment 0.
- In normal mode, `j/k/J/K/gg/gG` scroll the focused track if it is an alignment. Otherwise
  they do nothing, without an error.

### 1.4 Track mode keys

| Key | Action |
|---|---|
| `j`/`k`, `↓`/`↑`, with a count (`3j`) | Focus the next or previous file track. Stops at the ends. |
| `gg` / `gG` | Focus the first or last track (`gG` matches normal mode's existing binding). |
| `Enter` | Go to normal mode on the focused track. |
| `i` / `a` | Go to insert mode on the focused track (see 1.5). |
| `o` / `O` | Create an untitled BED track below or above the focused track, focus it, and go to insert mode. This matches vim's `o`, which opens a line and inserts. |
| `dd` | Remove the focused track. Only track mode has `dd`; normal mode is unchanged. A modified track is not removed; the message is `peaks.bed has unsaved changes. Write it with :w, or remove it with :bd!.` |
| `u` / `Ctrl-r` | Undo or redo the focused BED track's last edit step. At the ends, the message is `Already at oldest change` or `Already at newest change`. |
| `h/l/H/L`, `w/b/e/ge`, `W/B/E/gE`, `z`, counts | Move horizontally and zoom in, as in normal mode, so it's possible to look around without leaving the layer. |
| `s`, `:`, `/` | The same as in normal mode. |

`o` means zoom out in normal mode, and is taken by "new track" in track mode. To zoom out,
press `Enter` first. Moving tracks up and down with `J`/`K` is left for later.

### 1.5 Insert mode

**Entering insert mode.** It is refused, with a message, in these cases:
- The focused track isn't a BED track: `Insert mode is only available for BED tracks.`
- The BED track is indexed or bigBed:
  `peaks.bed.gz is read-only: tgv edits BED files that load whole (.bed and unindexed .bed.gz).`

**The cursor** is the existing current position, `alignment_view.focus.position`. Insert mode
draws it in the focused BED track as one reversed cell at `onscreen_x_coordinate(focus)`.
That function already handles the start of a contig, where the focus isn't in the middle
column. Other modes don't draw the cursor.

- `i` keeps the cursor where it is.
- `a` moves it right by one column first, like vim's append.

**Keys**

| Key | Action |
|---|---|
| `Space` | Add the bases under the cursor column, then move right by one column. Holding `Space` paints a run, the way typing advances the cursor. Counts are ignored. |
| `Backspace` | Move left by one column, then remove the bases under the cursor. |
| `Delete` | Remove the bases under the cursor, without moving. |
| `h/l/H/L`, arrows, `w/b/e/ge`, `W/B/E/gE`, `z/o`, counts | Move and zoom as in normal mode. Letters have nothing to type here, so movement keys stay useful. |
| `j/k` | Ignored: insert mode stays on one track. |
| `Esc` | Go to normal mode. |

**Zoom.** "The bases under the cursor column" is one base at zoom 1, and `zoom` bases when
zoomed out. They come from `coordinates_of_onscreen_x`. Moving by one column already scales
with the zoom (`Movement::Right(n)` → `n * zoom`), so painting stays consistent at any zoom.

**Adding bases [s, e]**
- Bases that are already covered are left alone.
- An uncovered run that touches the end of a feature on its left (that feature's end is
  s − 1) extends that feature. Typing appends to the word you are on, so the left neighbour
  is preferred.
- Otherwise, a run that touches the start of a feature on its right extends that feature's
  start.
- Otherwise the run becomes a new feature with no name, score, strand, or extra columns.
- Two features are never merged into one, so names and extra columns aren't lost. Painting
  through a gap leaves the two features end to end.

**Removing bases [s, e]**
- A feature fully inside the range is deleted.
- A feature that crosses one end of the range is trimmed.
- A feature that contains the range is split in two. The right part copies the name, score,
  strand, and extra columns.

**Feedback**
- Every edit redraws the BED area and the sidebar marker.
- Hover messages work as before, and show the edited feature.

### 1.6 Commands

| Command | Behaviour |
|---|---|
| `:w` | Write the focused BED track to its file. Untitled tracks need a path: `untitled-1 has no file name. Use :w <path>.` Tracks that aren't BED: `:w writes BED tracks. Use :ws to save the session.` |
| `:w <path>` | Untitled track: write it to the path, which becomes the track's file. Named track: write a copy, and the track still points to its own file, as in vim (`Wrote a copy to <path>. peaks.bed still has unsaved changes.`). |
| `:wa` | Write every modified BED track. Untitled tracks are skipped and named in the message. |
| `:wq [path]` | `:w`, then `:q`. |
| `:wqa` / `:xa` | `:wa`, then `:q`. |
| `:q` | Quit. If any track is modified: `peaks.bed has unsaved changes. Use :w, or :q! to quit without writing.` |
| `:q!` | Quit without writing. |
| `:ws [name\|path]` | Save the session. This is today's `:w`, with the same active-session rules. |
| `:wsq [name\|path]` | Save the session and quit. This is today's `:wq`. It refuses like `:q` while tracks are modified. |
| `:bd` / `:bd!` | Remove the focused track. `:bd` refuses for a modified track; `:bd!` removes it anyway. |

- **Session saving and edited tracks:** `:ws` leaves untitled tracks out, saying so. Modified
  tracks are saved by path, with a note that the session points to the file on disk.
- **Success message:** `peaks.bed: 42 features written.`

### 1.7 Modified marker

- Modified tracks show `● ` before the file name in their sidebar section, and `[+]` in the
  mode line.
- A track is modified when its current revision isn't its saved revision. So undoing back to
  the saved state clears the marker, as in vim and VS Code.
- New tracks have no saved revision, so they stay marked until the first `:w <path>`.
- Untitled tracks are labelled `untitled-1`, `untitled-2`, and so on.

### 1.8 What the file looks like after saving

- Rows keep their file order. Trimmed or extended features stay in their row. New features
  and split-off parts are added in sorted position if the file was sorted when it loaded, and
  at the end otherwise.
- New tracks are always written sorted.
- Every row has the file's original column count (at least BED3, and more as needed). Missing
  name, score, or strand are written as `.`, which the reader already accepts for scores.
  Extra columns 7 and up are written back as they were.
- Header lines (`#`, `track`, and `browser`) are written back at the top, in their original
  order.
- Contig names use the file's own names, from the contig header aliases. A contig the file
  didn't have uses the reference's name, and is added as an alias for that file.
- The file is written to a temporary file in the same directory and renamed over the target,
  so a crash can't leave a half-written file. `.bed.gz` is written with bgzf; `.bed` as plain
  text.

### 1.9 Agents

Edits update both the view and query states, so the MCP `query` tool's `bed` table and
`inspect_interval` see unsaved edits. No new session commands are needed.

---

## 2. Data model changes (reusing existing structs)

### Overview: `App` and `Dataset`

The edits split along the existing crate boundary:
- `Dataset` (in `gv-session`) owns the data, so it owns the edits, the undo history, and the
  saving.
- `App` (in `tgv`) owns the interaction. It knows which track has focus and which mode is
  active, and it turns keys into `Dataset` calls.

`App` already holds `dataset: Dataset` directly and serves agent requests from it, so no
message passing to a separate worker is needed.

**`Dataset`** gets no new fields that hold track data. The edit state lives inside the BED
repository it already owns:

```rust
pub struct Dataset {
    pub settings: Settings,
    pub repository: Repository,   // bed_repositories[i]: PlainBed now holds the edit state.
    pub tracks: Arc<TrackRegistry>, // Entries can be FilePath::UntitledBed(n).
    pub view: State,
    query: State,
    /// The number of the last untitled track, so names aren't reused after one is removed.
    untitled_count: u32,          // New.
}

pub struct PlainBed {
    pub path: String,             // Now public: the first write of an untitled track sets it.
    contigs: Vec<String>,
    whole_file: DataFrame,        // Still the source of truth. Edits change it in place.
    header_lines: Vec<String>,    // New.
    column_count: usize,          // New.
    sorted_on_load: bool,         // New.
    pub history: BedHistory,      // New: the undo and redo snapshots and the saved revision.
}
```

- History lives next to `whole_file` because a snapshot is a copy of `whole_file`. Keeping
  them together means that an undo can't get out of sync with the frame it restores.
- Removing a track drops its history along with its repository. `remove_track` needs no new
  bookkeeping.
- New methods: `edit_bed`, `begin_edit_step`, `undo`, `redo`, `write_bed`, and
  `is_modified` take a `TrackId`. `new_bed_track` returns one.
  - Each method finds the BED index through `tracks.get(id).repository_index`, then calls
    `as_editable_mut()`. A read-only BED file fails there with its error, so callers don't
    check.
  - `edit_bed`, `undo`, and `redo` then reset `view.bed_intervals[i]` and
    `query.bed_intervals[i]` to empty tables. `ensure_loaded` skips regions it already has
    (`has_complete_data`), and an empty table is never complete, so the next load of either
    state reads the edit. The viewer loads right away; the query state loads on the next
    agent request.
  - `edit_bed(id, kind, contig_index, start, end)` takes a dataset contig. It looks up the
    file's name for it, and registers the reference's name as the file's when the file
    doesn't have the contig yet.
  - `new_bed_track` calls `add_file_paths` with `FilePath::UntitledBed(n)`.
    `Repository::open_file` creates the empty `PlainBed` for it.
  - `write_bed` with a path on an untitled track swaps the registry entry to
    `BedPath(path)` through `Arc::make_mut`.
- `State::add_track` matches on `FilePath`, so it gets an `UntitledBed` arm that adds an
  empty BED slot.

**`App`** swaps the alignment-only focus for a track focus. The mode stays in `Registers`:

```rust
pub struct App {
    // ...unchanged fields...
    pub registers: Registers,         // current: KeyRegisterType gains Track and
                                      // Insert { track }, plus return_to.
    pub layout: MainLayout,           // Gains track_order: Vec<TrackId>.
    /// The track that track-mode keys, insert mode, `:w`, and `:bd` act on. `None` when
    /// there are no file tracks.
    pub focused_track: Option<TrackId>, // Replaces focused_alignment: usize.
    // ...unchanged fields...
}
```

- `focused_track` is a `TrackId` instead of a per-kind index. That way it stays valid when
  other tracks are removed, and the decrement fix-up in the `RemoveTrack` handler
  (`app.rs:738`) is replaced by the next/previous rule in 1.3.
- Keys that act on the focused track produce focus-relative actions (`EnterInsertMode`,
  `Undo`, `Redo`, `NewBedTrack`, and `CloseTrack`), and `App` resolves the track. Registers
  don't know the dataset, so they can't check whether a track is a BED track.
- `Insert { track }` repeats the focused track on purpose. Insert mode then can't run
  without a target, and keys edit the track insert mode started on even if a click moves
  the focus. Clicking another track during insert mode leaves insert mode first.
- The modified marker isn't stored on `App`. The sidebar labels come from
  `BedRepositoryEnum::is_modified` when the layout resolves, and the mode line from
  `dataset.is_modified(id)`, both on every frame, so undo, redo, and saving never leave a
  stale marker. `App::render` passes a `TrackFocus` (the focused track, its name, and
  whether it is modified) to the renderer.
- `:q` and `:wqa` check `dataset.is_modified` for every entry in `layout.track_order`.

### `crates/tgv/src/register.rs`

- `KeyRegisterType` gains `Track` and `Insert { track: TrackId }`. The insert mode's target
  track is part of the type, so insert mode always has a track.
- `Registers` gains `return_to: KeyRegisterType`, the mode that `:` and `/` return to.
  `handle_command_line` uses it instead of the hard-coded `SwitchKeyRegister(Normal)`.
- New handlers `handle_track` and `handle_insert`.
  - Both reuse `update_by_char` and `registers.normal` for counts and `g` sequences.
  - In track mode, `j/k/gg/gG` become focus moves instead of `Scroll`.
  - `handle_normal` gains handling for `Esc`, `i`, and `a`.

### `crates/gv-core/src/normal.rs`

- Unchanged. `handle_track` buffers `dd` itself, so normal mode doesn't gain it.
- Track mode maps the results: `Scroll::Down(n)` → `FocusMove::Next(n)`, and so on. `J/K`
  do nothing in track mode, since they are reserved for moving tracks.

### `crates/tgv/src/app.rs` (`App`)

- `focused_alignment: usize` → `focused_track: Option<TrackId>`.
  - The `Scroll` handler finds the alignment with `tracks.alignment_index(id)` when the
    focused track is an alignment, and does nothing otherwise.
  - The `RemoveTrack` fix-up uses the next/previous rule in 1.3.
- `alignment_view.y` stays indexed by alignment repository index.

### `crates/tgv/src/message.rs` (`Action`)

- `FocusAlignment(TrackId)` → `FocusTrack(TrackId)`.
- New: `MoveFocus(FocusMove)`, with `FocusMove` being `Next(usize)`, `Previous(usize)`,
  `First`, or `Last`.
- New: `ScrollAlignment { track: TrackId, scroll: Scroll }`, which the mouse wheel and drags
  use instead of focus + `Scroll`.
- New: `EnterInsertMode { append: bool }`, `EditBed { track: TrackId, edit: BedEditKind }`,
  `Undo`, and `Redo`. Entering insert mode begins the edit step, so there is no separate
  action for it.
- New: `NewBedTrack { below: bool }`.

### `crates/gv-core/src/message.rs` (`Message`)

- `SaveSession` and `SaveAndQuit` keep their meaning, but are reached from `:ws` and `:wsq`.
- New: `WriteTrack(Option<String>)`, `WriteAll`, `WriteAndQuit(Option<String>)`,
  `WriteAllAndQuit`, `ForceQuit`, and `CloseTrack { force: bool }`.
- `command::parse` (`crates/gv-core/src/command.rs`) parses `w`, `wa`, `wq`, `wqa`/`xa`, `q!`,
  `ws`, `wsq`, `bd`, and `bd!`. It reuses `parse_session_command` for `ws` and `wsq`.

### `crates/tgv/src/layout.rs` (`MainLayout`)

- Add `pub track_order: Vec<TrackId>`, the display order of file tracks.
  - `MainLayout::new` already takes `visible: &[TrackId]` in order. `set_tracks` now passes
    `track_order` (keeping it and appending new IDs) instead of the registry order.
  - `o`/`O` insert the new ID into `track_order` before `set_tracks`, so no separate
    method is needed.
- The registry has to stay in ID order, because lookups use a binary search. That is why the
  order lives in the layout and not in `TrackRegistry`.
- Track-mode `j/k`, focus fix-up after removal, and `:ws` all use `track_order`.
- `resolve` adds the `● ` prefix to labels, so wrapping and section heights account for
  it. It keeps its signature and asks the repository whether a BED track is modified.
- `rendering/sidebar.rs` draws the focus bar and the bold label, and
  `rendering/console.rs` draws the mode line.

### `crates/gv-core/src/settings.rs` (`FilePath`)

- Add `FilePath::UntitledBed(u32)`. A track without a file is a separate variant, not a fake
  path.
  - `path()` becomes `Option<&str>`, and `Display` shows the path or `untitled-N`.
  - `file_name` moves here from `tgv::layout`, so `gv-core` and `gv-session` errors can
    name files the same way.
  - Session saving skips untitled tracks.
- `:w <path>` changes the entry to `BedPath(path)` through `Arc::make_mut` on
  `Dataset.tracks`, the same way add and remove already update it.

### `crates/gv-core/src/bed/bed.rs`

- `BedSchema` gains `EXTRA: &str = "extra"`, a nullable `String` of the raw tab-joined columns
  7 and up.
  - `BedColumns::push_line` fills it for every reader, so `BedTable` keeps one schema.
  - Add it to `BedSchema::column_docs` and `empty`, so the SQL `bed` table exposes it.

### `crates/gv-core/src/bed/repository.rs` (`PlainBed`)

- New fields:
  - `header_lines: Vec<String>`, kept by `is_data_line` filtering at load.
  - `column_count: usize`, the most standard columns any row has.
  - `sorted_on_load: bool`.
  - `history: BedHistory`.
- `whole_file` stays the source of truth. `read_bed` already serves the whole contig from it,
  so the existing reload path picks up edits without changes.
- New `PlainBed::empty(untitled)` for new tracks.
- New `PlainBed::apply(&mut self, edit: &BedEdit) -> Result<bool, TGVError>`, which returns
  whether anything changed.
  - It uses lazy selections and transforms for the trim, split, and extend rules.
  - It renumbers `ROW_ID` in file order.
- New `PlainBed::write(&mut self, path: &str) -> Result<usize, TGVError>`, which returns the
  number of features written and marks the revision as saved when the path is the track's
  own.
- `BedRepositoryEnum` gains `as_editable_mut() -> Result<&mut PlainBed, TGVError>`. For
  indexed and bigBed files it returns the read-only error.

### New `crates/gv-core/src/bed/edit.rs`

```rust
pub enum BedEditKind { Add, Remove }

/// A change to a BED track, on the file's own contig name. 1-based, inclusive.
pub struct BedEdit { pub kind: BedEditKind, pub contig: String, pub start: u64, pub end: u64 }

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
struct Snapshot { frame: DataFrame, revision: u64 }
```

- `is_modified()` is `saved_revision != Some(revision)`.
- `mark_saved()`, undo, and redo also begin a new step, so an edit after them can't join a
  step whose snapshot is already taken.
- New errors on `TGVError`: `ReadOnlyBed`, `NotBedTrack`, and `UntitledTrack`, which name the
  file in their messages.
- `DataFrame` clones share their column buffers, so snapshots are cheap.

### `crates/gv-session/src/session.rs` (`Dataset`)

Following `add_files` and `remove_track`, add:
- `edit_bed(id, &BedEdit)`, which applies the edit, then reloads `view.bed_intervals[i]` and
  the private `query.bed_intervals[i]` from the repository.
- `begin_edit_step(id)`.
- `undo(id)` and `redo(id)`.
- `write_bed(id, Option<&str>)`.
- `new_bed_track() -> TrackId`.
- `is_modified(id)`.
- Contig alias registration for new contig names. It reuses the `merge_contigs` /
  `file_name(RepositoryFileIndex::Bed(i))` path that `State::load_bed_data` uses.

---

## 3. Implementation order

Each phase can be built and used on its own.

1. **Focus.**
   - Rename to `focused_track`, change the mouse focus rules, and add `ScrollAlignment`.
   - Add `track_order`, track mode (`Esc`/`Enter`, `j/k/gg/gG`), the focus bar, and the mode
     line.
   - Fix up `offline_app.rs:411,428`.
2. **Edit core.** `BedSchema::EXTRA`, the new `PlainBed` fields, `BedEdit`, `apply`,
   `BedHistory`, and the `Dataset::edit_bed` reload of both states.
3. **Insert mode.** The `Insert { track }` register, the cursor cell, `Space`, `Backspace`,
   and `Delete`, plus `u`/`Ctrl-r` in track mode.
4. **Writing.**
   - `PlainBed::write` (atomic, bgzf for `.gz`), and the modified marker.
   - The command changes: `:w`, `:wa`, `:wq`, `:wqa`, `:q`/`:q!`, `:ws`, and `:wsq`.
   - Rename the `NO_ACTIVE_SESSION_MESSAGE` text to mention `:ws`.
5. **Track creation and removal.** `FilePath::UntitledBed`, `o`/`O`, `dd`, `:bd`/`:bd!`, and
   the dirty guard on the sidebar "Remove" item.
6. **Docs.**
   - `doc/src/usage.md`: new "Track mode" and "Insert mode" tables, the command table, the
     mouse section (click to focus), and the vim comparison table.
   - `doc/src/session.md`: every `:w`/`:wq` becomes `:ws`/`:wsq` (lines 5, 44, 45, 59, and
     117–121).

## 4. Verification

- `cargo nextest run` after each phase. Existing tests that need updating:
  - the `command.rs` parse cases for `w`/`wq`
  - `offline_app.rs` focus tests
  - `normal.rs` cases
- Tests: ask the user before adding new ones. Good candidates are `rstest` cases for
  `PlainBed::apply` (extend left, extend right, end to end, trim, split, and delete) and a
  write → reload round trip that keeps the extra columns and header lines.
- Manual run:
  `cargo run -p tgv -- -g hg38 -r chr20:88108 crates/tgv/tests/data/simple.bam crates/tgv/tests/data/simple.bed`
  1. Press `Esc` and `j/k` to watch the focus bar move. Click a track to focus it.
  2. Press `i` on the BED track, `Space` ×5, then `Backspace`. Check the feature and the `●`
     marker.
  3. Press `Esc`, `Esc`, `u` (the edit is undone and the marker goes), then `Ctrl-r`.
  4. Run `:w /tmp/x.bed` and diff it against the input.
  5. Press `o`, paint, then `:w /tmp/new.bed`. The marker clears.
  6. `:q` with unsaved edits refuses, and `:q!` quits.
- MCP: with the viewer running, use the `query` tool on the `bed` table to check that unsaved
  edits and the `extra` column show up.
