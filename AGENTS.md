
## General conventions

### Correctness over convenience

- Model the full error space—no shortcuts or simplified error handling.
- Handle all edge cases, including race conditions, signal timing, and platform differences.
- Use the type system to encode correctness constraints.
- Prefer compile-time guarantees over runtime checks where possible.

### User experience as a primary driver

- Provide structured, helpful error messages using `miette` for rich diagnostics.
- Make progress reporting responsive and informative.
- Write user-facing messages in clear, present tense: "Nextest now supports..." not "Nextest now supported..."

### Pragmatic incrementalism

- "Not overly generic"—prefer specific, composable logic over abstract frameworks.
- Evolve the design incrementally rather than attempting perfect upfront architecture.
- Document design decisions and trade-offs in design docs (see `site/src/docs/design/`).
- When uncertain, explore and iterate; tgv is an ongoing exploration of what a genome viewer should do.

### Production-grade engineering

- Use type system extensively: newtypes, builder patterns, type states, lifetimes.
- Use message passing or the actor model to avoid data races.
- Test comprehensively, including edge cases, race conditions, and stress tests.
- Pay attention to what facilities already exist for testing, and aim to reuse them.
- Getting the details right is really important!

### Documentation

- Use inline comments to explain "why," not just "what".
- Don't add narrative comments in function bodies. Only add a comment if what you're doing is non-obvious or special in some way, or if something needs a deeper "why" explanation.
- Module-level documentation should explain purpose and responsibilities.
- **Always** use periods at the end of code comments.
- **Never** use title case in headings and titles. Always use sentence case.
- Always use the Oxford comma.
- Don't omit articles ("a", "an", "the"). Write "the file has a newer version" not "file has newer version".

## Code style


### Rust edition and formatting

- Use Rust 2024 edition.

### Type system patterns

- **Builder patterns** for complex construction (e.g., `TestRunnerBuilder`)
- **Type states** encoded in generics when state transitions matter
- **Non-exhaustive in stable crates**: The `nextest-metadata` crate has a stable API and public types there should be `#[non_exhaustive]` for forward compatibility. Internal crates like `nextest-runner` do not have stable APIs, so `#[non_exhaustive]` is not required (though error types may still use it).

### Error handling

- Use `thiserror` for error types with `#[derive(Error)]`.
- Provide rich error context using structured error types.
  - Parts of the code use `miette` for structured error handling.


### Serde patterns

- Use `serde_ignored` for ignored paths in configuration.
- Never use `#[serde(flatten)]`. Instead, copy fields to structs as necessary. The internal buffering leads to poor warnings from `serde_ignored`.
- Never use `#[serde(untagged)]` for deserializers, since it produces poor error messages. Instead, write custom visitors with an appropriate `expecting` method.

### Serialization format changes

When modifying any struct that is serialized to disk or over the wire:

1. **Trace the full version matrix**:
   - Old reader + new data: Can it deserialize? Does it lose information?
   - New reader + old data: Does `#[serde(default)]` produce correct values?
   - Old writer + new data: Can it round-trip without data loss? (This is the easy one to miss!)

2. **Bump format versions proactively**: If adding a field that will be semantically important, bump the version when adding the field, not when first using non-default values. This prevents older versions from silently corrupting data on write-back.

3. **`#[serde(default)]` is necessary but not sufficient**: It allows old readers to deserialize new data, but old writers will still drop unknown fields on write-back.


### Memory and performance

- Use `Arc` or borrows for shared immutable data.


## Testing practices

### Running tests

Always use `cargo nextest run` to run unit and integration tests. 


For doctests, use `cargo test --doc` (doctests are not supported by nextest).

### Test organization

- Unit tests in the same file as the code they test.
- Use `#[rstest]` and `#[case(...)]` paratermized tests when possible.

## Dependencies

### Look up APIs for dependencies

- For noodles API, look up `target/doc/noodles` first. 

### Workspace dependencies

- All versions managed in root `Cargo.toml` `[workspace.dependencies]`.
- Internal crates use exact version pinning: `version = "=0.17.0"`.
- Comment on dependency choices when non-obvious; example: "Disable punycode parsing since we only access well-known domains".

### Key dependencies
- **noodles**: Bioinformatics library.
- **tokio**: Async runtime, essential for concurrency model.
- **thiserror**: Error derive macros.
- **serde**: Serialization (config, metadata).
- **clap**: CLI parsing with derives.

## Quick reference

### Commands

```bash
# Run tests (ALWAYS use nextest for unit/integration tests)
cargo nextest run
cargo nextest run --all-features
cargo nextest run --profile ci

```
