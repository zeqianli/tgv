# Amino acid translation: three-frame rows under the reference

Status: designed, not implemented.

## Context

IGV can draw the visible reference translated into protein in its three reading frames, one row per frame. A codon is three bases, so DNA can be read starting at the 1st, 2nd, or 3rd base of any position. Each row shows codons as three-base boxes with a one-letter amino acid. Box shades alternate, M (the start codon) is green, and stop codons (`*`) are red. The frames depend only on genome position, not on gene annotations. A coding exon shows up as a long run with no stops in one row. IGV's strand toggle flips the sequence to its complement and the rows to the three minus-strand frames, which are read right to left.

tgv has no translation, codon, or view-strand code today. The sequence row (`AreaType::Sequence`, `crates/tgv/src/rendering/sequence.rs`) draws letters at zoom 1 and half-blocks at zoom 2, and draws nothing above `MAX_ZOOM_TO_DISPLAY_SEQUENCES` (2).

Decisions:
- IGV-style strand toggle: three rows for the current strand, and the strand also flips the sequence row to its complement.
- Translation is off by default and toggled from a right-click menu on the reference.
- The standard code (NCBI table 1) is used, except the vertebrate mitochondrial code (table 2) on chrM/MT.

## Design

### 1. Translation model: `crates/gv-core/src/sequence/translation.rs` (new)

- `enum AminoAcid` has the 20 amino acids, plus `Stop` and `Unknown` (used for codons with N or other ambiguous bases). Its methods are `one_letter(self) -> char` (`*` for stop, `X` for unknown) and `three_letter(self) -> &'static str` for hover text.
- `enum GeneticCode { Standard, VertebrateMitochondrial }`:
  - `translate(self, codon: [u8; 3]) -> AminoAcid` looks the codon up in a 64-entry table in NCBI TCAG order, built as a `const` array. Lowercase (soft-masked) bases work too.
  - `for_contig(name: &str) -> Self` returns `VertebrateMitochondrial` for `chrM`, `chrMT`, `M`, and `MT`, ignoring case.
- `enum Frame { First, Second, Third }` (`Frame::ALL`). It is anchored to absolute coordinates, so rows don't shuffle while panning. A frame-`f` codon covers `[p, p+2]` where `(p - 1) % 3 == f`, with 1-based `p`.
- `pub fn complement(base: u8) -> u8` keeps case and maps anything other than ACGT to N.
- `pub struct Codon { pub start: u64, pub amino_acid: AminoAcid }`, where `start` is 1-based and the codon covers `[start, start+2]`.
- `pub fn codons(sequence: &Sequence, code: GeneticCode, strand: &Strand, frame: Frame, left: u64, right: u64) -> impl Iterator<Item = Codon>`:
  - It yields every codon of `frame` that overlaps `[left, right]`.
  - It reads bases with `Sequence::base_at`, so codons cut by the viewport edge still translate, using the 6x cache flank (`CachePolicy::VIEWER.sequence_ratio`). If a codon's bases are missing, it is skipped.
  - On `Strand::Reverse` it translates `[complement(b3), complement(b2), complement(b1)]`.
- Add `pub mod translation;` to `sequence/mod.rs`.

### 2. View state: `crates/gv-core/src/state.rs`

Add `pub reference_display: ReferenceDisplay` to `State`, next to `bed_display_modes`:

```rust
pub struct ReferenceDisplay { pub translation: bool, pub strand: Strand }  // Default: off, Forward.
```

It lives in `State`, like the other display modes, so a headless or MCP render could use it later. `Strand` (`crates/gv-core/src/strand.rs`) needs `Copy` and `Default` (`Forward`).

### 3. Layout: `crates/tgv/src/layout.rs`

- Add `AreaType::Translation`, always pushed right after `AreaType::Sequence` in `MainLayout::new`. `desired_height` is `Some(0)`, so it is hidden by default.
- Add `MainLayout::set_translation_visible(&mut self, visible: bool)`, which sets that row's `track_heights` entry to `Some(3)` or `Some(0)`. `set_tracks` already keeps the heights of rows that still exist, so the setting survives adding and removing tracks.
- Replace the `Sequence` + `GeneTrack` section-merge arm in `resolve_with_labels` with one arm that merges every consecutive `Sequence | Translation | GeneTrack` area into one reference section. This also fixes the case where there is a sequence but no gene track (BYO FASTA).

### 4. Rendering

