# Local MCP server

Configure an MCP client to start `tgv serve` as a stdio server. For example, with Codex:

```sh
codex mcp add tgv -- tgv serve
```

The client launches TGV and communicates through standard input and output. No port, URL, or separate background process is needed. Each server process starts without a dataset and owns one dataset for its client connection. Global options precede the subcommand, for example `tgv --offline serve`. File paths passed to tools refer to the computer running TGV. Standard output is reserved for MCP messages; diagnostics go to the log file or standard error.

## Load and describe a dataset

Call the `load_dataset` tool with these arguments:

```json
{"reference":"hg38","files":["/data/reads.bam","/data/calls.vcf","/data/targets.bed"]}
```

The `reference` field is required; use a reference accepted by `-g`, or `null` for no reference. A successful result contains the reference and tracks with numeric `id`, `type`, and `source` fields. Adding or removing files replaces the entire dataset. Failed loading leaves the previous dataset intact. Track IDs reset on replacement, so call `get_dataset` again before using old IDs. Before the first load, `get_dataset` returns `{"loaded":false}`.

## Inspect an interval

Call `inspect_interval` with these arguments:

```json
{"region":{"contig":"chr20","start":88000,"end":88100},"tracks":[0,1,2]}
```

Coordinates are 1-based and inclusive. The optional `tracks` array selects distinct numeric IDs; omitting it selects all tracks. The structured result contains the effective `region`, `summary.tracks`, `summary.genes`, and `warnings`. If the contig length is known, an end beyond it is clamped; a start beyond it is invalid.

Each alignment entry in `summary.tracks` contains `overlapping_records` and `coverage`. The nested coverage has a `viewer_current` method and per-position `A`, `C`, `G`, `T`, `N`, `total`, and `softclip` counts, including zero-depth positions. Variant and BED entries contain overlap counts, up to 1,000 items, and a truncation flag. Alignment overlap counts use displayed read spans, including soft clips, through the same core query as the single-read TUI renderer. Coverage retains the current viewer calculation and is not an audited analytical metric.

## Draw a viewport

Call `draw_viewport` with these arguments:

```json
{"center":{"contig":"chr20","position":88050},"zoom":1,"half_width":100,"tracks":[0],"format":"ansi","canvas_width":120,"canvas_height":40}
```

Use `"format":"text"` for plain output or `"format":"ansi"` for terminal colors. The structured result contains the displayed `region`, `text`, a `legend`, and `warnings`. Drawing reuses the TUI renderer and does not calculate inspection statistics.

## Bounds and errors

Inspected intervals are limited to 100,000 bases. Draw canvas dimensions must each be 10–500 cells, and the resolved track area must have nonzero width. These bounds do not impose a read-depth or memory limit, so high-depth regions can still be expensive.

Validation and dataset failures are MCP tool errors with `isError: true`, a readable message, and structured `{"error":{"code":"...","message":"...","field":null}}` data. Malformed MCP requests and arguments receive protocol errors. Closing standard input stops the server and closes its repository after outstanding work finishes. A termination signal stops the process immediately.
