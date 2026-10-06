# Session files

A session file captures the current state of a tgv session so it can be restored later.
Sessions are opt-in: tgv reads a session only when you pass `--resume`, and writes one
only when you save it with `:w`. Nothing is saved automatically on exit.

Sessions are plain TOML files and can be edited by hand.

## File location

Named sessions are stored at `~/.tgv/sessions/<name>.toml`. Wherever tgv accepts a
session name, it also accepts a path. An argument is treated as a path when it starts
with `~`, contains `/`, or ends with `.toml`.

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


## Starting and saving sessions

- `tgv ...` starts without any session data.
- `tgv ... --resume <name>` loads `~/.tgv/sessions/<name>.toml`, and `tgv ... --resume <path.toml>`
  loads the file at that path. tgv exits with an error if the session cannot be loaded.
- CLI arguments passed with `--resume` override the matching fields from the session.

In the app:

- `:w <name>` saves to `~/.tgv/sessions/<name>.toml`, and `:w <path.toml>` saves to that path.
  The saved session becomes the active session.
- `:w` saves to the active session: the one passed to `--resume`, or the last one saved
  with `:w <name>`. If there is no active session, tgv shows an error.
- `:wq [...]` saves like `:w` and then quits. If the save fails, the app stays open.