- **`crates/tgv/src/rendering/translation.rs` (new):** `render_translation(area, buf, state, alignment_view, palette)` draws one row per `Frame`, in `Frame::ALL` order. It reads `state.reference_display` and uses `GeneticCode::for_contig` for the contig being viewed.
  - **Zoom 1:** a codon fills 3 cells. All 3 cells get the codon's background, and the middle cell gets `one_letter()`. Cells off screen are clipped.
  - **Zoom 2:** each cell covers 2 bases. It reuses the `▌` half-block trick from `render_sequence_at_2x`: the foreground is the color of the codon holding the left base, and the background is the color of the codon holding the right base. No letters are drawn.
  - **Zoom above 2:** nothing is drawn, matching the sequence row.
  - **Codon colors:** `Stop` is `AMINO_ACID_STOP` (red), `Met` is `AMINO_ACID_START` (green), and other codons alternate `AMINO_ACID_LIGHT`/`AMINO_ACID_DARK`. The alternation uses `(codon.start - 1) / 3 % 2`, so it is anchored to coordinates and stable while panning.
- **`crates/tgv/src/rendering/sequence.rs`:** on `Strand::Reverse`, both renderers draw and color `complement(base)`.
- **`crates/tgv/src/rendering/colors.rs`:** add `AMINO_ACID_FOREGROUND`, `AMINO_ACID_LIGHT`, `AMINO_ACID_DARK`, `AMINO_ACID_START`, and `AMINO_ACID_STOP` to `Palette` and its default. Use Tailwind colors that match the base palette.
- **`crates/tgv/src/rendering/mod.rs`:** dispatch `AreaType::Translation` under the same `MAX_ZOOM_TO_DISPLAY_SEQUENCES` guard as `Sequence`.
- **`crates/tgv/src/rendering/sidebar.rs`:**
  - Include `Translation` in the reference section's focus match.
  - When the strand is reverse, the reference label becomes `"{reference} (−)"`, so the flipped state is visible.

### 5. Interaction

- **`crates/tgv/src/message.rs`:**
  - Add `ContextMenuTarget::Reference { sidebar: bool }`.
  - Add `Action::ToggleTranslation` and `Action::SetReferenceStrand(Strand)`.
- **`crates/tgv/src/menu.rs`:** add `ContextMenu::reference_items(display: &ReferenceDisplay)`:
  - `✓ Show translation` / `Show translation`
  - a `Strand` submenu with `Forward (+)` and `Reverse (−)`, where the current choice is checked. This follows `interval_items`.
- **`crates/tgv/src/mouse.rs`:**
  - **Right-click:** on a `Sequence | Translation | GeneTrack` area or its sidebar section, right-click opens `ContextMenuTarget::Reference`. The sidebar variant also appends `sidebar_items(None)`.
  - **Left-click and focus:** these include `Translation` next to `Sequence | GeneTrack` (around line 224).
  - **Hover on `Translation`:** pick the frame from the row offset, then find the codon under the column. The message is `"{start}-{end} frame {n} ({strand}): {codon} {Met} (M)"`, where `codon` is in reading direction (the reverse complement on the minus strand).
  - **Hover on `Sequence`:** shows the displayed (complemented) base on the reverse strand.
- **`crates/tgv/src/app.rs`:**
  - `ToggleTranslation` flips `dataset.view.reference_display.translation`, calls `layout.set_translation_visible`, and re-resolves the layout.
  - `SetReferenceStrand` sets the strand.
  - Both push `RenderEvent::All`. `OpenContextMenu` handles the `Reference` target.

### 6. Docs

`doc/src/usage.md` gets a short "Amino acid translation" subsection covering what the three rows are, the colors, the strand toggle, and the chrM code.

## Reused pieces

- `Sequence::base_at` and `has_complete_data` (`crates/gv-core/src/sequence/mod.rs`).
- `AlignmentView::region`, `coordinates_of_onscreen_x`, and `onscreen_x_coordinate` (`crates/tgv/src/layout.rs`).
- The half-block technique from `render_sequence_at_2x`.
- `Strand` (`crates/gv-core/src/strand.rs`).
- The `interval_items` menu pattern and the `SetIntervalDisplayMode` action flow (`app.rs` around line 1078).

## Tests (pending confirmation)

Per AGENTS.md, tests need explicit approval. Proposed: `#[rstest]` cases in `translation.rs` for:
- the table lookups: ATG→M, TAA/TAG/TGA→Stop, TGA→W under the mitochondrial code, NNN→X, and lowercase input
- frame anchoring: the same codons regardless of `left`
- reverse-strand translation

## Verification

- `cargo nextest run` and `cargo clippy --workspace`.
- Run `tgv` on hg38 at a known gene and turn on "Show translation" from the right-click menu.
  - **Forward gene** (e.g. a GAPDH exon): at zoom 1, one forward frame shows an ORF with no stops across the CDS, starting with a green M at the annotated CDS start.
  - **Reverse gene** (e.g. a TP53 exon): flip to the reverse strand. The sequence row shows the complement, and one frame shows an ORF.
  - **Panning** by one base does not shuffle the rows or the shading.
  - **Zoom 2** shows colored blocks only, and zoom 3 or more shows blank rows.
  - **Hover** messages show the frame, the codon, and the amino acid.
  - **chrM:** TGA codons show W instead of `*`.
- Turning translation off restores the original layout heights.
