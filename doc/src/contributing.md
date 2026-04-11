# Contributing

TGV started as a hobby project (I don't write Rust at all for work). There is so much I want to build. Contribution is welcome!

Join the [discord](https://discord.gg/NKGg684M) to discuss ideas.

I intend to publish as well (see [similar tools](https://academic.oup.com/bioinformatics/article/33/10/1568/2949507)) and want to share authorship with major contributors.

## TGV's goal: Vim for omics

- The best, most versatile, and fastest way to explore omics files
- Bring [IGV](https://igv.org/)'s functionality into the terminal
- Powerful key bindings

Non-goals:
- Beautiful, high-resolution UI at all scales: terminal UI is character-based. In certain scenarios (e.g., zoom-out views), tgv cannot render genomics information as detailed as IGV / web-based genome browsers.

## Roadmap

Priorities:
- Learn the community need
- New features: file formats, better interactivity, more information displayed
- Stability: no off-by-1 errors, fix bugs

Secondary goals:
- Performance: the app runs smoothly despite being very under-optimized (e.g., [read processing](https://github.com/zeqianli/tgv/blob/main/src/models/alignment.rs); the app runs on an async runtime but doesn't actually use it.)

## TODOs / ideas

- Distribution
  - [ ] Conda
- Features
  - [ ] VCF / BED files
  - [ ] Local 2bit sequences for hg19 / hg38
  - [ ] Local feature tables for sequences and features locally
  - [ ] Alternative reference database hosting (Supabase?)
  - [x] Other reference sequences
  - [ ] Higher read display resolution (up to 1/8 characters) using unicode; better direction indicator
  - [ ] Allele count
- UI
  - [ ] Significant improvement in alignment display: mismatches, sort read by bases, etc.
  - [ ] Cigar string search
  - [ ] samtools interface filter (similar to asciigenome)
  - [ ] Cursor / view mode to study read / base details
- Key bindings
  - [ ] More user-friendly key bindings: arrows, mouse
  - [ ] Navigation between VCF / BED entries
  - [ ] Navigation between reads / read clusters
  - [x] Next / previous chromosomes (`{/}`)
  - [ ] Start / end of chromosomes (`0/$`)
  - [ ] Abbreviated coordinates in the command mode (e.g. 100k)
  - [ ] Go to `_gene_._exon_`
- Performance (now loading large BAM files is pretty slow)
  - [ ] Async
  - [ ] Improved caching
- Bugs / testing
  - [x] Better error handling (e.g., bad input mode command does not crash the application)
  - [ ] Test read display and cigar parsing
  - [x] Integration tests
- Others
  - Split view with tmux-like motion
  - kmer search?
  - Plugins
