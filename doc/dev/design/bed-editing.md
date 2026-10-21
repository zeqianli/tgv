# BED editing with track focus and insert mode

## Context

tgv is a read-only viewer today. The goal is to edit and save BED tracks from the TUI, with
vim-like keys. The model is a vim window per track: one track has the focus at a time, normal
mode moves along the genome in terms of the focused track, and insert mode edits the focused
BED track at the cursor.

The first version (commit `347e7ae`) had three modes: normal, track, and insert. Track mode
moved the focus and created, removed, and undid edits on tracks. This revision removes track
mode:

- **Normal mode** moves along the genome. Its word motions follow the focused track.
- **Insert mode** (reached with `i`/`a`) edits the focused BED track at the cursor. It takes
  vim's normal-mode editing grammar (counts, operators, and motions), because insert mode
  types no text. The only "typing" is `Space`, which paints.
- **Focus moves** with `Ctrl-j`/`Ctrl-k` and the vim window keys (`Ctrl-w j`, ...), in both
  modes.

Decisions already made with the user:
- `:w` writes tracks. Session saving moves to `:ws`.
- There are two independent histories:
  - The **navigation history** is the jump list on `App` (`crates/tgv/src/jumps.rs`). There
    is one for the whole app. `u`/`Ctrl-r` in normal mode move through it, and never touch
    edits.
  - The **edit history** belongs to each editable BED track (`BedHistory` inside its
    `PlainBed`). `u`/`Ctrl-r` in insert mode move through the history of the track being
    edited.
- Saving keeps the extra BED columns (7 and up) and the header lines.
- The sidebar marks modified tracks until they are saved. New tracks stay marked until they
  are first written.
- Track removal has no key or command. It's a right-click menu item. Track creation is
  `:new`, and copying a BED track is a right-click "Copy" and "Paste below".
- The reference has the focus at startup, so existing users see today's behaviour.
- Work marked **TODO** is designed here but left for a later round (section 5).

---

## 1. UX

### 1.1 Modes and transitions

```
          i / a  (the focused track is an editable BED)
  Normal ─────────────────────────────► Insert
    ▲  ◄──────────────────────────────    │
    │      Esc, Ctrl-j/k, Ctrl-w …        │
    │                                     │
  Esc only cancels pending input    Esc first cancels pending input
```

- **Normal → Insert:** `i` or `a` on an editable BED track. Entering is refused, with a
  message, in these cases:
  - The focused track isn't a BED track: `Insert mode is only available for BED tracks.`
  - The BED track is indexed or bigBed:
    `peaks.bed.gz is read-only: tgv edits BED files that load whole (.bed and unindexed .bed.gz).`
- **Insert → Normal:**
  - `Esc`. If a count, an operator, or `g` is pending, the first `Esc` only clears it, as in
    vim.
  - `Ctrl-j`, `Ctrl-k`, or a `Ctrl-w` focus key that moves the focus also leaves insert
    mode. The new track starts in normal mode. At the first or last track, where the focus
    doesn't move, insert mode stays on.
  - Clicking another track also leaves insert mode.
- **`Esc` in normal mode** only clears pending input. Otherwise it does nothing, without an
  error. (Today it enters track mode.)
- **`:` and `/`** work in both modes. After `Enter` or `Esc` they return to the mode they came
  from, through `Registers::return_to`. So `:w` doesn't leave insert mode.
- Popups, the context menu, and the contig list keep their current behaviour, and so does
  `Esc` in them.

### 1.2 Focus

**Focus targets**
- **The reference** is the sequence and gene rows together. It can take the focus, which is
  new. The cytoband and the ruler can't.
- **Every file track** can take the focus: alignment, variant, and BED.
- **The focus order** is the display order: the file tracks in `layout.track_order`, then
  the reference, which is drawn at the bottom.
- **At startup** the reference has the focus.
- **Without sequence or gene rows** (for example with `--no-reference`), nothing can show the
  reference's focus. The reference is then left out of the focus order, and the first file
  track has the focus at startup. With no file tracks either, the reference keeps it.

**Keys.** Vim's window keys are `Ctrl-w` and a direction. Tracks only stack vertically, so
only up and down matter.

