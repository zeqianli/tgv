# Small features

Small features to design and build later. Each entry says what it is, why it helps, and what
to decide before building it.

## Amino acid annotation on the sequence

**What:** Show the translated amino acids under the reference sequence, for the coding exons
of the gene track.

**Why:** Variants and reads in coding regions are read in codons. IGV shows the translation in
its sequence track, and users look for it.

**To decide:**
- **Which frame to show.** One option is the frame of the transcripts in view, from the CDS
  phase of each exon. The other is a toggle between the three forward or three reverse
  frames, as IGV does.
- **Where it goes.** It could be a new row under `Sequence`, or drawn in the gene track's
  exon bars.
- **How to draw a codon.** A codon spans three columns at zoom 1. Draw the one-letter code
  in the middle column, and shade the codon's other columns. Hide the row when zoomed out
  past one base per column.
- **Strand.** Minus-strand genes translate the reverse complement, read right to left.
- **Other codes.** Mark stop codons (`*`) and start codons (`M`) distinctly.

## HGVS support in search mode

**What:** Accept HGVS expressions after `/`, such as `/NM_000546.6:c.215C>G`,
`/TP53:c.215C>G`, `/NC_000017.11:g.7676154G>C`, and `/TP53:p.Pro72Arg`. Go to the position
they name.

**Why:** Variant reports and papers name variants in HGVS. Today the user has to convert them
to a genomic position first.

**To decide:**
- **Which forms to support first.** `g.` maps directly to a position. `c.` and `n.` need the
  transcript's exon structure, from the gene track's tables. `p.` needs the transcript's
  CDS, and a protein position maps to a codon, not one base.
- **Transcript versions.** What to do when the transcript version in the expression isn't
  in the annotation, for example falling back to the newest version and saying so.
- **Gene symbols.** `TP53:c.215C>G` names a gene, not a transcript. Decide whether it uses
  the canonical transcript.
- **Intronic offsets**, such as `c.672+1G>A`.
- **Reference checks.** Whether to check the reference base in the expression against the
  sequence, and say so when it doesn't match.
- **Where parsing lives.** The parser belongs in `gv_viewer::command::parse_search`. Mapping to
  a position needs the track service, like `Movement::Gene`, so it becomes a new `Movement`
  variant.
