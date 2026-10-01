# TUI track IDs

The TUI assigns one `TrackId` to each loaded file, in file order. A BAM or CRAM file uses the same ID for its coverage and alignment panels. The ID is a `usize` alias because it only identifies a track within the current TUI session; it is not persisted.

The TUI track registry owns the mapping from these IDs to the existing kind-specific repository indexes. Layout areas, resize messages, and mouse interactions carry track IDs. At the boundary where the TUI reads core state or sends a core message, it resolves the ID to a repository index. Sidebar filenames come directly from the file paths held by the repository; the registry does not copy them. The repository and its parallel state vectors remain indexed as before.

The registry is built once with the layout and shared with the app and resolved layout. Its entries follow the loaded file order, while `MainLayout.tracks` records the panel display order. This first step does not support dynamic track removal or reordering; either operation would need an explicit policy for maintaining IDs and indexed state.

The MCP server uses the same registry and numeric IDs. A selected drawing retains each ID from the current dataset, and a dataset replacement creates a fresh registry with IDs starting at zero.