| Key | Action |
|---|---|
| `Ctrl-j` / `Ctrl-k`, with a count (`3 Ctrl-j`) | Focus the next or previous target. Stops at the ends. |
| `Ctrl-w j` / `Ctrl-w k` | The same, as in vim. |
| `Ctrl-w w` / `Ctrl-w W` | Focus the next or previous target, wrapping around. |
| `Ctrl-w t` / `Ctrl-w b` | Focus the first or last target. |

- **`Ctrl-h` and `Ctrl-l` stay unbound.** Some terminals send `Ctrl-h` for Backspace.
- **In raw mode,** crossterm reports `Ctrl-j` as `Char('j')` with `CONTROL`, not `Enter`.
  Check this on real terminals.

**Mouse**
- **Left-click** on a track, on its sidebar section, or on the reference rows focuses it.
- **The wheel and vertical drags** scroll the alignment under the pointer, without changing
  the focus. This is unchanged.

**Removing the focused track** moves the focus to the next file track in display order. If
there is none, it moves to the previous file track, and otherwise to the reference. The focus
is never `None` now, because the reference is always there.

### 1.3 Normal mode keys

Word motions follow the focused track. Lowercase moves by the track's own unit. Uppercase
moves by genes where that means something.

| Focus | `w` / `b` / `e` / `ge` | `W` / `B` / `E` / `gE` | `j/k/J/K/gg/gG` |
|---|---|---|---|
| Reference | Exon starts and ends (as today) | Gene starts and ends (as today) | Nothing |
| Alignment | Exons (as today, for now) | Genes (as today) | Scroll the reads |
| BED | Feature starts and ends | Same as lowercase, for now | Nothing |
| VCF | Variants. `w`/`b` go to the next or previous variant's position. `e`/`ge` go to its last reference base, which differs from the position for indels and SVs. | Same as lowercase, for now | Nothing |

- **Everything else** in normal mode is unchanged: `h/l/H/L`, `z/o`, counts, `s`, `:`, and
  `/`.
- **`u`/`Ctrl-r`** move through the jump list.
- **Jumps beyond the loaded window** need a search:
  - A plain BED file is in memory, so it searches `whole_file` directly.
  - Indexed BED, bigBed, and VCF files query windows that double in size, from 64 kb out to
    the end of the contig. Only boundaries inside a window count, because features outside
    it aren't read.
  - Then the search moves on to the following contigs in the reference's order, or the
    previous contigs for `b`/`ge`.
- **With no more features,** the message is `No more features in peaks.bed.`, and the view
  doesn't move.

### 1.4 Insert mode keys

Insert mode reuses vim's normal-mode grammar. **Counts come first, as in vim:** `3x`, `3Space`,
`d3l`. A count before an operator multiplies the count before its motion, so `2d3l` erases six
columns.

**The cursor** is the existing current position, `alignment_view.focus.position`. Insert mode
draws it as one reversed cell in the BED row, at `onscreen_x_coordinate(focus)`. One **column**
is one base at zoom 1, and `zoom` bases when zoomed out. The bases under a column come from
`coordinates_of_onscreen_x`.

| Key | Action | Vim analogy |
|---|---|---|
| `[n]Space` | Paint the n columns starting at the cursor, and move right by n. | Typing |
| `[n]Backspace` | Move left by n columns, then erase them. | Backspace |
| `[n]Shift-Backspace` | Move left by n columns, then paint them. This is painting to the left. | — |
| `[n]x`, `Delete` | Erase the n columns starting at the cursor. The cursor stays. | `x` |
| `[n]X` | Erase the n columns left of the cursor, and move onto the first of them. | `X` |
| `d{motion}` | Erase the range of the motion (see below). | `d` |
| `u` / `Ctrl-r` | Undo or redo this track's last edit step. At the ends, the message is `Already at oldest change` or `Already at newest change`. | `u` / `Ctrl-r` |
| `h/l/H/L`, arrows, `w/b/e/ge`, `W/B/E/gE`, `z/o`, counts | Move and zoom as in normal mode. The focus is a BED track, so word motions move between its features. | Motions |
| `j/k/J/K` | Ignored: insert mode stays on one track. | — |
| `Esc` | Cancel pending input, or go to normal mode. | `Esc` |

