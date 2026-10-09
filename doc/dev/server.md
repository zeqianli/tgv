
## Lifecycle

Each server process starts with a headless session that has no dataset. When the user has TGV open, the server connects to the most recently started viewer and uses its dataset instead; see [Show results in the viewer](./server/view.md). Tool calls are processed one at a time, in order. Closing standard input stops the server and closes the dataset after outstanding work finishes. A termination signal stops the process immediately.


## Conventions

- Coordinates are 1-based, and interval ends are inclusive, in both requests and responses.
- Tracks have numeric IDs in the order of the loaded files. IDs reset whenever the dataset is replaced, so call `get_dataset` again before reusing old IDs.
- An optional `tracks` array selects distinct track IDs; omitting it selects all tracks.
- File paths refer to the computer running TGV, not to the client.
- Requests reject unknown fields.

## Errors

Validation and dataset failures are MCP tool results with `isError: true`. The text content contains `<code>: <message>`, and the structured content contains the error object. For example, an `inspect_interval` call with `"tracks":[0,0]` returns:

```json
{
  "content": [
    {
      "type": "text",
      "text": "invalid_input: Select distinct track IDs from the current dataset."
    }
  ],
  "structuredContent": {
    "error": {
      "code": "invalid_input",
      "field": "tracks",
      "message": "Select distinct track IDs from the current dataset."
    }
  },
  "isError": true
}
```

| Code | Meaning | `field` |
|------|---------|---------|
| `invalid_input` | The request is well-formed but invalid for the current dataset. | The offending request field, such as `tracks` or `center.contig`. |
| `no_dataset` | `inspect_interval` or `query` is called before `load_dataset`. | `null` |
| `no_viewer` | A viewer tool is called while no TGV viewer is running. | `null` |
| `dataset_fixed` | `load_dataset` is called while connected to a viewer. | `null` |
| `viewer_disconnected` | The viewer closed since the last call. | `null` |
| `internal_error` | Reading files or computing results fails. | `null` |

Malformed MCP requests, and arguments that do not match a tool's input schema, receive protocol errors instead of tool results.
