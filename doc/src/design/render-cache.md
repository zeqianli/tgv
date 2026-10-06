# Design: faster panning in the TUI

Status: proposed, not implemented. Revisit after measuring render times.

## Context

Every pan in the TUI re-renders every track to its viewport, because pans push `RenderEvent::AllTracks`. The idea under consideration is a cached render buffer for each track:

- Each track renders from its data into a buffer wider than the viewport.
- A small movement that stays inside the buffer copies the cells, with no Polars queries or other bioinformatics work.
- Moving outside the buffer, or zooming, renders the buffer again.

This document records what the code shows about that idea, where it would and wouldn't help, and the cheaper alternatives to try first.

## Where the per-frame cost is

The cost depends heavily on the track:

- **Alignment is the expensive one.**
  - `render_alignment` runs several Polars `filter` and `collect` passes over the full loaded tables, including the reads, CIGAR runs, and mismatches.
  - `draw_reads` allocates `read_rows = vec![None; tables.reads.height()]`, sized to all loaded reads, on every frame.
  - `render_paired_alignment` also joins the reads with the pair members on every frame.
  - With `CachePolicy::VIEWER` loading three times the viewport, a 300x BAM at zoom 32 can hold tens of thousands of reads and over 100,000 CIGAR runs.
- **Coverage and the gene track cost something.** Coverage bins its columns, and the gene track runs `query_segments`.
- **Variant, BED, sequence, and coordinate tracks are cheap.**

There is no timing around rendering. `elapsed_ms` is only logged for data loads, so the split between rendering, data loading, and terminal output is unknown.

## Where a wider cached buffer works

Keyboard pans move by `n * zoom` bases (`Movement::Left` and `Movement::Right` in `State::movement`), so the view shifts by whole columns. A pre-rendered wider buffer can be shifted and copied exactly. A ratatui `Buffer` over a wider `Rect`, plus a cell copy into the frame, is straightforward to build.

## Where it doesn't

1. **Content that depends on the visible window can't be shifted.**
   - The coverage y-axis autoscales to the visible maximum (`[0-max]`), so the whole track changes as the view pans.
   - Gene labels are placed and skipped relative to the screen edge (`right_most_label_onscreen_x` in `render_track`).
   - Variant and BED colors start from the first visible row's `row_id % 2`.

   These tracks would either re-render on every pan or need their behavior changed. Alignment and sequence are the only tracks that shift cleanly, and alignment is the only expensive one.
2. **Arbitrary moves miss the cache.** Goto, mouse clicks, and gene and exon jumps don't move by whole columns, unless the cache origin snaps to a column grid.
3. **Many changes invalidate the cache:** zoom, sort, filter, the paired view, a data reload, and the theme.
4. **Vertical scrolling** in the alignment area needs extra rows above and below the visible ones. The buffer can't hold every row, because deep BAM files stack thousands of rows.
5. **Terminal output doesn't shrink.** After a pan, almost every cell in the main area changes, so ratatui's diff still writes nearly the full screen to the terminal. A cached buffer saves compute, not output. If output dominates the frame, which is common for a full-screen redraw with colors, the cache makes little difference.

## Plan

1. **Measure.**
   - Add `log::trace!` timing for each area in `render_main`, and separately around `terminal.draw` and `State::ensure_loaded`.
   - Pan through a deep BAM, such as `HG002.GRCh38.300x_chr20.bam`, at several zoom levels, in both the unpaired and paired views.
   - Continue only if rendering is a significant part of the frame time.
2. **If alignment rendering dominates, make each frame cheaper before caching it.**
   - **Range queries:** sort the reads and CIGAR run tables by display start once per load, then binary-search the visible slice. Each frame then costs in proportion to what's visible, not to everything loaded. Today, every frame scans every loaded row.
   - **Reuse `read_rows`** across frames, or build it once per load or layout change.
   - **Cache the paired join** per load, because it doesn't depend on the viewport.

   These changes need no invalidation rules, and they also speed up zooms and arbitrary moves, which a cell cache doesn't help.
3. **If a cell cache is still worth it, limit it to the alignment area.**
   - Key the cache on the zoom, the alignment options, the data load, and the theme.
   - Use it only for whole-column moves within the buffer, and render again for everything else.
