# Session files

A session file captures the current state of a tgv session so it can be restored later.
The default session is created at `~/.tgv/sessions/default.toml` on first launch,
saved on clean exit, and loaded on the next launch when no explicit session path is given.

Sessions are plain TOML files and can be edited by hand.

## File location

| Purpose | Default path |
|---|---|
| Default session (auto-saved/loaded) | `~/.tgv/sessions/default.toml` |
| Named session | `~/.tgv/sessions/<name>.toml` |

## Example

```toml
version = 2
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
| `version` | integer | required | Schema version. TGV writes version `2` and reads versions `1` and `2`. |
| `locus` | string | required | Starting genomic position. See [locus format](#locus-format). |
| `genome` | string | `"hg38"` | Reference genome. Same as the `-g` / `--reference` flag. |
| `ucsc_host` | string | `"auto"` | UCSC mirror: `"auto"`, `"us"`, or `"eu"`. |
| `zoom` | integer | `1` | Initial zoom level, stored as bases per character. |

### Tracks

Tracks are declared as a TOML array of tables under the key `[[tracks]]`. The file
type is inferred from the path extension; it does not need to be stated explicitly.

Any number of BAM, CRAM, VCF, and BED tracks may be present.

#### Common fields

| Field | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Local path to the file. BAM tracks can also use `s3://` URLs. |
| `index` | string | no | BAM and CRAM only. Local path to the index file. S3 BAM tracks can also use an `s3://` index URL. Inferred from `path` when absent (`.bam` -> `.bam.bai`, `.cram` -> `.cram.crai`). |

#### BAM-specific fields

No additional fields beyond the common ones.

#### CRAM-specific fields

| Field | Type | Required | Description |
|---|---|---|---|
| `reference` | string | yes | Path to the FASTA file used to decode the CRAM. Separate from the viewer reference set by `genome`. |
| `reference_index` | string | no | Path to the `.fai` index. Inferred as `reference + ".fai"` when absent. |

```toml
version = 2
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

When tgv starts, the app session is built by the following priority:

1. If no explicit session is provided and `~/.tgv/sessions/default.toml` is not found, create a default session file.
2. Load the selected session file, or the default session when no explicit session is provided.
3. CLI arguments override fields from the loaded session.
4. Sessions can be saved in the app:

- `:w` saves to the active session path.
- `:w [session_name]` saves to `~/.tgv/sessions/[session_name].toml`.
- `:w [full_session_path.toml]` saves to the full session path.
- `:wq [...]` behaves similarly and quits the app.
