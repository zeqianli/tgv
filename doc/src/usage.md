# Usage

```sh
tgv [options] [files...]
```

For example, `tgv -g hg38 -r chr20:88108 /data/simple.bam /data/simple.vcf /data/simple.bed`.

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
- CRAM is not supported as a CLI input format. Configure CRAM tracks in a [session file](./session.md).

## Command-line options

| Option | Description |
|---|---|
| `-r`, `--region` _region_ | Starting region: `contig:position` (such as `12:25398142`) or a gene (such as `TP53`). Ranges are not accepted here; use `/` search in the app. |
| `-g`, `--reference` _reference_ | Reference genome. Defaults to `hg38`. Accepts UCSC assembly names and accessions (see `tgv list`), or a custom FASTA or 2bit file. |
| `--no-reference` | Don't display a reference genome. Requires at least one input file. |
| `--offline` | Always use the local cache. Quit if the cache isn't available. |
| `--online` | Always use the UCSC database or API. |
| `--host us\|eu\|auto` | UCSC mirror. Defaults to `auto`, which picks a mirror from the local time zone. |
| `--cache-dir` _dir_ | Cache directory. Defaults to `~/.tgv`. |
| `--phred 33\|64\|auto` | Base-quality encoding of the alignment files. Defaults to `auto`. |
| `--resume` _session_ | Resume a saved [session](./session.md), by name or path. |
| `--debug` | Write detailed logs. Intended for development. |

Subcommands:

- `tgv list` lists common reference genome names. `tgv list --all` lists all UCSC assemblies.
- `tgv download <reference>` downloads reference data to the cache, for use with `--offline`.
- `tgv mcp` (or `tgv serve`) runs the [local MCP server](./server.md).

Each run writes a log file to `~/.tgv/<timestamp>.log`.

## Key bindings

Quit: `:q`

tgv has two vim-like modes:

- Normal mode moves along the genome. Word motions (`w`, `b`, `e`, and `ge`) move between the
  focused track's features, and `j/k` scroll the focused track when it is an alignment.
- Insert mode, reached with `i` or `a`, edits the focused BED track at the cursor. `Esc`
  returns to normal mode.

One track has the focus at a time, like a window in vim. The reference (the sequence and gene
rows) has the focus at startup. Without sequence or gene rows, the first file track has it. Move the focus with `Ctrl-j` and `Ctrl-k`, or with vim's window
keys (`Ctrl-w` followed by `j`, `k`, `w`, `W`, `t`, or `b`), in either mode.

The focused track's section in the sidebar is filled: gray in normal mode, and green in insert
mode. In insert mode, the command row shows the mode and the track, such as
`-- INSERT -- peaks.bed [+]`. `[+]` marks unsaved changes. When the sidebar is hidden, normal
mode shows the focused track's name at the right of the command row.

### Normal mode

| Command  | Notes | Example |
|---------|-------------|---------|
| `:` | Enter command mode | |
| `/` | Enter search mode | |
| `h/j/k/l` or arrow keys | Move left / down / up / right | |
| `H/J/K/L` | Move left / down / up / right faster | |
| `w/b` | Beginning of the next / previous feature of the focused track | |
| `e/ge` | End of the next / previous feature of the focused track | |
| `W/B/E/gE` | Like `w/b/e/ge`, by genes on the reference and alignments | |
| `gg/gG` | Scroll to the first / last alignment row | |
| `Ctrl-j` / `Ctrl-k` | Focus the next / previous track. Stops at the first and last tracks. | `3 Ctrl-j` |
| `Ctrl-w j/k` | The same as `Ctrl-j` / `Ctrl-k` | |
| `Ctrl-w w/W` | Focus the next / previous track, wrapping around | |
| `Ctrl-w t/b` | Focus the first / last track | |
| `i` / `a` | Enter insert mode on the focused BED track. `a` first moves right by one column. | |
| `z/o` | Zoom in / out | |
| `s` | Toggle the sidebar | |
| `u` / `Ctrl-r` | Go back / forward through jumps: searches, contig and gene jumps, cytoband clicks, and agent moves | |
| `Esc` | Clear a pending count or `g` | |
| `_number_` + `_movement_` | Move by `_number_` steps | `20h`: left by 20 bases |

What the word motions move between depends on the focused track:

| Focus | `w/b/e/ge` | `W/B/E/gE` |
|---|---|---|
| Reference | Exons | Genes |
| Alignment | Exons | Genes |
| BED | Features | Features |
| VCF | Variants. `e/ge` go to the variant's last reference base. | Variants |

When the focused contig has no more features, the search goes on to the next contig, or the
previous one for `b/ge`.

### Insert mode

Insert mode edits BED files that load whole: `.bed` files and `.bed.gz` files without an
index. Indexed `.bed.gz` and bigBed files are read-only. The cursor is the current position,
drawn as a reversed cell in the BED track being edited. Other tracks don't show it. When zoomed out, one column covers `zoom` bases, and edits apply to all of
them.

