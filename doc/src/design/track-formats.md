# Design: more track file formats

Status: proposed, not implemented.

## Context

TGV reads BAM, CRAM, and remote BAM alignments, plus VCF, BED, indexed FASTA, and 2bit files. IGV and the UCSC Genome Browser also read signal tracks, binary and indexed feature files, and user gene annotations. This design adds:

- **bigWig** (`.bw`, `.bigwig`): signal tracks, such as ChIP-seq, ATAC-seq, and conservation scores.
- **bigBed** (`.bb`, `.bigbed`): indexed features with zoom levels.
- **Indexed BED** (`.bed.gz` with `.tbi` or `.csi`).
- **Indexed VCF** (`.vcf.gz` with `.tbi` or `.csi`) and **BCF** (`.bcf` with `.csi`).
- **GFF3** (`.gff3`, `.gff`, and `.gff3.gz` with `.tbi`): user gene annotations.

Today, VCF and BED files load whole, once, which doesn't scale to files like gnomAD or ClinVar. The indexed formats load by region instead.

The new formats fit the existing design wherever possible. The design adds no new framework or groundwork phase. Each format reuses a pattern that already exists, and the few shared pieces arrive with the first format that needs them.

## Patterns to reuse

| Need | Existing pattern |
|---|---|
| Several formats in one track kind | `AlignmentRepositoryEnum` (`Bam`, `RemoteBam`, and `Cram`) |
| Region-based loading | `GeneTable`: `contig_index` and the complete bounds, with `has_complete_data` |
| A width limit on loading | `AlignmentView::MAX_ZOOM_TO_DISPLAY_*` and the filter in `App::load_data` |
| Contig names from files | `update_or_add_contig(…, ContigSource::Annotation)`, plus the commented-out sync block in `Repository::new` |
| Row ID meaning for region data | The `genes` SQL table: "Scope: region", with row IDs within the loaded data |
| Signal rendering | `StackedSparkline`, `NINE_LEVELS`, and the y-axis label in `rendering/coverage.rs` |
| Gene-model rendering | `render_track` and `query_segments` |
| Input paths | `FilePath::VariantPath(String)` and `FilePath::BedPath(String)` stay as `String`. The repository chooses the reader. |

## Step 1: indexed VCF and BCF

Indexed VCF and BCF files belong to the existing variant kind.

### Core (`gv-core`)

- **Cargo:** add the `tabix`, `csi`, and `bcf` features to `noodles` in the root `Cargo.toml`.
- **Repository enum:** `VariantRepository` becomes `VariantRepositoryEnum { Vcf, IndexedVcf, Bcf }`.
  - `new(path)` chooses the variant from the extension and whether a `.tbi` or `.csi` file exists next to the data file. A `.bcf` without a `.csi` is an error.
  - The enum provides `is_indexed()`, `read_contigs()`, and the region read.
- **Readers:**
  - Indexed VCF uses `vcf::io::indexed_reader` and `query(&header, &region)`.
  - BCF uses `bcf::io::indexed_reader`. Its header is a `vcf::Header`, so `VariantTable.header` keeps its type.
- **Records:** BCF records aren't `vcf::Record`, so `VariantTable.records` becomes `Vec<vcf::variant::RecordBuf>`, the owned type that both formats convert to. `add_records` reads from the `RecordBuf` accessors.
- **Loaded bounds:** `VariantTable` gets `contig_index` and the complete left and right bounds, with `has_complete_data`, following `GeneTable`.
- **File contig names:**
  - Each repository keeps the contig names from its own header or index.
  - A small shared function in `contig_header.rs` returns the first of `contig.name` and `contig.aliases` that the file contains. The indexed readers query with that name instead of `get_alignment_name`.
  - `ContigHeader` doesn't change.
- **Contig sync:** uncomment the variant block in `Repository::new` and pass `ContigSource::Annotation`, so VCF and BCF headers contribute contigs and lengths.
- **Loading:** the variant arm of `State::ensure_loaded` skips the load when `variant_loaded[index]` is set or `variants[index].has_complete_data(region)` is true.
  - Plain files load whole, as they do today, and set `variant_loaded`.
  - Indexed files load `cache.track_region(region)` and set the bounds.

