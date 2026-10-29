# Architecture

TGV is a Cargo workspace with six crates. This page explains how the crates depend on each other, which crate owns which data, how requests reach that data, how the two front ends share one viewer, and how each front end's event loop combines input with agent requests.

## Crates

```text
   ┌──────────────────────────────┐            ┌──────────────────────────────┐
   │ tgv                          │            │ tgv-gui                      │
   │ TUI, `tgv`, `tgv mcp`        │            │ window, `tgv-gui` (gpui)     │
   └────┬───────────────┬─────────┘            └──────────────┬───────────────┘
        │               ▼                                     │
        │    ┌─────────────────────┐                          │
        │    │ gv-mcp              │                          │
        │    │ MCP tools over stdio│                          │
        │    └──────────┬──────────┘                          │
        ▼               │                                     ▼
   ┌────────────────────┼─────────────────────────────────────────────────────┐
   │ gv-viewer          │  Viewer, actions, keys, modes, viewport, menus,      │
   │                    │  session files, palette, data preparation            │
   └────────────────────┼─────────────────────┬────────────────────────────────┘
                        ▼                     ▼
                ┌────────────────────────────────────┐
                │ gv-session                         │
                │ Dataset, requests, SQL tables      │
                └──────────────────┬─────────────────┘
                                   ▼
                ┌────────────────────────────────────┐
                │ gv-core                            │
                │ data model, repositories, loading  │
                └────────────────────────────────────┘
```

Arrows point from a crate to the crates it depends on. The front ends also depend on `gv-core` and `gv-session` directly, for their data types.

| Crate | Owns | Doesn't know about |
|---|---|---|
| `gv-core` | `State` (the loaded region of each track), `Repository` (open BAM, VCF, BED, reference, and annotation readers), `State::ensure_loaded` and `CachePolicy`, `TrackRegistry`, table schemas, and file classification | Sessions, MCP, and terminals |
| `gv-session` | `Dataset`, the request types (`Request`, `SessionHandle`, `Requests`, `Responder`), request and response types such as `QueryRequest` and `ViewState`, `SessionError`, and the SQL tables built on Polars SQL | MCP and terminals |
| `gv-mcp` | The MCP tools, the mapping from `SessionError` to MCP error codes, `McpError`, and `serve` | Terminals |
| `gv-viewer` | `Viewer` (the controller, which owns the `Dataset` and hosts the session), `Action` and `UiAction`, the key grammar over a neutral `Key`, `Registers`, `JumpList`, `Viewport<Z>` and its `Scale`, `AreaType`, menu items, `TextPopup`, `SessionFile`, `Settings` and the shared CLI options, the RGB `Palette`, and the data preparation in `prepare` | Terminals, windows, and MCP |
| `tgv` | `App`, cell layout, decoding terminal keys, mouse handling, the menu widget, ratatui rendering, the CLI, and the `tgv mcp` subcommand | gpui |
| `tgv-gui` | `App`, pixel layout, decoding window keystrokes, mouse and trackpad gestures, the menu widget, gpui rendering, the dataset worker thread, and the `tgv-gui` binary | ratatui |

No crate depends on a front end. The front ends share their logic through `gv-viewer`, and their files mirror each other: `app.rs`, `keys.rs`, `layout.rs`, `mouse.rs`, `menu.rs`, `settings.rs`, and one rendering file per kind of area, with the same function names.

Each crate turns on the Polars features it needs in its own `Cargo.toml`. The SQL features belong to `gv-session`.

## Who owns the data

A `Dataset` owns everything loaded from one set of files:

```text
Dataset
├── settings     reference, backend, cache directory
├── repository   open readers, shared by both states
├── tracks       TrackRegistry: track IDs → files and repository indexes
├── view: State  what a viewer draws
│   └── alignments, sequence, genes, variants, BED features, cytobands
│                                       ← the region on screen, padded (CachePolicy::VIEWER)
└── query: State what agent requests load
    └── alignments, sequence, genes, variants, BED features
                                        ← the requested region exactly (CachePolicy::EXACT)
```

