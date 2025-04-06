# Roadmap

## Critical features/fixes

- Features
  - [ ] VCF / BED files
  - [ ] Local 2bit sequences for hg19 / hg38
  - [ ] Local feature tables for sequences and features locally
  - [ ] Alternative reference database hosting (probably Supabase)
  - [ ] Other reference sequences
  - [ ] Higher read display resolution (up to 1/8 characters) using unicode; better direction indicator
  - [ ] Allele count
- UI
  - [ ] Mis-match alignment alignment display
- Key bindings
  - [ ] More user-friendly key bindings: arrows, mouse
  - [ ] Navigation between VCF / BED entries
  - [ ] Navigation between reads / read clusters
  - [ ] Next / previous chromosomes (`{/}`)
  - [ ] Start / end of chromosomes (`0/$`)
  - [ ] Abbreviated coordinates in the command mode (e.g. 100k)
  - [ ] Go to `_gene_._exon_`
- Performance (now loading large bam files is pretty slow.)
  - [ ] Async
  - [ ] Improved caching
- Bugs / Tesing
  - [ ] Better error handling (e.g. band input mode command does not crash the application)
  - [ ] Test read display and cigar parsing
  - [ ] Intergration tests

## Ideas

- View / cursor mode: look at base-wide metrics using a cursor. Can add complex actions too (e.g. sort alignments by base)
- Split view with tmux-like motion
- kmer search?
- Plugins
