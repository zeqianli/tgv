# Architecture

TGV is a Cargo workspace with four crates. This page explains how the crates depend on each other, which crate owns which data, how requests reach that data, and how the TUI's event loop combines keyboard input with agent requests.

## Crates

```text
                ┌────────────────────────────────────┐
                │ tgv                                │
                │ TUI, `tgv` binary, `tgv mcp`       │
                └──────┬──────────────┬──────────────┘
                       │              │
                       │              ▼
                       │   ┌──────────────────────────┐
                       │   │ gv-mcp                   │
                       │   │ MCP tools over stdio     │
                       │   └────────────┬─────────────┘
                       ▼                ▼
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

Arrows point from a crate to the crates it depends on. `tgv` also depends on `gv-core` directly, for its data types.

| Crate | Owns | Doesn't know about |
|---|---|---|
| `gv-core` | `State` (the loaded region of each track), `Repository` (open BAM, VCF, BED, reference, and annotation readers), `State::ensure_loaded` and `CachePolicy`, `TrackRegistry`, table schemas, and file classification | Sessions, MCP, and terminals |
| `gv-session` | `Dataset`, the request types (`Request`, `SessionHandle`, `Requests`, `Responder`), request and response types such as `QueryRequest` and `ViewState`, `SessionError`, and the SQL tables built on Polars SQL | MCP and terminals |
| `gv-mcp` | The MCP tools, the mapping from `SessionError` to MCP error codes, `McpError`, and `serve` | Terminals |
| `tgv` | `App`, layout, key and mouse handling, rendering, session files, the CLI, and the `tgv mcp` subcommand | — |

No crate depends on `tgv`, so another front end, such as a GUI, can use `gv-session` the same way the TUI does.

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
│  .highlight()    │ ◄────── oneshot ─────── │ Responder::respond(result)   │
│  .view_state()   │   Result<T, SessionError>                              │
└──────────────────┘                         └──────────────────────────────┘
```

| Request | Headless worker | TUI |
|---|---|---|
| `Data`: describe, inspect, or query | `Dataset::serve`, or a `no_dataset` error before the first load | `Dataset::serve` on the query state |
| `LoadDataset` | Replaces the dataset | Fails with `DatasetFixed` |
| `View`: navigate, highlight, clear highlights, or view state | Fails with `NoViewer` | Moves or marks the view |
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

### Viewer: the TUI

