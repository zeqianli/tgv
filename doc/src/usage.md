# Usage

## Supported formats

- BAM (indexed and sorted). A `.bai` file is needed.
  - Local paths and `s3://` URLs are supported.
  - The index path is inferred as `<bam>.bai`. There is no separate CLI option for a custom index path.
  - For `s3://` BAMs, place the `.bai` object at the inferred path and configure S3 credentials in the environment.
- VCF (`.vcf` and `.vcf.gz`) and BED (`.bed` and `.bed.gz`) files are supported as positional input files.
- Custom FASTA and 2bit reference genomes are passed with `-g` / `--reference`, not as positional track files. FASTA references require a `.fai` index beside the FASTA file.
- CRAM is not supported as a CLI input format. Configure CRAM tracks in a session file.

## Key bindings

Quit: `:q`

Normal mode

| Command  | Notes | Example |
|---------|-------------|---------|
| `:` | Enter command mode | |
| `h/j/k/l` | Move left / down / up / right | |
| `H/J/K/L` | Move left / down / up / right faster | |
| `w/b` | Beginning of the next / previous exon |  |
| `e/ge` | End of the next / previous exon | |
| `W/B` | Beginning of the next / previous gene | |
| `E/gE` | End of the next / previous gene | |
| `z/o` | Zoom in / out | |
| `s` | Toggle the sidebar | |
| `_number_` + `_movement_` | Move by `_number_` steps | `20h`: left by 20 bases |

The sidebar starts open and shows the reference, current position, filenames,
and alignment depths alongside their tracks. Drag its vertical divider to
change its width. Drag a line between alignment tracks to resize the adjacent
alignments. Filenames wrap within their track sections, which are separated by
underscore lines. When the sidebar is hidden, its labels and alignment depths
are hidden too. The command and message rows remain in the main track column.

Command mode

| Command | Notes | Example |
|---------|-------------|---------|
| `:q` | Quit | |
| `:w _name_` | Save the session to `~/.tgv/sessions/_name_.toml`, or to a path | `:w brca`, `:w ./brca.toml` |
| `:w` | Save to the active session (from `--resume` or the last `:w _name_`) | |
| `:wq [_name_]` | Save the session like `:w` and quit | `:wq brca` |
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
| `w/b/e/ge` | Exon | word | |
| `W/B/E/gE` | Gene | WORD | |
| `j/k` | Alignment track | Line | |
| `z/o` | Zoom | NA | `o` does a different thing in Vim |
