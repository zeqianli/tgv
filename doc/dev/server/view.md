# Show results in the viewer

When the user has TGV open, `tgv mcp` connects to it, and these tools act on what the user sees. `navigate` moves the view to a region, and `view_state` reports what the view shows. Without a running viewer, they fail with `no_viewer`.

## Connecting to a viewer

- **Where viewers listen.** Every TGV viewer, the terminal UI (`tgv`) or the window (`tgv-gui`), listens on a socket at `$XDG_RUNTIME_DIR/tgv/<pid>.sock`, or in a `tgv-$USER` directory under the system temporary directory when `XDG_RUNTIME_DIR` is unset. Only the user who started TGV can open that directory. Sockets left behind by viewers that exited without cleaning up are removed.
- **Which viewer `tgv mcp` uses.**
  - With one viewer open, it connects to that viewer.
  - With several open, calls fail with `viewer_ambiguous`, and the error lists the viewers. The agent chooses one from the question's context, such as a file, a locus, or the TUI or the GUI, asks the user when it is still unclear, and calls `attach` with the viewer's `pid`. `list_viewers` describes the open viewers at any time.
  - Until the agent loads a dataset into the headless session with `load_dataset`, `tgv mcp` checks for viewers before every call, so the user can open TGV after the agent starts. After a headless load, calls stay headless and keep that dataset.
- **What changes once connected.** All tools use the viewer's dataset. `get_dataset` describes the files the viewer shows, `inspect_interval` and `query` read them through a separate query state that never changes what the user sees, and `load_dataset` fails with `dataset_fixed`.
- **When the viewer closes.** The next call fails with `viewer_disconnected`, and later calls look for viewers again.

## `list_viewers`

`list_viewers` takes no arguments and describes each open viewer, newest first, and the `pid` of the viewer this server uses, if any:

```json
{
  "viewers": [
    {"pid": 80070, "kind": "gui", "reference": "hg38", "files": ["/data/a.bam"], "locus": "chr20:90100", "started": 1792606579}
  ],
  "attached": null
}
```

`kind` is `tui` for the terminal UI and `gui` for the window. `started` is in seconds since the Unix epoch.

## `attach`

`attach` takes a viewer's `pid`, sends later calls to that viewer, and returns its description:

```json
{"pid": 80070}
```

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

- The view centers on the region and picks the smallest zoom that fits the region in the track area: a whole number of bases per column in the terminal UI, and any number of bases per pixel in the window. The displayed interval can therefore be wider than the region.
- The viewer's message line tells the user how to return, for example "An agent moved the view to chr20:90100. Press u to go back to chr20:88108."
- The region isn't limited to 100,000 bases. Wide regions show coverage and annotations without reads, as when the user zooms out.

## `view_state`

`view_state` takes no arguments and reports the displayed interval and the zoom, in bases per screen unit: per column in the terminal UI, and per pixel in the window:

```json
{"region": {"contig": "chr20", "start": 88063, "end": 88153}, "zoom": 1}
```

## Errors

| Code | `field` | Cause |
|------|---------|-------|
| `no_viewer` | `null` | No TGV viewer is running. |
| `invalid_input` | `region` | A `navigate` interval names an unknown contig, starts at 0, or ends before it starts. |
| `viewer_disconnected` | `null` | The viewer closed since the last call. Later calls look for viewers again. |
| `viewer_ambiguous` | `null` | Several viewers are open and the agent hasn't attached to one. The message lists them. |
| `viewer_not_found` | `pid` | `attach` names a process that no viewer runs in. |

For example, navigating to an unknown contig returns:

```json
{"error": {"code": "invalid_input", "field": "region", "message": "State error: Contig chrZ not found"}}
```
