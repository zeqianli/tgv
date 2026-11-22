
## General conventions

- Use the type system to encode correctness constraints.
- Prefer compile-time guarantees over runtime checks where possible.
- User experienced: make progress reporting responsive and informative.
- "Not overly generic"—prefer specific, composable logic over abstract frameworks.
- Evolve the design incrementally rather than attempting perfect upfront architecture.
- Use type system extensively: newtypes, builder patterns, type states, lifetimes.
- Use message passing or the actor model to avoid data races.
- Test comprehensively, including edge cases, race conditions, and stress tests.
- Pay attention to what facilities already exist for testing, and aim to reuse them.
- Getting the details right is really important!
- **Never** use title case in headings and titles. Always use sentence case.
- Always use the Oxford comma.

## Code preference

- Use Rust 2024 edition.
- **Never** write comments in code. This includes `//` comments, doc comments (`///` and `//!`), and module-level documentation.
- **Builder patterns** for complex construction 
- **Type states** encoded in generics when state transitions matter
- Prefer keeping simple one-off logic at the call site instead of creating small helper functions that only wrap a single operation.
- Never write simple getters that only return a field, such as `reads(&self)`. Expose the field as `pub` and access it directly. Keep methods for operations that compute, select, or validate data.

## tgv
- Coordinates: Keep **all** internal genomic coordinates one-based, with inclusive interval endpoints. 
- Row IDs, operation indexes, and offsets into sequences remain zero-based indexes.
- When parallel vectors or indexed state are expected to have matching shapes, initialize them to the correct length at construction or update boundaries, then access valid indexes directly with `[index]`. Do not add lazy `ensure_*` helpers, `get(...).unwrap_or_default()`, or similar defaulting around every indexed access when the correct invariant is that the index is valid.
- Provide rich error context using structured error types.
- Use `Arc` or borrows for shared immutable data.
- Use direct typed column reductions for simple DataFrame statistics, such as sums, maxima, and visibility checks. Keep lazy expressions for row selection, transformations, joins, and grouped aggregations.
- Benchmarks checked into the repo are slow. Do not run cargo bench unleass I asked to.

## Crate layout

- `gv-core`: the data model, repositories, `State`, data loading, `TrackRegistry`, and file classification.
- `gv-session`: the session worker that owns a loaded dataset and serves typed commands through `SessionHandle`, plus the SQL tables and the local socket (`SessionSocket` and `SessionConnection`) that lets agents drive a viewer. 
- `gv-mcp`: MCP tools
- `gv-viewer`: abstraction of key components of a genome viewer: main state, message passing, keyboard and mouse actions, colors, etc.
- `tgv`: the TUI genome viewer. Also controls launches the mcp through `tgv mcp`
- `tgv-gui`: the GUI genome viewer.

## Documentation

- `doc/src` is the user-facing mdBook. `doc/src/SUMMARY.md` defines the chapter tree; every page must be listed there.
- `doc/dev` is developer-only: the architecture, design notes, feedback, and the MCP tool reference. It is not part of the mdBook, and pages in `doc/src` must not link into it.

## Testing practices

Always use `cargo nextest run` to run unit and integration tests. For doctests, use `cargo test --doc` (doctests are not supported by nextest).
- Do not add new tests unless the user explicitly asks for them. If tests seem especially beneficial, ask for confirmation before adding them.
- Unit tests in the same file as the code they test.
- Use `#[rstest]` and `#[case(...)]` parameterized tests when possible, especially for related scenarios that share the same setup and assertions.
- Do not add unit tests for rendering code unless explicitly asked.

### Workspace dependencies

- All versions managed in root `Cargo.toml` `[workspace.dependencies]`.
- Comment on dependency choices when non-obvious; example: "Disable punycode parsing since we only access well-known domains".

### Key dependencies
- **noodles**: Bioinformatics library.
- **tokio**: Async runtime, essential for concurrency model.
- **thiserror**: Error derive macros.
- **serde**: Serialization (config, metadata).
- **clap**: CLI parsing with derives.
