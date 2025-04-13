# Terminal Genome Viewer

Explore omics data without leaving the terminal.

Light, blazing fast 🚀, vim motion, memory safe.

<https://github.com/user-attachments/assets/b250f901-8e4d-4d5d-b150-fa9195b08e14>

*TGV is at a very early stage. Please don't rely on it for your papers (yet) :)*

*Contribution and bug reports are welcome! Also join the [Discord](https://discord.gg/NKGg684M) to discuss ideas.*

## Installation

See [Installation](https://github.com/zeqianli/tgv/wiki/Installation)

## Quick start

```bash
# Browse the hg38 human genome. Internet connection required.
tgv
```

- Quit: `:q`
- Movement:
  - Left / down / up / right: `h/j/k/l`:
  - Faster left / right: `y/p`
  - Next gene / previous gene / next exon / previous exon: `W/B/w/b`
  - Repeat movements: `_number_` + `_movement_` (e.g. `20B`: left by 20 genes)
- Zoom in / out: `z/o`
- Go to gene: `:_gene_` (e.g. `:TP53`)
- Go to a chromosome position: `:_chr_:_position_`: (e.g. `:1:2345`)

[Full key bindings](https://github.com/zeqianli/tgv/wiki/Usage)

## View alignments

```bash
# View BAM file aligned to the hg19 human reference genome
tgv sorted.bam -g hg19

# Start at a coordinate
tgv sorted.bam -r 12:25398142 -g hg19

# View a indexed remote BAM, starting at TP53, using the hg38 reference genome
tgv s3://my-bucket/sorted.bam -r TP53

# Use --no-reference for non-human alignments
# (Sequence / feature display not supported yet)
tgv non_human.bam -r 1:123 --no-reference
```

Supported formats (see [wiki](https://github.com/zeqianli/tgv/wiki/Usage)):
- BAM (index and sorted; `.bai` file is needed): local, AWS S3, HTTP, FTP, Google Cloud

## Contribution is welcome!

See [wiki](https://github.com/zeqianli/tgv/wiki/Contribution-is-welcome!). Also join the [Discord](https://discord.gg/NKGg684M) to discuss ideas.

## FAQ

- **How to quit TGV?**  
  [Just like vim :)](https://stackoverflow.com/questions/11828270/how-do-i-exit-vim) Press `Esc` to ensure you're in normal mode, then type `:q` and press Enter.

- **Where are the reference genome data from?**  
  - Sequences: [UCSC Genome Browser API](https://genome.ucsc.edu/goldenPath/help/api.html)
  - Annotation: [UCSC MariaDB](https://genome.ucsc.edu/goldenPath/help/mysql.html), `hg19` / `hg38`, table `ncbiRefSeqSelect` (same as IGV)
