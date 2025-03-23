# Roadmap

## Critical features/fixes

- UI
  - [x] Help screen
  - [x] Coverage Y-axis
- Bugs / testing
  - [ ] Test read display and cigar parsing
  - [x] Test coverage calculation & display
  - [ ] Intergration tests
  - [ ] Feature movements get stuck sometimes (e.g. `gE` / `ge`)
  - [ ] Feature coloring is different from IGV

## Features / fixes that will for sure be added

- Features
  - [ ] VCF / BED files
  - [ ] Local 2bit sequences for hg19 / hg38
  - [ ] Local feature tables for sequences and features locally
  - [ ] Alternative reference database hosting (probably Supabase)
- UI
  - [x] Error message view
  - [ ] Coordinates
- Movements
  - [ ] Navigation between VCF / BED entries
  - [ ] Navigation between reads / read clusters
  - [ ] Next / previous chromosomes (`{/}`)
  - [ ] Start / end of chromosomes (`0/$`)
  - [ ] Abbreviated coordinates in the command mode (e.g. 100k)
  - [ ] Go to `_gene_._exon_`
- CLI
  - [x] More flexible CLI interface
- Performance (now loading large bam files is pretty slow.)
  - [ ] Async
  - [ ] Improved caching
- Stability
  - [ ] Genome coordinate upper bound

## Nice-to-have features

- Features
  - [ ] Stream BAM files (S3, http)
  - [ ] Allele count
  - [ ] Other reference sequences

## Ideas

- View mode: look at base-wide metrics using a cursor. Can add complex actions too (e.g. sort alignments by base)
- Split view with tmux-like motion
- Mouse support
- kmer search?
- Plugins
