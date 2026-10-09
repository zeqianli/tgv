# Local MCP server

TGV runs as a local [MCP](https://modelcontextprotocol.io) server so that agents can query genomic files and interact with the user's TGV session.

## Setup

Configure an MCP client to start `tgv mcp` as a stdio server. You should install tgv first before installing the mcp.

```sh
# Codex
codex mcp add tgv -- tgv mcp

# Claude
claude mcp add tgv -- tgv mcp

# Other harnesses: ask your agent how to install MCPs
```

The client launches TGV and communicates through standard input and output. No port, URL, or separate background process is needed. `tgv serve` is an alias for `tgv mcp`. Global options precede the subcommand, for example `tgv --offline mcp`. Positional file arguments and `--resume` are rejected; load files with [`load_dataset`](./server/dataset.md) instead. Standard output is reserved for MCP messages; diagnostics go to the log file or standard error.

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
