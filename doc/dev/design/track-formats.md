# Design: more track file formats

Status: steps 1 and 2 (indexed VCF, BCF, indexed BED, and bigBed) are implemented; steps 3 and 4 are proposed.

## Context

TGV reads BAM, CRAM, and remote BAM alignments; plain and indexed VCF and BCF files; plain and indexed BED and bigBed files; and indexed FASTA and 2bit references. Variant and BED tracks load by region, like alignments. IGV and the UCSC Genome Browser also read signal tracks and user gene annotations. This design adds:

- **bigWig** (`.bw`, `.bigwig`): signal tracks, such as ChIP-seq, ATAC-seq, and conservation scores.
- **GFF3** (`.gff3`, `.gff`, and `.gff3.gz` with `.tbi`): user gene annotations.

The new formats fit the existing design wherever possible. The design adds no new framework or groundwork phase. Each format reuses a pattern that already exists, and the few shared pieces arrive with the first format that needs them.

## Patterns to reuse

| Need | Existing pattern |
|---|---|
| Several formats in one track kind | `AlignmentRepositoryEnum`, `VariantRepositoryEnum`, and `BedRepositoryEnum`, each dispatching to one struct per format |
| Region-based loading | `GeneTable`, `VariantTable`, and `BedTable`: `contig_index` and the complete bounds, with `has_complete_data` |
| A width limit on loading | `AlignmentView::MAX_ZOOM_TO_DISPLAY_*`, including `MAX_ZOOM_TO_DISPLAY_INDEXED_FEATURES`, and the filter in `App::load_data` |
| Contig names from files | Each repository's `read_contigs`, which `Repository::new` adds with `ContigSource::Annotation` |
| Row ID meaning for region data | The `genes` SQL table: "Scope: region", with row IDs within the loaded data |
| Signal rendering | `StackedSparkline`, `NINE_LEVELS`, and the y-axis label in `rendering/coverage.rs` |
| Gene-model rendering | `render_track` and `query_segments` |
| Input paths | `FilePath::VariantPath(String)` and `FilePath::BedPath(String)` stay as `String`, and the repository chooses the reader. |
| Whole-file parsing for plain files | `PlainBed` and `PlainVcf`: parse once in `open` into one frame with a contig-name column, then filter by contig on read |

## Step 3: bigWig

bigWig needs a new signal kind. Like the existing kinds, it goes through every layer.

### Wiring

- **Settings:** `FilePath::SignalPath(String)`, and `.bw` and `.bigwig` in `classify_and_build_tracks`.
- **Repository:**
  - Add `RepositoryFileIndex::Signal(usize)` and `Repository.signal_repositories: Vec<SignalRepository>`, where `SignalRepository` wraps `BigWigRead`.
  - Add `Signal` arms to `Repository::file_path` and the contig sync. The bigWig chromosome list includes lengths.
- **Registry:** `TrackRegistry::signal_index`.
- **State:** `signals: Vec<SignalTable>`, and `add_signal_track()` called where the app adds the other tracks.
- **Session files:** read and write signal tracks in `tgv/src/session.rs`.

### Table (`gv-core/src/signal.rs`)

```rust
pub struct SignalTable {
    pub data: DataFrame,
    pub contig_index: usize,
    data_complete_left_bound: u64,
    data_complete_right_bound: u64,
    /// The bases summarized by each row; 1 for raw values.
    pub bases_per_row: u64,
}
```

`SignalSchema` has the `CONTIG_INDEX`, `START`, `END`, `MEAN`, `MIN`, and `MAX` columns.

### Resolution

- **Request:** `LoadRequest` gets `bases_per_column: u64`. The viewer passes `alignment_view.zoom`. The server passes 1, which reads raw values, and its `MAX_QUERY_WIDTH` of 100,000 bases keeps that small.
- **Zoom level:** read from the coarsest bigWig zoom level whose `reduction_level` is at most `bases_per_column`, through `get_zoom_interval`. When no zoom level qualifies, read raw values through `get_interval`, with `MIN`, `MEAN`, and `MAX` all equal to the value.
- **Reuse:** `has_complete_data(region, bases_per_column)` requires the region bounds and a `bases_per_row` within about four times the requested resolution. Without the resolution check, zooming out would reuse raw values and load too many rows, and zooming in would show coarse summaries.

### Rendering

- **Layout:** `AreaType::Signal(TrackId)`, with a desired height of 4.
- **Renderer (`rendering/signal.rs`):**
  - Bin the rows into screen columns by `MAX`, and draw them with `StackedSparkline` and a `[0-max]` label, like coverage.
  - Move `round_up_max_coverage` and `displayed_coverage_bounds` out of `coverage.rs` so both renderers can use them.
  - The first version clamps negative values at 0 and shows the real minimum in the label. A centered baseline for signed data, such as phyloP or log fold-change, can come later.

### Server and docs

- Add a `signal` SQL table keyed by `track_id`, `contig_index`, and `start`, with "Scope: region", and add `TrackKind::Signal`.
- Update `doc/src/server/query.md`, `doc/src/server/dataset.md`, and the CLI help.

## Step 4: GFF3

GFF3 needs a new annotation kind. Its data is gene models, so it produces a `GeneTable` and reuses the gene renderer.

### Wiring

- **Cargo:** add the `gff` feature to `noodles`.
- **Settings:** `FilePath::AnnotationPath(String)`, for `.gff3`, `.gff`, and `.gff3.gz`.
- **Repository:** `RepositoryFileIndex::Annotation(usize)` and `Repository.annotation_repositories`.
- **Registry, state, and session files:** `TrackRegistry::annotation_index`, `State.annotations: Vec<GeneTable>`, and the session file entries, as in step 3.
- **Layout:** `AreaType::Annotation(TrackId)`, with a desired height of 2.

### Parser (`gv-core/src/gff.rs`)

- **Assembly:**
  - Group records by their `Parent` attribute into transcripts.
  - Exon children become `EXON_STARTS` and `EXON_ENDS`.
  - The span of CDS children becomes `CDS_START` and `CDS_END`.
  - The name comes from the `gene_name` or `Name` attribute.
  - GFF3 coordinates are already one-based and inclusive.
- **Loading by contig:** a window query would return a transcript that overlaps the window but drop its exons outside the window. Loading whole contigs avoids incomplete gene models.
  - **Plain files:** parse the file once into a DataFrame for all contigs, then build a `GeneTable::from_data(…, contig_index, (1, contig_length))` for each contig.
  - **bgzipped files with a `.tbi` index:** query the whole contig.
- **Loading:** `ensure_loaded` uses the existing `GeneTable::has_complete_data`.

### Rendering

`render_track` in `rendering/track.rs` takes a `&GeneTable` instead of `&State`. Both the reference gene track and the annotation tracks call it.

### Server and docs

- Add an `annotations` SQL table with the gene columns and `track_id`, with "Scope: region".
- Update the server docs and the CLI help.

## Order of work

1. **Step 3** adds a track kind, so it changes `layout.rs`, `track_registry.rs`, `app.rs`, `session.rs`, and `gv-session`.
2. **Step 4** reuses the wiring from step 3 and changes `render_track`.

Each step is one change that includes its documentation updates.

## Open questions

- **Test data:** small fixtures would cover each new reader: a `.bw` and a `.gff3`. The existing indexed VCF, BCF, and BED tests build their fixtures at test time from `simple.vcf` and `simple.bed`; bigWig and GFF3 tests could do the same, or use small checked-in files.
