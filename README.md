# Terminmal Genome Viewer

*This is a work in progress.*

Light, fast, in terminal, vim motion.

![demo](demo.gif)

## Installation

### Prerequisites

1. Install Rust:

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. Add Rust to your path (or restart your terminal):

   ```bash
   source "$HOME/.cargo/env"
   ```

### Install TGV

```bash
# Clone the repository
git clone https://github.com/zeqianli/tgv.git
cd tgv

cargo install --path .
```

## Supported formats

- BAM (index and sorted): supports local, AWS, Google Cloud, or HTTP/HTTPS files.
  - Local: place the `.bai` file in the same directory; or specify the index file with `-i`.
  - Remote:
    - `s3`: set credentials in environmental variables. See: <https://www.htslib.org/doc/htslib-s3-plugin.html>
    - `gss`: TODO not tested. Please provide feedback if it works!
    - Note that the custom `bai` path (`-i`) is not supported for due to [rust-htslib](https://github.com/rust-bio/rust-htslib) API limitation.
- Internet connection is required if reference genomes are used.

## Usage

```bash
# Help
tgv -h

# Browse hg38 genome
tgv

# View BAM file aligned to the hg19 human reference genome
tgv sorted.bam -g hg19

# Start at a coordinate
tgv sorted.bam -r 12:25398142 -g hg19

# Start at a gene; remote BAM
tgv s3://my-bucket/sorted.bam -r TP53 -g hg19

# Use --no-reference for non-human alignments
# (Sequence / feature display not supported yet)
tgv non_human.bam -r 1:123 --no-reference

```

## Key bindings

Quit: `:q`

Normal mode

| Command  | Notes | Example |
|---------|-------------|---------|
| `:` | Enter command mode | |
| `h/j/k/l` | Move left / down / up / right | |
| `y/p` | Fast move left / right | |
| `w/b` | Beginning of the next / last exon |  |
| `e/ge` | End of the next / last exon | |
| `W/B` | Begining of the next / last gene | |
| `E/gE` | End of the next / last gene | |
| `z/o` | Zoom in / out | |
| `_number_` + `_movement_` | Move by `_number_` steps | `20h`: left by 20 bases|

Command mode:

|Command |Notes| Example|
|---------|-------------|---------|
| `:q` | Quit | |
| `:h` | Help | |
| `:_pos_` | Go to position on same contig | `:1000` |
| `:_contig_:_pos_` | Go to position on specific contig | `:17:7572659` |
| `:_gene_` | Go to `_gene_`.| `:KRAS`|
| `Esc` | Switch to Normal Mode | |

Compare TGV and Vim concepts:

|Command|TGV|Vim|Notes|
|-------|-----|--|--|
|`h/l`|Horizontal movement|Character ||
|`y/p`|Fast horizontal movement|NA|`y/p` do different things in Vim|
|`w/b/e/ge`|Exon|word||
| `W/B/E/gE` | Gene |WORD||
|`j/k`|Alignment track|Line||
|`z/o`| Zoom | NA |`o` does a different thing in Vim.|

See `ROADMAP.md` for future key bindings ideas.

## FAQ

- **How to quit TGV?**  
  [Just like vim :)](https://stackoverflow.com/questions/11828270/how-do-i-exit-vim) Press `Esc` to ensure you're in normal mode, then type `:q` and press Enter.

- **What file formats are supported?**  
  BAM files (sorted and indexed) for hg19 / hg38 genomes.
  
  Future plans: BED, VCF, S3 BAM files, HTTP BAM files. See `ROADMAP.md`.

- **Where does the reference genome data come from?**  
  - Reference sequences: [UCSC Genome Browser API](https://genome.ucsc.edu/goldenPath/help/api.html)
    - Uses endpoint: `https://api.genome.ucsc.edu/getData/sequence`
  - Gene annotations: [UCSC MariaDB](https://genome.ucsc.edu/goldenPath/help/mysql.html)
    - Server: `genome-mysql.soe.ucsc.edu`
    - Database: `hg19` / `hg38`
    - Table: `ncbiRefSeqSelect` (same as IGV's default)
