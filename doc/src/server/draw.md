# Draw a viewport

`draw_viewport` renders a genomic viewport with the TUI renderer and returns it as plain text or ANSI-colored text. It lets an agent, or a person reading the agent's output, see reads, coverage, tracks, and the reference sequence the way TGV displays them. Drawing does not calculate inspection statistics; use [`inspect_interval`](./inspect.md) for numbers.

## Request

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `center.contig` | string | Yes | A contig name or alias in the dataset, such as `chr20`. |
| `center.position` | integer | Yes | The 1-based center position. |
| `zoom` | integer | Yes | Bases per column. `1` shows individual bases. |
| `half_width` | integer | Yes | Bases loaded on each side of the center, at most 49,999. |
| `tracks` | array of integers | No | Distinct track IDs to draw. All tracks are drawn when this is omitted. |
| `format` | `"text"` or `"ansi"` | Yes | Plain text, or text with ANSI terminal colors and attributes. |
| `canvas_width` | integer | Yes | The canvas width in cells, 10–500. |
| `canvas_height` | integer | Yes | The canvas height in cells, 10–500. |

```json
{
  "center": {"contig": "chr20", "position": 88108},
  "zoom": 1,
  "half_width": 40,
  "tracks": [0, 1, 2],
  "format": "text",
  "canvas_width": 100,
  "canvas_height": 30
}
```

## Response

```json
{
  "region": {"contig": "chr20", "start": 88068, "end": 88148},
  "text": "hg38              │ ▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅ … (shortened; the full drawing is below)",
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

The `text` field contains this drawing, with trailing spaces and blank rows removed here:

```text
hg38              │ ▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅▅ 64Mb
                  │
chr20:88108       │        88,080bp            88,100bp            88,120bp            88,140bp
__________________│            |                   |                   |                   |
                  │[0-370]
                  │▅▅▅▅▅▆▅▅▅▅▅▅▅▅▅▅▆▆▆▆▆▅▅▅▄▄▅▅▅▆▆▆▅▅▅▅▅▅▄▅▄▅▆▆▇▇▇▇▇▆▇▆▆▇▇▇▇▇▆▇▇▆▆▆▇▇██▇▇▇▇▇▇▇▆▆▆▆▆▆
                  │█████████████████████████████████████████████████████████████████████████████████
                  │████████████████████████████████████████ ████████████████████████████████████████
                  │█████████████████████████████████████████████████████████████████████████████████
HG002.GRCh38.300x_│                                        █
chr20.bam         │►   ------------------------------------C----------------------------------------
                  │►   ◄----------------------------------------------------------------------------
                  │-   ◄----------------------------------------------------------------------------
                  │--►   ----------------------------------C----------------------------------------
                  │--►    ---------------------------------C----------------------------------------
                  │---    ◄--------------------------------C----------------------------------------
                  │---►   ◄--------------------------------C-----------------------C----------------
0% (1 / 377)      │----   ◄-------------------------------------------------------------------------
__________________│----    --------------------------------C----------------------------------------
simple.vcf        │
__________________│
simple.bed        │
__________________│
                  │CACTTCCATTTCGATTATTCTGTTGTATCTATTTCATTGTTGTGTCCTATTAGTTCTCCTACCATCTTGAATTCTTCTTTG
                  │>--------->--------->--------->--------->--------->--------->--------->--------->
```

The left column is the sidebar: the reference, the current position, track file names, and the alignment depth label. The right column shows, from top to bottom, the chromosome ideogram, the ruler, the coverage track with its `[0-370]` depth scale, the reads, the VCF and BED tracks, the reference sequence, and the gene track. Reads show `-` for matching bases, base letters for mismatches (the `C` column is the T>C site at 88,108), and `►`/`◄` for read orientation.

### Fields

- `region` is the interval actually displayed, which can differ from `center` ± `half_width`.
- `text` is the rendered canvas, one line per row, with `canvas_height` rows.
- `legend` briefly explains the symbols.
- `warnings` lists display limitations:

| Code | Meaning |
|------|---------|
| `reference_unavailable` | The dataset has no reference sequence. |
| `genes_unavailable` | The dataset has no gene annotation service. |
| `render_limited` | A track is hidden, or some of its read rows are clipped, at this canvas size. Includes `track_id`. |
| `render_binned` | `zoom` is greater than 1, so each column spans multiple bases. |

## Behavior

- The displayed span is the width of the track area (the canvas width minus the sidebar) times `zoom`, centered on `center.position`. In the example, a 100-cell canvas leaves 81 track columns, so 81 bases are shown even though `half_width` is 40. `half_width` only affects the interval that is loaded and validated.
- When the contig length is known, `zoom` is reduced and the viewport is shifted so that the display stays inside the contig.
- Reads are not drawn when `zoom` is greater than 32; a `render_limited` warning is reported instead.
- Drawing reuses the TUI palette and symbols. BED intervals and variants are drawn as background colors, so they appear only in `"ansi"` output; in `"text"` output their rows are blank, as in the example above.
- Unicode drawing characters are preserved, and control characters are replaced with `�`.

## Bounds and errors

The canvas width and height must each be 10–500 cells. The displayed span (track columns × `zoom`) must be at most 100,000 bases.

| Code | `field` | Cause |
|------|---------|-------|
| `invalid_input` | `zoom` | `zoom` is 0, or the displayed span exceeds 100,000 bases. |
| `invalid_input` | `draw` | The canvas width or height is outside 10–500. |
| `invalid_input` | `center` | The position is 0, `half_width` exceeds 49,999, or the interval exceeds the coordinate range. |
| `invalid_input` | `center.contig` | The contig is not in the dataset. |
| `invalid_input` | `center.position` | The center is past the contig end, or the displayed viewport exceeds the coordinate range. |
| `invalid_input` | `canvas_width` | The layout leaves no columns for tracks. |
| `invalid_input` | `tracks` | `tracks` is empty, repeats an ID, or contains an ID that is not in the dataset. |
| `no_dataset` | `null` | No dataset is loaded. |

For example, `"zoom": 2000` on a 100-cell canvas returns:

```json
{
  "error": {
    "code": "invalid_input",
    "field": "zoom",
    "message": "The displayed span must be at most 100000 bases; reduce the zoom or width."
  }
}
```