**`d{motion}` ranges** follow vim's inclusive and exclusive rules.
- **Exclusive motions** (`h`, `l`, `H`, `L`, `w`, `b`, `W`, `B`) erase from the cursor up to
  the target, without the target column.
  - So `d3l` erases the cursor column and the two columns to its right: three columns.
  - `dw` erases up to the start of the next feature.
- **Leftward motions** erase from the target up to the cursor, without the cursor column.
  `d3h` erases the three columns left of the cursor.
- **Inclusive motions** (`e`, `ge`, `E`, `gE`) include the target. `de` erases through the
  end of the feature.
- **After the erase,** the cursor moves to the start of the range, as in vim.
- **A motion that fails** (for example, no next feature) erases nothing and shows the motion's
  message.

**Shift-Backspace and the terminal**
- Terminals only tell `Shift-Backspace` from `Backspace` with the kitty keyboard protocol.
  `main.rs` pushes `KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES` when
  `supports_keyboard_enhancement()` is true, and pops it on exit.
- On terminals without it (macOS Terminal.app, for one), `Shift-Backspace` arrives as
  `Backspace` and erases.
  - The first time insert mode starts on such a terminal, tgv says:
    `This terminal sends Shift-Backspace as Backspace. Paint leftward with h and Space.`
  - `Ctrl-w` focus keys and `Ctrl-j`/`Ctrl-k` work without the protocol: in raw mode,
    crossterm reports `Ctrl-j` as `Char('j')` with `CONTROL`, not as `Enter`.
  - Visual mode (TODO) will give those terminals a proper way to paint left.
- With the protocol on, `Esc` arrives without a delay, and `Tab`/`Ctrl-i` and
  `Enter`/`Ctrl-j` are distinct. Check every existing binding under the protocol. For example,
  `Shift-w` must still arrive as `Char('W')`.

**Edit steps (undo granularity)**
- **Every command is one undo step,** for example `3x`, `d3l`, or `de`.
- **A run of `Space`, `Backspace`, and `Shift-Backspace` presses is one step,** like a run of
  typed text in vim. Any other key ends the run, and so do leaving insert mode, undo, redo,
  and `:w`.
- **The existing `BedHistory::begin_step`** marks the boundary. `App` calls
  `Dataset::begin_edit_step` before each command, and before the first key of a run.

**Adding bases [s, e]** (unchanged)
- Bases that are already covered are left alone.
- An uncovered run that touches the end of a feature on its left (that feature's end is
  s − 1) extends that feature. Otherwise, a run that touches the start of a feature on its
  right extends that feature's start. Otherwise the run becomes a new feature with no name,
  score, strand, or extra columns.
- Two features are never merged into one, so names and extra columns aren't lost. Painting
  through a gap leaves the two features end to end.

**Removing bases [s, e]** (unchanged)
- A feature fully inside the range is deleted.
- A feature that crosses one end of the range is trimmed.
- A feature that contains the range is split in two. The right part copies the name, score,
  strand, and extra columns.

### 1.5 Display

- **The focused target's sidebar section gets a background fill** across its whole area.
  This replaces the coloured `┃` divider.
  - In normal mode, a subtle neutral tint (`Palette::FOCUS_NORMAL`).
  - In insert mode, a green tint (`Palette::FOCUS_INSERT`).
  - The label is drawn bold in both.
  - Command and search input keep the fill of the mode they return to.
- **The reference's sidebar section** sits beside the sequence and gene rows, and is
  labelled with the genome name (for example `hg38`), so it has something to fill.
- **The console line:**
  - Insert mode shows `-- INSERT -- peaks.bed [+]` in green. `[+]` appears when the track is
    modified.
  - In normal mode with the sidebar hidden, the focused target's name shows right-aligned,
    like vim's ruler, so the focus is never invisible. With the sidebar shown, normal mode
    shows nothing, as in vim.
- **The insert cursor** is drawn only in the BED track being edited. Other tracks, including
  the ruler and the sequence, don't show it.

### 1.6 Track menu

The right-click menu on a file track's sidebar section, its BED or VCF row, or its alignment
area shares these items:

