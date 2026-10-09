# Design: safer viewer connections for agents

Status: to do.

## Context

`tgv mcp` connects to a running TGV window and sends agent requests to it (see [Show results in the viewer](../server/view.md)). In the common case, a window is open before the agent's first call, so the agent and the user share one dataset. Some orders of events still let the agent work on data the user isn't looking at:

1. **The agent loads a dataset before any window is open.** From then on, `tgv mcp` stays headless on purpose, so the agent keeps the dataset it loaded. If the user then opens TGV, `navigate` and `highlight` fail with `no_viewer`. Nothing reaches the wrong window, but the message ("No viewer displays this session; …") is misleading when a window is open: the real cause is that the server holds its own dataset.
2. **The window closes and a different one opens.** The next call fails with `viewer_disconnected`, and later calls go to the headless session. If the agent never loaded a dataset headlessly, the following call quietly connects to whichever window is open now, even if it shows other files. The agent's track IDs, contigs, and coordinates may then point at different data. **Not detected.**
3. **Several windows are open.** `tgv mcp` connects to the most recently started window and stays with it. If the user looks at another window, the agent's moves and highlights land where the user isn't looking. **Not detected.**

## Changes

1. **Report which viewer answered.** Add the viewer's identity, its process ID and socket path, to `get_dataset` along with its files, and to the `navigate`, `highlight`, and `view_state` replies. The agent can then tell when the viewer changes.
2. **Make a viewer switch explicit.** When `tgv mcp` connects to a viewer other than the one it was last connected to, the first call fails with a new `viewer_changed` error instead of switching silently. The error names the new viewer and tells the agent to call `get_dataset` before reusing track IDs. Connecting for the first time stays silent.
3. **Explain why viewer tools fail when a window is open.** When a viewer is running but the server keeps its headless dataset, fail viewer tools with a message that says so, for example "A TGV window is open, but this server uses the dataset it loaded; …".
4. **Let the user see and choose.** The TUI shows "An agent connected" when a connection opens. Optionally, `list_viewers` and `attach` tools let the agent pick the window the user means instead of the newest.

## Where the changes go

- **Routing:** `McpHandler::call` in `crates/gv-mcp/src/lib.rs` tracks the last viewer, detects switches, and chooses the `no_viewer` message.
- **Errors:** add `SessionError::ViewerChanged` and its code in `crates/gv-session/src/error.rs`.
- **Viewer identity:** `SessionSocket` and `SessionConnection` in `crates/gv-session/src/socket.rs`, and the reply types in `crates/gv-session/src/schema/`.
- **Notice:** `App::serve` in `crates/tgv/src/app.rs`, when a socket connection opens.
- **Docs:** the error tables in `doc/src/server.md` and `doc/src/server/view.md`.

## Verification

- Close a window and open another on different files; the agent's next call fails with `viewer_changed`.
- Load a dataset headlessly, then open a window; `navigate` fails with the new message.
- With two windows open, `get_dataset` reports which one the agent uses.
