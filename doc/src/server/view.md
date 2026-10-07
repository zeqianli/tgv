# Show results in the viewer

When the user has TGV open, `tgv mcp` connects to it, and these tools act on what the user sees. `navigate` moves the view to a region, `highlight` marks intervals, `clear_highlights` removes the marks, and `view_state` reports what the view shows. Without a running viewer, they fail with `no_viewer`.

## Connecting to a viewer

- **Where viewers listen.** Every TGV window listens on a socket at `$XDG_RUNTIME_DIR/tgv/<pid>.sock`, or in a `tgv-$USER` directory under the system temporary directory when `XDG_RUNTIME_DIR` is unset. Only the user who started TGV can open that directory. Sockets left behind by viewers that exited without cleaning up are removed.
- **Which viewer `tgv mcp` uses.** It connects to the most recently started viewer. Until the agent loads a dataset into the headless session with `load_dataset`, `tgv mcp` checks for a viewer before every call, so the user can open TGV after the agent starts. After a headless load, calls stay headless and keep that dataset.
- **What changes once connected.** All tools use the viewer's dataset. `get_dataset` describes the files the viewer shows, `inspect_interval` and `query` read them through a separate query state that never changes what the user sees, and `load_dataset` fails with `dataset_fixed`.
- **When the viewer closes.** The next call fails with `viewer_disconnected`, and later calls go to the headless session.

## `navigate`

### Request

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `region.contig` | string | Yes | A contig name or alias in the dataset, such as `chr20`. |
| `region.start` | integer | Yes | The 1-based first position. |
| `region.end` | integer | Yes | The 1-based last position, inclusive. |

```json
{"region": {"contig": "chr20", "start": 90000, "end": 90200}}
```

### Response

The response is the new view, in the same form as `view_state`:

```json
{"region": {"contig": "chr20", "start": 89964, "end": 90236}, "zoom": 3}
```

### Behavior

- The view centers on the region and picks the smallest zoom, in bases per column, that fits the region in the track area. The displayed interval can therefore be wider than the region.
- The viewer's message line tells the user how to return, for example "An agent moved the view to chr20:90100. Type :chr20:88108 to go back."
- The region isn't limited to 100,000 bases. Wide regions show coverage and annotations without reads, as when the user zooms out.

## `highlight`

### Request

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `intervals` | array of regions | Yes | The 1-based inclusive intervals to mark, each with `contig`, `start`, and `end`. |
| `label` | string or `null` | No | A short description that the viewer shows with the marks. |

```json
{"intervals": [{"contig": "chr20", "start": 90090, "end": 90110}], "label": "candidate site"}
```

### Response

```json
{}
```

### Behavior

- Highlights replace earlier highlights.
- The viewer tints the columns of each interval in the coordinate ruler, and its message line reports the count and the label, for example "An agent highlighted 1 intervals: candidate site."
- Intervals on other contigs appear when the user moves to those contigs.

## `clear_highlights`

`clear_highlights` takes no arguments, removes every highlight, and returns `{}`.

## `view_state`

`view_state` takes no arguments and reports the displayed interval and the zoom, in bases per column:

```json
{"region": {"contig": "chr20", "start": 88063, "end": 88153}, "zoom": 1}
```

## Errors

| Code | `field` | Cause |
|------|---------|-------|
| `no_viewer` | `null` | No TGV viewer is running. |
| `invalid_input` | `region` | A `navigate` or `highlight` interval names an unknown contig, starts at 0, or ends before it starts. |
| `viewer_disconnected` | `null` | The viewer closed since the last call. Later calls go to the headless session. |

For example, navigating to an unknown contig returns:

```json
{"error": {"code": "invalid_input", "field": "region", "message": "State error: Contig chrZ not found"}}
```