The `App` itself is the host. It owns the `Dataset` and a `Requests` stream, and it keeps the matching `SessionHandle` in `App::session`. `main` binds a `SessionSocket` to that handle, so agents in other processes can drive the TUI; see [Connecting agents to a viewer](#connecting-agents-to-a-viewer).

```text
App
├── dataset: Dataset          data (see above)
├── alignment_view            focus, zoom, scroll: what part of `view` is on screen
├── highlights                intervals an agent marked
├── layout, resolved_layout   areas on screen
├── registers, mouse_register key and mouse input state
├── settings                  palette, initial actions, session path
├── session: SessionHandle    sends requests to this App
└── requests: Requests        receives them
```

### Connecting agents to a viewer

```text
 agent ──stdio──► tgv mcp (gv-mcp)
                    ├── viewer found: SessionConnection ──socket──► SessionSocket in the TUI
                    │                                                 └─► App::session ─► App::serve
                    └── no viewer: headless SessionHandle ─► session worker
```

- **`Call`** is the serializable form of each request: describe, load a dataset, inspect, query, navigate, highlight, clear highlights, and view state. `SessionHandle::call` runs a `Call` in-process and returns its reply as JSON, so the headless path and the socket path share one dispatch.
- **`SessionSocket`** listens at `$XDG_RUNTIME_DIR/tgv/<pid>.sock`, in a directory only the user can open. Each connection sends one JSON `Call` per line and gets one JSON reply per line: `{"ok": …}`, or `{"error": {"code", "message", "field"}}`. Dropping the socket removes its file.
- **`SessionConnection`** connects to the most recently started viewer and removes sockets that no process listens on. Remote errors come back as `SessionError::Remote`, keeping their codes.
- **`tgv mcp`** looks for a viewer before each call until the agent loads a dataset headlessly. Once connected, it keeps the connection; when the viewer closes, it reports `viewer_disconnected` and returns to headless.
- **Error codes** come from `SessionError::code`, so socket replies and MCP tool errors report the same codes.

## Actions and requests

The TUI uses two kinds of message, with separate jobs:

| | `Action` (`tgv::message`) | `Request` (`gv_session`) |
|---|---|---|
| Purpose | Changes the TUI's view or input state | Calls the session from another task or process |
| Produced by | Key and mouse handling, session-file loading, and agent view requests | `SessionHandle` methods |
| Applied by | `App::handle`, the only code that changes what the screen shows | The session host: the headless worker or `App::serve` |
| Reply | None; `App::handle` returns the areas to redraw | A typed result through a `Responder` |

An agent's view request becomes actions. `App::serve_view` validates the request, translates it into actions, and applies them with `App::handle`, the same path key presses take. It then replies with the result:

```text
keys, mouse ──► Registers / MouseRegister ──► Vec<Action> ─┐
                                                           ├──► App::handle ──► view, highlights
Request::View ──► App::serve_view (validate, translate) ───┘          │
       ▲                                                              │
       └────────────── Responder::respond(ViewState or error) ◄───────┘
```

Data requests never change the screen, so `Dataset::serve` answers them without actions.

## The TUI event loop

`App::run` draws, then waits for whichever comes first: a terminal event or a session request.

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
        │ take every event already │  │ App::serve               │ │
        │ queued (zero timeout)    │  │  Data → Dataset::serve   │ │
        │ skip superseded mouse    │  │  View → Actions, then    │ │
        │ moves                    │  │         App::handle      │ │
        └────────────┬─────────────┘  │  LoadDataset → refuse    │ │
                     ▼                └────────────┬─────────────┘ │
        ┌──────────────────────────┐               │               │
        │ keys and mouse → Actions │               │               │
        │ App::handle(actions)     │               │               │
        │  move, zoom, scroll, ... │               │               │
        │  load_data() if the view │               │               │
        │  moved                   │               │               │
        └────────────┬─────────────┘               │               │
                     └───────────────┬─────────────┘               │
                                     ▼                             │
                         areas to redraw (RenderEvents) ───────────┘
```

**Terminal events** arrive through crossterm's async `EventStream`. After the first event, the loop takes every other event already queued, so a burst of key repeats or wheel scrolls produces one frame. That drain uses `tokio::time::timeout(Duration::ZERO, …)`, which polls the stream with the loop's own waker. Polling with a no-op waker, as `now_or_never` does, leaves crossterm unable to wake the loop after the first key.

**Session requests** run inside the loop, between batches of terminal events. Only the loop touches the `Dataset`, so nothing else mutates it. A long agent query pauses input while it runs.

**View requests** from an agent, and the actions they become:

| Request | Actions | Effect |
|---|---|---|
| `navigate` | `Move(ContigNamePosition)`, `Zoom(Fit { bases })`, and a status message | Centers the region, fits it to the track area, loads it into `view`, and shows "An agent moved the view to X. Press u to go back to Y." |
| `highlight` | `SetHighlights` and a status message | Tints the intervals' columns in the coordinate ruler, and reports the count and label in the message line. |
| `clear_highlights` | `ClearHighlights` | Removes the highlights. |
| `view_state` | None; it only reads | Returns the displayed interval and zoom. |

**Data loading for the view** goes through `App::load_data`. It works out the displayed region from `alignment_view` and the track area. Then it calls `dataset.view.ensure_loaded` with `CachePolicy::VIEWER`, which reads files only when the region isn't already loaded, and pads each load so that nearby panning reuses the data.

## Where to change things

| To change | Edit |
|---|---|
| A new SQL table or column | gv-core schemas (`column_docs`) and `gv-session/src/tables.rs` |
| A new request | `gv-session/src/session.rs`, both hosts (`Dataset::run` and `App::serve`), and a tool in `gv-mcp` if agents need it |
| How the viewer reacts to agents | `App::serve_view`, which translates requests into actions, in `tgv/src/app.rs` |
| What gets loaded and cached | `State::ensure_loaded` and `CachePolicy` in `gv-core/src/state.rs` |
