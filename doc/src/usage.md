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
| `:w` | Save the active session | |
| `:wq` | Save the active session and quit | |
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
