# Inspect an interval

`inspect_interval` returns a quick overview of an interval: read counts and depth statistics for alignment tracks, overlapping records for variant and BED tracks, and overlapping genes. The results do not depend on any drawing. For per-position counts, individual reads, or any other detail, use [`query`](./query.md).

## Request

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `region.contig` | string | Yes | A contig name or alias in the dataset, such as `chr20`. |
| `region.start` | integer | Yes | The 1-based first position. |
| `region.end` | integer | Yes | The 1-based last position, inclusive. |
| `tracks` | array of integers | No | Distinct track IDs to inspect. All tracks are inspected when this is omitted. |

```json
{"region": {"contig": "chr20", "start": 88106, "end": 88110}}
```

## Response

This interval covers a heterozygous T>C site at `chr20:88108`. The site appears in the VCF and inside a BED target.

```json
{
  "region": {"contig": "chr20", "start": 88106, "end": 88110},
  "summary": {
    "tracks": [
      {
        "type": "alignment",
        "track_id": 0,
        "overlapping_records": 360,
        "coverage": {
          "method": "viewer_current",
          "positions": 5,
          "zero_depth_positions": 0,
          "mean_depth": 345.0,
          "min_depth": 338,
          "max_depth": 353
        }
      },
      {
        "type": "variant",
        "track_id": 1,
        "overlapping_records": 1,
        "truncated": false,
        "items": [
          {"start": 88108, "end": 88108, "reference": "T", "alternate": ["C"]}
        ]
      },
      {
        "type": "bed",
        "track_id": 2,
        "overlapping_records": 1,
        "truncated": false,
        "items": [
          {"start": 88101, "end": 88200}
        ]
      }
    ],
    "genes": {
      "available": true,
      "overlapping_records": 1,
      "truncated": false,
      "items": [
        {"id": "NM_153325.4", "name": "DEFB125", "start": 87672, "end": 97094, "strand": "+"}
      ]
    }
  },
  "warnings": []
}
```

### Fields

- `region` is the effective interval after clamping.
- `summary.tracks` has one entry per selected track, in dataset order. The `type` field indicates which other fields the entry has:
  - `alignment`: `overlapping_records` counts reads whose displayed span overlaps the interval. `coverage` summarizes the aligned-base depth over the interval: the number of `positions`, the `zero_depth_positions`, and the `mean_depth`, `min_depth`, and `max_depth`.
  - `variant`: `items` lists the overlapping records with `start`, `end`, `reference`, and `alternate` alleles.
  - `bed`: `items` lists the overlapping intervals, converted to 1-based inclusive coordinates. The BED line `chr20 88100 88200` appears as `88101`–`88200`.
- `summary.genes` lists overlapping genes, sorted by start. `available` is `false` when the reference has no gene annotations.
- `warnings` lists the data that is unavailable to the dataset. `reference_unavailable` and `genes_unavailable` each carry a `message`.

## Behavior

- Variant, BED, and gene lists include at most 1,000 items. `overlapping_records` is the full count, and `truncated` is `true` when items are omitted.
- Alignment overlap counts use displayed read spans, including soft clips, through the same core query as the single-read TUI renderer.
- Coverage uses the viewer's current calculation (`viewer_current`). It is not an audited analytical metric.
- If the contig length is known, an end beyond it is clamped to the contig end.

## Bounds and errors

The interval may span at most 100,000 bases. The bound does not limit read depth or memory, so high-depth regions can still be expensive.

| Code | `field` | Cause |
|------|---------|-------|
| `invalid_input` | `tracks` | `tracks` is empty, repeats an ID, or contains an ID that is not in the dataset. |
| `invalid_input` | `region` | The contig is unknown, the start is 0 or past the contig end, the start exceeds the end, or the interval is wider than 100,000 bases. |
| `no_dataset` | `null` | No dataset is loaded. |

For example, an unknown contig returns:

```json
{"error": {"code": "invalid_input", "field": "region", "message": "State error: Contig chrZ not found"}}
```
