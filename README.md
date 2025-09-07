# Terminal Genome Viewer

[![Discord Badge]][Discord Server] [![Crates version]](https://crates.io/crates/tgv) ![Conda Version](https://img.shields.io/conda/v/bioconda/tgv)

<https://github.com/user-attachments/assets/4d5e38ee-d62e-4859-a790-96436d5fde6a>


[**Installation**](https://github.com/zeqianli/tgv/wiki/Installation)

- cargo: `cargo install tgv --locked`
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
- `:_gene_` / `:_chr_:_position_`: Go to gene: (e.g. `:TP53`) / chromosome position (e.g. `:1:2345`)
- `_number_` + `_movement_`: Repeat movements (e.g. `20B`: left by 20 genes)
- `:ls`: Switch chromosomes.
- Mouse is supported

[Full key bindings](https://github.com/zeqianli/tgv/wiki/Usage)

## Usage

If you use a reference genome frequently, downloading a local cache is highly recommended. This makes TGV much faster.

```bash
# Cache are in ~/.tgv by default.
tgv download hg38
```

Browse alignments:

```bash
# View BAM file aligned to the hg38 human reference genome
tgv sorted.bam

# VCF and BED support
tgv sorted.bam -v variants.vcf -b intervals.bed

# View a indexed remote BAM, starting at TP53, using the hg19 reference genome
tgv s3://my-bucket/sorted.bam -r TP53 -g hg19

# BAM file with no reference genome
tgv non_human.bam -r 1:123 --no-reference
```

[Supported formats](https://github.com/zeqianli/tgv/wiki/Usage)

## FAQ

**Why?**

Browsing alignment files is essential for genomics. Genomics research is often in the terminal (SSH session to HPCs or the cloud). [IGV](https://github.com/igvteam/igv) is popular but cumbersome for remote sessions. Terminal-based applications ([1](https://github.com/dariober/ASCIIGenomecu), [2](https://www.htslib.org/doc/samtools-tview.html)) are not as feature-rich. Rust bioinformatics community is vibrant ([3](https://lh3.github.io/2024/03/05/what-high-performance-language-to-learn) [4](https://github.com/sharkLoc/rust-in-bioinformatics)) and Ratatui makes powerful terminal UIs. So TGV is born!

**How to quit TGV?**  
[Just like vim :)](https://stackoverflow.com/questions/11828270/how-do-i-exit-vim) Press `Esc` to ensure you're in normal mode, then type `:q` and press Enter.

## Contribution is welcome
 
**I'm new to Rust and I want a learning project**

Contributing to tgv is a great way to learn Rust. There are many small, isolated components that need improvement, some requiring <10 lines of code change. You can find them by searching for `FIXME` comments. You can also find issues that are extra friendly to new contributors tagged with "contributor friendly".

**I have a bug / Something isn't working**

Search the issue tracker and discussions for similar issues.If your issue hasn't been reported already, open an issue and make sure to fill in the template completely.

**I have an idea for a feature / I've implemented a feature**

Thank you. Because TGV is still at an early stage, API changes frequently, so please coordinate with me in the discord / github discussion before starting to avoid large diffs.

## Acknowledgements

- [ratatui](https://ratatui.rs/)
- [UCSC Genome Browser](https://genome.ucsc.edu/)
- [rust-htslib](https://github.com/rust-bio/rust-htslib), [htslib](https://github.com/samtools/htslib), [noodles](https://github.com/zaeleus/noodles), [twobit](https://github.com/jbethune/rust-twobit), [bigtools](https://github.com/jackh726/bigtools)

[![Star History Chart](https://api.star-history.com/svg?repos=zeqianli/tgv&type=Date)](https://www.star-history.com/#zeqianli/tgv&Date)

[Discord Badge]: https://img.shields.io/discord/1358313687399792662?label=discord&logo=discord&style=flat-square&color=1370D3&logoColor=1370D3
[Discord Server]: https://discord.gg/rZkgjHqPR8
[Crates version]: https://img.shields.io/crates/v/tgv
