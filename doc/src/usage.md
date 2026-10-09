# Usage

## Supported formats

- BAM (indexed and sorted). A `.bai` file is needed.
  - Local paths and `s3://` URLs are supported.
  - The index path is inferred as `<bam>.bai`. There is no separate CLI option for a custom index path.
  - For `s3://` BAMs, place the `.bai` object at the inferred path and configure S3 credentials in the environment.
- Variant files: VCF (`.vcf`, `.vcf.gz`, and `.vcf.bgz`) and BCF (`.bcf`).
  - A bgzipped VCF with a `.tbi` or `.csi` index next to it, and a BCF with a `.csi` index, load by region, so large files such as gnomAD or ClinVar stay fast. A BCF without a `.csi` index is an error.
  - Other VCF files load whole the first time they're shown.
- Feature files: BED (`.bed` and `.bed.gz`) and bigBed (`.bb` and `.bigbed`).
  - A bgzipped BED with a `.tbi` or `.csi` index, and any bigBed file, load by region. Other BED files load whole the first time they're shown.
  - BED tracks show each feature's name when it fits, and its strand as `›` or `‹` chevrons.
- Indexed variant and BED tracks show `zoom in to view …` when zoomed out beyond 100 bases per column, instead of loading a large region.
- Custom FASTA and 2bit reference genomes are passed with `-g` / `--reference`, not as positional track files. FASTA references require a `.fai` index beside the FASTA file.
- CRAM is not supported as a CLI input format. Configure CRAM tracks in a session file.

## Key bindings

Quit: `:q`

Normal mode

| Command  | Notes | Example |
|---------|-------------|---------|
| `:` | Enter command mode | |
| `/` | Enter search mode | |
| `h/j/k/l` | Move left / down / up / right | |
| `H/J/K/L` | Move left / down / up / right faster | |
| `w/b` | Beginning of the next / previous exon |  |
| `e/ge` | End of the next / previous exon | |
| `W/B` | Beginning of the next / previous gene | |
| `E/gE` | End of the next / previous gene | |
| `z/o` | Zoom in / out | |
| `s` | Toggle the sidebar | |
| `u` / `Ctrl-r` | Go back / forward through jumps: searches, contig and gene jumps, and agent moves | |
| `_number_` + `_movement_` | Move by `_number_` steps | `20h`: left by 20 bases |

The sidebar starts open and shows the reference, current position, filenames,
and alignment depths alongside their tracks. Drag its vertical divider to
change its width. Drag a line between alignment tracks to resize the adjacent
alignments. Filenames wrap within their track sections, which are separated by
underscore lines. When the sidebar is hidden, its labels and alignment depths
are hidden too. The command and message rows remain in the main track column.
An alignment's display options, such as `Paired` or `Sorted by base at 88108`,
are listed above its depth.

Scroll the mouse wheel over an alignment to scroll its reads. Hold Shift while
scrolling, or scroll horizontally on a trackpad, to pan left and right. Drag a
track to pan in both directions.

Click the cytoband to go to that part of the contig, keeping the zoom.

Hover over a track for details on the message line. Over a read, it shows the
position, read name, the read's base and base quality there, CIGAR, and MAPQ.
Click a read to see its SAM record; press `Esc` to close it. Other keys and
clicks are ignored while it is open.

Alignment tracks follow IGV's defaults: duplicate and QC-fail reads are hidden,
reads with MAPQ 0 are drawn in a darker gray, and mismatched bases with a base
quality below 20 are dimmed.

Right-click an alignment track for a menu. It applies to the clicked track only:

- Sort at _pos_: sort the reads covering the clicked position by base, strand,
  start, mapping quality (highest first), insert size (largest first), or read
  name. Each sorted read gets its own row.
- Filter by base at _pos_: show only the reads with the chosen base, or a soft
  clip, at the clicked position. The submenu lists the choices present there,
  with read counts.
- View as pairs: show mates on one row.
- Reset display options.

The base items need one base per column, so they are disabled when zoomed out.
Right-click the sidebar for an option to hide it (also `s`). Right-clicking a
file's section in the sidebar also offers "Remove _file name_", which removes
that track. Click outside the menu or press `Esc` to close it.

### Adding and removing files

Add files while tgv runs with `:e` or `:open`, followed by one or more paths.
Paths use shell quoting, such as `:e 'my reads.bam'`, and `~` stands for the
home directory. New tracks go after the existing ones, in command-line order,
and the other tracks keep their heights. tgv shows "Opening _file_…" while it
opens them. If any file can't be opened, none are added, and the message says
why, for example `simple.bam needs an index: simple.bam.bai` or
`Not a file: simple.bam`.

Drag files from a file manager into the terminal to add them. Terminals such
as Ghostty, iTerm2, kitty, WezTerm, and Terminal.app paste the dropped paths,
and in normal mode tgv opens them like `:e`. A paste is handled the same way,
so pasting paths also opens them. In command and search mode, a paste is
inserted into the command line instead, so you can paste a locus after `/`.

- The terminal doesn't report where a file was dropped, so dropped tracks are
  added at the end.
- Over SSH, a dropped path names a file on your local machine, so tgv reports
  it as not a file.

Remove a track by right-clicking its section in the sidebar. `:w` saves the
current files, including the ones you added and leaving out the ones you
removed.

Command mode

| Command | Notes | Example |
|---------|-------------|---------|
| `:q` | Quit | |
| `:w _name_` | Save the session to `~/.tgv/sessions/_name_.toml`, or to a path | `:w brca`, `:w ./brca.toml` |
| `:w` | Save to the active session (from `--resume` or the last `:w _name_`) | |
| `:wq [_name_]` | Save the session like `:w` and quit | `:wq brca` |
| `:e _path_…` / `:open _path_…` | Add file tracks | `:e /data/simple.vcf /data/simple.bed` |
| `:ls` / `:contigs` | List contigs (`j/k` to select, `Esc`, `Enter`) | |
| `:paired` | Show alignments as read pairs | |
| `:clear` / `:default` | Restore the default alignment display | |
| `Esc` | Switch to normal mode | |

Search mode

| Command | Notes | Example |
|---------|-------------|---------|
| `/_pos_` | Go to position on same contig | `/1000` |
| `/_contig_:_pos_` | Go to position on specific contig | `/17:7572659` |
| `/_contig_:_start_-_end_` | Show a region, zoomed to fit. Commas are allowed. | `/chr1:1,000-2,000`, `/1000-2000` |
| `/_gene_` | Go to `_gene_` | `/KRAS` |
| `Esc` | Switch to normal mode | |

## Compare TGV and Vim concepts

| Command | TGV | Vim | Notes |
|-------|-----|--|--|
| `h/l` | Horizontal movement | Character | |
| `w/b/e/ge` | Exon | word | |
| `W/B/E/gE` | Gene | WORD | |
| `j/k` | Alignment track | Line | |
| `z/o` | Zoom | NA | `o` does a different thing in Vim |
| `u/Ctrl-r` | Jump history | Undo / redo | Like `Ctrl-o` / `Ctrl-i` in Vim |
