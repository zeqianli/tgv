# Terminal Genome Viewer

[![Discord Badge]][Discord Server] [![Crates version]](https://crates.io/crates/tgv) ![Conda Version](https://img.shields.io/conda/v/bioconda/tgv)

<https://github.com/user-attachments/assets/405fa5fb-bd65-4d0c-b922-5b6cb7784c69>

*TGV is at a very early stage so expect bugs. Please don't rely on it for your papers (yet) :)*

*Contribution and bug reports are welcome! Join our Discord to discuss ideas.*

## Installation

[Full instruction](https://github.com/zeqianli/tgv/wiki/Installation)

- cargo (recommended): `cargo install tgv --locked`
- brew: `brew install zeqianli/tgv/tgv`
- bioconda: `conda install bioconda::tgv`
- Pre-built binaries: [Github releases](https://github.com/zeqianli/tgv/releases/)

## Quick start

```bash
# Browse the hg38 human genome (internet needed)
tgv

# Or your favorite genome (see `tgv --list` or `tgv --list-more`)
tgv -g cat 
```

- `:q`: Quit
- `h/j/k/l/y/p`: Left / down / up / right / faster left / faster right
- `W/B/w/b`: Next gene / previous gene / next exon / previous exon
- `z/o`: Zoom in / out
- `:_gene_`: Go to gene: (e.g. `:TP53`)
- `:_chr_:_position_`: Go to a chromosome position (e.g. `:1:2345`)
- `_number_` + `_movement_`: Repeat movements (e.g. `20B`: left by 20 genes)
- `:ls`: Switch chromosomes.
- Mouse is also supported

[Full key bindings](https://github.com/zeqianli/tgv/wiki/Usage)

## Usage

**Optional**: If you use a reference genome frequently, creating a local cache is highly recommended. This makes TGV much faster and reduces UCSC server load.

```bash
# Cache are in ~/.tgv by default.
tgv download hg38
```

Browse alignments:

```bash
# View BAM file aligned to the hg38 human reference genome
tgv sorted.bam

# VCF and BED file supports
tgv sorted.bam -v variants.vcf -b intervals.bed

# View a indexed remote BAM, starting at TP53, using the hg19 reference genome
tgv s3://my-bucket/sorted.bam -r TP53 -g hg19

# BAM file with no reference genome
tgv non_human.bam -r 1:123 --no-reference
```

[Supported formats](https://github.com/zeqianli/tgv/wiki/Usage)

## FAQ

- **Why?**
  
  Browsing alignment files to compare sequences is essential for genomics research. Omics research is often in the terminal (SSH session to HPCs or the cloud). [IGV](https://github.com/igvteam/igv) is popular but cumbersome for remote sessions. Terminal-based applications ([1](https://github.com/dariober/ASCIIGenomecu), [2](https://www.htslib.org/doc/samtools-tview.html)) are not as feature-rich.

  Rust bioinformatics community is super vibrant ([3](https://lh3.github.io/2024/03/05/what-high-performance-language-to-learn), [4](https://github.com/sharkLoc/rust-in-bioinformatics)) and Ratatui makes powerful terminal UIs. So TGV is born!

- **How to quit TGV?**  
  [Just like vim :)](https://stackoverflow.com/questions/11828270/how-do-i-exit-vim) Press `Esc` to ensure you're in normal mode, then type `:q` and press Enter.

## Acknowledgements

- [ratatui](https://ratatui.rs/)
- [UCSC Genome Browser](https://genome.ucsc.edu/)
- [rust-htslib](https://github.com/rust-bio/rust-htslib), [htslib](https://github.com/samtools/htslib), [twobit](https://github.com/jbethune/rust-twobit), [bigtools](https://github.com/jackh726/bigtools)

[![Star History Chart](https://api.star-history.com/svg?repos=zeqianli/tgv&type=Date)](https://www.star-history.com/#zeqianli/tgv&Date)

[Discord Badge]: https://img.shields.io/discord/1358313687399792662?label=discord&logo=discord&style=flat-square&color=1370D3&logoColor=1370D3
[Discord Server]: https://discord.gg/rZkgjHqPR8
[Crates version]: https://img.shields.io/crates/v/tgv