Insert mode types no text, so it takes vim's editing keys. Counts come first, as in vim:
`3x`, `5Space`, `d3l`.

| Command | Notes |
|---------|-------------|
| `Space` | Add the bases under the cursor, then move right by one column. Hold it to paint a run. |
| `Backspace` | Move left by one column, then remove the bases there |
| `Shift-Backspace` | Move left by one column, then add the bases there: painting to the left |
| `x` / `Delete` | Remove the bases under the cursor |
| `X` | Remove the bases left of the cursor, and move onto them |
| `d` + _motion_ | Remove the bases from the cursor to where the motion goes. `d3l` removes three columns from the cursor; `de` removes through the end of the feature; `dw` removes up to the next feature. |
| `u` / `Ctrl-r` | Undo / redo the track's last edit step |
| `h/l`, `H/L`, `w/b/e/ge`, `W/B/E/gE`, `z/o`, arrow keys | Move and zoom as in normal mode. `w/b/e/ge` move between the track's features. |
| `Ctrl-j` / `Ctrl-k`, `Ctrl-w` keys | Go to normal mode and move the focus |
| `:` / `/` | Enter command or search mode, then return to insert mode |
| `Esc` | Clear a pending count or `d`, or go to normal mode |

- Every command is one undo step, such as `3x` or `de`. A run of `Space`, `Backspace`, and
  `Shift-Backspace` presses is one step, like a run of typed text in vim.
- Insert mode and normal mode keep separate histories. `u` in normal mode goes back through
  jumps, and `u` in insert mode undoes the track's edits.
- Terminals only tell `Shift-Backspace` from `Backspace` with the kitty keyboard protocol,
  which kitty, WezTerm, Ghostty, foot, and recent iTerm2 support. In other terminals, such as
  Terminal.app, it arrives as `Backspace` and removes bases. tgv says so the first time
  insert mode starts.
- Adding bases leaves covered bases alone. An uncovered run that touches the end of a
  feature on its left extends that feature; otherwise, one that touches the start of a
  feature on its right extends that one. Otherwise, the run becomes a new feature with no
  name, score, or strand. Features never merge, so their names and extra columns are kept.
- Removing bases deletes features inside the range, trims features that cross one end of
  it, and splits features that contain it. The right part of a split copies the name,
  score, strand, and extra columns.

### Command mode

| Command | Notes | Example |
|---------|-------------|---------|
| `:q` | Quit. If a track has unsaved changes, tgv stays open and names it. | |
| `:q!` | Quit without writing tracks | |
| `:w` | Write the focused BED track to its file | |
| `:w _path_` | Write an untitled track to the path, which becomes its file. For a track with a file, write a copy; the track keeps its own file. | `:w ./peaks.bed` |
| `:wa` | Write every modified BED track. Untitled tracks are skipped and named. | |
| `:wq [_path_]` | `:w`, then `:q` | |
| `:wqa` / `:xa` | `:wa`, then `:q` | |
| `:ws _name_` | Save the session to `~/.tgv/sessions/_name_.toml`, or to a path | `:ws brca`, `:ws ./brca.toml` |
| `:ws` | Save to the active session (from `--resume` or the last `:ws _name_`) | |
| `:wsq [_name_]` | Save the session like `:ws` and quit. Refuses while tracks have unsaved changes, like `:q`. | `:wsq brca` |
| `:new` | Create an untitled BED track below the focused track, and focus it | |
| `:e _path_…` / `:open _path_…` | Add file tracks | `:e /data/simple.vcf /data/simple.bed` |
| `:ls` / `:contigs` | List contigs (`j/k` or arrow keys to select, `{`/`}` to move by 30, `Esc`, `Enter`) | |
| `:paired` | Show all alignment tracks as read pairs, clearing their sort and filter | |
| `:clear` / `:default` | Restore the default display on all alignment tracks | |
| `Esc` | Return to normal or insert mode, whichever you came from | |

### Search mode

| Command | Notes | Example |
|---------|-------------|---------|
| `/_pos_` | Go to position on same contig | `/1000` |
| `/_contig_:_pos_` | Go to position on specific contig | `/17:7572659` |
| `/_contig_:_start_-_end_` | Show a region, zoomed to fit. Commas are allowed. | `/chr1:1,000-2,000`, `/1000-2000` |
| `/_gene_` | Go to `_gene_` | `/KRAS` |
| `Esc` | Return to normal or insert mode, whichever you came from | |

## Mouse and sidebar

The sidebar starts open and shows the reference, current position, filenames,
and alignment depths alongside their tracks. Drag its vertical divider to
change its width. Drag a line between alignment tracks to resize the adjacent
alignments. Filenames wrap within their track sections, which are separated by
underscore lines. When the sidebar is hidden, its labels and alignment depths
are hidden too. The command and message rows remain in the main track column.
An alignment's display options, such as `Paired` or `Sorted by base at 88108`,
are listed above its depth.

