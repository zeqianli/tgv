# Local MCP server

TGV runs as a local [MCP](https://modelcontextprotocol.io) server so that agents can query genomic files and interact with the user's TGV session.

## Setup

Configure an MCP client to start `tgv mcp` as a stdio server. Install tgv first, then add it to your MCP client.

```sh
# Codex
codex mcp add tgv -- tgv mcp

# Claude
claude mcp add tgv -- tgv mcp

# Other harnesses: ask your agent how to install MCPs
```

The client launches TGV and communicates through standard input and output. No port, URL, or separate background process is needed. `tgv serve` is an alias for `tgv mcp`. Global options precede the subcommand, for example `tgv --offline mcp`. Positional file arguments and `--resume` are rejected; load files with the `load_dataset` tool instead. Standard output is reserved for MCP messages; diagnostics go to standard error and to a log file at `~/.tgv/<timestamp>.log`.

## Tools

| Tool | Purpose |
|------|---------|
| `get_dataset` | Describes the loaded reference and tracks. |
| `load_dataset` | Loads or replaces the dataset. |
| `inspect_interval` | Returns an overview of reads, depth, variants, BED intervals, and genes in an interval. |
| `describe_tables` | Lists the SQL tables, their columns, and example queries. |
| `query` | Runs a read-only SQL query over reads, coverage, variants, BED intervals, genes, and the reference. |
| `navigate` | Moves the user's viewer to a region. |
| `highlight` | Marks intervals in the user's viewer. |
| `clear_highlights` | Removes the marks. |
| `view_state` | Reports what the user's viewer shows. |

- `navigate`, `highlight`, `clear_highlights`, and `view_state` need a running viewer.
- `load_dataset` can't replace a dataset that a viewer is showing. Add or remove files in the viewer instead.
- `inspect_interval` and the region tables in `query` cover at most 100,000 bases.
- Coordinates are one-based, with inclusive interval endpoints.
