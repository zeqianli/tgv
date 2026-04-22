# Usage

## Supported formats

- BAM (indexed and sorted). A `.bai` file is needed.
  - Local, AWS, Google Cloud, or HTTP/HTTPS
  - Local: place the `.bai` file in the same directory; or specify the index file with `-i`.
  - `s3`: set credentials in environmental variables. See: <https://www.htslib.org/doc/htslib-s3-plugin.html>
  - `gss`: Not tested. Please provide feedback if it works.
  - Note that the custom `bai` path (`-i`) is not supported for remote use due to [rust-htslib](https://github.com/rust-bio/rust-htslib) API limitation.

## Key bindings

Quit: `:q`

Normal mode

| Command  | Notes | Example |
|---------|-------------|---------|
| `:` | Enter command mode | |
| `h/j/k/l` | Move left / down / up / right | |
| `y/p` | Fast move left / right | |
| `w/b` | Beginning of the next / previous exon |  |
| `e/ge` | End of the next / previous exon | |
| `W/B` | Beginning of the next / previous gene | |
| `E/gE` | End of the next / previous gene | |
| `z/o` | Zoom in / out | |
| `{/}` | Fast move up / down | |
| `_number_` + `_movement_` | Move by `_number_` steps | `20h`: left by 20 bases |

Command mode

| Command | Notes | Example |
|---------|-------------|---------|
| `:q` | Quit | |
| `:h` | Help | |
| `:_pos_` | Go to position on same contig | `:1000` |
| `:_contig_:_pos_` | Go to position on specific contig | `:17:7572659` |
| `:_gene_` | Go to `_gene_` | `:KRAS` |
| `:ls` / `:contigs` | List contigs (`j/k` to select, `Esc`, `Enter`) | |
| `Esc` | Switch to normal mode | |

Filter / sort reads in command mode:
```
# Restore
CLEAR

# Filter by base at position 123
FILTER BASE(123)=C
```

## Compare TGV and Vim concepts

| Command | TGV | Vim | Notes |
|-------|-----|--|--|
| `h/l` | Horizontal movement | Character | |
| `y/p` | Fast horizontal movement | NA | `y/p` do different things in Vim |
| `w/b/e/ge` | Exon | word | |
| `W/B/E/gE` | Gene | WORD | |
| `j/k` | Alignment track | Line | |
| `z/o` | Zoom | NA | `o` does a different thing in Vim |
| `{/}` | Fast vertical movement | Paragraph | |
