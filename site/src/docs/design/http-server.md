# Local HTTP inspection

The prototype exposes one dataset through five versioned HTTP endpoints. `Server` directly owns core state, repositories, settings, and the shared track registry. It derives dataset descriptions from the registry and repository instead of storing another file mapping. The worker holds an optional `Server` before the first load. It reuses file classification, the existing per-track loaders, coverage, layout, and renderer. There are no server-specific bioinformatics readers or counting algorithms.

Dataset replacement constructs fresh core state, repositories, and a track registry before swapping them into the worker. A failed replacement leaves the current dataset intact. Track IDs are numeric file positions, shared with the TUI representation, and reset on replacement. Requests always address the current dataset; there is no revision check. Inspections use explicit inclusive coordinates instead of a shared cursor. One worker owns the dataset; HTTP tasks send commands and receive replies. The worker does not require repository types to be `Send`.

`POST /v1/inspect` takes an explicit, inclusive interval and returns only structured statistics. Every inspection reloads its regional alignment data so cached reads from an earlier, wider viewport cannot affect current coverage. The existing symmetric region representation may load an extra base, but reported positions and interval summaries are restricted to the requested interval.

`POST /v1/draw` takes a named contig and a 1-based center position, together with a zoom, a genomic half-width, a canvas size, and a text or ANSI format. The half-width defines the core region requested for loading. The actual displayed interval is derived from canvas width and zoom, may shift at contig boundaries, and is returned with the drawing. Drawing does not calculate the inspection statistics.

An optional `tracks` array selects numeric IDs for inspection or drawing. The server validates distinct, current IDs and reports selected tracks in dataset file order. Drawing a subset preserves each track's dataset ID rather than renumbering it within the layout.

The API labels coverage `viewer_current` and preserves the existing implementation, including its limitations. Auditing coverage, analytical counting options, dynamic track removal, navigation, and multiple sessions are deferred. HTTP request types are separate from session serialization, so the persisted session format is unchanged.

Verification uses live HTTP requests and existing fixtures. No server tests are added while the API is being explored.
