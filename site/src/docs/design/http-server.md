# Local HTTP inspection

The prototype exposes one dataset through four versioned HTTP endpoints. `Server` directly owns core state, repositories, settings, the file mapping, and the current dataset description. The worker holds an optional `Server` before the first load. It reuses file classification, the existing per-track loaders, coverage, layout, and renderer. There are no server-specific bioinformatics readers or counting algorithms.

Dataset replacement constructs fresh core state and repositories before swapping them into the worker. Track IDs are revision-scoped. Inspections validate their revision when executed, and use explicit inclusive coordinates instead of a shared cursor. One worker owns the dataset; HTTP tasks send commands and receive replies. The worker does not require repository types to be `Send`.

Structured results are collected before display padding is loaded. Every inspection reloads its regional alignment data so cached reads from an earlier, wider viewport cannot affect current coverage. The existing symmetric region representation may load an extra base, but reported positions and interval summaries are restricted to the requested interval.

The API labels coverage `viewer_current` and preserves the existing implementation, including its limitations. Auditing coverage, analytical counting options, dynamic track removal, navigation, and multiple sessions are deferred. HTTP request types are separate from session serialization, so the persisted session format is unchanged.

Verification uses live HTTP requests and existing fixtures. No server tests are added while the API is being explored.
