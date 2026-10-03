# Interval tables in memory

Gene, BED, and variant data use Polars DataFrames as their persistent interval representation. `IntervalTable`, defined in `gv-core::intervals`, provides a single `query(contig_index, start, end)` operation that returns a DataFrame. It replaces `SortedIntervalCollection`; `GeneTable` also replaces the generic `Track<Gene>` and its boundary indexes.

## Coordinates, identity, and queries

Each table exposes a public `data` field and has non-null `UInt64` columns `row_id`, `contig_index`, `start`, and `end`. Coordinates are one-based, with inclusive endpoints. Row IDs are zero-based and assigned before sorting. Stored rows use deterministic `(contig_index, start, end, row_id)` order, and overlap queries retain that order. Duplicate boundaries remain separate rows.

The query predicate selects the requested contig and rows with `start <= query_end` and `end >= query_start`. A zero query start produces an error. Reversed bounds and unknown contigs produce an empty DataFrame with the original schema. Row selection, sorting, concatenation, and exon projection use lazy expressions and direct `.collect()` calls. Simple statistics use typed column reductions. There are no table getters, duplicate overlap APIs, or custom query executors.

## Gene tables and navigation

`GeneTable` lives in `gv-core/src/gene.rs`. Its `start` and `end` columns represent transcription bounds. The remaining columns describe each transcript: `id`, `name`, and `strand` are strings; `cds_start` and `cds_end` are `UInt64`; `exon_starts` and `exon_ends` are `List(UInt64)`; and `has_exons` is Boolean. Strand values are `+` and `-`. Empty exon lists remain typed, non-null lists. Construction checks the contig, transcription bounds, CDS bounds, paired exon-list lengths, and ordered, nonoverlapping exon bounds. A noncoding CDS can retain the start-after-end representation produced by conversion from an empty UCSC interval.

The table separately retains the loaded contig and complete-data bounds. Empty query results do not imply that a region is unavailable. UCSC API payloads normalize to `UcscGeneRow`, preserving their source coordinate convention. Table ingestion appends their fields directly to typed column builders and converts zero-based starts to one-based coordinates. Malformed exon coordinate lists produce errors instead of silently dropping values.

Gene navigation filters and sorts the table. Positive k values select genes after the position by start, or before the position by end; k zero selects a covering gene. Exon navigation temporarily explodes paired coordinate lists, excludes unavailable or empty lists, and selects by exon bounds. Previous-exon navigation retains its inclusive end predicate, while previous-gene navigation uses a strict end predicate. Stable sorts retain duplicates, and saturating service queries select the boundary gene or exon when the requested ordinal is unavailable. No exon DataFrame or boundary maps remain stored.

There are no internal `Gene` or `SubGeneFeature` structs. Lookup, navigation, and service queries return DataFrames; optional selections contain zero or one row. Navigation reads interval columns directly when constructing a `Focus`. Backend navigation and name queries retain their errors for unavailable targets. The UCSC cache stores gene tables by contig and resolves names through table queries.

## Gene drawing segments

Gene drawing queries overlapping transcripts once and reads names, strands, and transcription bounds directly from the selected columns. Short onscreen genes and genes with unavailable exon data retain their whole-gene presentation.

`query_segments` derives a temporary DataFrame with `gene_row_id`, `start`, `end`, `kind`, and `feature_index`. It explodes paired exon lists and numbers the complete transcript's exons by strand before splitting at CDS boundaries or filtering to the viewport. Kinds are `coding_exon`, `noncoding_exon`, and `intron`. Empty CDS intervals produce noncoding exon segments. Introns use inclusive bounds `[previous_exon_end + 1, next_exon_start - 1]`; adjacent exons produce no intron. Signed intermediate arithmetic allows segment boundary calculations without unsigned underflow or overflow, and final coordinates remain `UInt64`.

The renderer indexes temporary segment ranges by stable gene row ID, preserving transcript drawing order and drawing coding exons over noncoding exons and introns. Segment frames and row ranges are not retained in application state. Drawing kinds map to a renderer-local enum; no subfeature objects or reconstruction of exon vectors are needed.

## BED and variant records

`BedTable` stores the common interval columns and a public vector of original `noodles::bed::Record<3>` objects. `VariantTable` stores original `noodles::vcf::Record` objects and the VCF header alongside its DataFrame. In both cases, `records[row_id]` retrieves the source record even after coordinate sorting. BED start conversion uses noodles' one-based positions directly.

Both tables support consuming `default().add_records(...)` builders. Repositories read batches of 1,024 records. A batch is parsed into columns before concatenation and sorting, and original records are appended after the DataFrame is collected successfully. Missing optional values remain null, and schema types remain valid for empty batches and files.

Variant columns include `ids`, `alternate`, and `filters` as nullable `List(String)`, `reference` as String, and `quality_score` as nullable Float32. Noodles supplies field parsing; FILTER `PASS` remains a one-element list containing `PASS`, distinct from a missing filter. Variant end positions retain the existing REF-length calculation. INFO, genotypes, and additional BED payloads stay in the original records rather than gaining additional columnar schemas in this change.

BED and variant drawing consume queried interval columns directly. Mouse descriptions format queried core fields and use original records for source contig names. Server inspection consumes table rows, retains its response types and limits, and sorts gene response ties by transcript ID as before. No session format or server wire format changes are introduced.

## Runtime and validation

Production uses a multi-threaded Tokio runtime. Existing app integration tests use the same runtime flavor because Polars' synchronous collection uses `block_in_place` internally when it needs its asynchronous executor. Queries remain direct `.collect()` calls; there is no Rayon wrapper. Existing navigation tests move to the gene table module and continue checking bounds, ordinals, empty results, and loaded-region state.
