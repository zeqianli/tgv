# Design: a GUI viewer

Status: proposed, not implemented.

## Context

The session architecture is in place (see [Architecture](../architecture.md)):

- `gv-core` holds the data model and loading.
- `gv-session` owns a `Dataset` with separate view and query states, serves typed requests, and listens on a local socket.
- The TUI hosts a session in its event loop, and `tgv mcp` drives it over the socket.

The remaining step is a second viewer: a GUI.

## Plan

- **Crate.** `gv-gui` depends on `gv-session` and `gv-core`, never on `tgv`, and hosts a session the same way the TUI does:
  - It owns a `Dataset` and a `Requests` stream.
  - It serves `Request::View` by translating requests into its own view changes.
  - It binds a `SessionSocket`, so `tgv mcp` can reach it.
- **Rendering.** The GUI draws from `dataset.view`, as the TUI does, and loads with `CachePolicy::VIEWER`.
- **Agent notices.** Agent moves and highlights show a notice with a way back, as in the TUI.

## Open questions

- Should `highlight` take only intervals, or also read IDs from a query result, so the viewer can mark the exact reads behind a count?
- Should several viewers share one session, such as a TUI and a GUI on the same dataset, or does each viewer host its own?
- Does the agent need to see what the user is looking at, through `view_state`, without the user asking? That raises privacy questions in clinical settings.
