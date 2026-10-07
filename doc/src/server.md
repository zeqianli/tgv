# Local MCP server

TGV runs as a local [MCP](https://modelcontextprotocol.io) server so that agents can load genomic files, query and summarize data over intervals with SQL, and show results in the user's TGV viewer.

## Setup

Configure an MCP client to start `tgv mcp` as a stdio server. For example, with Codex or Claude Code:

```sh
codex mcp add tgv -- tgv mcp
claude mcp add tgv -- tgv mcp
```

[Connect an agent](./server/install.md) has instructions for Claude Code, Codex, Gemini CLI, Pi, Claude Desktop, Cursor, VS Code, and other harnesses.

The client launches TGV and communicates through standard input and output. No port, URL, or separate background process is needed. `tgv serve` is an alias for `tgv mcp`. Global options precede the subcommand, for example `tgv --offline mcp`. Positional file arguments and `--resume` are rejected; load files with [`load_dataset`](./server/dataset.md) instead. Standard output is reserved for MCP messages; diagnostics go to the log file or standard error.

## Lifecycle

Each server process starts with a headless session that has no dataset. When the user has TGV open, the server connects to the most recently started viewer and uses its dataset instead; see [Show results in the viewer](./server/view.md). Tool calls are processed one at a time, in order. Closing standard input stops the server and closes the dataset after outstanding work finishes. A termination signal stops the process immediately.

## Tools

| Tool | Chapter | Purpose |
|------|---------|---------|
| `get_dataset` | [Load and describe a dataset](./server/dataset.md) | Describes the loaded reference and tracks. |
| `load_dataset` | [Load and describe a dataset](./server/dataset.md) | Loads or replaces the dataset. |
| `inspect_interval` | [Inspect an interval](./server/inspect.md) | Returns an overview of reads, depth, variants, BED intervals, and genes in an interval. |
| `describe_tables` | [Query data with SQL](./server/query.md) | Lists the SQL tables, their columns, and example queries. |
| `query` | [Query data with SQL](./server/query.md) | Runs a read-only SQL query over reads, coverage, variants, BED intervals, genes, and the reference. |
| `navigate` | [Show results in the viewer](./server/view.md) | Moves the user's viewer to a region. |
| `highlight` | [Show results in the viewer](./server/view.md) | Marks intervals in the user's viewer. |
| `clear_highlights` | [Show results in the viewer](./server/view.md) | Removes the marks. |
| `view_state` | [Show results in the viewer](./server/view.md) | Reports what the user's viewer shows. |

The examples in each chapter use the same dataset: an HG002 chr20 BAM, a small VCF, and a small BED file on hg38, inspected around `chr20:88108`.

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
