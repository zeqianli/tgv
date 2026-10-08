# Query data with SQL

`query` runs a read-only [Polars SQL](https://docs.pola.rs/api/python/stable/reference/sql/index.html) statement over the dataset and returns the result rows. Agents use it for any question that [`inspect_interval`](./inspect.md) does not answer directly, such as allele counts by strand, mapping-quality distributions, fragment lengths, or depth per BED target. Queries can join any of the tables, including range joins on genomic coordinates. `describe_tables` lists the tables, their columns, usage notes, and example queries, so an agent can call it once before writing SQL.

## `describe_tables`

### Request

`describe_tables` takes no arguments and does not need a loaded dataset.

```json
{}
```

### Response

The response contains `tables`, `notes`, and `examples`. Each table has a `name`, a `scope`, a `description`, its `key` columns, and its `columns` with SQL types and meanings. This excerpt shows the start of the `mismatches` table:

```json
{
  "name": "mismatches",
  "scope": "region",
  "description": "One row per read base in an `M` operation that differs from the reference, for reads in `reads`. Empty without a reference sequence. Insertions and deletions are in `cigar_ops`.",
  "key": [
    "track_id",
    "read_id",
    "ref_pos"
  ],
  "columns": [
    {
      "description": "The track ID from `get_dataset`.",
      "dtype": "u64",
      "name": "track_id"
    },
    {
      "description": "The read containing the mismatch.",
      "dtype": "u64",
      "name": "read_id"
    },
    {
      "...": "..."
    }
  ]
}
```

The full list of tables is in [Tables](#tables).

## `query`

### Request

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `region` | object | No | The `contig`, 1-based `start`, and inclusive `end` that region tables cover, at most 100,000 bases. Omit it to query only the `tracks` table. |
| `sql` | string | Yes | A single Polars SQL `SELECT` or `WITH` statement. |
| `limit` | integer | No | The maximum number of rows to return, 1–10,000. Defaults to 500. |

This query counts reads supporting each allele at `chr20:88108`, split by strand, excluding duplicates:

```json
{
  "region": {"contig": "chr20", "start": 88000, "end": 88200},
  "sql": "SELECT r.track_id, coalesce(m.base, 'ref') AS allele, r.reverse, count(*) AS reads, avg(r.mapq) AS mean_mapq FROM reads r LEFT JOIN (SELECT * FROM mismatches WHERE ref_pos = 88108) m ON r.track_id = m.track_id AND r.read_id = m.read_id WHERE r.pos <= 88108 AND r.end >= 88108 AND NOT r.duplicate GROUP BY 1, 2, 3 ORDER BY 1, 2, 3"
}
```

### Response

```json
{
  "region": {"contig": "chr20", "end": 88200, "start": 88000},
  "columns": [
    {"dtype": "u64", "name": "track_id"},
    {"dtype": "str", "name": "allele"},
    {"dtype": "bool", "name": "reverse"},
    {"dtype": "u32", "name": "reads"},
    {"dtype": "f64", "name": "mean_mapq"}
  ],
  "rows": [
    [0, "C", false, 86, 70.0],
    [0, "C", true, 63, 70.0],
    [0, "ref", false, 113, 70.0],
    [0, "ref", true, 76, 70.0]
  ],
  "row_count": 4,
  "truncated": false,
  "warnings": []
}
```

The 149 `C` reads and 189 reference reads match the coverage counts at the site, and both alleles appear on both strands.

### Fields

- `region` is the effective region after clamping, or `null` when the request omits it.
- `columns` lists each result column's `name` and Polars `dtype`.
- `rows` contains one array per row, in column order. Lists, such as `alternate`, are JSON arrays, and nulls are `null`.
- `row_count` is the number of returned rows. `truncated` is `true` when the query has more rows than `limit`.
- `warnings` reports the reference and gene annotation services that are unavailable, with the same codes as `inspect_interval`.

### Cross-table queries

Joins on equal keys, such as `track_id` and `read_id`, work with every join type. A join whose `ON` clause uses inequalities, such as a genomic overlap test, runs as an efficient range join, but only as an inner join. To keep rows with no overlap, aggregate the inner join in a CTE, then `LEFT JOIN` it back on the left table's key. This query reports the mean depth of every BED target overlapping the region, including targets without coverage:

```json
{
  "region": {"contig": "chr20", "start": 87990, "end": 88200},
  "sql": "WITH depth AS (SELECT b.track_id AS bed_track, b.row_id, avg(c.total) AS mean_depth FROM bed b JOIN coverage c ON c.pos >= b.start AND c.pos <= b.end GROUP BY 1, 2) SELECT b.row_id, b.name, b.start, b.end, coalesce(d.mean_depth, 0) AS mean_depth FROM bed b LEFT JOIN depth d ON b.track_id = d.bed_track AND b.row_id = d.row_id ORDER BY b.start"
}
```

```json
{
  "region": {"contig": "chr20", "end": 88200, "start": 87990},
  "columns": [
    {"dtype": "u64", "name": "row_id"},
    {"dtype": "str", "name": "name"},
    {"dtype": "u64", "name": "start"},
    {"dtype": "u64", "name": "end"},
    {"dtype": "f64", "name": "mean_depth"}
  ],
  "rows": [
    [3, null, 88001, 88010, 340.4],
    [4, null, 88013, 88013, 340.0],
    [5, null, 88101, 88200, 350.02]
  ],
  "row_count": 3,
  "truncated": false,
  "warnings": []
}
```

`describe_tables` returns more examples, including a fragment-length histogram, indel counts, and passing variants with their depth.

## Behavior

- Coordinates are 1-based, and interval ends are inclusive.
- Coordinates, counts, and IDs are unsigned, and unsigned arithmetic wraps around instead of going negative. Cast to BIGINT before subtracting, as in `CAST(pos AS BIGINT) - 88108`.
- Queries use Polars SQL. CTEs, subqueries, GROUP BY, window functions, and INNER, LEFT, RIGHT, FULL, CROSS, SEMI, and ANTI joins are supported.
- An ON clause with inequalities, such as an overlap test, runs as an efficient range join, but only as an inner join. For a left overlap join, aggregate the inner join in a CTE, then LEFT JOIN it back on the left table's key.
- Region tables hold data for the `region` argument, at most 100,000 bases. Only the `tracks` table is available without a region.
- `reads` holds reads whose aligned span overlaps the region, while `coverage` counts all loaded reads except duplicates and QC failures, so coverage near the region edges can include reads outside `reads`.
- Results return at most `limit` rows; `truncated` reports whether more rows exist. Aggregate in SQL instead of returning raw rows when possible.
- Tables expose the documented columns of the core data under their core names and types. Byte-coded columns, such as CIGAR operations and bases, are text, and base qualities are Phred+33 text.
- `/` divides as floating point. Use `CAST(floor(a / b) AS BIGINT)` for integer bins.
- Each query runs in a fresh SQL context. Only `SELECT` and `WITH` statements are accepted, and file table functions such as `read_csv` are unavailable.
- Region tables load all alignment tracks for the region, so a query costs about as much as inspecting it. Filter and aggregate before joining large tables, such as `cigar_ops` with `coverage`, to keep results small.

## Bounds and errors

| Code | `field` | Cause |
|------|---------|-------|
| `invalid_input` | `sql` | The statement is not a `SELECT` or `WITH` statement, fails to parse, refers to an unknown table or column, or fails to run. |
| `invalid_input` | `region` | The contig is unknown, the start is 0 or past the contig end, the start exceeds the end, or the region is wider than 100,000 bases. |
| `invalid_input` | `limit` | The limit is outside 1–10,000. |
| `no_dataset` | `null` | No dataset is loaded. |

The error message includes the Polars message, so an agent can correct its query. For example, querying `reads` without a region returns:

```json
{"error": {"code": "invalid_input", "field": "sql", "message": "relation 'reads' was not found. Region tables need the `region` argument; call describe_tables to list them."}}
```

## Tables

The `tracks` table is always available. Region tables need `region`.

### `tracks`

One row per loaded track.

Scope: dataset. Key: `track_id`.

| Column | Type | Meaning |
|--------|------|---------|
| `track_id` | `u64` | The track ID from `get_dataset`. |
| `type` | `str` | `alignment`, `variant`, or `bed`. |
| `source` | `str` | The path the track is loaded from. |

### `reads`

One row per positioned read whose aligned span (`pos` to `end`, without soft clips) overlaps the region.

Scope: region. Key: `track_id`, `read_id`.

| Column | Type | Meaning |
|--------|------|---------|
| `track_id` | `u64` | The track ID from `get_dataset`. |
| `contig` | `str` | The contig name. |
| `read_id` | `u64` | The read ID within the loaded reads. |
| `qname` | `str` | The read name, or null. |
| `pos` | `u64` | The 1-based alignment start, or null for unpositioned reads. |
| `mapq` | `u8` | The mapping quality, or null when unavailable. |
| `next_pos` | `u64` | The mate's 1-based alignment start, or null. |
| `tlen` | `i32` | The observed template length; negative for the rightmost segment. |
| `paired` | `bool` | SAM flag 0x1: the template has multiple segments. |
| `proper_pair` | `bool` | SAM flag 0x2: each segment is properly aligned. |
| `unmapped` | `bool` | SAM flag 0x4: the read is unmapped. |
| `mate_unmapped` | `bool` | SAM flag 0x8: the mate is unmapped. |
| `reverse` | `bool` | SAM flag 0x10: the read is reverse complemented. |
| `mate_reverse` | `bool` | SAM flag 0x20: the mate is reverse complemented. |
| `first_segment` | `bool` | SAM flag 0x40: the read is the first segment. |
| `last_segment` | `bool` | SAM flag 0x80: the read is the last segment. |
| `secondary` | `bool` | SAM flag 0x100: the alignment is secondary. |
| `qc_failed` | `bool` | SAM flag 0x200: the read fails quality checks. |
| `duplicate` | `bool` | SAM flag 0x400: the read is a PCR or optical duplicate. |
| `supplementary` | `bool` | SAM flag 0x800: the alignment is supplementary. |
| `end` | `u64` | The last aligned reference position, inclusive. |
| `mate_same_contig` | `bool` | Whether the mate maps to the same contig; null when the mate has no reference. |

### `cigar_ops`

One row per CIGAR operation of each read in `reads`.

Scope: region. Key: `track_id`, `read_id`, `op_index`.

| Column | Type | Meaning |
|--------|------|---------|
| `track_id` | `u64` | The track ID from `get_dataset`. |
| `read_id` | `u64` | The read that owns the operation. |
| `op_index` | `u32` | The zero-based operation index within the CIGAR string. |
| `kind` | `str` | The CIGAR operation, as a BAM operation code. Decoded to the CIGAR letter: `M`, `I`, `D`, `N`, `S`, `H`, `P`, `=`, or `X`. |
| `ref_start` | `u64` | The 1-based reference position where the operation starts. For operations that do not consume the reference, it is the next reference position, so an insertion lies between `ref_start - 1` and `ref_start`. |
| `op_len` | `u32` | The operation length. |
| `seq` | `str` | The read bases of operations that consume the read; otherwise null. |
| `qual` | `str` | The base qualities, aligned with `seq`. Encoded as Phred+33 text. |
| `ref_end` | `u64` | The last reference position for `M`, `D`, `N`, `=`, and `X`; null for other operations. |

### `mismatches`

One row per read base in an `M` operation that differs from the reference, for reads in `reads`. Empty without a reference sequence. Insertions and deletions are in `cigar_ops`.

Scope: region. Key: `track_id`, `read_id`, `ref_pos`.

| Column | Type | Meaning |
|--------|------|---------|
| `track_id` | `u64` | The track ID from `get_dataset`. |
| `read_id` | `u64` | The read containing the mismatch. |
| `op_index` | `u32` | The `M` operation containing the base. |
| `ref_pos` | `u64` | The 1-based reference position. |
| `base` | `str` | The read base, which differs from the reference. Decoded to a one-character string. |
| `qual` | `u8` | The base's Phred quality as a number, or null when the read has no qualities. |
| `reference_base` | `str` | The reference base at `ref_pos`. |

### `base_mods`

One row per base modification call (MM and ML tags) on reads in `reads`.

Scope: region. Key: `track_id`, `read_id`, `display_pos`.

| Column | Type | Meaning |
|--------|------|---------|
| `track_id` | `u64` | The track ID from `get_dataset`. |
| `read_id` | `u64` | The read carrying the modification call. |
| `display_pos` | `u64` | The 1-based reference position, with soft-clipped bases projected beside the alignment. |
| `code` | `str` | The modification code, such as `m` for 5mC; null when `chebi_id` is set. Decoded to a one-character string. |
| `chebi_id` | `u32` | The ChEBI ID of the modification; null when `code` is set. |
| `probability` | `u8` | The ML probability from 0 to 255, or null when absent. |

### `coverage`

One row per alignment track and region position, including zero-depth positions. Counts use the viewer's coverage calculation over all loaded reads, except duplicates and QC failures, which the viewer hides by default. `reference_base` is null without a reference sequence.

Scope: region. Key: `track_id`, `pos`.

| Column | Type | Meaning |
|--------|------|---------|
| `track_id` | `u64` | The track ID from `get_dataset`. |
| `pos` | `u64` | The 1-based reference position. |
| `A` | `u64` | Aligned `A` bases. |
| `T` | `u64` | Aligned `T` bases. |
| `C` | `u64` | Aligned `C` bases. |
| `G` | `u64` | Aligned `G` bases. |
| `N` | `u64` | Aligned bases other than A, C, G, and T. |
| `total` | `u64` | All aligned bases; deletions are not counted. |
| `softclip` | `u64` | Soft-clipped bases projected onto the position. |
| `reference_base` | `str` | The reference base. Decoded to a one-character string. |

### `reference`

One row per region position with a loaded reference base. Empty without a reference sequence.

Scope: region. Key: `pos`.

| Column | Type | Meaning |
|--------|------|---------|
| `pos` | `u64` | The 1-based reference position. |
| `base` | `str` | The reference base; lowercase marks soft-masked sequence. |

### `variants`

One row per VCF or BCF record overlapping the region, in every variant track.

Scope: region. Key: `track_id`, `row_id`.

| Column | Type | Meaning |
|--------|------|---------|
| `track_id` | `u64` | The track ID from `get_dataset`. |
| `contig` | `str` | The contig name. |
| `row_id` | `u64` | The zero-based record index within the loaded data, in file order: within the whole file for plain VCF files, and within the loaded region for indexed VCF and BCF files. |
| `start` | `u64` | The 1-based VCF POS. |
| `end` | `u64` | The last reference position covered by the reference allele. |
| `ids` | `list[str]` | The VCF ID values. |
| `reference` | `str` | The reference allele. |
| `alternate` | `list[str]` | The alternate alleles. |
| `quality_score` | `f32` | The VCF QUAL, or null. |
| `filters` | `list[str]` | The VCF FILTER values, such as `PASS`. |

### `bed`

One row per feature overlapping the region, in every BED and bigBed track.

Scope: region. Key: `track_id`, `row_id`.

| Column | Type | Meaning |
|--------|------|---------|
| `track_id` | `u64` | The track ID from `get_dataset`. |
| `contig` | `str` | The contig name. |
| `row_id` | `u64` | The zero-based feature index within the loaded data, in file order: within the whole file for plain BED files, and within the loaded region for indexed BED and bigBed files. |
| `start` | `u64` | The 1-based first position, converted from the BED 0-based start. |
| `end` | `u64` | The last position, inclusive. |
| `name` | `str` | The BED name, or null when absent. |
| `score` | `u16` | The BED score, from 0 to 1000 by convention, or null when absent or `.`. |
| `strand` | `str` | `+` or `-`, or null when absent. |

### `genes`

One row per annotated transcript overlapping the region.

Scope: region. Key: `row_id`.

| Column | Type | Meaning |
|--------|------|---------|
| `contig` | `str` | The contig name. |
| `row_id` | `u64` | The transcript row ID within the loaded annotations. |
| `start` | `u64` | The transcript start. |
| `end` | `u64` | The transcript end, inclusive. |
| `id` | `str` | The transcript accession, such as `NM_153325.4`. |
| `name` | `str` | The gene name, such as `DEFB125`. |
| `strand` | `str` | `+` or `-`. |
| `cds_start` | `u64` | The first coding position; greater than `cds_end` for noncoding transcripts. |
| `cds_end` | `u64` | The last coding position. |
| `exon_starts` | `list[u64]` | The exon starts, in genomic order. |
| `exon_ends` | `list[u64]` | The exon ends, inclusive and paired with `exon_starts`. |
| `has_exons` | `bool` | Whether the annotation source provides exons. |

### `gene_features`

Exon and intron segments of the transcripts in `genes` that overlap the region. Coding exons are split at the CDS bounds. Join `genes` on `gene_row_id = row_id`.

Scope: region. Key: `gene_row_id`, `kind`, `start`.

| Column | Type | Meaning |
|--------|------|---------|
| `gene_row_id` | `u64` | The transcript row ID in the gene table. |
| `start` | `u64` | The segment start. |
| `end` | `u64` | The segment end, inclusive. |
| `kind` | `str` | `coding_exon`, `noncoding_exon`, or `intron`. |
| `feature_index` | `u64` | The exon or intron number in transcription order, starting at 1. |
