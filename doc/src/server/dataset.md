# Load and describe a dataset

`load_dataset` loads a reference and a list of data files as the server's dataset, replacing any existing one. `get_dataset` describes the loaded dataset without changing it. Both tools return the same description, so a client can read the track IDs from either. When the server is connected to a TGV viewer, `get_dataset` describes the files the viewer shows, and `load_dataset` fails with `dataset_fixed`; see [Show results in the viewer](./view.md).

## `load_dataset`

### Request

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `reference` | string or `null` | Yes | A reference accepted by `-g`, such as `hg38`, a UCSC assembly, or a FASTA or 2bit path. Use `null` for no reference. |
| `files` | array of strings | Yes | BAM, variant (`.vcf`, `.vcf.gz`, `.vcf.bgz`, and `.bcf`), and BED (`.bed`, `.bed.gz`, `.bb`, and `.bigbed`) paths. Bgzipped VCF and BED files with a `.tbi` or `.csi` index, BCF files with a `.csi` index, and bigBed files load by region. Track IDs follow this order. |

```json
{
  "reference": "hg38",
  "files": [
    "/data/HG002.GRCh38.300x_chr20.bam",
    "/data/simple.vcf",
    "/data/simple.bed"
  ]
}
```

### Response

```json
{
  "reference": "hg38",
  "tracks": [
    {"id": 0, "source": "/data/HG002.GRCh38.300x_chr20.bam", "type": "alignment"},
    {"id": 1, "source": "/data/simple.vcf", "type": "variant"},
    {"id": 2, "source": "/data/simple.bed", "type": "bed"}
  ]
}
```

Each track has a numeric `id`, a `type` of `alignment`, `variant`, or `bed`, and the `source` path it was loaded from.

### Behavior

- Loading always replaces the entire dataset. To add or remove a file, call `load_dataset` again with the full list.
- Track IDs reset on replacement.
- If loading fails, the previous dataset stays loaded.
- `reference` is required so that omitting it cannot be confused with choosing no reference.

### Errors

Loading errors are `invalid_input`, except while connected to a viewer:

| `field` | Cause |
|---------|-------|
| `reference` | The reference is not a string or `null`, or it fails to parse. |
| `files` | `reference` is `null` and `files` is empty, a file type is unsupported, a file fails to open, or the reference cannot be found. |

While connected to a TGV viewer, `load_dataset` fails with `dataset_fixed` and `field: null`, because the viewer's files can't be replaced.

For example, loading `/data/missing.bam` returns this structured content:

```json
{
  "error": {
    "code": "invalid_input",
    "field": "files",
    "message": "File IO error: No such file or directory (os error 2)"
  }
}
```

## `get_dataset`

### Request

`get_dataset` takes no arguments.

```json
{}
```

### Response

Before the first successful load, the response is:

```json
{"loaded": false}
```

After a load, the response is the same description that `load_dataset` returns:

```json
{
  "reference": "hg38",
  "tracks": [
    {"id": 0, "source": "/data/HG002.GRCh38.300x_chr20.bam", "type": "alignment"},
    {"id": 1, "source": "/data/simple.vcf", "type": "variant"},
    {"id": 2, "source": "/data/simple.bed", "type": "bed"}
  ]
}
```
