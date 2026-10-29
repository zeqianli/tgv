# The window (experimental)

`tgv-gui` shows the same tracks as `tgv` in a window, drawn at pixel resolution. Pans and
zooms are continuous, like IGV, and the keys, modes, commands, and layout are the terminal
UI's. It runs on macOS and Linux, with Wayland or X11 on Linux.

## Running it

On Linux, install the native build dependencies and the Liberation Mono font first. On
Debian or Ubuntu:

```sh
sudo apt-get install pkg-config libfontconfig1-dev libfreetype6-dev libwayland-dev \
    libxkbcommon-dev libxkbcommon-x11-dev libssl-dev fonts-liberation
```

The Linux GUI also needs a Vulkan-capable graphics driver. Build it for the system's native
GNU target; the musl release archive contains only the terminal viewer.

Build it from the repository and pass the same options and files as `tgv`:

```sh
cargo install --path crates/tgv-gui
tgv-gui -g hg38 -r chr20:88108 /data/simple.bam /data/simple.vcf /data/simple.bed
```

Every option in [Usage](./usage.md#command-line-options) works, including `--resume`. The
`mcp`, `download`, and `list` subcommands are `tgv`'s only.

## Keys

All of `tgv`'s keys work the same way: `h` and `l` pan, `z` and `o` zoom, `w`, `b`, and `e` move
between features, `:` and `/` open the command line, `u` and `Ctrl-r` go back and forward, and
`i` starts insert mode on a BED track. One `h` or `l` moves about 8 pixels.

Insert mode zooms in until a base is at least 8 pixels wide, so the cursor is always one base,
and every edit paints or erases whole bases.

Command shortcuts:

Use `Super` in place of `⌘` on Linux. A desktop shortcut may take precedence; the viewer's
keys and commands also provide these actions.

| Shortcut | Action |
|---|---|
| `⌘Q` or `⌘W` | Quit, as `:q` does. Closing the window does the same. |
| `⌘O` | Open files. |
| `⌘S` | Write the focused BED track, as `:w` does. |
| `⌘=` and `⌘-` | Zoom in and out. |
| `⌘V` | Paste into the command or search line. |

## Mouse and trackpad

| Gesture | Action |
|---|---|
| Drag | Pan. Over an alignment, a vertical drag also scrolls its reads. |
| Horizontal scroll | Pan. |
| Vertical scroll | Scroll the reads of the alignment under the pointer. |
| Pinch (macOS only) | Zoom around the pointer. |
| `Ctrl` or `⌘` with a vertical scroll | Zoom around the pointer. |
| Double click | Zoom in around the point. |
| Click | Focus a track. A click on a read shows its record, and a click on the cytoband jumps there. |
| Right click | Open the track's menu. |
| Drop files | Open them where the highlighted gap shows: above or below the track under the pointer. |

Hovering describes the read, base, gene, variant, or feature under the pointer in the message
line.

## Agents

Agents reach the window through `tgv mcp`, as they reach the terminal UI. See
[Local MCP server](./server.md).