- **Cytobands load once.** The view loads the reference's cytobands for every contig the first time it needs them, in one query, and keeps them for the session. The query state doesn't load them.
- **The two states load regions separately.** An agent query on another contig loads into `query` and never replaces what `view` shows.
- **Every track kind loads by region.** Each table records its loaded bounds, and `State::ensure_loaded` reads a track only when `has_complete_data(region)` is false. The repository decides how to read the region:
  - Indexed files (bgzipped VCF and BED with a `.tbi` or `.csi` index, BCF, and bigBed) are queried through their index.
  - Plain VCF and BED files are parsed once into the repository, which then answers each read with the whole contig. Both states read through the one repository, so a plain file is parsed once.
- **The registry records the files.** Each `TrackEntry` holds the track's ID, its `RepositoryFileIndex`, and its `FilePath`. Settings describe only the reference, and `Dataset::new` takes the startup files separately, so `get_dataset` and `:w` read the current files from the registry.
- **Each file keeps its own contig names.** A `ContigHeader` groups names that mean the same contig, such as `chr1` and `1`, into one `Contig`. Each contig records the name that each track file uses, keyed by `RepositoryFileIndex`, so a read asks the file for exactly the name the file listed. A file that doesn't list the contig reads an empty table.
- **Tracks can be added and removed while a dataset is loaded.** `Dataset::add_files` opens the files, records their contig names in both states' contig headers, and appends a track slot to each per-kind vector, either for every file or for none. `Dataset::remove_track` removes a track from every per-kind vector and from the contig headers, so later tracks of the same kind shift down by one; it returns the removed `RepositoryFileIndex` so front ends can shift their own per-kind state, such as `AlignmentView::y`. Track IDs are never reused, so the remaining tracks keep their IDs.
- **One `Repository` is safe without locks.** A host handles one request at a time, so the two states never read files concurrently.

## Requests

Callers hold a `SessionHandle` and send `Request`s. Whoever owns the `Dataset`, the *host*, receives them from the matching `Requests` stream and answers each one through its `Responder`.

```text
 caller                                   host (owns the Dataset)
┌──────────────────┐   Request + Responder   ┌──────────────────────────────┐
│ SessionHandle    │ ──────── mpsc ────────► │ Requests::recv               │
│  .describe()     │                         │                              │
│  .inspect()      │                         │ Data      → Dataset::serve   │
│  .query()        │                         │ LoadDataset                  │
│  .load_dataset() │                         │ View      → navigate, ...    │
│  .navigate()     │                         │ Shutdown                     │
│  .navigate()    │ ◄────── oneshot ─────── │ Responder::respond(result)   │
│  .view_state()   │   Result<T, SessionError>                              │
└──────────────────┘                         └──────────────────────────────┘
```

| Request | Headless worker | TUI |
|---|---|---|
| `Data`: describe, inspect, or query | `Dataset::serve`, or a `no_dataset` error before the first load | `Dataset::serve` on the query state |
| `LoadDataset` | Replaces the dataset | Fails with `DatasetFixed` |
| `View`: navigate or view state | Fails with `NoViewer` | Moves the view |
| `Shutdown` | Closes the dataset and stops | Ignored; the user decides when to quit |

Requests whose caller has stopped waiting are skipped.

## Hosts

### Headless: `tgv mcp`

```text
 MCP client ──stdio──► gv-mcp (rmcp handler, async runtime threads)
                         │ SessionHandle
                         ▼
                       session worker (one blocking thread)
                         loop: Requests::recv → serve → respond
                         owns Option<Dataset>
```

- `Session::spawn` starts the worker on a blocking thread, because repository readers aren't `Send`. Synchronous reads there don't stall the MCP handler.
- The worker starts without a dataset. `load_dataset` creates one.

### Viewers

Both front ends host the session through `gv_viewer::Viewer`, which owns the `Dataset` and a `Requests` stream, and keeps the matching `SessionHandle` in `Viewer::session`. Each viewer describes itself to agents with `ViewerInfo`: its kind (`tui` or `gui`), its files, and its locus.

### Viewer: the TUI

