# MCP feedback: variant-support investigation (2026-10-07)

Feedback from an agent (Claude Code) session that used the TGV MCP server to
check read support for variant calls.

## Session context

- Live TGV viewer showing `HG002.GRCh38.300x_chr20.bam` against `hg38`.
- Benchmark VCF: `HG002_GRCh38_1_22_v4.2.1_benchmark.vcf.gz` (not loaded in
  TGV; see "Live dataset can't take extra tracks" below).
- Tasks:
  - Allele support for chr20:88108 T>C.
  - Calls and unexplained signals near chr20:297567.
  - Whether a cluster of low-fraction G alleles at chr20:297443–297620 came
    from poor sequencing quality.

The tables and `describe_tables` worked well, and so did navigate and
highlight. Most of the friction came from one gap: getting per-read base and
quality information meant rebuilding a pileup from CIGAR strings in SQL.

## Errors hit

| Error | Cause | Suggested fix |
|---|---|---|
| `only 'ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW' is currently supported` | Needed a running total of read-consuming CIGAR lengths to find a base's position in the read. | Precompute it: add a `read_start` (offset in the read) column to `cigar_ops`. |
| `unsupported function 'unicode'` | No way to turn a Phred+33 quality character into a number. | Give qualities as numbers (`list[u8]`, or per-base rows). |
| `regex parse error: + ... repetition operator missing expression` from `strpos` | Polars treats the `strpos` pattern as a regex, which is surprising. | Document the quirk, or pass literal strings through safely. |
| `unsupported function 'regexp_replace'` | Not in the Polars SQL subset. | List the supported functions in `describe_tables`. |

Workaround used: group by the raw quality character, and use ASCII string
comparison (`qch < '5'` means Q<20). It works, but it's not obvious.

### Silent footgun: lowercase soft-masked reference bases

`coverage.reference_base` is lowercase in soft-masked regions. A query that
compared it against the uppercase `A/C/G/T` count columns flagged every masked
position as 100% non-reference. Nothing errored; the results were just wrong.

Suggested fix: keep `reference_base` uppercase and add a `soft_masked: bool`
column (same for `reference.base`).

## Suggestions, in priority order

### 1. Add a `pileup` table, one row per read per reference position

Columns:

- `track_id`, `read_id`, `ref_pos`
- `base`, and `bq` as an integer
- `read_pos`: strand-aware position in the read
- `is_del`, `ins_after` (inserted bases after this position)
- `reverse`, `mapq`

Every question in the session would have been a simple GROUP BY:

- allele counts by strand
- base quality for alt vs ref bases
- position in the read for alt bases

Instead each one needed a ~30-line query over `cigar_ops` with window
functions and `substr`, and that ran into the unsupported features above.

### 2. Add a higher-level `allele_support` tool

- **Input:** positions, or VCF records.
- **Output, per allele:**
  - counts by strand
  - mean base quality and fraction of bases below Q20
  - mean MAPQ
  - mean position in the read
- **Defaults:** exclude duplicate, secondary and supplementary reads; optional
  minimum base quality.

This is the most common question asked of a viewer like this. A dedicated tool
avoids SQL mistakes and keeps defaults consistent.

### 3. Live dataset can't take extra tracks

`load_dataset` can't replace a dataset that a viewer is displaying, so the
benchmark VCF couldn't be attached to the live session. The agent fell back to
`zcat | awk` in the shell. Options:

- an `add_track` tool for the live dataset, or
- the ability to query a VCF that isn't loaded, given its path.

### 4. Add columns to existing tables

- `reads`:
  - read length
  - soft-clip length at each end
  - `NM`
  - selected aux tags (`HP`, `PS`, `RG`)

  `HP` matters: with a haplotagged BAM it lets the agent split evidence by
  haplotype.
- `mismatches`: `bq` and `read_pos`.
- `coverage`:
  - forward and reverse counts
  - deletion and insertion counts
  - counts limited to bases at Q20 or higher, to match what callers and IGV
    report

### 5. Fix small footguns

- Unsigned coordinates wrap around when you subtract. Expose them as `i64`, or
  at least use signed types for anything derived.
- `get_dataset` returns a relative path (`test/output/...`). Return the
  absolute path.
- `highlight` takes one label for the whole set of intervals. Allow a label
  per interval.

### 6. Make errors self-correcting

- When a function or construct isn't supported, include a short hint in the
  error, for example "use `pileup.bq`" or "supported window frames: ...".
- In `describe_tables`, add:
  - the list of supported functions
  - known dialect quirks (such as `strpos` taking a regex)
  - an example query that reads base qualities
