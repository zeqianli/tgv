# Session files

A session file captures the current state of a tgv session so it can be restored later.
The default session is automatically written to `~/.tgv/sessions/default.toml` on exit
and loaded on the next launch (when no explicit session path is given).

Sessions are plain TOML files and can be edited by hand.

## File location

| Purpose | Default path |
|---|---|
| Default session (auto-saved/loaded) | `~/.tgv/sessions/default.toml` |
| Named session | `~/.tgv/sessions/<name>.toml` |

## Schema

### Top-level fields

| Field | Type | Default | Description |
|---|---|---|---|
| `version` | integer | — | Schema version. Must be `1`. Required. |
| `locus` | string | — | Starting genomic position. Required. See [locus format](#locus-format). |
| `genome` | string | `"hg38"` | Reference genome. Same as the `-g` / `--reference` flag. |
| `ucsc_host` | string | `"auto"` | UCSC mirror: `"auto"`, `"us"`, or `"eu"`. |
| `cache_dir` | string | `"~/.tgv"` | Local cache directory. `~` is expanded. |

### Tracks

Tracks are declared as a TOML array of tables under the key `[[tracks]]`. The file
type is inferred from the path extension; it does not need to be stated explicitly.

At most one alignment track (BAM or CRAM) may be present.

#### Common fields

| Field | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Path or URL to the file. Supports local paths and `s3://` URLs. |
| `index` | string | no | Path or URL to the index file. Inferred from `path` when absent (`.bam` → `.bam.bai`, `.cram` → `.cram.crai`, `.vcf.gz` → `.vcf.gz.tbi`). |

#### BAM-specific fields

No additional fields beyond the common ones.

#### CRAM-specific fields

| Field | Type | Required | Description |
|---|---|---|---|
| `reference` | string | yes | Path to the FASTA file used to decode the CRAM. Separate from the viewer reference set by `genome`. |
| `reference_index` | string | no | Path to the `.fai` index. Inferred as `reference + ".fai"` when absent. |

#### VCF and BED tracks

No additional fields beyond the common ones.

### Locus format

The `locus` field accepts the same formats as the `-r` / `--region` flag:

| Format | Example | Description |
|---|---|---|
| `contig:position` | `chr17:7572659` | 1-based position on a contig. |
| `gene` | `TP53` | Jump to the gene's start. Requires a reference genome. |

## Example sessions

### Minimal session

```toml
version = 1
locus = "chr1:1"
genome = "hg38"
```

### BAM file with a named locus

```toml
version = 1
locus = "chr17:7572659"
genome = "hg38"

[[tracks]]
path = "/data/sample.bam"
```

### Remote BAM on S3

```toml
version = 1
locus = "KRAS"
genome = "hg38"

[[tracks]]
path = "s3://my-bucket/sample.bam"
```

### BAM + VCF + BED

```toml
version = 1
locus = "chr1:925952"
genome = "hg19"

[[tracks]]
path = "/data/sample.bam"

[[tracks]]
path = "/data/variants.vcf.gz"

[[tracks]]
path = "/data/annotations.bed"
```

### CRAM with a local FASTA decoding reference

```toml
version = 1
locus = "chr1:925952"
genome = "hg38"

[[tracks]]
path = "/data/sample.cram"
reference = "/data/GRCh38.fa"
```

## Relationship to CLI flags

Every session field has a direct CLI equivalent. A session file is a
persistent snapshot of what you would otherwise type on the command line.

| Session field | CLI flag | Notes |
|---|---|---|
| `locus` | `-r` / `--region` | Same format. |
| `genome` | `-g` / `--reference` | Same values. |
| `ucsc_host` | `--host` | |
| `cache_dir` | `--cache-dir` | |
| `tracks[].path` | positional `files` argument | |

When both a session file and CLI flags are provided, CLI flags take precedence.