The `App` wraps a `Viewer<ColumnScale>` and serves its requests inline, in the event loop. `main` binds a `SessionSocket` to that handle, so agents in other processes can drive the TUI; see [Connecting agents to a viewer](#connecting-agents-to-a-viewer).

```text
App
├── viewer: Viewer<ColumnScale>
│   ├── dataset: Dataset          data (see above)
│   ├── viewport                  focus, zoom, scroll: what part of `view` is on screen
│   ├── registers                 modes and the command line
│   ├── settings                  palette, initial actions, session path
│   ├── session: SessionHandle    sends requests to this viewer
│   └── requests: Requests        receives them
├── layout, resolved_layout       areas on screen
├── mouse_register                mouse input state
└── context_menu, popup           widgets
```

### Viewer: the window

The window can't wait for loads the way a terminal can: gestures send dozens of events a second, and gpui draws on the main thread. So `tgv-gui` runs its `Viewer<PixelScale>` on a worker thread with its own Tokio runtime, and the window draws snapshots.

```text
 gpui main thread                               dataset worker thread
 App                                            Viewer<PixelScale>
  keys ─────────────── WorkerMessage::Keys ────► registers → actions → handle
  gestures, clicks ──── WorkerMessage::Actions ─► handle → load_data
  layout ───────────── WorkerMessage::Resize ──► resize
  draws Arc<Frame> ◄────────── Frame ────────── a snapshot after each batch
                                                serves agents ◄── SessionSocket
```

- **A `Frame` is cheap.** It holds a clone of the view `State`, whose tables share their columns and whose alignments share their records, plus the viewport, the modes, the focus, the track labels, and the effects for the window.
- **The worker batches input.** It applies every message already queued, then loads once and sends one frame. Agent requests never pause the window.
- **The window predicts pans and zooms.** Drags, scrolls, and zooms around the pointer move a local copy of the viewport with the shared `Viewport` code, and the window draws it at once from the last frame. A sub-base pixel offset keeps drags smooth when a base is many pixels wide. The prediction is dropped once a frame covers every message sent.

### Connecting agents to a viewer

```text
 agent ──stdio──► tgv mcp (gv-mcp)
                    ├── one viewer, or the attached one: SessionConnection ──socket──► SessionSocket
                    │                                   in the TUI or the window ─► Viewer::serve
                    ├── several viewers: viewer_ambiguous, until the agent calls attach
                    └── no viewer: headless SessionHandle ─► session worker
```

- **`Call`** is the serializable form of each request: describe, load a dataset, inspect, query, navigate, view state, and viewer info. `SessionHandle::call` runs a `Call` in-process and returns its reply as JSON, so the headless path and the socket path share one dispatch.
- **`SessionSocket`** listens at `$XDG_RUNTIME_DIR/tgv/<pid>.sock`, in a directory only the user can open. Each connection sends one JSON `Call` per line and gets one JSON reply per line: `{"ok": …}`, or `{"error": {"code", "message", "field"}}`. Dropping the socket removes its file.
- **`SessionConnection::list_viewers`** connects to every viewer, newest first, asks each for its `ViewerInfo`, and removes sockets that no process listens on. `SessionConnection::connect_to` connects to one viewer by process ID. Remote errors come back as `SessionError::Remote`, keeping their codes.
- **`tgv mcp`** looks for viewers before each call until the agent loads a dataset headlessly or attaches. With one viewer, it connects. With several, it fails with `viewer_ambiguous`, whose message lists them, until the agent calls `attach`. Once connected, it keeps the connection; when the viewer closes, it reports `viewer_disconnected` and looks again on the next call.
- **Error codes** come from `SessionError::code`, so socket replies and MCP tool errors report the same codes.

## Actions and requests

The viewers use two kinds of message, with separate jobs:

| | `Action` (`gv_viewer::message`) | `Request` (`gv_session`) |
|---|---|---|
| Purpose | Changes a viewer's view or input state | Calls the session from another task or process |
| Produced by | Key and mouse handling, session-file loading, and agent view requests | `SessionHandle` methods |
| Applied by | `Viewer::handle`, the only code that changes what the screen shows | The session host: the headless worker or `Viewer::serve` |
| Reply | None; `Viewer::handle` returns the areas to redraw | A typed result through a `Responder` |

Actions are grouped into navigation, input, editing, track, session, and interaction categories. Core command messages convert to those same categories before the viewer handles them. Status messages remain a separate action.

Some actions only a front end can apply, such as a layout change or closing a menu. They are `Action::Ui`, and `Viewer::handle` leaves them as `Effect`s, with the context menus and popups it builds, for the front end to take with `Viewer::take_effects`.

An agent's view request becomes actions. `Viewer::serve_view` validates the request, translates it into actions, and applies them with `Viewer::handle`, the same path key presses take. It then replies with the result:

```text
keys, mouse ──► Registers / MouseRegister ──► Vec<Action> ─┐
                                                           ├──► Viewer::handle ──► view
Request::View ──► Viewer::serve_view (validate, translate) ┘          │
       ▲                                                              │
       └────────────── Responder::respond(ViewState or error) ◄───────┘
```

Data requests never change the screen, so `Dataset::serve` answers them without actions.

## The TUI event loop

`App::run` draws, then waits for whichever comes first: a terminal event or a session request. The window's worker loop has the same shape, with window messages in place of terminal events.

```text
                    ┌──────────────────────────────┐
                    │ draw the changed areas       │◄──────────────┐
                    └──────────────┬───────────────┘               │
                                   ▼                               │
                    ┌──────────────────────────────┐               │
                    │ tokio::select! — wait for    │               │
                    │ whichever comes first        │               │
                    └───────┬──────────────┬───────┘               │
            terminal event  │              │  session request      │
                            ▼              ▼                       │
        ┌──────────────────────────┐  ┌──────────────────────────┐ │
        │ take every event already │  │ Viewer::serve            │ │
        │ queued (zero timeout)    │  │  Data → Dataset::serve   │ │
        │ skip superseded mouse    │  │  View → Actions, then    │ │
        │ moves                    │  │         Viewer::handle   │ │
        └────────────┬─────────────┘  │  LoadDataset → refuse    │ │
                     ▼                └────────────┬─────────────┘ │
        ┌──────────────────────────┐               │               │
        │ keys and mouse → Actions │               │               │
        │ Viewer::handle(actions)  │               │               │
        │  move, zoom, scroll, ... │               │               │
        │  load_data() if the view │               │               │
        │  moved                   │               │               │
        └────────────┬─────────────┘               │               │
                     └───────────────┬─────────────┘               │
                                     ▼                             │
                         areas to redraw (RenderEvents) ───────────┘
```

**Terminal events** arrive through crossterm's async `EventStream`. After the first event, the loop takes every other event already queued, so a burst of key repeats or wheel scrolls produces one frame. That drain uses `tokio::time::timeout(Duration::ZERO, …)`, which polls the stream with the loop's own waker. Polling with a no-op waker, as `now_or_never` does, leaves crossterm unable to wake the loop after the first key.

**Session requests** run inside the loop, between batches of terminal events. Only the loop touches the `Dataset`, so nothing else mutates it. A long agent query pauses input while it runs. The window runs the same loop on its worker thread, so there it pauses only the next frame.

**View requests** from an agent, and the actions they become:

| Request | Actions | Effect |
|---|---|---|
| `navigate` | `Move(ContigNamePosition)`, `Zoom(Fit { bases })`, and a status message | Centers the region, fits it to the track area, loads it into `view`, and shows "An agent moved the view to X. Press u to go back to Y." |
| `view_state` | None; it only reads | Returns the displayed interval and zoom. |
| viewer info | None; it only reads | Returns the viewer's kind, files, locus, and start time, for `list_viewers` and `attach`. |

**Data loading for the view** goes through `Viewer::load_data`. It works out the displayed region from the viewport and the track width that the front end set. The scale decides what to load: reads, the sequence, and indexed features each have a zoom limit, `ColumnScale`'s in bases per column and `PixelScale`'s in bases per pixel. Then it calls `dataset.view.ensure_loaded` with `CachePolicy::VIEWER`, which reads files only when the region isn't already loaded, and pads each load so that nearby panning reuses the data.

## Where to change things

| To change | Edit |
|---|---|
| A new SQL table or column | gv-core schemas (`column_docs`) and `gv-session/src/tables.rs` |
| A new request | `gv-session/src/session.rs`, both hosts (`Dataset::run` and `Viewer::serve`), and a tool in `gv-mcp` if agents need it |
| How the viewers react to agents | `Viewer::serve_view`, which translates requests into actions, in `gv-viewer/src/viewer.rs` |
| What a key or a command does | `gv-viewer/src/keys.rs`, `register.rs`, and `Viewer::handle`, shared by both front ends |
| How a track is drawn | The matching file in `tgv/src/rendering/` and `tgv-gui/src/rendering/`, from the shared preparation in `gv-viewer/src/prepare/` |
| Which viewer an agent uses | `McpHandler::call` in `gv-mcp/src/lib.rs`, and `SessionConnection::list_viewers` in `gv-session/src/socket.rs` |
| What gets loaded and cached | `State::ensure_loaded` and `CachePolicy` in `gv-core/src/state.rs` |
