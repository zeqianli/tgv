# Design: a GUI viewer

Status: the first version is implemented, in `gv-viewer` and `tgv-gui`. See [As built](#as-built)
for where it differs from the plan below. Not yet done: animated zooms, IME input, an app
bundle and signing, and Linux and Windows.

## Context

TGV's crates are ready for a second front end: no crate depends on `tgv`, and `gv-session`
says that a GUI can host a session the way the TUI does (see
[Architecture](../architecture.md)). The GUI is that second front end, built with
[gpui](https://www.gpui.rs/).

The goals:

1. **Continuous rendering, like IGV.**
   - Pans and zooms are smooth, at any scale, rather than in whole terminal columns.
   - Reads, coverage, genes, and features are drawn at pixel resolution.
2. **The same interaction as the TUI.** The vim key grammar, modes, command line, search,
   jumps, focus, BED editing, context menus, and track order all carry over, and so does the
   layout from top to bottom.
3. **As much shared code as possible.**
   - Logic that doesn't depend on the front end moves out of `tgv` into a crate that both
     front ends use.
   - What stays per front end, which is layout, input decoding, widgets, and drawing, uses the
     same structure in both.
4. **A mirrored code layout.** The GUI crate's files, types, function names, and the order of
   things inside them match the TUI's, so a change in one is easy to find in the other.

What stops the TUI code from being shared today:

- **The controller is part of the TUI `App`.**
  - `App::handle` in `crates/tgv/src/app.rs` covers jumps, focus order, word motions, BED
    edits, `:w`, removing tracks, opening files, and agent view requests (`serve_view`,
    `navigate_actions`, and `highlight_actions`).
  - `App::load_data` decides what to load at each zoom.
  - Almost none of this depends on ratatui. It needs only the width of the track area.
- **The viewport is measured in terminal cells.**
  - `AlignmentView { focus, zoom: u64, y }` in `crates/tgv/src/layout.rs` defines zoom as
    bases per column and computes against ratatui's `Rect`.
  - The column also appears in gv-core: `State::movement` takes `zoom` and moves
    `Movement::Left(n)` by `n * zoom` bases (`crates/gv-core/src/state.rs:153`).
  - It appears in the session file (`SessionFile::zoom`, "bases per character") and in the
    agent `ViewState::zoom`, which is "bases per screen column".
- **Input and colors use terminal types.**
  - `PendingKeys` (`keys.rs`) and `Registers` (`register.rs`) take crossterm `KeyEvent`s.
  - `Palette` (`rendering/colors.rs`) holds ratatui `Color`s.
- **Some pure data preparation lives in the rendering files.** Examples are coverage binning
  (`calculate_binned_coverage`), ruler ticks (`calculate_intermarker_distance`), coordinate
  text, the visible-window filters for gene segments and reads, sidebar option descriptions,
  read details, and hover descriptions.

## As built

The plan below held, with these changes:

- **One shared action enum.** Instead of a front-end `Action` that wraps the shared one,
  `gv_viewer::message::Action` has an `Ui(UiAction)` variant for the actions only a front end
  applies: layout changes and closing menus and popups. `Viewer::handle` leaves them, and the
  context menus and popups it builds, as `Effect`s that the front end takes with
  `Viewer::take_effects`. The window adds its own `GuiAction` for the gestures it predicts.
- **Keys go to the worker.** `Registers` live in the `Viewer`, so the window sends keys and
  the worker parses them, as the terminal does in its loop. Only gestures are predicted in the
  window: drags, scrolls, and zooms around the pointer. `h`, `z`, and the other keys wait for
  the worker's frame, which takes about a millisecond unless a load runs.
- **No `phase` in the viewport.** The window keeps the pixels of a pan that haven't added up to
  a base as an offset of its own, so the shared `Viewport` stays in whole bases.
- **Insert mode zooms in further.** One `h` step is `PixelScale::KEY_STEP_PIXELS` (8) pixels,
  so insert mode keeps a base at least 8 pixels wide. A column is then one base, and the cursor
  is one base, as the decision below intends.
- **Pinch through AppKit.** gpui 0.2.2 has no pinch events, so `tgv-gui/src/pinch.rs` installs
  a local AppKit event monitor for magnify events and forwards each step to the window. Pinches
  and Control or Command with a vertical scroll zoom continuously around the pointer, with
  `Zoom::Set`, which carries an exact number of bases per screen unit.
- **More shared preparation.** `gv_viewer::prepare` holds `alignment` (the `AlignmentScene` of
  bodies, clips, deletions, arrows, insertions, mismatches, and modifications), `track` (genes
  and their segments), `coverage`, `coordinate`, `sidebar`, and `hover` (the message line's
  descriptions). The terminal draws from the same scenes, cell for cell as before.
- **The popup has no front-end file.** `TextPopup` is data in `gv-viewer`, and each front end
  draws it in `rendering/menu.rs`.
- **`ViewerInfo` has no socket path.** Agents attach by process ID, and the socket path follows
  from it. `SessionConnection::find_viewer` is gone; `list_viewers` replaces it.
- **Metal shaders compile at startup.** gpui's `runtime_shaders` feature is on, so building
  doesn't need Xcode's Metal toolchain.

## Crates

```text
          ┌────────────────────────┐   ┌────────────────────────┐
          │ tgv                    │   │ tgv-gui                │
          │ TUI, `tgv`, `tgv mcp`  │   │ GUI, `tgv-gui`         │
          └───────┬────────────────┘   └───────────┬────────────┘
                  │  ratatui, crossterm            │  gpui
                  ▼                                ▼
          ┌────────────────────────────────────────────────────┐
          │ gv-viewer                                          │
          │ Viewer, Action, keys, registers, jumps, viewport,  │
          │ session files, menu contents, palette, track prep  │
          └───────────────────────┬────────────────────────────┘
                                  ▼
                          gv-session ──► gv-core
```

| Crate | Owns | Doesn't know about |
|---|---|---|
| `gv-viewer` (new) | `Viewer` (the controller, which owns the `Dataset`), the shared `Action` and its parts, the key grammar over a neutral `Key`, `Registers`, `JumpList`, `Viewport<Z>`, `AreaType`, track order, menu contents, popup contents, `SessionFile`, `Palette` in RGB, and the per-track data preparation | Terminals, windows, and pixels |
| `tgv` | Decoding crossterm keys, cell layout, `MouseRegister`, the menu and popup widgets, ratatui rendering, and the `tgv` and `tgv mcp` binaries | gpui |
| `tgv-gui` (new) | Decoding gpui keystrokes, pixel layout, `MouseRegister` with gestures, the menu and popup elements, gpui rendering, the dataset worker thread, and the `tgv-gui` binary | ratatui |

- **A separate binary** keeps gpui out of the `tgv` build and its install. Agents still connect
  through `tgv mcp`, which finds either kind of viewer through `SessionConnection`.
- **tgv-gui never depends on tgv**, as the previous proposal required.

## What moves into `gv-viewer`

Each module keeps its name, so `tgv/src/keys.rs` becomes `gv-viewer/src/keys.rs` plus a small
decoder that stays in `tgv/src/keys.rs`.

| Module | Moves from `tgv` | Change on the way |
|---|---|---|
| `message.rs` | `Action`, `FocusTarget`, `FocusMove`, `Motion`, `Operator`, `ColumnSpan`, `CursorAfter`, `AlignmentOptionUpdate`, `UpdateLayoutAction`, and `Highlight` (from `app.rs`) | Variants anchored to the screen, such as `OpenContextMenu { column, row }`, `ClosePopup`, and `ContextMenuChanged`, stay in each front end's own `Action`, which wraps the shared one (see below). |
| `keys.rs` | `Mode`, `PendingKeys`, and the grammar in `interpret` | Takes a neutral `Key { code: KeyCode, modifiers: Modifiers }` instead of a crossterm `KeyEvent`. The existing `rstest` grammar tests move with it unchanged. |
| `register.rs` | `KeyRegisterType`, `Registers`, command line editing, and the contig list cursor | Takes `Key`. It refers to `Scene`, which moves too. |
| `jumps.rs` | `ViewPoint` and `JumpList` | `ViewPoint<Z>` stores the zoom type of the front end (see the viewport section). |
| `viewport.rs` | `AlignmentView` and `OnScreenCoordinate` from `layout.rs` | Becomes `Viewport<Z>`, generic over the scale, and measured against a width in units rather than a `Rect`. |
| `layout.rs` | `AreaType` and the track order part of `MainLayout`: `track_order`, `set_tracks`, and display order | Heights stay per front end. `AreaType::desired_height` becomes a trait method that each front end implements, in rows or in pixels. |
| `viewer.rs` | `App::handle` (the shared variants), `apply_movement`, `focus_order`, `focus`, `resolve_motion`, `edit_columns`, `erase_to_word`, `edit_bed_range`, `new_track_at`, `write_track`, `write_all`, `remove_track`, `open_pending`, `apply_track_change`, `load_data`, `serve`, `serve_view`, `navigate_actions`, `highlight_actions`, `view_state`, `unsaved_changes_message`, and `session_notes` | Methods keep their names. `handle` returns neutral `Changes` instead of `RenderEvent`s. |
| `menu.rs` | `MenuItem` and the content builders: `ContextMenu::alignment_items`, `track_items`, and `sidebar_items` | Panel geometry and mouse handling stay in each front end's `menu.rs`. |
| `popup.rs` | The rows that `TextPopup::read_details` builds | Drawing stays per front end. |
| `session.rs` | `SessionFile`, `TrackEntry`, `parse_locus`, and the conversions to and from settings | `TryFrom<&App>` becomes `TryFrom<&Viewer<Z>>`. `zoom` becomes an `f64` (see the decisions section). |
| `settings.rs` | The `Settings` wrapper around `gv_core::settings::Settings`, its initial actions, and its session path, plus the shared CLI flags as a clap `ViewerArgs` | Each front end's `Cli` flattens `ViewerArgs` and adds its own flags. |
| `colors.rs` | `Palette` and the helpers that pick colors (`base_color`, `mismatch_color`, `softclip_color`, `modification_color`, and `cytoband_color`) | Slots become a `gv-viewer` `Rgb` type, with no ratatui dependency (see the decisions section). |
| `prepare/` | The pure helpers from `rendering/*.rs`: `get_linear_space`, `calculate_binned_coverage`, `calculate_intermarker_distance`, `get_abbreviated_coordinate_text`, `to_thousand_separated`, `get_abbreviated_length_string`, the segment layout in `get_rendering_info`, the visible-window read filters, `describe_option`, `alignment_depth_description`, and the hover descriptions and `read_at` from `mouse.rs` | File names match the rendering files they serve, for example `prepare/coverage.rs`. They take a bin count or a unit width, never a `Rect`. |

What stays in each front end, with the same file names in both: `main.rs`, `app.rs`,
`keys.rs` (the decoder only), `layout.rs` (heights and geometry), `mouse.rs`, `menu.rs`
(the widget), `popup.rs` (the widget), `settings.rs` (`Cli` and the front-end settings), and
`rendering/`.

### Two levels of action

```rust
// gv-viewer: what the viewer does, wherever the input came from.
pub enum Action { Core(Message), Motion(Motion), Focus(FocusTarget), EditColumns { .. }, .. }

// tgv and tgv-gui: the shared actions plus the actions that only make sense on one screen.
pub enum Action {
    Viewer(gv_viewer::Action),
    OpenContextMenu { target: ContextMenuTarget, anchor: Anchor }, // A cell or a point.
    ContextMenuChanged(..),
    ClosePopup,
    UpdateLayout(UpdateLayoutAction),
    SwitchScene(Scene),
}
```

- **Routing.** Each front end's `App::handle` handles its own variants and passes
  `Action::Viewer` to `Viewer::handle`. Keys and agent requests produce shared actions. Mouse
  input produces both kinds.
- **Results.** `Viewer::handle` returns `Changes`, for example "the viewport moved", "track
  data changed", "a track was added or removed", or "the message changed".
  - The TUI turns `Changes` into `RenderEvent`s, so its partial redraws keep working.
  - The GUI calls `cx.notify()`.

### The viewport, independent of resolution

The core idea is that the viewer reasons in bases, and each front end says how wide one
screen unit is.

```rust
pub trait Scale: Copy + Debug + PartialEq {
    /// Bases covered by `units` screen units: columns in the TUI, pixels in the GUI.
    fn bases(&self, units: f64) -> f64;
    fn zoom_in(self, factor: u64) -> Self;
    fn zoom_out(self, factor: u64) -> Self;
    fn fit(bases: u64, width_units: f64) -> Self;
    /// The most bases per unit that still show each kind of data. `load_data` uses these.
    const MAX_TO_DISPLAY_ALIGNMENTS: f64;
    const MAX_TO_DISPLAY_SEQUENCES: f64;
    const MAX_TO_DISPLAY_INDEXED_FEATURES: f64;
}

pub struct ColumnScale(pub u64);                  // TUI: bases per column, as today.
pub struct PixelScale { pub bases_per_pixel: f64 } // GUI: continuous.

pub struct Viewport<Z: Scale> { pub focus: Focus, pub zoom: Z, pub y: Vec<usize>, pub phase: f64 }
pub struct Viewer<Z: Scale> { pub dataset: Dataset, pub viewport: Viewport<Z>, pub jumps: JumpList<Z>, .. }
```

- **The TUI keeps its behavior exactly.**
  - `ColumnScale` reproduces today's integer zoom and its limits (32, 2, and 100 bases per
    column).
  - The TUI's insta snapshots must not change. That is the check that the move into
    `gv-viewer` changed nothing.
- **The GUI scale is continuous.**
  - `PixelScale` zooms by any factor, so a pinch or Ctrl+wheel zoom is smooth.
  - `z` and `o` still double and halve the zoom, and the GUI animates the change.
  - The limits are in bases per pixel, chosen so that alignments appear at about the same
    region width as in the TUI. With 8-pixel cells, for example, the alignment limit is about
    `32 / 8 = 4` bases per pixel.
- **`phase` holds the fraction of a base**, so a drag pans smoothly by pixels when one base
  is many pixels wide.
  - Genomic coordinates stay one-based and inclusive.
  - `phase` is a screen offset, not a coordinate. It is always 0 in the TUI.
- **gv-core movement takes bases.**
  - `Movement::Left` and `Movement::Right` carry bases, and `State::movement` loses its
    `zoom` parameter.
  - `Viewer::apply_movement` converts a count of columns, such as `h` or `30l`, through
    `Z::bases`.
  - In the GUI, one `h` step is a fixed number of pixels (`KEY_STEP_PIXELS`), so `h` covers
    about the same fraction of the screen as one TUI column.
- **Hit testing is shared.** `onscreen_x_coordinate`, `coordinates_of_onscreen_x`,
  `coordinate_of_onscreen_y`, and `onscreen_y_coordinate` move to `Viewport<Z>` and work on
  `f64` units. Each front end's mouse code converts cells or pixels to units.

## Where the GUI's dataset lives: a worker thread

This is the main way the GUI differs from the TUI, so this section explains the reasons.

### Why the question comes up

- **The TUI blocks while it loads.**
  - The TUI `App` owns the `Dataset` and awaits loads inside its event loop (`App::load_data`
    calls `dataset.view.ensure_loaded`).
  - While a BAM or a remote file is read, the terminal shows the last frame and input waits.
    Agent queries also pause input, as `architecture.md` notes.
  - In a terminal this is acceptable. Every move is a separate key press, each causes at most
    one load, and `CachePolicy::VIEWER` padding makes most moves load nothing.
- **A GUI can't do the same.**
  - **Gestures fire constantly.** Drags, trackpad scrolls, and pinches send 60 to 120 events
    a second. Any frame that waits for a 200 ms read stutters the whole window, and macOS
    shows the spinning cursor when the main thread stalls for more than about two seconds.
  - **gpui renders and handles input on the main thread.** Whatever owns the `Dataset` there
    blocks the window whenever it reads.
  - **The `Dataset` can't move between threads.** Repository readers aren't `Send`
    (`Session::spawn` notes this). gv-core's loads are async on Tokio, and gpui's executor
    isn't Tokio. Some of the work is synchronous, too: plain BED and VCF parsing, coverage,
    and stacking.
- **So there are two choices:**
  - Block the window thread during loads, as the TUI does.
  - Give the `Dataset` its own thread and send the window something it can draw while a load
    runs.

### The design

```text
 gpui main thread                                   dataset worker thread (current-thread Tokio)
┌────────────────────────────────────┐             ┌───────────────────────────────────────┐
│ App (gpui entity)                  │  Vec<Action>│ Viewer<PixelScale>                    │
│  keys → Registers → Action ────────┼────────────►│  handle(actions) → load_data()        │
│  mouse → MouseRegister → Action    │             │  serve(Request) for agents ◄── socket │
│  preview: Option<Viewport>         │ Arc<Frame>  │                                       │
│  frame: Arc<Frame> ◄───────────────┼─────────────│  publishes a Frame after each batch,  │
│  render() draws frame (+ preview)  │             │  and one before a load starts         │
└────────────────────────────────────┘             └───────────────────────────────────────┘
```

- **The worker owns `Viewer<PixelScale>`**, and with it the `Dataset`, the viewport, the
  jumps, the focus, and the highlights.
  - It runs the same `Viewer::handle` and `Viewer::serve` that the TUI runs inline.
  - Like the TUI's event loop, it receives every message already queued before it applies any,
    so it loads at most once per batch.
- **The worker also hosts the session.**
  - `SessionSocket` binds to the worker's `SessionHandle`, so agent requests go to the worker.
  - A slow agent query never freezes the window. It only delays the next frame.
- **A `Frame` is a cheap snapshot** of everything rendering reads:
  - the viewport and a revision number;
  - `Arc` clones of the view state's tables;
  - the track registry, the highlights, the focus target, the messages, and the modified
    tracks;
  - `loading: Option<Region>`, so the status bar shows "Loading chr1:…" while old data stays
    on screen.

  Polars `DataFrame`s already share their columns through `Arc`. Two gv-core changes make
  `State` cheap to clone:
  - `Alignment::records` becomes `Arc<Vec<RecordBuf>>`.
  - The table types derive `Clone`.
- **The window predicts pure viewport changes.**
  - Pans and zooms that need no data are applied to a local copy of the viewport (`App::preview`)
    with the same `Viewport` code, and drawn at once from the last frame. These are drags,
    wheel and pinch zooms, `h`, `l`, `z`, `o`, and fitting to a region.
  - The last frame was loaded with viewer padding, so a nearby preview has data to draw.
  - The window sends the same actions to the worker. When a frame arrives whose revision
    covers them, the preview is dropped.
  - Actions that need data wait for the worker's frame, as they wait for the event loop in
    the TUI. These are gene and word motions, edits, and `:w`.
- **The window talks to the worker through channels that don't depend on an executor**, as
  gv-session's `mpsc` channels already do.
  - A `cx.spawn` task on the main thread receives frames and calls `cx.notify()`.
  - The worker wakes gpui through an `async_channel` or `futures` channel.

### Compared with the TUI

| | TUI today | GUI with a worker |
|---|---|---|
| While a load runs | The frame and input freeze | Rendering and gestures continue, and a progress message shows |
| An agent query | Pauses input | Delays only the next frame |
| Where `Viewer` runs | Inline in the event loop | On the worker thread |
| Cost | — | A snapshot per batch (`Arc` clones), one frame of latency for actions that need data, and preview reconciliation |

Both front ends run the same `Viewer`. Later, the TUI can move its `Viewer` onto the same
worker to stop freezing during loads, without changing the shared code. That is not part of
this plan.

## The tgv-gui crate

### File layout, matching `tgv`

```text
crates/tgv-gui/src/
├── main.rs        Cli parsing, gpui Application::run, window, SessionSocket::bind on the worker
├── lib.rs
├── app.rs         App: Render impl, handle(), the frame and preview, Scene
├── worker.rs      the dataset worker thread (GUI only; the TUI's equivalent is its event loop)
├── keys.rs        gpui Keystroke → gv_viewer::Key
├── message.rs     the GUI Action: Viewer(..) plus actions anchored to the window
├── layout.rs      MainLayout and ResolvedMainLayout in pixels; desired_height in pixels
├── mouse.rs       MouseRegister: click, drag pan, wheel scroll and zoom, pinch, dividers, hover
├── menu.rs        ContextMenu element over gv_viewer::menu items
├── popup.rs       TextPopup element
├── settings.rs    Cli (flattens ViewerArgs), GUI Settings: font, row heights, theme
└── rendering/
    ├── mod.rs           render_main, render_highlights, render_alignment_divider
    ├── alignment.rs     render_alignment, render_paired_alignment
    ├── bed.rs           render_bed
    ├── colors.rs        Rgb → Hsla, GUI-only colors
    ├── console.rs       render_console, render_mode_line
    ├── contig_list.rs   render_contig_list
    ├── coordinate.rs    render_coordinates
    ├── coverage.rs      render_coverage
    ├── cytoband.rs      render_cytobands
    ├── intervals.rs     shared interval drawing, the "zoom in to see" notice
    ├── menu.rs          context menu and popup drawing
    ├── sequence.rs      render_sequence
    ├── sidebar.rs       render_sidebar
    ├── status_bar.rs    render_status_bar
    ├── track.rs         render_track (genes)
    └── variants.rs      render_variants
```

- **Rendering functions keep the TUI's names and order.**
  - The TUI takes `(buf: &mut Buffer, area: Rect, …)`.
  - The GUI takes `(window: &mut Window, bounds: Bounds<Pixels>, frame: &Frame, viewport:
    &Viewport<PixelScale>, palette: &Palette, …)`.
  - `render_main` matches on `AreaType` in the same order as the TUI's.
- **Naming.** gpui's context type is also called `App`. The GUI keeps `App` for its own entity
  and refers to gpui's as `gpui::App`, so the names still mirror the TUI.

### Input

- **Keys go through the shared grammar, not gpui's keymap.**
  - The focused root element's `on_key_down` decodes `KeyDownEvent::keystroke` into a `Key`
    (`keys::key_from_keystroke`) and passes it to `Registers::handle_key_event`.
  - Every key, count, operator, mode, and command behaves exactly as in the TUI.
  - gpui's `actions!` and `KeyBinding`s handle only platform shortcuts, which map to the same
    actions: `cmd-q` to `Quit`, `cmd-o` to an open dialog that becomes `OpenFiles`, `cmd-s` to
    `WriteTrack`, and `cmd-=` and `cmd--` to `Zoom`.
- **The command line and search** draw the `Registers` buffer and cursor in the console area,
  as the TUI does. IME composition is out of scope for the first version.
- **The mouse** (`mouse.rs`, with the TUI's `MouseRegister` method names):
  - **Panning:** left-drag pans continuously, and a horizontal trackpad scroll pans.
  - **Scrolling and zooming:** a vertical wheel over an alignment scrolls its rows.
    Ctrl+wheel or a pinch zooms around the pointer.
  - **Clicks:** a click on the cytoband jumps there, a double click zooms in around the
    point, a click on a read opens its details, and a right-click opens the context menu
    from `gv_viewer::menu`.
  - **Hover and dividers:** hovering shows the shared hover description in the status bar,
    and dividers and the sidebar edge are dragged to resize.
  - **Files:** dropping files on the window opens them (`OpenFiles`), like a paste in the
    TUI.

### Layout

- **The areas and their order are the TUI's:** cytoband, coordinate ruler, coverage and
  alignment pairs separated by dividers, sequence, gene track, variants, BED tracks, the
  console or mode line, and the status line. The sidebar sits on the left, and `s` toggles it.
- **`ResolvedMainLayout` holds `Bounds<Pixels>`**, with the TUI's methods,
  `get_area_type_at_position` among them.
- **Heights are in pixels**, set in GUI settings. Suggested defaults: cytoband 24, ruler 28,
  coverage 60, sequence 16, genes 40, variants 16, and 14 per BED lane. Alignments take the
  rest.
- **gpui's flex layout** places the sidebar and the track column. Each track is a `canvas()`
  element that paints quads and shaped text directly, and doesn't build a tree of elements
  per read.

### Rendering at pixel resolution

Every track draws from `Frame` and `Viewport<PixelScale>`. Below, bpp means bases per pixel.

| Track | At high zoom | At mid zoom | Beyond the display limit |
|---|---|---|---|
| Sequence | Letters when a base is at least about 7 pixels wide | Colored bars, one per base, when bpp ≤ 1 | Hidden; above `MAX_TO_DISPLAY_SEQUENCES`, sequence isn't loaded |
| Alignment | Read bodies with mismatch letters, insertion markers, deletion lines, and soft clips; strand arrows at the ends | Read bodies with mismatches as colored ticks at least 1 pixel wide; rows of about 10 pixels, or squished to 2 to 3 pixels when the depth doesn't fit | "Zoom in to see alignments", as in the TUI |
| Coverage | One bar per base, stacked with alternate-allele colors | `calculate_binned_coverage` with one bin per pixel; the y-axis autoscales to the visible maximum, as in the TUI | Hidden with alignments |
| Genes | Exons as thick boxes, UTRs thinner, introns as lines with strand chevrons, and labels | The same, with labels placed so they don't overlap, using the shared segment layout in pixels | Gene bodies only |
| BED and variants | Features with names and strand chevrons; lanes from `bed-display.md` (expanded, squished, or collapsed) | Features at least 1 pixel wide | Indexed files show the TUI's "zoom in" notice |
| Ruler | Tick spacing from `calculate_intermarker_distance` with a minimum spacing in pixels | — | — |
| Cytoband | Ideogram with rounded ends, the centromere drawn as a pinch, and a box around the current view | — | — |
| Highlights | Translucent bands across every track, instead of the TUI's tinted ruler columns | — | — |

- **Sizes snap to device pixels**, so thin features stay sharp.
- **Paint cost follows what is visible.**
  - Reads are filtered to the visible window with the shared filters, as in the TUI.
  - gpui batches quads on the GPU, so thousands of reads per frame are affordable.
  - The ideas in `render-cache.md`, such as range queries over sorted tables, apply to both
    front ends.

### Insert mode and BED editing

- **The cursor is the column at the center of the view**, as in the TUI.
  - In the GUI, insert mode keeps the zoom at 1 base per pixel or closer (see the decisions
    section), so the cursor is always the single base under the center line.
- **`ColumnSpan` is still counted in columns.** `Viewer::edit_columns` converts it through the
  scale, so the paint rules from `bed-editing.md` stay in one place.

## Agents

- **Agents reach the GUI the same way as the TUI.** The worker serves `Request::View` through
  the shared `Viewer::serve_view`.
  - `navigate` becomes `Move` and `Zoom(Fit)`, then shows the notice "Press u to go back to
    Y".
  - `highlight` draws the translucent bands.
- **The GUI binds its own `SessionSocket`** at the same path pattern as the TUI.
  - With one viewer open, `tgv mcp` connects to it as it does today.
  - With several open, the agent chooses one (see "Choosing a viewer in MCP").

## Changes outside the new crates

- **gv-core**
  - `Movement::Left` and `Movement::Right` carry bases, and `State::movement` loses `zoom`.
  - `Alignment::records` becomes `Arc<Vec<RecordBuf>>`, and the table types derive `Clone`
    so a frame can share them.
- **gv-session**
  - `ViewState::zoom` becomes an `f64`.
  - `ViewerInfo`, `SessionConnection::list_viewers`, and `SessionError::ViewerAmbiguous` are
    new.
- **gv-mcp**
  - The `list_viewers` and `attach` tools are new, and routing fails with `viewer_ambiguous`
    while several viewers are open and the agent hasn't attached to one.
- **tgv**
  - The modules listed above move into `gv-viewer`, and `App` becomes `App { viewer:
    Viewer<ColumnScale>, layout, resolved_layout, registers, mouse_register, context_menu,
    popup, scene, render_buffer, .. }`.
  - The architecture tables in `architecture.md` and the AGENTS.md crate layout gain
    `gv-viewer` and `tgv-gui`.

## Implementation order

1. **gv-core:** `Movement` in bases, and a cheap `Clone` for `State`. The TUI's behavior and
   snapshots don't change.
2. **Zoom becomes a float:** `SessionFile::zoom` and `ViewState::zoom` become `f64`, and the
   TUI rounds the session zoom when it loads one.
3. **Create `gv-viewer` and move `tgv` onto it**, one module at a time:
   - `jumps`, `message`, `keys` (with `Key`), and `register`;
   - `viewport` (`Viewport<ColumnScale>`), `colors`, `prepare`, `menu`, `popup`, and
     `session`;
   - finally `Viewer` and the `handle`, `serve`, and `load_data` paths.

   After each step, `cargo nextest run` passes, and the insta snapshots are unchanged.
4. **The tgv-gui skeleton:**
   - the window, `worker.rs`, the frames, `layout.rs`, and `keys.rs`;
   - the ruler, cytoband, gene, and sequence tracks;
   - the status line and console.
5. **Alignment and coverage at pixel resolution**, plus `mouse.rs` gestures and the preview
   viewport.
6. **Variants, BED with lanes, and insert-mode editing**, plus context menus, popups, the
   contig list, and file drops.
7. **The rest:**
   - the socket and agents, including `list_viewers`, `attach`, and `viewer_ambiguous`;
   - session files;
   - packaging: an app bundle, and signing per `macos-signing.md`.

## Verification

- **After step 3:** `cargo nextest run` passes, and every TUI insta snapshot and the
  `AppHarness` integration tests pass unchanged. The existing grammar tests in `keys.rs` run
  from `gv-viewer`.
- **For the GUI**, run `tgv-gui` on the test data, for example `tests/data/covid.sorted.bam`
  with `-g covid --offline --cache-dir tests/data/cache`, and check:
  - drag, pinch, and wheel zooms are smooth;
  - `h`, `l`, `z`, `o`, `w`, `b`, `gg`, `G`, `:`, `/`, `u`, and `Ctrl-r` behave as in the
    TUI;
  - a load shows "Loading" while the old data stays on screen.
- **With only the GUI open,** run `tgv mcp` and call `navigate`, `highlight`, and
  `view_state`. The GUI moves and shows the notice.
- **With the TUI and the GUI open on different files,** call a viewer tool:
  - it fails with `viewer_ambiguous`, and the error lists both viewers;
  - `list_viewers`, then `attach`, routes later calls to the chosen viewer.
- **Tests:** gpui's `TestAppContext` can drive `App` headlessly if GUI tests are wanted later.
  Adding them is a separate decision.

## Decisions

- **Zoom is a float in session files.**
  - `SessionFile::zoom` becomes an `f64` that counts bases per screen unit.
  - The GUI writes bases per pixel. The TUI rounds the value when it loads it,
    `ColumnScale(zoom.round().max(1.0) as u64)`, in its own session loading.
  - A zoom doesn't need to survive exactly when a session goes back and forth between the
    TUI and the GUI.
  - The session schema keeps its version, because session files are still experimental.
  - `ViewState::zoom` becomes an `f64` with the same meaning, so agents see one unit.
- **One palette, without ratatui.**
  - Today `Palette` in `crates/tgv/src/rendering/colors.rs` stores ratatui `Color`s, and
    `DARK_THEME` uses ratatui's `tailwind` constants.
  - The shared `Palette` in `gv-viewer/src/colors.rs` stores a `gv-viewer` `Rgb { r, g, b }`
    type. `DARK_THEME` spells out those tailwind values as RGB literals, so `gv-viewer`
    doesn't depend on ratatui.
  - The color helpers, such as `base_color` and `cytoband_color`, return `Rgb`.
  - Each front end converts at its edge: the TUI with `From<Rgb> for ratatui::style::Color`,
    which gives `Color::Rgb`, in `tgv/src/rendering/colors.rs`, and the GUI with
    `From<Rgb> for gpui::Hsla` in `tgv-gui/src/rendering/colors.rs`.
  - `Modifier` and `Style` stay in the TUI.
- **Insert mode needs at least one pixel per base in the GUI.**
  - Entering insert mode zooms in to at most 1 base per pixel, centered on the cursor, so
    every edit covers whole bases.
  - Zooming out past that limit in insert mode stops at 1 base per pixel.
  - The cursor is then always the single base under the center line.
- **macOS first.** Linux and Windows come later.
- **The TUI and the GUI don't share a session.**
  - Each viewer process hosts its own `Dataset` and binds its own `SessionSocket`, as the TUI
    does today.
  - The MCP server's job is to pick the right viewer (see the next section).

## Choosing a viewer in MCP

Today, `SessionConnection::find_viewer` connects to the most recently started viewer. With a
TUI and a GUI open at once, that guess is often wrong. `viewer-connection.md` describes the
same problem for several TUI windows. The change:

- **Viewers describe themselves.**
  - A new `Call::Describe`-style reply, `ViewerInfo`, holds:
    - the viewer kind (`tui` or `gui`);
    - the process ID and the socket path;
    - the files open in the viewer;
    - the displayed locus;
    - the time the viewer started.
  - `SessionConnection` gains `list_viewers()`, which connects to every live socket and
    collects `ViewerInfo`. It removes stale sockets, as `find_viewer` does today.
- **One viewer is used without asking.** It is the same as today.
- **Several viewers need a choice.**
  - A new `list_viewers` MCP tool returns every `ViewerInfo`.
  - A new `attach` tool takes a process ID and makes that viewer the target of later calls.
  - Until the agent attaches, a viewer tool call with several viewers open fails with a new
    `viewer_ambiguous` error. The error lists the viewers and tells the agent two things:
    - Choose the viewer from the context of the question, for example a file name, a locus,
      or "the GUI".
    - If it is still ambiguous, ask the user to confirm before calling `attach`.
  - The server instructions in `gv-mcp` say the same.
- **Where the changes go:**
  - `ViewerInfo` and `SessionError::ViewerAmbiguous` with its code go in
    `crates/gv-session/src/schema/` and `crates/gv-session/src/error.rs`.
  - `list_viewers` goes in `crates/gv-session/src/socket.rs`.
  - The tools and the routing go in `McpHandler::call`, in `crates/gv-mcp/src/lib.rs`.
  - The error tables in `doc/src/server.md` and `doc/src/server/view.md` gain the new error.
- **It covers part of `viewer-connection.md`.** Item 4 of that doc (`list_viewers` and
  `attach`) becomes part of this plan. Its `viewer_changed` error still applies when the
  attached viewer closes and another opens.
