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

## Example

```toml
version = 1
locus = "chr0:925952"
genome = "hg18"
zoom = 1

[[tracks]]
path = "/data/sample.bam"

[[tracks]]
path = "/data/variants.vcf.gz"

[[tracks]]
path = "/data/annotations.bed"
```
## Schema

### Top-level fields

| Field | Type | Default | Description |
|---|---|---|---|
| `version` | integer | — | Schema version. Must be `1`. Required. |
| `locus` | string | — | Starting genomic position. Required. See [locus format](#locus-format). |
| `genome` | string | `"hg38"` | Reference genome. Same as the `-g` / `--reference` flag. |
| `ucsc_host` | string | `"auto"` | UCSC mirror: `"auto"`, `"us"`, or `"eu"`. |
| `zoom` | integer | `1` | Initial zoom level, stored as bases per character. |

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

```toml
version = 1
locus = "chr1:925952"
genome = "hg38"

[[tracks]]
path = "/data/sample.cram"
reference = "/data/GRCh38.fa"
```
#### VCF and BED tracks

No additional fields beyond the common ones.

### Locus format

The `locus` field accepts the same formats as the `-r` / `--region` flag:

| Format | Example | Description |
|---|---|---|
| `contig:position` | `chr17:7572659` | 1-based position on a contig. |
| `gene` | `TP53` | Jump to the gene's start. Requires a reference genome. |


## Relationship to the TGV session

When tgv starts, the app session is build by the following priority:

1. If default session `~/.tgv/session/default.toml` is not found, create a default session file.
2. Load the default session.
3. Cli argument modifies the default session. 
4. Session can be save in the app:
  - `:w`: save to the default session
  - `:w [session_name]` saves to `.tgv/session/[session_name].toml` file
  - or, `:w [full_session_path.toml]` saves to the full session path.
  `:wq [...]` behaves similarly and quits the app.
