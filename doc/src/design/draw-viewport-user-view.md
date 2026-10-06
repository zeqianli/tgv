# Proposal: let the user view drawings directly

Status: proposed, not implemented.

## Context

Today [`draw_viewport`](../server/draw.md) puts the whole drawing, ANSI or plain, in the tool response. The only way the user sees it is through the agent. Terminal MCP clients such as Codex and Claude Code can't show a tool result to the user while keeping it out of the model's context, and asking the agent to run a fetch command doesn't help, because shell output goes back to the model too.

The new response gives the user two commands to run themselves:

1. A **view command** (`cat <file>`). The user runs it with `!` in Codex or Claude Code to see the colored drawing right away.
2. A **browse command** (`tgv <files> -g <reference> -r <contig>:<position>`). The user runs it in a separate terminal to explore the region interactively.

The server is a stdio process, so a separate shell can't read its memory. Each drawing is therefore written as an ANSI file in a temporary directory owned by the server process, and the directory is deleted when the server shuts down.

## Wire changes

These changes go in `crates/tgv/src/server/schema/draw.rs`.

- In `DrawRequest`, replace `format: RenderFormat` with `include_text: bool` (`#[serde(default)]`, false). When it's true, the response also has the plain-text drawing for the model. Sending ANSI to the model is useless, so `ansi` leaves the wire format.
- Keep `RenderFormat` and `export_buffer` as an internal enum, and drop `Deserialize` and `JsonSchema`. The file always uses `Ansi`, and `text` uses `Text`.
- `DrawResponse` becomes:
  - `draw_id: u64`, a counter that increases with each draw in this server session.
  - `region`, `legend`, and `warnings`, unchanged.
  - `view_command: String`, such as `cat '/tmp/tgv-draws-XXXX/draw-3.ansi'`.
  - `browse_command: String`, such as `tgv '/data/HG002.GRCh38.300x_chr20.bam' '/data/simple.vcf' -g hg38 -r chr20:88108`.
  - `text: Option<String>`, with `#[serde(skip_serializing_if = "Option::is_none")]`.
- `DrawResponse::from_buffer` takes the precomputed commands and the optional text instead of `format`.

## Examples

These examples are illustrative and haven't been captured from a running server. Placeholders are in angle brackets.

### Default request

```json
{
  "center": {"contig": "chr20", "position": 88108},
  "zoom": 1,
  "half_width": 40,
  "tracks": [0, 1, 2],
  "canvas_width": 100,
  "canvas_height": 30
}
```

### Default response

```json
{
  "draw_id": 3,
  "region": {"contig": "chr20", "start": 88068, "end": 88148},
  "view_command": "cat '/tmp/tgv-draws-<random suffix>/draw-3.ansi'",
  "browse_command": "tgv '/data/HG002.GRCh38.300x_chr20.bam' '/data/simple.vcf' '/data/simple.bed' -g hg38 -r chr20:88108",
  "legend": "The existing TGV palette and symbols are used. Base letters identify bases; arrows indicate orientation; coverage occupies a separate track. Read rows may be clipped. Unicode drawing characters are preserved.",
  "warnings": [
    {
      "code": "render_limited",
      "track_id": 0,
      "message": "The visualization omits this track or some read rows; structured results are independent of the display."
    }
  ]
}
```

The response has no `text` field, so the drawing stays out of the model's context.

### What the agent tells the user

The agent passes both commands along without running them:

````text
I drew chr20:88,068–88,148, centered on the T>C site at 88,108.

To see the drawing here, run:

    !cat '/tmp/tgv-draws-<random suffix>/draw-3.ansi'

To browse the region yourself, run this in another terminal:

    tgv '/data/HG002.GRCh38.300x_chr20.bam' '/data/simple.vcf' '/data/simple.bed' -g hg38 -r chr20:88108
````

### What the user sees

In Codex or Claude Code, the user types the view command after `!`:

```text
> !cat '/tmp/tgv-draws-<random suffix>/draw-3.ansi'
hg38              │ ▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅ 64Mb
                  │
chr20:88108       │        88,080bp            88,100bp            88,120bp            88,140bp
__________________│            |                   |                   |                   |
                  │[0-370]
                  │▅▅▅▅▅▆▅▅▅▅▅▅▅▅▅▅▆▆▆▆▆▅▅▅▄▄▅▅▅▆▆▆▅▅▅▅▅▅▄▅▄▅▆▆▇▇▇▇▇▆▇▆▆▇▇▇▇▇▆▇▇▆▆▆▇▇██▇▇▇▇▇▇▇▆▆▆▆▆▆
HG002.GRCh38.300x_│►   ------------------------------------C----------------------------------------
chr20.bam         │►   ◄----------------------------------------------------------------------------
<remaining read rows, the VCF and BED tracks, the reference sequence, and the gene track>
```

The terminal shows the TUI's colors here, including the BED and VCF backgrounds that plain text leaves out. The colors are lost in this rendered page.

Running the browse command opens the interactive TUI at `chr20:88108`, at the default zoom instead of the requested `zoom`.

### Request with `include_text`