| Item | Shown when | Action |
|---|---|---|
| `Copy peaks.bed` | The track is an editable BED. | Remember the track for pasting. |
| `Paste peaks.bed below` | A track is copied. | Create an untitled BED track below the clicked track, with the copied track's features as they are at paste time. Focus it. |
| `Remove peaks.bed` | Always. | Remove the track. |
| `Remove peaks.bed (discard changes)` | The track is modified. Replaces the item above. | Remove it without writing. |
| `Insert track above` / `Insert track below` | The sidebar's menu only. | Create an untitled BED track next to the clicked track, and focus it. |

- **Copying is lazy.** `App` keeps `track_clipboard: Option<TrackId>`. Pasting clones the
  source's `PlainBed` then. That includes its unsaved edits, header lines, column count, and
  sorted flag. A `DataFrame` clone shares its buffers, so pasting is cheap.
- **The pasted track** is `untitled-N`. It has a fresh history and no saved revision, so it's
  marked modified until `:w <path>`.
- **Removing the copied track** clears `track_clipboard`, so "Paste" never points at a track
  that's gone.
- **Indexed BED and bigBed** aren't copyable, because a copy would have to load the whole
  file. That's left for later.

### 1.7 Commands

| Command | Behaviour |
|---|---|
| `:w` | Write the focused BED track to its file. Untitled tracks need a path: `untitled-1 has no file name. Use :w <path>.` Other tracks: `:w writes BED tracks. Use :ws to save the session.` |
| `:w <path>` | Untitled track: write it to the path, which becomes the track's file. Named track: write a copy, and the track still points to its own file, as in vim (`Wrote a copy to <path>. peaks.bed still has unsaved changes.`). |
| `:wa` | Write every modified BED track. Untitled tracks are skipped and named in the message. |
| `:wq [path]` | `:w`, then `:q`. |
| `:wqa` / `:xa` | `:wa`, then `:q`. |
| `:q` | Quit. If any track is modified: `peaks.bed has unsaved changes. Use :w, or :q! to quit without writing.` |
| `:q!` | Quit without writing. |
| `:ws [name\|path]` | Save the session (the old `:w`). |
| `:wsq [name\|path]` | Save the session and quit (the old `:wq`). It refuses like `:q` while tracks are modified. |
| `:new` | Create an untitled BED track below the focused track and focus it, in normal mode, as vim's `:new` does. With the reference focused, the new track goes last among the file tracks. |

- **`:bd` and `:bd!` are removed.**
- **Session saving and edited tracks:** `:ws` leaves untitled tracks out, saying so. Modified
  tracks are saved by path, with a note that the session points to the file on disk.
- **Success message:** `peaks.bed: 42 features written.`

### 1.8 Modified marker

- **Modified tracks** show `● ` before the file name in their sidebar section, and `[+]` in
  the mode line.
- **A track is modified** when its current revision isn't its saved revision. So undoing
  back to the saved state clears the marker, as in vim and VS Code.
- **New and pasted tracks** have no saved revision. They stay marked until the first
  `:w <path>`.
- **Untitled tracks** are labelled `untitled-1`, `untitled-2`, and so on.

### 1.9 What the file looks like after saving

(Unchanged.)
- **Row order:** rows keep their file order. Trimmed or extended features stay in their row.
  New features and split-off parts go in sorted position if the file was sorted when it
  loaded, and at the end otherwise. New tracks are always written sorted.
- **Columns:** every row has the file's original column count (at least BED3). Missing name,
  score, or strand are written as `.`. Extra columns 7 and up are written back as they were.
- **Headers:** `#`, `track`, and `browser` lines are written back at the top, in their
  original order.
- **Contig names** use the file's own names. A contig the file didn't have uses the
  reference's name, and is added as an alias for that file.
- **Writing is atomic:** the file is written to a temporary file in the same directory and
  renamed over the target. `.bed.gz` is written with bgzf, and `.bed` as plain text.

### 1.10 Agents

Edits update both the view and query states, so the MCP `query` tool's `bed` table and
`inspect_interval` see unsaved edits. No new session commands are needed.

---

## 2. Architecture

### 2.1 Key grammar: modelled on vim

