# HGVS support

Status: proposed.

## Goals

- **Search:** search mode (`/`) accepts HGVS descriptions and jumps to the matching genomic
  coordinates on the current reference. Examples are `KRAS:p.12`, `KRAS:c.-1`,
  `NM_004985.5:c.35G>A`, and `NC_000012.12:g.25245350`. Gene symbols and RefSeq accessions both
  work.
- **Variant annotation:** clicking a VCF variant shows its `g.`, `c.`, and `p.` descriptions.
  Because the annotation is fast once the data is loaded, hovering a variant or a reference base
  also shows a short HGVS hint in the status bar.
- **Local first:** `tgv download hg38` prepares the HGVS data by default, and `--no-hgvs` opts out.
  hg19 maps to the GRCh37 data. At runtime, HGVS never touches the network.

The implementation uses [ferro-hgvs](https://github.com/fulcrumgenomics/ferro-hgvs).

## ferro-hgvs facts (v1.0.0, MIT, edition 2021)

- `ferro_hgvs::parse_hgvs(&str) -> Result<HgvsVariant, FerroError>` parses all axes. Accessions
  must be `NM_`, `NC_`, `ENST`, and so on, so ferro does not resolve gene symbols. tgv replaces
  `KRAS` with a transcript accession first.
- **Preparing data:** `ferro_hgvs::prepare::prepare_references(&PrepareConfig)` writes
  `manifest.json` into `output_dir`. The relevant `PrepareConfig` fields are:
  - `download_transcripts`
  - `download_genome` and `download_genome_grch37`
  - `download_cdot` and `download_cdot_grch37`
  - `genome` (`"grch38"` or `"grch37"`)
  - `skip_existing`, which makes re-runs incremental
- **Loading:** `MultiFastaProvider::from_manifest(path)` loads the prepared data. It memory-maps
  `sequence_store.pac` when present.
- **Projection** uses `VariantProjector::new(Projector, provider)`:
  - `project_variant_all(&HgvsVariant) -> Vec<VariantProjection>` returns `genomic`, `coding`, and
    `protein` for each transcript, plus `transcript_id`, `gene_symbol`, `is_intronic`, and
    `is_utr`.
  - `project_variant(&variant, transcript_id)` projects onto one transcript.
  - `project_to_genomic_normalized(&variant)` maps `c.` and `n.` to `g.`.
- **Transcript models:** `provider.cdot_mapper()` returns `Option<&CdotMapper>`.
  `get_transcript(accession)` returns a `CdotTranscript` with the contig as `NC_…`, the strand, and
  the exons. It provides `cds_to_tx`, `tx_to_genome`, `genome_to_tx`, `tx_to_cds`, and
  `intron_for_genome_pos`.
- `ferro_hgvs::vcf::to_hgvs::vcf_to_genomic_hgvs(&VcfRecord, alt_index)` builds `g.` from a VCF
  record.
- **Pitfalls:**
  - The loaders print with `eprintln!`, which would corrupt the TUI.
  - cdot carries no reliable MANE status for RefSeq, so ferro cannot pick a gene's preferred
    transcript.
  - The first load builds an on-disk cdot cache, which is slow.
  - The crate is heavy: nom, rkyv, blocking reqwest, clap, and indicatif are unconditional
    dependencies.

## Design

### The `gv-hgvs` crate

A new crate, `crates/gv-hgvs`, depends on `gv-core`, `ferro-hgvs`, `tokio`, and `thiserror`. It
handles HGVS parsing, resolution, and annotation against the prepared ferro data, and a worker
thread serves the requests. Keeping it separate isolates the heavy dependency, and `gv-core`
needs no changes.

#### `build.rs`

- `enum HgvsBuild { Grch38, Grch37 }`, with `fn from_reference(&Reference) -> Option<Self>`.
  `Hg38` maps to `Grch38`, `Hg19` maps to `Grch37`, and every other reference maps to `None`.
- The data directory is `Reference::cache_dir(cache)/hgvs`, for example `~/.tgv/hg38/hgvs/`.
- `ContigMap` is an embedded table for each build, such as `resources/grch38_contigs.csv` with
  rows like `NC_000012.12,chr12`. It resolves in both directions. tgv contig names match against
  `Contig::name` and `Contig::aliases`.

#### `prepare.rs`

- `pub fn prepare(build, dir) -> Result<(), HgvsError>` builds a `PrepareConfig` with the
  transcripts, the cdot data, and the genome for the chosen build only. It then calls
  `prepare_references`.
- It then pre-builds the cdot cache, the library equivalent of `ferro check --build-cache`, so
  that the first TUI launch is fast.
- ferro's own progress bars are acceptable here, because this step runs in the CLI, not the TUI.

#### `query.rs`: the search grammar

These are pure functions.

- `HgvsQuery::parse(input) -> Option<Result<HgvsQuery, HgvsError>>` matches
  `<target>:<axis>.<rest>`, where the axis is one of `g`, `c`, `n`, `m`, `r`, or `p`,
  case-insensitive.
  - `None` means the input isn't HGVS, so `parse_search` handles it.
  - `Some(Err(HgvsError::InvalidQuery))` means the input is HGVS-shaped but invalid, such as
    `KRAS:p.G12`. The error is shown to the user rather than falling back to a gene search.
- Input counts as HGVS only when the colon is followed by `axis.`. Plain positions and HLA-style
  contigs, such as `chr1:100` and `HLA-A*01:01`, never match, so the existing `parse_search`
  behavior is unchanged.
- `enum HgvsTarget { Transcript(TranscriptAccession), Gene(GeneSymbol), Genomic(ContigAccession) }`
  uses a newtype for each kind of accession.
- `enum HgvsLocation` has four variants:
  - `Expression(String)` is a full variant, such as `c.35G>A`. ferro parses it later.
  - `Coding { start: CodingPosition, end: Option<CodingPosition> }` uses
    `CodingPosition { base: CdsBase::{Upstream(n) | Cds(n) | Downstream(n)}, offset: i64 }`. It
    covers `c.-1`, `c.35`, `c.*12`, and `c.35+2`.
  - `Protein { start: Codon, end: Option<Codon> }` takes codon numbers only: `p.12` or
    `p.12_13`. `Codon` is a newtype over a nonzero `u64`. Amino acid letters, as in `p.G12`,
    `p.Gly12`, or `p.G12D`, aren't part of the search syntax. tgv rejects them with "Protein search
    takes a codon number, such as KRAS:p.12." Because the query names no amino acid, there is
    nothing to check against the reference.
  - `Genomic { start, end }`.

#### `worker.rs`: the actor

The worker follows the `Session` and `Responder` pattern in `crates/gv-session/src/session.rs`.

- `HgvsService::spawn(build, dir) -> HgvsHandle` starts a dedicated `std::thread`. The thread owns
  a `VariantProjector<MultiFastaProvider>` and loads it in the background right away. It publishes
  its status as `watch<HgvsStatus { Loading, Ready, Failed(String) }>`.
- `HgvsHandle` is a cloneable mpsc sender. Each request carries a `oneshot` reply. There are three
  requests.
- **`resolve(ResolvedQuery) -> GenomicSpan { contig, start, end }`** returns the tgv contig name
  and a 1-based, inclusive interval.
  - `Coding` and `Protein` locations map directly through `CdotTranscript`, without normalization.
  - Codon `n` is `c.(3n-2)_(3n)`, and the range `p.a_b` is `c.(3a-2)_(3b)`. The span is the
    minimum and maximum of the genome positions, which stays correct across exon junctions.
  - Intronic offsets apply relative to the transcript strand.
  - `Expression` locations go through `parse_hgvs` and `project_to_genomic_normalized`.
- **`annotate(VariantKey { contig, pos, reference, alternate }, scope) -> VariantHgvs`**
  - The scope is `AnnotationScope::{Transcripts(Vec<TranscriptAccession>) | AllOverlapping}`.
  - The result is `VariantHgvs { genomic, transcripts: Vec<TranscriptHgvs> }`. Each
    `TranscriptHgvs` has `transcript`, `gene`, `coding`, `protein`, and a region that is
    `Exonic`, `Intronic`, or `Utr`.
  - It calls `vcf_to_genomic_hgvs`. It then calls `project_variant` for each given transcript, or
    `project_variant_all` for every overlapping transcript.
  - A bounded LRU keyed by `(VariantKey, scope)` caches the results.
- **`describe_base(contig, pos, transcripts) -> Vec<String>`** returns descriptions such as
  `NM_004985.5:c.35`. It uses `genome_to_tx` and `tx_to_cds`, so it takes microseconds.

#### `error.rs`

`HgvsError` derives `thiserror::Error` and has these variants:

- `NotPrepared { reference, dir }`, with the message "Run `tgv download hg38` to enable HGVS."
- `UnsupportedReference`
- `InvalidQuery`
- `UnknownGene`
- `UnknownTranscript`
- `PositionOutsideTranscript`
- `UnmappedContig`
- `Ferro(#[from] FerroError)`
- `Loading`
- `WorkerStopped`

### Transcript choice for gene symbols

tgv's gene track for hg38 and hg19 is the UCSC `ncbiRefSeqSelect` table, which holds the RefSeq
Select and MANE transcripts (`crates/gv-core/src/tracks/mod.rs`, `local_db.rs`).

- `KRAS` resolves through the existing `TrackService::query_gene_name`, which `State::gene` in
  `crates/gv-core/src/state.rs` already uses. The transcript comes from `GeneSchema::ID`, which
  should be a versioned `NM_` accession.
- The same gene-track rows supply the preferred transcripts for the hover hints.

### Download

These changes go in `crates/tgv/src/settings.rs` and `crates/tgv/src/main.rs`.

- `Commands::Download` gains `#[arg(long)] no_hgvs: bool`.
- After the UCSC download, if `no_hgvs` is false and `HgvsBuild::from_reference` returns `Some`,
  tgv prints the estimated size and runs `gv_hgvs::prepare` in `spawn_blocking`. The estimate
  covers the transcripts, the cdot data, and a genome of about 3 GB.
- For other references, it prints "HGVS data is only available for hg38 and hg19, so tgv skips
  it."
- Re-running the download is incremental.

### Runtime integration in `crates/tgv`

#### Startup

- If the reference maps to a build and `<dir>/manifest.json` exists, tgv spawns `HgvsService` and
  keeps an `Option<HgvsHandle>` on `App`.
- While the TUI runs, file descriptor 2 is redirected to the tgv log file with `dup2` (through
  `libc`), so ferro's `eprintln!` output cannot corrupt the screen.
- `restore_keyboard` and the panic hook restore file descriptor 2.

#### Search

- In `crates/tgv/src/register.rs`, tgv tries `HgvsQuery::parse` before
  `gv_core::command::parse_search`. A match emits `Action::ResolveHgvs(query)`, and an invalid
  HGVS query shows its error.
- `app.rs` resolves a `Gene` target through the gene track, then sends the request to the worker.
- Replies arrive on a new `hgvs_events` mpsc branch of the `tokio::select!` loop, so the UI never
  blocks.
- On success, tgv pushes `Movement::ContigNamePosition` and `Zoom::Fit`, the same as region
  search. It also sends `Action::SetHighlights` labeled with the query.
- Errors appear as status messages, such as "HGVS is loading…" or "KRAS: no RefSeq Select
  transcript".

#### Clicking a variant

- In `crates/tgv/src/mouse.rs`, under `MouseEventKind::Up`, a click on `AreaType::Variant` without
  a drag emits `Action::OpenVariantDetails { track, key }`. This sits next to the existing read
  details case.
- The variant row pick moves out of `interval_at` into a shared `variant_at` helper.
- `app.rs` immediately opens `TextPopup::variant_details`, which sits next to `read_details` in
  `popup.rs`. The popup shows CHROM, POS, REF, ALT, QUAL, FILTER, and "HGVS: loading…".
- `app.rs` then requests `annotate(…, AllOverlapping)`. When the reply matches the open popup's
  key, the popup fills in the `g.` row, then one `c.`/`p.` row per transcript, with the Select
  transcript first.
- Multi-allelic records get one block per ALT.

#### Hover

- The `Variant` and `Sequence` arms of `MouseEventKind::Moved` keep their current message. They
  also emit `Action::HgvsHover(token, request)`, using the Select transcripts that
  `state.track.query(...)` returns at that position.
- The reply appends a hint to the status message, such as `NM_004985.5:c.35G>A p.(Gly12Asp)` or
  `c.35`. It does so only if its token still equals `mouse_register.last_hover`.
- Stale replies are dropped. The LRU makes repeat hovers instant.

### Workspace and docs

- The root `Cargo.toml` adds the `gv-hgvs` workspace member and
  `ferro-hgvs = { version = "1", default-features = false }`. A comment explains that the default
  features stay off so the runtime never fetches proteins over the network.
- `AGENTS.md` adds `gv-hgvs` to the crate layout.
- `doc/src/usage.md` documents the search syntax, the variant popup, the hover hints, and
  `tgv download --no-hgvs`.

## Out of scope

- MCP and session tools. The worker crate makes an `hgvs` MCP tool an easy follow-up.
- Ensembl (`ENST`) accessions. ferro needs `--ensembl` data (about 1 GB or more), which could
  become a later download option.

## Files

- **New:** `crates/gv-hgvs/` contains `Cargo.toml`, `src/lib.rs`, `src/build.rs`,
  `src/prepare.rs`, `src/query.rs`, `src/worker.rs`, `src/error.rs`, and the `resources/*.csv`
  contig tables.
- **Edited:**
  - `Cargo.toml`
  - `crates/tgv/Cargo.toml`
  - In `crates/tgv/src/`: `settings.rs`, `main.rs`, `register.rs`, `message.rs` (new `Action`
    variants), `app.rs`, `mouse.rs`, and `popup.rs`
  - `AGENTS.md`
  - `doc/src/usage.md`

## Verification

1. Build and test the new crate on its own:

   ```sh
   cargo build -p gv-hgvs
   cargo nextest run -p gv-hgvs
   ```

2. Run `cargo run -p tgv -- download hg38`. Check that `~/.tgv/hg38/hgvs/manifest.json` and the
   cdot cache exist. With `--no-hgvs`, the HGVS step is skipped.
3. Run `cargo run -p tgv -- -g hg38` and try these searches:

   | Search | Result |
   | --- | --- |
   | `/KRAS:p.12` | chr12:25245349-25245351, highlighted |
   | `/KRAS:p.12_13` | chr12:25245346-25245351, highlighted |
   | `/KRAS:p.G12` | The "Protein search takes a codon number" error |
   | `/KRAS:c.35` | chr12:25245350 |
   | `/KRAS:c.-1` | chr12:25245385 |
   | `/NM_004985.5:c.35G>A` | chr12:25245350 |
   | `/NC_000012.12:g.25245350` | chr12:25245350 |
   | `/TP53` | Still goes to the gene |
   | `/chr1:100` | Unchanged |

4. Load a VCF with the variant chr12:25245350 C>T.
   - Clicking it shows `NC_000012.12:g.25245350C>T` and `NM_004985.5:c.35G>A p.(Gly12Asp)`.
   - Hovering it shows the short hint.
   - Hovering the reference base shows `NM_004985.5:c.35`.
5. Without the prepared data, HGVS search and the popup show the "Run `tgv download hg38`"
   message, and the TUI renders with no stray stderr output.

## Tests

`query.rs` gets unit tests in the same file, written as `#[rstest]` cases on `HgvsQuery::parse`.

- **Targets:** `KRAS` parses as a gene symbol, `NM_004985.5` and the unversioned `NM_004985` as
  transcripts, and `NC_000012.12` as a genomic accession.
- **Protein positions:** `p.12`, `p.12_13`, and the uppercase axis `P.12` parse. `p.0`,
  `p.13_12`, `p.`, `p.G12`, `p.Gly12`, and `p.G12D` return `Some(Err(_))`.
- **Coding positions:** `c.35`, `c.-1`, `c.*12`, `c.35+2`, `c.36-1`, and `c.34_36` parse.
  `c.0` returns `Some(Err(_))`.
- **Genomic positions:** `g.25245350` and `g.25245349_25245351` parse.
- **Expressions:** `c.35G>A` and `g.25245350C>T` parse as `Expression`.
- **Non-matches:** `chr1:100`, `chr1:1,000-2,000`, `HLA-A*01:01`, `TP53`, and `1234` return `None`,
  so `parse_search` still handles them.
