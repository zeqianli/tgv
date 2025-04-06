
# TGV key bindings

Quit: `:q`

Normal mode

| Command  | Notes | Example |
|---------|-------------|---------|
| `:` | Enter command mode | |
| `h/j/k/l` | Move left / down / up / right | |
| `y/p` | Fast move left / right | |
| `w/b` | Beginning of the next / last exon |  |
| `e/ge` | End of the next / last exon | |
| `W/B` | Begining of the next / last gene | |
| `E/gE` | End of the next / last gene | |
| `z/o` | Zoom in / out | |
| `_number_` + `_movement_` | Move by `_number_` steps | `20h`: left by 20 bases|

Command mode:

|Command |Notes| Example|
|---------|-------------|---------|
| `:q` | Quit | |
| `:h` | Help | |
| `:_pos_` | Go to position on same contig | `:1000` |
| `:_contig_:_pos_` | Go to position on specific contig | `:17:7572659` |
| `:_gene_` | Go to `_gene_`.| `:KRAS`|
| `Esc` | Switch to Normal Mode | |

## Compare TGV and Vim concepts

|Command|TGV|Vim|Notes|
|-------|-----|--|--|
|`h/l`|Horizontal movement|Character ||
|`y/p`|Fast horizontal movement|NA|`y/p` do different things in Vim|
|`w/b/e/ge`|Exon|word||
| `W/B/E/gE` | Gene |WORD||
|`j/k`|Alignment track|Line||
|`z/o`| Zoom | NA |`o` does a different thing in Vim.|

See `ROADMAP.md` for future key bindings ideas.