The string register (`Registers::normal: String`, parsed by `gv_core::normal::update_by_char`)
can't express `Ctrl-w` prefixes, operators, or (later) visual mode. A typed grammar replaces
it. It follows the structure of vim's own implementation and of Zed's vim mode
(`crates/vim` in the Zed repository), which is the closest Rust reference for this design:

- **Neovim** splits a command into `cmdarg_T` (the count, a prefix like `g` or `Ctrl-w`, and
  the command char) and `oparg_T` (the pending operator, its count, and whether the motion is
  inclusive, exclusive, or linewise). Motions only move the cursor. The operator code then
  acts on the range between the old and new cursor (`do_pending_operator`).
- **Zed** has the same split as Rust types: `Operator`, `Motion` (with `inclusive()`), and
  `Object` enums, plus a `Mode` enum.

The grammar moves into a new module, `crates/tgv/src/keys.rs`. Key parsing is a front-end
concern, so it moves out of `gv-core`, and the `normal.rs` rstest cases move with it.

```rust
/// Keys typed so far in a command, like vim's `cmdarg_T` and `oparg_T`.
#[derive(Debug, Default)]
pub struct PendingKeys {
    count: Option<usize>,
    prefix: Option<Prefix>,
    /// The operator waiting for its motion, with the count typed before it.
    operator: Option<(Operator, Option<usize>)>,
}

enum Prefix { G, CtrlW }

/// Operators take a motion. Yank comes with visual mode (TODO).
pub enum Operator { Erase }

pub enum WordMotion { NextStart, NextEnd, PreviousStart, PreviousEnd }

/// Focus-agnostic: `App` resolves `Word` against the focused target.
pub enum Motion {
    Left(u64),
    Right(u64),
    Word { motion: WordMotion, big: bool, count: usize },
}

impl Motion {
    /// Vim's rule: `e`, `ge`, `E`, and `gE` include the target.
    pub fn inclusive(&self) -> bool;
}

/// A complete command, ready for `App`.
pub enum KeyCommand {
    Move(Motion),
    Operate { operator: Operator, motion: Motion },
    Paint { columns: u64, direction: Direction },  // Space, Shift-Backspace.
    Erase { columns: u64, direction: Direction },  // x, X, Backspace, Delete.
    Focus(FocusMove),
    Scroll(Scroll),
    Zoom(Zoom),
    // ...the rest of today's normal-mode messages.
}
```

- **`Registers::normal: String`** becomes `pending: PendingKeys`.
- **One `parse` function** serves both modes, and is told which mode it's in. Insert mode
  accepts operators and edit keys. Normal mode rejects them with today's
  `Invalid normal mode input` message.

### 2.2 Focus

```rust
/// What normal-mode motions, insert mode, and `:w` act on.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FocusTarget {
    Reference,
    Track(TrackId),
}

pub struct App {
    pub focused: FocusTarget,          // Replaces focused_track: Option<TrackId>.
    pub track_clipboard: Option<TrackId>,
    pub jumps: JumpList,               // The one navigation history. Unchanged.
    // ...
}
```

- **The focus is never `None`,** so `NO_FOCUS_MESSAGE` goes away.
- **`FocusMove`** (`Next(n)`, `Previous(n)`, `First`, `Last`, plus new `NextWrap` and
  `PreviousWrap`) is resolved over `layout.track_order` followed by `Reference`.
- **`KeyRegisterType::Insert { track: TrackId }`** stays. Insert mode always has a target,
  and keys edit the track insert mode started on.

### 2.3 Focus-aware word motions

`gv-core`'s eight `Movement::{Next,Previous}{Exons,Genes}{Start,End}` variants become one:

```rust
pub enum FeatureSource { Exons, Genes, Bed(usize), Variants(usize) }

pub enum Movement {
    // ...
    Feature { source: FeatureSource, motion: WordMotion, count: usize },
}
```

- **`App` resolves `Motion::Word`** to a `FeatureSource` from the focus:
  - Reference and alignment: `big` gives `Genes`, otherwise `Exons`.
  - BED: `Bed(i)`, and VCF: `Variants(i)`, with either case of the key.
