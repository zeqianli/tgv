# Local HTTP server

Start an empty local server:

```sh
tgv serve --port 8765
```

The server binds to `127.0.0.1`. Port `0` selects an available port and prints its address. Existing global options precede the subcommand, for example `tgv --offline serve`. The server does not load or save TUI sessions. Paths in requests refer to the server's filesystem. This prototype is intended for trusted local clients, not public hosting.

## Load a dataset

```sh
curl -sS http://127.0.0.1:8765/v1/dataset \
  -X PUT -H 'Content-Type: application/json' \
  -d '{"reference":"hg38","files":["/data/reads.bam","/data/calls.vcf","/data/targets.bed"]}'
```

The `reference` field is required: use a reference accepted by `-g`, or `null` for no reference. File formats and index discovery follow the CLI. BAM indexes use the `.bam.bai` suffix. The response includes the resolved reference, a numeric `revision`, and tracks with `id`, `type`, and `source` fields.

Add or remove files by sending the entire replacement dataset. Each successful replacement rebuilds app state and increments the revision. Failed loading preserves the previous dataset. Track IDs are meaningful only within their revision. `GET /v1/dataset` returns the current description; before loading, it returns `409`.

## Inspect an interval

```sh
curl -sS http://127.0.0.1:8765/v1/inspect \
  -H 'Content-Type: application/json' \
  -d '{"dataset_revision":1,"region":{"contig":"chr20","start":88000,"end":88100},"render":{"format":"text","width":120,"height":40}}'
```

Coordinates are 1-based and inclusive, including BED coordinates in responses. The revision is required. Optional `tracks` selects distinct IDs such as `["t0", "t2"]`; omitting it selects all tracks. There is no shared navigation cursor.

The response contains:

- `dataset_revision` and the resolved `region`.
- `summary.tracks`: alignment-record overlap counts or variant/BED items, counts, and truncation indicators.
- `summary.genes`: annotation availability, overlapping gene IDs, names, intervals, and strands.
- `coverage`: the `viewer_current` method and per-track, per-position `A`, `C`, `G`, `T`, `N`, `total`, and `softclip` values, including zero-depth positions.
- `render`: optional text, dimensions, actual display bounds, and a legend.
- `warnings`: unavailable annotations/reference, display binning, or hidden tracks/read rows.

Coverage is exported from the current viewer without an analytical audit or changes to its calculations. It is not promised to match samtools. Soft clips retain the viewer's projected positions and separate counts. Summary alignment counts use the aligned reference span, while coverage preserves existing viewer behavior. Variant overlaps use the reference allele span; this is not structural-variant interval interpretation.

Omit `render` for structured evidence only. An empty `render` object requests text at 120 × 40; `format: "ansi"` adds terminal colors. Both preserve Unicode drawing symbols from the TUI. Plain text cannot convey every color-only annotation. Display dimensions and padding do not change structured evidence. Some read rows may not fit in the visualization.

## Bounds and errors

Requests are limited to 1 MiB, intervals to 100,000 bases, and render dimensions to 40–500 columns and 10–500 rows. Annotation lists contain at most 1,000 items per track and retain total counts. Coverage is not truncated. These bounds do not impose a read-depth or memory limit; high-depth regions and whole-file VCF/BED loading can still be expensive.

Errors use `{"error":{"code":"...","message":"...","field":null}}`. Status codes include `400` for malformed requests, `409` for missing or stale datasets, `422` for invalid inputs or loading failures, and `500` for unexpected failures. Unknown routes and unsupported methods return structured `404` and `405` errors.

`GET /v1/health` returns `{"status":"ok"}` without waiting for dataset operations. Dataset operations execute serially. Once replacement work begins, a disconnected client may still have its replacement committed; read the current dataset before retrying after an interrupted response. Ctrl-C or a Unix termination signal shuts down the listener and closes repositories after outstanding work finishes.
