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
├── settings     reference, files, backend, cache directory
├── repository   open readers, shared by both states
├── tracks       TrackRegistry: track IDs → repository indexes
├── view: State  what a viewer draws
│   ├── alignments, sequence, genes     ← the region on screen, padded (CachePolicy::VIEWER)
│   └── variants, BED intervals ─┐
└── query: State what agent requests load
    ├── alignments, sequence, genes     ← the requested region exactly (CachePolicy::EXACT)
    └── variants, BED intervals ─┤
                                 └──► Arc<VariantTable>, Arc<BedTable>: each file is loaded once
```

- **The two states load regions separately.** An agent query on another contig loads into `query` and never replaces what `view` shows.
- **Whole-file variant and BED tables are shared.** Each state points to the same `Arc` table through `State::share_whole_file_tracks`, so a large VCF isn't held twice.
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

The `App` itself is the host. It owns the `Dataset` and a `Requests` stream, and it keeps the matching `SessionHandle` in `App::session` for agents and tests. Today nothing outside the process holds that handle. Connecting a local socket to it lets an agent drive the TUI.

```text
App
├── dataset: Dataset          data (see above)
├── alignment_view            focus, zoom, scroll: what part of `view` is on screen
├── highlights                intervals an agent marked
├── layout, resolved_layout   areas on screen
├── registers, mouse_register key and mouse input state
├── settings                  palette, initial messages, session path
├── session: SessionHandle    sends requests to this App
└── requests: Requests        receives them
```

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
        │ skip superseded mouse    │  │  View → navigate,        │ │
        │ moves                    │  │         highlight, ...   │ │
        └────────────┬─────────────┘  │  LoadDataset → refuse    │ │
                     ▼                └────────────┬─────────────┘ │
        ┌──────────────────────────┐               │               │
        │ keys and mouse → Messages│               │               │
        │ App::handle(messages)    │               │               │
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

**View requests** from an agent:

| Request | Effect |
|---|---|
| `navigate` | Centers the region, picks a zoom that fits the track area, loads it into `view`, and shows "An agent moved the view to X. Type :Y to go back." |
| `highlight` | Replaces `App::highlights`, tints their columns in the coordinate ruler, and reports the count and label in the message line. |
| `clear_highlights` | Removes the highlights. |
| `view_state` | Returns the displayed interval and zoom. |

**Data loading for the view** goes through `App::load_data`. It works out the displayed region from `alignment_view` and the track area. Then it calls `dataset.view.ensure_loaded` with `CachePolicy::VIEWER`, which reads files only when the region isn't already loaded, and pads each load so that nearby panning reuses the data.

## Where to change things

| To change | Edit |
|---|---|
| A new SQL table or column | gv-core schemas (`column_docs`) and `gv-session/src/tables.rs` |
| A new request | `gv-session/src/session.rs`, both hosts (`Dataset::run` and `App::serve`), and a tool in `gv-mcp` if agents need it |
| How the viewer reacts to agents | `App::serve_view` in `tgv/src/app.rs` |
| What gets loaded and cached | `State::ensure_loaded` and `CachePolicy` in `gv-core/src/state.rs` |
