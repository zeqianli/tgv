# Terminal Genome Viewer

Explore omics data without leaving the terminal.

Light, blazing fast 🚀, vim motion, memory safe.

(*TGV is under heavy development. Contribution and bug reports are welcome!*)

https://github.com/user-attachments/assets/ce33b31d-d3eb-4395-9ab4-ab3a501aa1be


## Installation

### Prerequisites: install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add Rust to your path (or restart your terminal):
source "$HOME/.cargo/env"
```

### Install stable release

```bash
cargo install tgv
```

### Install the latest development branch

```bash
# Clone the repository
git clone https://github.com/zeqianli/tgv.git
cd tgv

cargo install --path .
```

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

[Full key bindings and comparison with Vim.](docs/key_bindings.md)

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

## Supported formats

- BAM (index and sorted). `.bai` file is needed.
  - Local, AWS, Google Cloud, or HTTP/HTTPS
  - Local: place the `.bai` file in the same directory; or specify the index file with `-i`.
  - `s3`: set credentials in environmental variables. See: <https://www.htslib.org/doc/htslib-s3-plugin.html>
  - `gss`: TODO not tested. Please provide feedback if it works!
  - Note that the custom `bai` path (`-i`) is not supported for remote use for due to [rust-htslib](https://github.com/rust-bio/rust-htslib) API limitation.

See [ROADMAP.md](ROADMAP.md) for future plans.

## FAQ

- **How to quit TGV?**  
  [Just like vim :)](https://stackoverflow.com/questions/11828270/how-do-i-exit-vim) Press `Esc` to ensure you're in normal mode, then type `:q` and press Enter.

- **Where does the reference genome data come from?**  
  - Reference sequences: [UCSC Genome Browser API](https://genome.ucsc.edu/goldenPath/help/api.html)
    - Uses endpoint: `https://api.genome.ucsc.edu/getData/sequence`
  - Gene annotations: [UCSC MariaDB](https://genome.ucsc.edu/goldenPath/help/mysql.html)
    - Database: `hg19` / `hg38`
    - Table: `ncbiRefSeqSelect` (same as IGV's default)