When the agent needs to read the pileup itself, it adds `"include_text": true`:

```json
{
  "center": {"contig": "chr20", "position": 88108},
  "zoom": 1,
  "half_width": 40,
  "include_text": true,
  "canvas_width": 100,
  "canvas_height": 30
}
```

The response is the same as above, plus a `text` field holding the plain drawing:

```json
{
  "draw_id": 4,
  "region": {"contig": "chr20", "start": 88068, "end": 88148},
  "view_command": "cat '/tmp/tgv-draws-<random suffix>/draw-4.ansi'",
  "browse_command": "tgv '/data/HG002.GRCh38.300x_chr20.bam' '/data/simple.vcf' '/data/simple.bed' -g hg38 -r chr20:88108",
  "text": "hg38              │ ▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅ … <full plain-text drawing, 30 rows>",
  "legend": "<same legend as above>",
  "warnings": []
}
```

### Custom reference and paths with spaces

With a BYO FASTA reference, the browse command keeps the full reference path, and single quotes protect paths that contain spaces or quotes:

```json
{
  "browse_command": "tgv '/data/my runs/sample'\\''s.bam' -g '/refs/custom genome.fa' -r chr1:12345"
}
```

When the server uses `--cache-dir` or `--offline`, those flags are added too, for example `... -r chr20:88108 --cache-dir '/scratch/tgv-cache' --offline`.

### Failed file write

If the server can't write the drawing file, `draw_viewport` returns an internal error:

```json
{
  "error": {
    "code": "internal_error",
    "field": null,
    "message": "The drawing fails to save to '/tmp/tgv-draws-<random suffix>/draw-5.ansi': <I/O error>"
  }
}
```

## Server changes

- `DatasetState::run` (`crates/tgv/src/server/dataset_state.rs`) creates one `tempfile::TempDir`, with the prefix `tgv-draws-`, before its loop. It passes the directory path and a `next_draw_id` counter to `draw`. The directory lives in `run`, not in `DatasetState`, so that it outlives dataset replacement. When the `TempDir` is dropped at shutdown, the files are removed. In `crates/tgv/Cargo.toml`, move `tempfile` from `[dev-dependencies]` to `[dependencies]` as `tempfile.workspace = true`.
- In `DatasetState::draw`, after `render_main`:
  - Write `RenderFormat::Ansi.export_buffer(&buffer)` to `<dir>/draw-<id>.ansi`. Map an I/O failure to `TGVError::McpInternal`.
  - Build `browse_command` from:
    - the selected tracks' `self.repository.file_path(...)`, the same source `description()` uses,
    - the reference, and
    - `<contig>:<center.position>`.
  - Don't use `Reference`'s `Display` for the reference: it cuts BYO FASTA and 2bit paths down to the file name. Match `Reference::BYOIndexedFasta(path) | Reference::BYOTwoBit(path)` to emit the full path, and emit `--no-reference` for `Reference::NoReference`.
  - Forward `--cache-dir`, `--offline`, and `--online` only when the server's settings set them.
  - Shell-quote every path inline with single quotes, replacing `'` with `'\''`.
  - The CLI can't set the zoom, so the browse command opens at the default zoom.
- Update the `draw_viewport` tool description in `crates/tgv/src/server/mod.rs`: "Draw a genome viewport for the user. Show `view_command` and `browse_command` to the user to run themselves; do not run them yourself. Set `include_text` only when you need to read the drawing." Update the server `instructions` to match.

## Documentation changes

- `doc/src/server/draw.md`:
  - Replace `format` with `include_text` in the request table.
  - Update the example response and the field list with `draw_id`, `view_command`, `browse_command`, and the optional `text`.
  - Add behavior notes:
    - Files last for the server session.
    - The ANSI file always holds the colored output, so BED and VCF backgrounds show up there.
    - The browse command opens at the default zoom.
    - The user runs `!<view_command>` in Codex or Claude Code.
  - Remove mentions of `format`, and add the `internal_error` case for a failed file write.
- `doc/src/server.md`: change the `draw_viewport` row in the tool index to "Renders a viewport to a file and returns commands to view or browse it."
- Capture the example responses by running `tgv serve` against the standard example files.

## Caveats

- When a user runs `!<command>`, both clients may still add its output to the conversation. This design saves the model from reading the drawing by default and from echoing it back, but it doesn't guarantee zero tokens.
- Whether `!` output renders ANSI colors in each client needs a manual check.
- A later step could add a `tgv attach` viewer that follows the agent's draws live over a local socket. That would remove the need for both commands.

## Verification

- Run `cargo build` and `cargo nextest run`, and update any server tests that send `format`.
- Run `tgv serve` against `HG002.GRCh38.300x_chr20.bam`, `simple.vcf`, and `simple.bed` with `"reference":"hg38"`, centered at `chr20:88108`. Then check that:
  - the file exists and `cat` renders it in color,
  - `browse_command` opens the TUI at the region,
  - `include_text: true` returns plain text, and
  - the temporary directory is gone after the server exits.
- Try `!cat …` by hand in Codex and in Claude Code.
