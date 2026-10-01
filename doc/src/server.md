# Local HTTP server

Start an empty local server:

```sh
tgv serve --port 8765
```

The server binds to `127.0.0.1`. Port `0` selects an available port and prints its address. Global options precede the subcommand, for example `tgv --offline serve`. This prototype is intended for trusted local clients, not public hosting. File paths in requests refer to the server's filesystem.

## Load a dataset

```sh
curl -sS http://127.0.0.1:8765/v4/dataset \
  -X PUT -H 'Content-Type: application/json' \
  -d '{"reference":"hg38","files":["/data/reads.bam","/data/calls.vcf","/data/targets.bed"]}'
```

The `reference` field is required; use a reference accepted by `-g`, or `null` for no reference. A successful response contains the reference and tracks with numeric `id`, `type`, and `source` fields. Adding or removing files means replacing the entire dataset. Failed loading leaves the previous dataset intact. Track IDs reset on replacement, so read the dataset description again before using old IDs. `GET /v4/dataset` returns `{"loaded":false}` before the first load.

## Inspect an interval

```sh
curl -sS http://127.0.0.1:8765/v4/inspect \
  -H 'Content-Type: application/json' \
  -d '{"region":{"contig":"chr20","start":88000,"end":88100},"tracks":[0,1,2]}'
```

Coordinates are 1-based and inclusive. Optional `tracks` selects distinct numeric IDs; omitting it selects all tracks. The response contains the effective `region`, `summary.tracks`, `summary.genes`, and `warnings`. If the contig length is known, an end beyond it is clamped; a start beyond it is invalid.

Each alignment entry in `summary.tracks` contains `overlapping_records` and `coverage`. The nested coverage has a `viewer_current` method and per-position `A`, `C`, `G`, `T`, `N`, `total`, and `softclip` counts, including zero-depth positions. Variant and BED entries contain overlap counts, up to 1,000 items, and a truncation flag. Alignment overlap counts use displayed read spans, including soft clips, through the same core query as the single-read TUI renderer. Coverage retains the current viewer calculation and is not an audited analytical metric.

## Draw a viewport

```sh
curl -sS http://127.0.0.1:8765/v4/draw \
  -H 'Content-Type: application/json' \
  -d '{"center":{"contig":"chr20","position":88050},"zoom":1,"half_width":100,"tracks":[0],"format":"ansi","canvas_width":120,"canvas_height":40}'
```

Use `"format":"text"` for plain output or `"format":"ansi"` for terminal colors. The response contains only the actual displayed `region`, `text`, a `legend`, and `warnings`; the request supplies the canvas dimensions and format. Drawing reuses the TUI renderer and does not calculate inspection statistics.

## Bounds and errors

Requests are limited to 1 MiB and inspected intervals to 100,000 bases. Draw canvas dimensions must each be 10–500 cells, and the resolved track area must have nonzero width. These bounds do not impose a read-depth or memory limit, so high-depth regions can still be expensive.

Errors use `{"error":{"code":"...","message":"...","field":null}}`. Status codes include `400` for malformed requests, `409` for a missing dataset, `422` for invalid inputs, and `500` for unexpected failures. `GET /v4/health` returns `{"status":"ok"}` without waiting for dataset operations. Ctrl-C or a Unix termination signal shuts down the listener and closes repositories after outstanding work finishes.
