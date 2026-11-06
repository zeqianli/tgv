# Remote BAM loading performance

Status: ideas, not started. Nothing here has been profiled yet.

## Context

Remote BAM files on object storage load through `RemoteBamRepository` in
`crates/gv-core/src/alignment/repository.rs`. It wraps an opendal `Operator` (S3) in
`into_futures_async_read(..)` and hands the reader to noodles' async BAM reader. Loading a
remote region is dominated by round trips and by re-downloading bytes, not by CPU. These
notes collect the ideas for cutting both, roughly in the order they should be tried.

Versions checked: noodles-bam 0.88, noodles-bgzf 0.46, and opendal 0.53.

## Measure first

The existing logs already split the time:

- `Read alignment records ... elapsed_ms` covers I/O and decoding in `read_alignment`.
- `Loaded alignment data ... elapsed_ms` adds stacking, coverage, and display options.
- The `Object storage request:` lines show each remote operation.

Run with `RUST_LOG=debug` against a remote BAM and record a baseline for opening a file, the
first view, and a pan past the cached window. Count the HTTP requests, too. The opendal
`LoggingLayer` or S3 access logs can confirm the request counts below.

## Avoidable round trips

### Every query sends a HEAD request

`into_futures_async_read(..)` with an unbounded range makes opendal stat the object to learn
its length (`ReadContext::parse_into_range` in opendal). This happens on every query, and
also when the constructor reads the index and the header.

Fix: stat the BAM once in `RemoteBamRepository::new`, store the length, and pass `0..len`.
Read the index with the length known, or with a bounded range, as well.

### Every index chunk is a new GET, one after another

A BAI query (`index.query(...)`) returns several chunks across bin levels. noodles seeks the
BGZF reader to each chunk start. opendal's `FuturesAsyncReader::poll_seek` reuses its buffer
only when the target is inside it. Otherwise it drops the stream and starts a new request
from the target offset to the end of the object. The chunks are fetched serially, so a query
costs one round trip per chunk.

Fix: do the chunk I/O ourselves.

1. Compute the chunks with `index.query(reference_sequence_id, interval)`.
2. Merge chunks whose gap is small enough that one request is cheaper than two. Start with a
   gap threshold of about 256 KiB and tune it.
3. For each merged range, read compressed bytes from the start offset's compressed position
   to the end offset's compressed position, plus one BGZF block (at most 64 KiB). The end
   virtual offset points at the start of the last block, so that block has to be fetched
   whole.
4. Fetch the merged ranges concurrently with `operator.read_with(key).range(a..b)`.
5. Decode from memory. Wrap the bytes in a cursor with a BGZF reader, seek to each chunk's
   virtual offset, read records until the chunk end, and apply the same overlap filter that
   noodles uses (`intersects` in `noodles-bam/src/io/reader/query.rs`).

This turns N serial round trips into about one round of parallel requests. It also bounds
every request, so no request streams toward the end of the object.

Correctness notes:

- Keep noodles' semantics: records overlap the region by aligned span (soft clips excluded),
  have a matching reference ID, and appear in file order.
- Deduplicate when merged ranges overlap, by decoding each chunk only once.

### Opening a file is serial

`RemoteBamRepository::new` downloads the full BAI, then reads the header, one after the
other, each with its own HEAD request.

Fixes:

- Run the index and the header reads concurrently (`tokio::try_join!`).
- Read the header with a bounded range, such as the first 1 MiB, and extend only if the
  header is larger.
- Cache the BAI on disk, keyed by the object path and its ETag, so reopening a large remote
  BAM doesn't download the index again.

## Re-downloaded bytes

### No block cache between queries

Nearby views fetch the same compressed BGZF blocks again. An LRU cache of blocks, keyed by
compressed file offset, would serve repeated and overlapping queries from memory. It fits
naturally under the range reads above: check the cache before deciding which ranges to
fetch. Decide whether to cache compressed or decompressed blocks by measuring. Compressed
blocks are about 3× smaller, and decompression is cheap with `libdeflate`.

### Every cache miss reloads the whole window

`State::ensure_loaded` (`crates/gv-core/src/state.rs`) throws away the loaded alignment and
reads the whole padded window (`CachePolicy::VIEWER`, 3× the view) again when the view
leaves it. Panning just past the edge re-downloads mostly the same data.

Options, from simple to involved:

- Shift the padded window toward the direction of motion, so continued panning misses less
  often.
- With the block cache, the reload mostly hits memory, so this matters less.
- Load only the new part and merge it in. This needs read IDs to stay stable across loads,
  which they don't today ("read positions are re-shuffled at every load" in
  `load_alignment_data`).

## Hiding latency

### Load tracks concurrently

`ensure_loaded` loads alignment tracks one after another, and the reference sequence load
finishes before any alignment starts. With several remote BAMs, loading them with
`join_all` is close to an N× drop in latency. The reference is only needed when building
tables, so its load can overlap the record fetches, too.

### Prefetch in the background

After a load, fetch the next window in the direction of panning on a background task. The
result can land in the block cache, or in a second `State` that's swapped in when ready
(`State` is cheap to clone). This doesn't make a single load faster, but it hides most of
the wait while panning.

### Keep the TUI responsive

The TUI awaits `ensure_loaded` inside its event loop (`crates/tgv/src/app.rs`), so the
screen freezes during a slow remote load. The GUI already loads on a worker thread
(`crates/tgv-gui/src/worker.rs`). Moving TUI loads onto a task lets it keep drawing, show
progress, and cancel a load that's stale because the user moved again.

## CPU work that also helps remote loads

These aren't remote-specific, but they shorten every load once the network is fast.

- **Repeated concatenation in `AlignmentTables::add_records`** (`tables.rs`). Each batch of
  1,024 records is concatenated into all five tables and collected again, so the work grows
  roughly with the square of the read count. Collect batches into a `Vec` and concatenate
  once.
- **Enable `libdeflate` on noodles-bgzf.** It's a feature flag in the workspace
  `Cargo.toml`, and it typically inflates BGZF about 2× faster than flate2.
- **Build batches in parallel.** `build_batch` is independent per batch given its read-ID
  offset, so batches can go to rayon or `spawn_blocking` while the next records decode.
  Watch for oversubscription between Tokio's blocking pool, Polars' pool, and rayon.

## Constraints

- Repository types need not be `Send` (see `Session::spawn` in
  `crates/gv-session/src/session.rs`). Move plain data, like byte buffers, `RecordBuf`s, and
  `Arc<Sequence>`, across threads, not readers or operators.
- Read IDs must stay in stable record order, because `Alignment` relies on it.
- `RemoteBamRepository::new` unwraps the `s3://` prefix and the bucket split. It should
  return an error instead, before more is built on top of it.
