# Design: a shared session for viewers and agents

Status: phase 1 implemented; phases 2–5 proposed.

## Context

TGV has two front ends today: the TUI, and the MCP server behind `tgv serve`. A GUI is planned. The goal is for an agent to work through MCP against the same data a person is looking at. For example, the agent queries reads at a site, then moves the TUI or GUI there and highlights the evidence. That shared view is what TGV offers beyond samtools and similar tools: the human can check the agent's claim in the viewer.

The current layout doesn't support this:

- **No sharing.** `tgv serve` runs a separate process with its own `DatasetState`, so it can't see or move a running TUI.
- **The TUI owns its data directly.** `App` owns `State`, `Repository`, and `TrackRegistry`, and loads data inside its own event loop. Nothing else can send it commands.
- **The server depends on the TUI crate.** `crates/tgv/src/server/` imports TUI layout and rendering code for `draw_viewport`, plus `TrackRegistry`, `classify_and_build_tracks`, and `Settings` from `crates/tgv`.
- **gv-core knows about MCP.** It holds MCP-specific error variants (`McpInvalidInput`, `McpNoDataset`, and `McpInternal` in `TGVError`).
- **Every build pays for MCP.** The Polars SQL and join features are set in the root `Cargo.toml`, so every crate compiles them.

`draw_viewport` is removed as part of this design. A text drawing returned to the agent isn't a view for the human. Its replacement is navigation: the agent moves a real viewer.

## Architecture

The process that renders also owns the data. A viewer process hosts the session. When an agent connects through MCP, `tgv mcp` attaches to that session over a local socket. If no viewer is running, `tgv mcp` hosts a headless session itself, which is what `tgv serve` does today.

```text
            ┌──────────────── viewer process (TUI or GUI) ────────────────┐
 user ────► │  UI ──commands──► Session actor                              │
            │   ▲               (State, Repository, view state, SQL tables)│
            │   └───view events──┘          ▲                              │
            │                                │ socket listener             │
            └────────────────────────────────┼─────────────────────────────┘
                                             │ protocol messages
 agent ──stdio──► `tgv mcp` ─────────────────┘
                     └─ no viewer running: host a headless session in-process
```

This shape follows from three facts:

- **Viewers need the data locally.** They redraw constantly from reads, coverage, and the reference. Sending read tables across a process boundary for every frame would be wasteful.
- **The agent queries what the user sees.** Agent queries run on the same `State` as the viewer, so the human can check the agent's answers in the viewer.
- **MCP clients launch their own servers.** A client starts each server as a stdio subprocess, so it can't connect to a running viewer directly. `tgv mcp` bridges the gap: it speaks MCP on stdio and the session protocol on the socket.

## Crates

| Crate | Owns | Depends on |
|---|---|---|
| `gv-core` | The data model, repositories, `State`, `State::ensure_loaded`, `TrackRegistry`, and file classification. | — |
| `gv-session` | The session actor, which grows out of today's `DatasetState` loop. It accepts typed commands, holds the view state, sends view events to subscribers, and builds the SQL tables (today's `tables.rs` and query execution). Later it also runs the socket listener. | `gv-core` |
| `gv-protocol` | Serializable command, reply, and event types, plus the socket client. Added in phase 4. | `serde` |
| `gv-mcp` | The MCP tools, which translate MCP calls into session commands. It has two backends: an in-process session handle (headless) and a socket client (attached). | `gv-session`, later `gv-protocol` |
| `tgv` | The TUI. It hosts a `gv-session`, renders the view state, and applies view events. Its binary also provides `tgv mcp`. | `gv-session`, `gv-mcp` |
| `gv-gui` (later) | The GUI, with the same role as the TUI. | `gv-session` |

These rules hold:

- **Dependencies point away from front ends.** No crate depends on `tgv` or `gv-gui`. The TUI and GUI are symmetric.
- **MCP lives in its own crate.** MCP-specific errors move into `gv-mcp`'s own `thiserror` enum, which wraps `TGVError`. gv-core loses its `Mcp*` variants.
- **Each crate declares the Polars features it needs.** The `sql`, `semi_anti_join`, `iejoin`, `cross_join`, and `dtype-u128` features move from the root `Cargo.toml` to `gv-session`. `cargo build -p tgv` without MCP then compiles no `polars-sql`.
- **The GUI can reuse query support.** A GUI query panel can use `gv-session` directly.

## Session commands and events

The session actor handles one command at a time, as `DatasetState::run` does today, and replies on a `oneshot` channel. The UI and the agent send the same commands, so there is no shared mutable state between them.

Commands, with the same names and payloads in-process and on the socket:

| Command | Effect |
|---|---|
| `Describe` | Returns the reference and tracks. |
| `LoadDataset` | Replaces the dataset, as `load_dataset` does today. |
| `DescribeTables` | Returns the SQL table catalog. |
| `Query { region, sql, limit }` | Runs SQL over the session's tables. |
| `Inspect { region, tracks }` | Returns the summary overview. |
| `Navigate { region }` | Moves the view. |
| `Highlight { intervals, label }` | Marks intervals in the view, for example the reads or sites behind a claim. |
| `ClearHighlights` | Removes highlights. |
| `ViewState` | Returns the current view: contig, bounds, zoom, and visible tracks. |