- **`WordMotion`** lives in `gv-core`, and `keys.rs` uses it.
- **`State::movement`** dispatches:
  - `Exons` and `Genes` go to today's functions.
  - `Bed(i)` goes to `BedRepositoryEnum::feature_after` / `feature_before`. `PlainBed`
    searches `whole_file`. The indexed and bigBed readers search with the doubling windows
    from 1.3.
  - `Variants(i)` goes to the matching variant repository search.
- **The searches** take the file's contig names through the existing alias lookup, and
  return dataset coordinates (1-based, inclusive).

### 2.4 Insert-mode commands in `App`

- **`Operate { Erase, motion }`:**
  1. Resolve the motion to a target focus, without moving the view.
  2. Build the range from the cursor and the target with `inclusive()`.
  3. Call `begin_edit_step`, then `edit_bed(Remove)`.
  4. Move the cursor to the range start.
- **`Paint` and `Erase`** turn columns into bases with `coordinates_of_onscreen_x`, then
  call `edit_bed`.
- **`App` tracks whether the last key was part of a run** (`Space`, `Backspace`, or
  `Shift-Backspace`), to decide when to call `begin_edit_step` (1.4).
- **`u`/`Ctrl-r`** in insert mode call `Dataset::undo`/`redo` on the insert track. In normal
  mode they go to `jumps`. The two never share code paths.

### 2.5 Track menu and `:new`

- **`ContextMenu::track_items(track, file_name, modified, clipboard)`** builds the items in
  1.6.
  - `sidebar_items` and `alignment_items` append them.
  - A new `ContextMenuTarget::Track { track }` opens them on BED and VCF rows.
- **New and changed `Action`s:**
  - `CopyTrack(TrackId)`.
  - `PasteTrackBelow(TrackId)`.
  - `RemoveTrack { track: TrackId, force: bool }` (`force` used to be fixed at `false`).
- **`Dataset::duplicate_bed(source) -> Result<TrackId, TGVError>`:**
  - It adds `FilePath::UntitledBed(n)` the way `new_bed_track` does.
  - It replaces the empty `PlainBed` with a clone of the source's, carrying a fresh
    `BedHistory::new(false)`.
- **`:new`** parses to `Message::NewBedTrack`, which replaces
  `Action::NewBedTrack { below }`.

### 2.6 Code removed

- `KeyRegisterType::Track`, `Registers::handle_track`, and the track-mode `o`/`O`, `dd`,
  `u`/`Ctrl-r`, `J`/`K`, and `Enter` handling.
- `Message::CloseTrack`, the `:bd`/`:bd!` parsing and its test cases, and the `CloseTrack`
  handler in `app.rs`.
- `Action::NewBedTrack { below }`.
- The `┃` focus bar in `rendering/sidebar.rs`.
- `gv_core::normal`, once `keys.rs` replaces it.

### 2.7 Unchanged from the first version

- The data model: `PlainBed` with `whole_file`, `header_lines`, `column_count`,
  `sorted_on_load`, and `history`.
- `BedEdit`, `BedEditKind`, and `BedHistory`.
- The `Dataset` methods `edit_bed`, `begin_edit_step`, `undo`, `redo`, `write_bed`,
  `new_bed_track`, and `is_modified`.
- `FilePath::UntitledBed`, `BedSchema::EXTRA`, and `layout.track_order`.

---

## 3. Implementation order

Status: phases 1 to 7 are implemented.


Each phase builds and runs on its own.

1. **Two modes.**
   - Remove track mode and the code in 2.6, except `gv_core::normal`.
   - Make `Esc` in normal mode clear pending input only.
   - Add `:new` and the `Remove` / `Remove (discard changes)` items.
   - Make `u`/`Ctrl-r` in insert mode undo and redo.
   - Make `:` and `/` return to insert mode.
2. **Focus.**
   - Add `FocusTarget` with the reference, focused at startup.
   - Add `Ctrl-j`/`Ctrl-k` and the `Ctrl-w` keys, and click-to-focus on the reference rows.
   - Push the kitty keyboard flags in `main.rs`, and check the existing bindings under them.
3. **Display.** The sidebar fill, the reference's sidebar section, and the console line.
4. **Key grammar.**
   - Add `keys.rs` with `PendingKeys`, and port the `normal.rs` cases.
   - Add the insert keys: counts on `Space`/`Backspace`, `Shift-Backspace`, `x`/`X`,
     `Delete`, and `d{motion}`, plus the edit-step grouping.
