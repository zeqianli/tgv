# Terminal Genome Viewer

[![Crates version]](https://crates.io/crates/tgv) ![Conda Version](https://img.shields.io/conda/v/bioconda/tgv)

<https://github.com/user-attachments/assets/d811a987-cac2-4c01-b21e-d38efe7789f6>

Explore genomes everywhere

- tgv is a blazingly-fast TUI genome viewer. SSH sessions no longer feels like a black box.
- Keyboard-driven nagivation with vim-like commands
- Rich file format support: common bioinformatics formats (bam, vcf, bed, bigbed), UCGC reference genomes, object storage storage (S3 / GCS)


First-class agent support
- Performant and flexible bioinformatics data engine for your agents 
- Mise-en-place so that your agents can cook
- tgv organized bioinformatics data as [Polars dataframes](https://pola.rs/posts/release-polars-2/), one of the most performant data engine. 
- The internal is fully exposed to agents and agents can perform complex analysis in just a few lines of SQL queries.
- No more pasting different bash commands together and one-offs cripts. No mismatch 1-base and 0-based standards across different bioinformatics engines, 
- reduces pasting together

Designed for human-agent collaboration

- Before tgv: [TODO  screenshot]
- After tgv: interactive visualization, 


[**Installation**](doc/src/installation.md)

- cargo: `cargo install tgv --locked`
- brew: `brew tap zeqianli/tgv && brew install tgv`
- bioconda: `conda install bioconda::tgv`
- Pre-built binaries: [GitHub Releases](https://github.com/zeqianli/tgv/releases/)

## Quick start

```bash
# Browse the hg38 human genome (internet needed)
tgv

# Install tgv mcp
codex mcp add tgv -- tgv mcp
claude mcp add tgv -- tgv mcp 
```

- `:q`: Quit
- `h/j/k/l`: Left / down / up / right. `H/J/K/L` for faster navigation
- `W/B/w/b`: Next gene / previous gene / next exon / previous exon
- `z/o`: Zoom in / out
- `/_gene_` / `/_chr_:_position_`: Go to gene: (e.g. `:TP53`) / chromosome position (e.g. `:1:2345`)
- `_number_` + `_movement_`: Repeat movements (e.g. `20B`: left by 20 genes)
- `:ls`: Switch chromosomes.
- Mouse is supported

[Full key bindings](doc/src/usage.md#key-bindings)

## Usage

If you use a reference genome frequently, downloading a local cache is highly recommended. This makes TGV much faster.

```bash
# The cache is in ~/.tgv by default.
tgv download hg38
```

Browse alignments:

```bash
# View BAM file(s) aligned to the hg38 human reference genome
tgv file1.sorted.bam file2.sorted.bam

# VCF and BED support
tgv sorted.bam variants.vcf intervals.bed

# View an indexed S3 BAM, starting at TP53, using the hg19 reference genome
tgv s3://my-bucket/sorted.bam -r TP53 -g hg19

# BAM file with no reference genome
tgv non_human.bam -r 1:123 --no-reference
```

[Documentation](doc/src/SUMMARY.md)

[![Star History Chart](https://api.star-history.com/svg?repos=zeqianli/tgv&type=Date)](https://www.star-history.com/#zeqianli/tgv&Date)
