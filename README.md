# Terminal Genome Viewer

[![Discord Badge]][Discord Server]


<https://github.com/user-attachments/assets/1c74ed21-c026-4535-8627-e4acd9a4313d>

*TGV is at a very early stage so expect bugs. Please don't rely on it for your papers (yet) :)*

*Contribution and bug reports are welcome! Join our Discord to discuss ideas.*

## Installation
- brew: `brew install zeqianli/tgv/tgv`
- cargo: `cargo install tgv`
- Pre-built binaries: [Github releases](https://github.com/zeqianli/tgv/releases/)

[Troubleshooting](https://github.com/zeqianli/tgv/wiki/Installation)

## Quick start

```bash
# Browse the hg38 human genome (internet required). See FAQ for some interesting genome regions. 
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

[Full key bindings](https://github.com/zeqianli/tgv/wiki/Usage)

```bash
# View BAM file aligned to the hg38 human reference genome
tgv sorted.bam

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


## Some interesting genome regions

- `tgv -r 12:25245351`: One of the most prevalent and most studied mutation sites in cancer [[1]](https://www.oncokb.org/gene/KRAS/G12C?refGenome=GRCh38)
- `tgv -r 11:6868417`: Mutations here make you less likely to hate cilantro [[2]](https://flavourjournal.biomedcentral.com/articles/10.1186/2044-7248-1-22). And you can test your baby for it! [[3]](https://www.babypeek.com/unity-patients)
- `tgv -g GCF_000005845.2`: Arguably the most researched organism (E. coli K-12 substr. MG1655). Note how compact it is compared to the human genome? [[4]](https://en.wikipedia.org/wiki/Bacterial_genome#Bacterial_genomes)
- `tgv -g covid -r NC_045512v2:21563`: The spike protein in SARS-CoV-2 [[5]](https://en.wikipedia.org/wiki/Coronavirus_spike_protein)

## Acknowledgements

- [ratatui](https://ratatui.rs/)
- [UCSC Genome Browser](https://genome.ucsc.edu/)
- [rust-htslib](https://github.com/rust-bio/rust-htslib), [htslib](https://github.com/samtools/htslib)

[Discord Badge]: https://img.shields.io/discord/1358313687399792662?label=discord&logo=discord&style=flat-square&color=1370D3&logoColor=1370D3
[Discord Server]: https://discord.com/invite/z2c9TY7e