Click a track, or its section in the sidebar, to focus it. Click the sequence or the genes to
focus the reference. Clicking another track in insert mode returns to normal mode. Hovering
doesn't move the focus.

Scroll the mouse wheel over an alignment to scroll its reads, whatever has focus. Hold Shift while
scrolling, or scroll horizontally on a trackpad, to pan left and right. Drag an
alignment track to pan in both directions, or drag a variant or BED track to
pan left and right.

Click the cytoband to go to that part of the contig, keeping the zoom.

Hover over a track for details on the message line: genes and exons, reference
bases, coverage, variants, BED features, and reads. Over a read, it shows the
position, read name, CIGAR, and MAPQ, plus the read's base and base quality
there when each column shows one base. Click a read to see its SAM record;
press `Esc` to close it. Other keys and clicks are ignored while it is open.

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
file's section in the sidebar, a BED or VCF track, or an alignment track also
offers items for that track:

- Copy _file name_: remember a BED track for pasting. Only BED files that load
  whole can be copied.
- Paste _file name_ below: create an untitled BED track below the clicked
  track, with the copied track's features as they are when you paste,
  including unsaved edits.
- Remove _file name_: remove the track. For a track with unsaved changes, the
  item reads "Remove _file name_ (discard changes)".
- Insert track above / below: create an untitled BED track next to the track,
  and focus it. Only the sidebar's menu has these.

Click outside the menu or press `Esc` to close it.

## Adding and removing files

Add files while tgv runs with `:e` or `:open`, followed by one or more paths.
Paths use shell quoting, such as `:e 'my reads.bam'`, and `~` stands for the
home directory. New tracks go after the existing ones, in command-line order,
and the other tracks keep their heights. tgv shows "Opening _file_…" while it
opens them. If any file can't be opened, none are added, and the message says
why, for example `/data/simple.bam needs an index: /data/simple.bam.bai` or
`Not a file: /data/simple.bam`.

Drag files from a file manager into the terminal to add them. Terminals such
as Ghostty, iTerm2, kitty, WezTerm, and Terminal.app paste the dropped paths,
and in normal mode tgv opens them like `:e`. A paste is handled the same way,
so pasting paths also opens them, but `~` is not expanded in pasted paths. In
command and search mode, a paste is inserted into the command line instead, so
you can paste a locus after `/`.

- The terminal doesn't report where a file was dropped, so dropped tracks are
  added at the end.
- Over SSH, a dropped path names a file on your local machine, so tgv reports
  it as not a file.

Remove a track from its right-click menu. `:ws` saves the current files in the session,
including the ones you added and leaving out the ones you removed.

## Editing BED tracks

Edit a BED track in insert mode, and write it with `:w`. Tracks with unsaved changes show
`● ` before their name in the sidebar, and `[+]` in the mode line. Undoing back to the
written state clears the marker. Tracks created with `:new` or pasted from the track menu are
named `untitled-1`, `untitled-2`, and so on, and stay marked until they are first written
with `:w _path_`.

A written file keeps its layout:

- Rows keep their file order. New features and the right parts of split features go in
  sorted position if the file was sorted when it loaded, and at the end otherwise. New
  tracks are always written sorted.
- Every row has the file's column count, at least three. Missing names, scores, and
  strands are written as `.`. Columns 7 and up are written back as they were.
- Comment, `track`, and `browser` lines are written back at the top, in their original
  order.
- Contig names are the file's own. A contig the file didn't have gets the reference's name.
- The file is written to a temporary file in the same directory and renamed over the
  target, so a crash can't leave half a file. Paths ending in `.gz` are written with bgzip.

`:ws` saves edited tracks by path, with a note that the session points to the file on disk.
Untitled tracks are left out, and tgv says so. Agents see unsaved edits in `inspect_interval`
and in the `query` tool's `bed` table.

## Compare TGV and Vim concepts

| Command | TGV | Vim | Notes |
|-------|-----|--|--|
| `h/l` | Horizontal movement | Character | |
| `w/b/e/ge` | Feature of the focused track: exon, BED feature, or variant | word | |
| `W/B/E/gE` | Gene on the reference and alignments | WORD | |
| `j/k` | Alignment track | Line | |
| `Ctrl-j/k`, `Ctrl-w` | Move the focus between tracks | Move between windows | `Ctrl-j/k` are a common remap of `Ctrl-w j/k` |
| `z/o` | Zoom | NA | `o` does a different thing in Vim |
| `x/X`, `d` | Remove bases in insert mode | Delete characters / delete | |
| `u/Ctrl-r` | Jump history in normal mode, edits in insert mode | Undo / redo | In normal mode, like `Ctrl-o` / `Ctrl-i` in Vim |
| `:w` / `:ws` | Write the BED track / save the session | Write the buffer | |
| `:new` | New BED track | New window | |