5. **Focus-aware motions.**
   - Add `Movement::Feature` and the plain BED search.
   - Then the windowed search for indexed BED, bigBed, and VCF.
6. **Track copy and paste.** `track_items`, `ContextMenuTarget::Track`, `track_clipboard`, and
   `duplicate_bed`.
7. **Docs.**
   - `doc/src/usage.md`: modes, focus keys, the normal-mode word motions per track, insert
     keys, the track menu, commands, and the vim comparison table.
   - `doc/src/session.md`: the `:ws`/`:wsq` changes.

## 4. Verification

- **Tests:** run `cargo nextest run` after each phase.
- **Existing tests that change:**
  - The `command.rs` cases for `bd`/`bd!`, which are removed.
  - The `normal.rs` cases, which move to `keys.rs`.
  - The `offline_app.rs` focus tests: the default focus is now the reference.
- **New tests need the user's approval.** Good candidates:
  - `rstest` cases for `keys.rs`: counts before operators, `2d3l`, `Ctrl-w` prefixes, and
    `Esc` with pending input.
  - Inclusive and exclusive `d{motion}` ranges.
  - `PlainBed` feature search across contigs.
- **Manual run:**
  `cargo run -p tgv -- -g hg38 -r chr20:88108 crates/tgv/tests/data/simple.bam crates/tgv/tests/data/simple.bed`
  1. Check that the reference section is filled. Press `Ctrl-k` and watch the fill move.
     Click a track to focus it.
  2. On the BED track, press `w`/`b` and check that they move between features. Press `u`
     and check that it goes back through the jumps.
  3. Press `i`, `5Space`, `3Shift-Backspace`, `d2l`, and `u`/`Ctrl-r`. Check the features,
     the `●` marker, and the undo steps.
  4. Press `Esc` twice and `u`. Check that it moves through the jump list and doesn't touch
     the edits.
  5. Right-click the BED track, choose Copy, then right-click the BAM track and choose Paste
     below. Run `:w /tmp/copy.bed` and diff it against the input.
  6. Run `:new`, paint, and `:w /tmp/new.bed`. The marker clears.
  7. Check that `:q` with unsaved edits refuses, and that `:q!` quits.
  8. Repeat step 3 in Terminal.app, and check for the `Shift-Backspace` notice.

## 5. TODO (next round)

- **Visual mode.**
  - `v` starts a selection at the cursor. Any motion extends it, including `/` search. The
    selection is highlighted in the BED row.
  - `Space` paints the selection, `d`/`x` erases it, and `y` yanks it.
  - It becomes `KeyRegisterType::Visual { track, anchor }`, and the operator code from 2.4
    runs on the selection range. This is vim's `VIsual` and `do_pending_operator` path.
- **Yank and paste.**
  - `y{motion}` and visual `y` copy the features in the range, clipped to it, into an unnamed
    register on `App`. The features are stored as offsets from the range start, with their
    name, score, strand, and extra columns.
  - `p` pastes the span starting at the column after the cursor, and `P` pastes it ending
    just before the cursor, as in vim.
  - The register works across tracks and contigs.
  - Open question: should pasted features add rows as they are (BED allows overlaps), or
    follow the paint rules, which merge their coverage into existing features?
- **Text objects** for `d`, `y`, and `v`:
  - `if`: the BED feature under the cursor.
  - `ie` / `ig`: the exon or gene under the cursor, from the gene track.
  - For example, `vie Space` annotates the exon under the cursor.
  - These are Zed's `Object` and vim's `current_*` functions.
- **`J`**, vim's "join lines": join the feature under the cursor with the next one, filling
  the gap and keeping the left feature's name and columns. Waiting on the user's decision.
- **Uppercase word motions on BED and VCF** tracks. They do the same as lowercase for now.
- **Alignment word motions.** They use exons for now.
- **Undo moving the cursor** back to where the step began, as vim does.
- **Copying indexed BED and bigBed tracks.**
- **Mouse painting:** drag on a BED row in insert mode to paint, and drag with a modifier to
  erase.
- **Not planned:** `.` (repeat the last edit), `dd`/`D`, and bracket motions (`]e`, `[g`).
