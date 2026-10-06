
## General conventions

- Use the type system to encode correctness constraints.
- Prefer compile-time guarantees over runtime checks where possible.
- User experienced: make progress reporting responsive and informative.
- Write user-facing messages in clear, present tense: "Nextest now supports..." not "Nextest now supported..."
- "Not overly generic"—prefer specific, composable logic over abstract frameworks.
- Evolve the design incrementally rather than attempting perfect upfront architecture.
- Use type system extensively: newtypes, builder patterns, type states, lifetimes.
- Use message passing or the actor model to avoid data races.
- Test comprehensively, including edge cases, race conditions, and stress tests.
- Pay attention to what facilities already exist for testing, and aim to reuse them.
- Getting the details right is really important!
- Use inline comments to explain "why," not just "what".
- Don't add narrative comments in function bodies. Only add a comment if what you're doing is non-obvious or special in some way, or if something needs a deeper "why" explanation.
- Module-level documentation should explain purpose and responsibilities.
- **Always** use periods at the end of code comments.
- **Never** use title case in headings and titles. Always use sentence case.
- Always use the Oxford comma.
- Don't omit articles ("a", "an", "the"). Write "the file has a newer version" not "file has newer version".

## Code preference

- Use Rust 2024 edition.
- **Builder patterns** for complex construction (e.g., `TestRunnerBuilder`)
- **Type states** encoded in generics when state transitions matter
- **Non-exhaustive in stable crates**: The `nextest-metadata` crate has a stable API and public types there should be `#[non_exhaustive]` for forward compatibility. Internal crates like `nextest-runner` do not have stable APIs, so `#[non_exhaustive]` is not required (though error types may still use it).
- Prefer keeping simple one-off logic at the call site instead of creating small helper functions that only wrap a single operation.
- Do not write simple getters that only return a field, such as `reads(&self)`. Expose the field as `pub` and access it directly. Keep methods for operations that compute, select, or validate data.
- DataFrame schemas: Define column names as associated constants on the owning schema type, and use those constants in schema declarations, builders, queries, rendering, and tests. Do not use string literals for DataFrame column references, including temporary query columns.
- Coordinates:Keep all internal genomic coordinates one-based, with inclusive interval endpoints. This includes alignment tables, annotations, viewport queries, and rendering.
- Row IDs, operation indexes, and offsets into sequences remain zero-based indexes.
- When parallel vectors or indexed state are expected to have matching shapes, initialize them to the correct length at construction or update boundaries, then access valid indexes directly with `[index]`.
- Do not add lazy `ensure_*` helpers, `get(...).unwrap_or_default()`, or similar defaulting around every indexed access when the correct invariant is that the index is valid.
- Use explicit optional lookups only when a missing value is a real domain case, not as a substitute for maintaining indexed state invariants.
- Use `thiserror` for error types with `#[derive(Error)]`.
- Provide rich error context using structured error types.
- Use `Arc` or borrows for shared immutable data.
- Use direct typed column reductions for simple DataFrame statistics, such as sums, maxima, and visibility checks. Keep lazy expressions for row selection, transformations, joins, and grouped aggregations.
- Benchmarks checked into the repo are slow. Do not run cargo bench unleass I asked to.

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
