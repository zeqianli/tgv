# Design: adding and removing tracks at runtime

Status: proposed, not implemented.

## Context

Tracks are fixed when the viewer starts. To change them, the user quits and starts again with other files.

How tracks are wired:

- `TrackRegistry` gives each file a `TrackId`. `get(id)` reads `entries[id]`, so an ID is also a position in the list.
- Each `TrackId` maps to a per-kind index, such as `Alignment(2)`. That index points into parallel vectors:
  - In `Repository`: the alignment, variant, and BED repositories.
  - In both `State`s (`view` and `query`): `alignments`, `alignment_options`, `paired_alignments`, `variants`, and `bed_intervals`.
  - In the TUI: `AlignmentView.y` and `App::focused_alignment`.
- `MainLayout` builds its track rows from the registry once, at startup.
- `ContigHeader` merges contigs from the reference and the BAM headers. `update_or_add_contig` only appends, so adding a BAM never changes existing contig indexes.
- `load_dataset` replaces the whole dataset, and the TUI rejects it with `DatasetFixed`.

Adding is mostly appending. Removing is the hard part, because every vector indexed by kind shifts.

## Plan

### 1. One place that adds and removes tracks

- **Stable IDs.** Look entries up by `id` instead of by position, and never reuse an ID. The TUI and agents can then keep referring to a track after others are removed.
- **`Dataset::add_files(paths)`** in `gv-session`:
  - Classifies the paths with `classify_and_build_tracks`.
  - Opens each repository.
  - Merges new contigs into both states' contig headers.
  - Appends a track slot to each state, and a registry entry with a fresh ID.
  - Appends to `settings.file_paths`, so `:w` and `get_dataset` include the new files.
- **`Dataset::remove_track(id)`:**
  - Removes the track from every per-kind vector.
  - Shifts the indexes of later tracks of the same kind.
  - Returns the removed `RepositoryFileIndex`, so front ends can apply the same shift.
- **Registry swap.** The registry is shared as an `Arc`. A change builds a new registry and hands it to the layout.

Shifting indexes on removal is the recommendation. The alternative leaves holes, such as `Option` slots, which is simpler but grows over a long session. Longer term, keying per-track state by `TrackId` everywhere would remove the shifting entirely, as a larger refactor.

### 2. TUI commands and layout

- **`:e PATH…`** adds files, possibly also as `:open`:
  - New tracks go at the end of the file tracks, in command-line order.
  - Existing tracks keep their requested heights.
  - The TUI extends `AlignmentView.y`, loads the visible region, and redraws.
- **Removing:** right-clicking a track's sidebar section offers "Remove *file name*", next to "Hide sidebar (s)". The TUI applies the same index shift to `AlignmentView.y`, the focused alignment, and the layout rows.
- **Errors** say what is missing, such as "simple.bam needs an index: simple.bam.bai" or "Unsupported file type".
- **Agents.** Later, matching `add_tracks` and `remove_tracks` MCP tools can use the same `Dataset` methods.

### 3. Drag and drop

Terminals don't send a drop event. Ghostty, iTerm2, kitty, WezTerm, and Terminal.app paste the dropped paths as shell-escaped text. With bracketed paste enabled (`crossterm::event::EnableBracketedPaste`), a paste arrives as one `Event::Paste(String)`.

- **Normal mode:** split the paste into paths and undo the shell escaping, for example with `shlex`.
  - If every path exists, add the files as `:e` does.
  - Otherwise, show "Not a file: …".
- **Command and search mode:** insert the pasted text into the command line. This also lets users paste a locus into `/`.
- **Limits:**
  - The terminal doesn't report where a drop landed, so dropped tracks are appended.
  - A paste and a drop look the same to tgv.
  - Over SSH, the pasted path names a file on the local machine, so it is reported as not found.

### 4. Responsive loading

Opening an S3 BAM or a large header can take seconds, and the event loop would freeze while it waits.

- **First version:** show "Opening X…" before waiting.
- **Then:** open repositories on a background task, which sends each opened repository back to the event loop as a message. The TUI adds it on arrival, and the user can keep navigating meanwhile.

## Verification

- Harness tests:
  - Add a BAM, a VCF, and a BED.
  - Remove the middle alignment.
  - Check the remaining tracks' IDs, layout rows, scroll positions, and saved session paths.
- Manual: drag files into Ghostty in normal mode and in command mode.

## Open questions

- Should removal shift indexes, or leave holes for now?
- Should the command be `:e`, `:open`, or both?
- Should background opening be in the first version, or a follow-up?
- Should agents be able to add and remove tracks in a viewer, or only in a headless session?
