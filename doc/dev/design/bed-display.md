# BED display: lanes for overlapping features

Status: the next feature to implement, after BED editing (`bed-editing.md`).

## Context

`crates/tgv/src/rendering/bed.rs` draws every feature on one terminal row, and the BED track
has a fixed height of 1 (`AreaType::Bed` in `crates/tgv/src/layout.rs`). Overlapping features
are painted over each other in load order, so the last one wins. The two alternating colours
(`BED1` and `BED2`) tell neighbours apart, but they can't show overlaps. With three or four
stacked features, the user sees one bar and maybe one name, with no hint that anything is
underneath.

## How other genome browsers handle it

All of them pack overlapping features into **lanes** (rows), and let the user choose the
density:

| Browser | Modes | Notes |
|---|---|---|
| **IGV** | Collapsed, Expanded, Squished | **Collapsed** is one row, like tgv today. **Expanded** packs features into lanes greedily, with labels. **Squished** uses the same lanes, thin and without labels. The mode is switched from the right-click menu. Lanes are packed over the loaded region, so features don't jump between rows while the user pans. |
| **UCSC** | dense, squish, pack, full | **dense** is one row, optionally shaded by score. **pack** is lanes with labels, **squish** is half-height lanes, and **full** is one row per feature. There's a cap on rows, with an overflow indicator. |
| **JBrowse 2** | Normal, Compact, Collapsed | Packs by default, and shows "max height reached" when the lanes don't fit. |

The packing is the same everywhere. Features are sorted by start, and each one goes into the
first lane whose last feature ended before it starts.

## Design

### Lanes are computed in `gv-core` and stored on the table

- `BedSchema` gains `LANE: &str = "lane"`, a `u32` filled when data loads.
  - Plain BED files are packed once per contig.
  - Indexed BED and bigBed files are packed over the loaded window.
- Lanes are stable while the user pans and zooms, because packing uses genomic overlap, not
  on-screen overlap.
  - At low zoom, features that are close but don't overlap can touch on screen in the same
    lane. The alternating colours already handle that.
- Edits re-pack the edited contig. Edits already reload the BED slot, so the packing joins
  that path.
- The lane is visible in the SQL `bed` table, so agents see it too. It goes in
  `BedSchema::column_docs`.

### Display modes

The right-click menu switches between three modes, next to the track items from BED editing.

- **Expanded** (the default): one terminal row per lane, with names.
  - The number of lanes is capped (5 to start with).
  - The track height follows the lane count. The count is fixed per contig, or per loaded
    window for indexed files, and not per frame, so the layout doesn't jump while the user
    pans.
  - When the cap hides lanes, the last row shows a `+N` marker, and so does the sidebar label.
- **Squished**: two lanes per terminal row, drawn with half blocks (`▀`, `▄`, and `█`) using
  a foreground and a background colour. Deep overlaps fit in half the height. There are no
  names.
- **Collapsed**: one row, as today. Columns where features overlap are drawn darker, or show
  the count when 3 or more overlap, so overlaps are visible even in one row.

### Picking features

- Hover messages and the context menu choose the feature in the lane under the pointer.
  Today the hover message can't tell stacked features apart.
- This sets up the "feature under the cursor" text object (`if`) from the BED editing TODO
  list.

### Interaction with BED editing

- The paint rules never create overlaps. Painting extends a neighbour or fills a gap, so
  overlaps only come from loaded files.
- In insert mode, the cursor spans every lane, because painting and erasing act on the bases
  across all features.
- Erasing over stacked features trims or deletes all of them, which the current remove rules
  already do. With lanes, the user sees what they're about to change.

### Later

VCF tracks have the same problem with overlapping indels and SVs. Reuse the packing for
variants after BED.

## Implementation order

1. `BedSchema::LANE` and the greedy packing, in the plain BED load path and the indexed
   readers.
2. A per-track display mode in `State`, which the layout uses for the BED track's height.
   The right-click menu switches it.
3. Rendering for the three modes, the `+N` marker, and the sidebar label.
4. Lane-aware hover and context-menu picking.
5. Docs: `doc/src/usage.md` (display modes) and `doc/dev/server/query.md` (the `lane` column).
