# Introduction

tgv is a terminal genome viewer with Vim-style key bindings. It shows alignments, variants, and genomic features next to a reference genome and its gene annotations, all inside a terminal.

- It reads BAM, VCF, BCF, BED, and bigBed files. CRAM tracks can be loaded from a session file.
- It uses UCSC reference genomes and annotations, or a custom FASTA or 2bit reference.
- It can save and restore the open files and position in a [session file](./session.md).
- It runs as a [local MCP server](./server.md), so agents can query the loaded files and move the viewer.

Start with [Installation](./installation.md), then see [Usage](./usage.md) for key bindings and command-line options.