Events that the session broadcasts to viewers:

| Event | Viewer response |
|---|---|
| `ViewChanged { region, source }` | Redraw at the region. When `source` is the agent, show a notice such as "Agent moved to chr20:88,108" that the user can dismiss or undo. |
| `HighlightsChanged` | Redraw highlights. |
| `DatasetReplaced` | Rebuild the layout for the new tracks. |

An agent never silently takes over the screen. Every agent-driven view change is labeled, and the user's own navigation always wins. If the user moves while an agent command is queued, the viewer shows the agent's request as a notice instead of jumping.

## Problems to solve first

### The agent and the user share one cache

Each track holds a single loaded region. An agent query at chr7 would evict the chr20 region the user is viewing, and the viewer would then reload. There are two options:

1. **A cache that holds several regions.** It would hold a few complete regions per track, evicting the least recently used one first, and `ensure_loaded` would check all of them. This keeps one `State`. It needs care with stacking and display state, which today assume one region.
2. **A separate query state.** The session would hold a second `State` for agent queries, sharing the same `Repository`, and with it the open file handles and remote readers. The viewer state is never touched by queries. Queries and the view can then disagree about the data, but only if the file changes on disk.

Option 2 is recommended as the first step. It is smaller, and it keeps the viewer's state isolated. `Navigate` still moves the viewer's state, so the user sees the same data the agent describes after the viewer loads it.

### The TUI stops owning `State`

`App` currently owns `State`, `Repository`, and `TrackRegistry`, and calls `ensure_loaded` from `App::load_data`. Under this design, the UI sends commands to the session actor and renders from state the actor publishes. Rendering needs fast read access to the alignment tables, so the actor publishes shared, read-only snapshots of the loaded data, for example `Arc<LoadedRegion>`, and does not copy them. This is the largest refactor in the plan.

### Finding and securing the session socket

- **Socket path.** Each viewer listens on `$XDG_RUNTIME_DIR/tgv/<pid>.sock`. Where that variable is unset, it uses a per-user temporary directory. Only the user can read or write it.
- **Attaching.** `tgv mcp` attaches to the only running session. If several are running, a `list_sessions` tool reports them and an `attach` tool selects one. With none running, it runs headless.
- **Windows.** Named pipes are needed there; deferred until a Windows build is needed.

## Phases

Each phase leaves the TUI and the MCP server working.

1. **Separate the MCP crate.**
   - Remove `draw_viewport`, its schema, `doc/src/server/draw.md`, and its `SUMMARY.md` entry.
   - Move `TrackRegistry` and `classify_and_build_tracks` into `gv-core`.
   - Create `gv-session` from `DatasetState`, `tables.rs`, and the query schema.
   - Create `gv-mcp` with the tool definitions and a headless backend.
   - Keep `tgv serve`, and add `tgv mcp` as its new name.
   - Move the `Mcp*` errors and the Polars SQL features out of gv-core and the root manifest.
2. **Add the query state.** `gv-session` holds a separate query `State` that shares the `Repository`.
3. **Have the TUI host a session.**
   - `App` sends commands to an in-process `gv-session` and renders published snapshots.
   - Add `Navigate`, `Highlight`, and `ViewState`, the view events, and the agent notice in the TUI.
4. **Add the socket transport.**
   - Add `gv-protocol` with the command, reply, and event types and the socket client.
   - `gv-session` listens on the socket.
   - `tgv mcp` attaches when a viewer is running. It adds the MCP tools `navigate`, `highlight`, and `view_state`, plus `list_sessions` and `attach` when there are several sessions.
5. **Build the GUI.** `gv-gui` hosts a `gv-session` the same way the TUI does.

## Verification

- **Phase 1:**
  - The TUI code imports nothing from `gv-mcp` or `gv-session`. The `tgv` package still depends on `gv-mcp` for the `tgv mcp` subcommand, so `cargo tree -p tgv -i polars-sql` shows `polars-sql` through `gv-mcp`. `gv-core` builds without it.
  - The existing MCP queries in `doc/src/server/` return the same results when run through `tgv mcp`.
- **Phase 2:** An agent query on a different contig doesn't reload the region the viewer has loaded. A test checks the viewer state's loaded bounds before and after the query.
- **Phase 3:** Navigation by keyboard and by `Navigate` command produces identical view state. The offline TUI tests pass unchanged.
- **Phase 4:**
  - With a TUI open on the HG002 example dataset, an agent connected through `tgv mcp` runs the allele-count query at `chr20:88108`, calls `navigate` and `highlight`, and the TUI shows the site, the highlight, and the agent notice.
  - Without a TUI, the same calls fall back to a headless session.

## Open questions

- Should `Highlight` take only intervals, or also read IDs from a query result, so the viewer can mark the exact reads behind a count?
- Should multiple viewers be able to share one session, such as a TUI and a GUI on the same dataset, or does each viewer host its own?
- Does the agent need to see what the user is looking at, through `ViewState`, without the user asking? That raises privacy questions in clinical settings.
