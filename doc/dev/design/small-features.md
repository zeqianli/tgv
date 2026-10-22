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

## Further BED editing

The main BED editing implementation ships: track focus, insert mode, painting, erasing,
undo, redo, writing, copying tracks, and track menus. The following work remains:

- **Visual mode.**
  - `v` starts a selection at the cursor. Any motion extends it, including `/` search. The
    selection is highlighted in the BED row.
  - `Space` paints the selection, `d`/`x` erases it, and `y` yanks it.
  - It becomes `KeyRegisterType::Visual { track, anchor }`, and the operator code runs on
    the selection range.
- **Yank and paste.**
  - `y{motion}` and visual `y` copy the features in the range, clipped to it, into an unnamed
    register on `App`. The features are stored as offsets from the range start, with their
    name, score, strand, and extra columns.
  - `p` pastes the span starting at the column after the cursor, and `P` pastes it ending
    just before the cursor, as in vim.
  - The register works across tracks and contigs.
  - Open question: should pasted features add rows as they are (BED allows overlaps), or
    follow the paint rules, which merge their coverage into existing features?
- **Text objects** for `d`, `y`, and `v`:
  - `if`: the BED feature under the cursor.
  - `ie` / `ig`: the exon or gene under the cursor, from the gene track.
  - For example, `vie Space` annotates the exon under the cursor.
  - These are Zed's `Object` and vim's `current_*` functions.
- **`J`**, vim's "join lines": join the feature under the cursor with the next one, filling
  the gap and keeping the left feature's name and columns. Waiting on the user's decision.
- **Uppercase word motions on BED and VCF** tracks. They do the same as lowercase for now.
- **Alignment word motions.** They use exons for now.
- **Undo moving the cursor** back to where the step began, as vim does.
- **Copying indexed BED and bigBed tracks.**
- **Mouse painting:** drag on a BED row in insert mode to paint, and drag with a modifier to
  erase.
- **Not planned:** `.` (repeat the last edit), `dd`/`D`, and bracket motions (`]e`, `[g`).

## Background track opening

Runtime track creation, removal, `:e` / `:open`, and bracketed-paste file loading ship.
Opening a file currently shows “Opening …” before waiting. A follow-up can open repositories
on a background task and send completed repositories to the event loop, so navigation stays
responsive during slow remote opens.
