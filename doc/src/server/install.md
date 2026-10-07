# Connect an agent

Agent harnesses start `tgv mcp` as a local stdio server: the harness runs the command and talks to it over standard input and output. Every harness needs the same two things, the command `tgv` and the argument `mcp`, but each stores them in its own place.

## Before you start

- **Install TGV** (see [Installation](../installation.md)) and check that `tgv --version` works in a terminal.
- **Use the full path in desktop apps.** Apps launched from a dock, a menu, or a file manager often don't see your shell's `PATH`, so they can't find `tgv`. Run `command -v tgv` (for example, `~/.cargo/bin/tgv`) and use that path as the command.
- **Put global options before `mcp`.** For example, `tgv --offline mcp` uses only cached reference data, and `tgv --cache-dir /scratch/tgv mcp` uses another cache. In configuration files, those become `"args": ["--offline", "mcp"]`.
- **Open TGV to share a view.** With a TGV window open, the agent works on the files it shows and can move and mark the view. See [Show results in the viewer](./view.md).

## Claude Code

```sh
claude mcp add tgv -- tgv mcp
```

The `--` separates Claude Code's options from the server command. By default, the server is available in the current project only (`--scope local`). Add `--scope user` to use it in every project, or `--scope project` to share it with your team through a `.mcp.json` file in the project:

```json
{
  "mcpServers": {
    "tgv": {
      "type": "stdio",
      "command": "tgv",
      "args": ["mcp"]
    }
  }
}
```

Run `/mcp` inside Claude Code to check that `tgv` is connected.

## Codex

```sh
codex mcp add tgv -- tgv mcp
```

This writes the server to `~/.codex/config.toml`. To configure it by hand, or for one project in a trusted project's `.codex/config.toml`, add:

```toml
[mcp_servers.tgv]
command = "tgv"
args = ["mcp"]
```

## Gemini CLI

```sh
gemini mcp add --scope user tgv tgv mcp
```

Without `--scope user`, the server is saved for the current project in `.gemini/settings.json`. The equivalent entry in `~/.gemini/settings.json` or `.gemini/settings.json` is:

```json
{
  "mcpServers": {
    "tgv": {
      "command": "tgv",
      "args": ["mcp"]
    }
  }
}
```

Run `gemini mcp list` to check the server.

## Pi

Pi doesn't support MCP on its own. The [`pi-mcp-adapter`](https://github.com/nicobailon/pi-mcp-adapter) extension adds it. Install the extension, then restart Pi:

```sh
pi install npm:pi-mcp-adapter
```

Then add TGV to `~/.config/mcp/mcp.json` for every project, or to `.mcp.json` in a project:

```json
{
  "mcpServers": {
    "tgv": {
      "command": "tgv",
      "args": ["mcp"]
    }
  }
}
```

The adapter exposes all MCP servers through one `mcp` tool and starts `tgv mcp` the first time the agent calls one of its tools.

## Claude Desktop

Add the server to `claude_desktop_config.json`, then restart Claude Desktop. The file is in `~/Library/Application Support/Claude/` on macOS and in `%APPDATA%\Claude\` on Windows; **Settings → Developer → Edit Config** opens it.

```json
{
  "mcpServers": {
    "tgv": {
      "command": "/Users/you/.cargo/bin/tgv",
      "args": ["mcp"]
    }
  }
}
```

Use the full path from `command -v tgv`, since Claude Desktop doesn't read your shell's `PATH`.

## Cursor

Add the server to `~/.cursor/mcp.json` for every project, or to `.cursor/mcp.json` in a project:

```json
{
  "mcpServers": {
    "tgv": {
      "type": "stdio",
      "command": "tgv",
      "args": ["mcp"]
    }
  }
}
```

## VS Code

Add the server to your user profile from a terminal:

```sh
code --add-mcp '{"name":"tgv","command":"tgv","args":["mcp"]}'
```

Or add it to `.vscode/mcp.json` in a workspace. VS Code uses the `servers` key, not `mcpServers`:

```json
{
  "servers": {
    "tgv": {
      "command": "tgv",
      "args": ["mcp"]
    }
  }
}
```

## Other harnesses

Most harnesses accept the same stdio entry, usually under an `mcpServers` key: the command `tgv`, the arguments `["mcp"]`, and no environment variables. TGV writes only MCP messages to standard output and logs to `~/.tgv`, so harnesses that read the server's output directly work without extra settings.

## Check the connection

Ask the agent to call `get_dataset`. Without a TGV window open, it reports `{"loaded": false}`; with a window open, it lists the window's reference and tracks. If the harness reports that the server failed to start, run `tgv mcp` in a terminal: it should wait silently for input. Press Ctrl+D to stop it.
