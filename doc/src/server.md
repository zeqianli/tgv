# Local MCP server

TGV runs as a local [MCP](https://modelcontextprotocol.io) server so that agents can load genomic files, query and summarize data over intervals with SQL, and draw viewports with the TUI renderer.

## Setup

Configure an MCP client to start `tgv serve` as a stdio server. For example, with Codex or Claude Code:

```sh
codex mcp add tgv -- tgv serve
claude mcp add tgv -- tgv serve
```

The client launches TGV and communicates through standard input and output. No port, URL, or separate background process is needed. Global options precede the subcommand, for example `tgv --offline serve`. Positional file arguments and `--session` are rejected; load files with [`load_dataset`](./server/dataset.md) instead. Standard output is reserved for MCP messages; diagnostics go to the log file or standard error.

## Lifecycle

Each server process starts without a dataset and owns one dataset for its client connection. Tool calls are processed one at a time, in order, by a single dataset worker. Closing standard input stops the server and closes the dataset after outstanding work finishes. A termination signal stops the process immediately.

## Tools

| Tool | Chapter | Purpose |
|------|---------|---------|
| `get_dataset` | [Load and describe a dataset](./server/dataset.md) | Describes the loaded reference and tracks. |
| `load_dataset` | [Load and describe a dataset](./server/dataset.md) | Loads or replaces the dataset. |
| `inspect_interval` | [Inspect an interval](./server/inspect.md) | Returns an overview of reads, depth, variants, BED intervals, and genes in an interval. |
| `describe_tables` | [Query data with SQL](./server/query.md) | Lists the SQL tables, their columns, and example queries. |
| `query` | [Query data with SQL](./server/query.md) | Runs a read-only SQL query over reads, coverage, variants, BED intervals, genes, and the reference. |
| `draw_viewport` | [Draw a viewport](./server/draw.md) | Renders a viewport as plain or ANSI-colored text. |

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
| `no_dataset` | `inspect_interval`, `query`, or `draw_viewport` is called before `load_dataset`. | `null` |
| `internal_error` | Reading files or computing results fails. | `null` |

Malformed MCP requests, and arguments that do not match a tool's input schema, receive protocol errors instead of tool results.