### Viewer (`tgv`)

- **Zoom gate:** add `AlignmentView::MAX_ZOOM_TO_DISPLAY_INDEXED_FEATURES`. `App::load_data` drops indexed variant and BED tracks above that zoom, through the same filter that drops alignments.
- **Rendering (`rendering/variants.rs`):**
  - If the table neither holds the whole file nor covers the region, draw a dim `zoom in to view variants` line. This needs no extra flag.
  - Alternate colors by on-screen order instead of `row_id % 2`, so colors don't change when the region reloads.
  - Optionally, color by allele type (SNV, insertion, or deletion) and dim sites that don't pass filters.
- **Mouse:** the variant hover message in `mouse.rs` reads the contig name through the `RecordBuf` accessor.

### Server and docs

- **Row IDs:** `row_id` covers the whole file for plain files and the loaded region for indexed files. Update `VariantSchema::column_docs` and the `variants` table in `doc/src/server/query.md` to say so.
- **Help text:** add the new extensions to the supported formats in the CLI help and the error message in `classify_and_build_tracks`.

## Step 2: indexed BED and bigBed

Indexed BED and bigBed files belong to the existing BED kind.

### Core (`gv-core`)

- **Repository enum:** `BedRepository` becomes `BedRepositoryEnum { Bed, IndexedBed, BigBed }`, following step 1.
- **Indexed BED:** query through noodles `csi` or `tabix`, and parse each line with the existing BED reader. Check whether noodles 0.108 provides a dedicated indexed BED reader first.
- **bigBed:**
  - Use `BigBedRead::get_interval` and convert zero-based, half-open coordinates to one-based, inclusive ones with `start + 1` and the same `end`.
  - Move the autoSql parsing in `tracks/downloader.rs` into a shared function, so UCSC tracks and user bigBed files use the same code.
  - `bigtools` reads synchronously. This matches the current BED and VCF reads, so leave a `// PERF:` note rather than adding `spawn_blocking` now.
- **Schema:** add nullable `NAME`, `SCORE`, and `STRAND` constants and columns to `BedSchema`. BED fills them from `other_fields()`, and bigBed fills them from `rest`.
- **Records:** `BedTable.records: Vec<bed::Record<3>>` can't hold bigBed entries.
  - Its only reader is the BED hover message in `mouse.rs`, which uses it for the contig name. The contig header already provides that name from `CONTIG_INDEX`.
  - Remove `records` from `BedTable`. This is the one change that departs from the existing design.
- **Loaded bounds, loading, and the zoom gate:** the same as step 1.
- **Contig sync:** uncomment the BED block in `Repository::new`.

### Viewer (`tgv`)

- **Rendering (`rendering/bed.rs`):**
  - Alternate colors by on-screen order.
  - When `STRAND` is present, fill the bar with `›` or `‹` chevrons.
  - Draw `NAME` inside the bar when it fits.
  - Keep the one-row height. Packing overlapping features into lanes (the TODO in `render_simple_intervals`) is a separate change.
- **Mouse:** the BED hover message reads the contig name from the contig header.

### Server and docs

- Update `BedSchema::column_docs`, the `bed` table in `doc/src/server/query.md`, and the help text.

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

1. **Steps 1 and 2** change `variant.rs`, `bed.rs`, one arm each in `state.rs`, the filter in `app.rs`, `mouse.rs`, and the variant and BED renderers. The repository and table code is self-contained and can go first.
2. **Step 3** adds a kind, so it changes `layout.rs`, `track_registry.rs`, `app.rs`, `session.rs`, and `gv-session`.
3. **Step 4** reuses the wiring from step 3 and changes `render_track`.

Each step is one change that includes its documentation updates.

## Open questions

- **BED records:** remove `BedTable.records`, as step 2 proposes, or keep it for plain BED files only.
- **Test data:** small indexed fixtures next to `simple.vcf` and `simple.bed` would cover each reader: a `.vcf.gz` with a `.tbi`, a `.bcf` with a `.csi`, a `.bed.gz` with a `.tbi`, a `.bb`, a `.bw`, and a `.gff3`.
