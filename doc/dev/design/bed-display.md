# Interval display: lanes for overlapping features

Status: implemented for BED, bigBed, VCF, and BCF.

## Scope

BED, indexed BED, bigBed, VCF, and BCF share lane packing, display modes, height limits,
and picking rules. bigBed is a BED repository format, so it needs no separate display kind.
Future interval tracks can reuse these pieces without inheriting BED editing or VCF fields.
Signal tracks and gene models keep their own rendering.

## Packing and table schemas

`IntervalSchema::LANE`, re-exported by `BedSchema` and `VariantSchema`, is a zero-based
`u32` column exposed in SQL and the schema documentation. A shared core packing operation
sorts row indexes by contig, start, end, and row ID, then assigns each interval to the first
available lane. It preserves the frame's row order and row IDs. Coordinates remain one-based
and inclusive: a lane is reusable only when its previous end is less than the next start.
Use end-time and free-lane heaps so deeply overlapping data does not require quadratic work.

Plain files pack over each whole contig, including unsorted input. Indexed BED, bigBed,
indexed VCF, and BCF pack over the loaded window. Panning and zooming within cached data
never repack; crossing a loaded window can change the assignment. Plain BED files retain
their lane assignments after loading. Edits preserve these positions, including deliberately inserted empty lanes, rather than repacking them.

VCF intervals use the table's reference span, including INFO/END for symbolic variants when
present. Alleles at the same position remain separate records and take separate lanes.

## Display state and layout

Store a display mode per BED and variant slot in `State`, initialized and removed with that
slot. Reloading data and editing preserve the selected mode. The default is expanded.
Right-click menus on the track and its sidebar offer expanded and collapsed. Squished
remains a core option for a future GUI and is absent from TUI menus.
These are viewer options; saving them to session files is outside this change.

- **Expanded:** one terminal row per lane, with BED names or VCF IDs when they fit.
- **Squished:** retained in the core display options for a future GUI. The TUI maps this
  option to expanded layout and rendering. It contains no squished or half-block renderer.
- **Collapsed:** one row, with darker cells for two overlapping features and a count for
  three or more. Counts describe features sharing a terminal column, which can include
  nearby disjoint intervals at low zoom. Picking chooses the last painted matching feature.

Cap TUI interval tracks at five terminal rows and five lanes. Empty or unloaded tracks retain one row. Compute desired height from the
whole loaded table rather than the visible viewport. Layout consumes that desired height
before distributing remaining space to alignments. Sidebar names wrap within the allocated
rows and do not increase the track height. If terminal space clips a track further,
treat those lanes as hidden too. Show `+N` for hidden lanes at the right of the last row and
in the sidebar label. Collapsed mode intentionally summarizes every lane and has no overflow.

## Picking and editing

Hover and right-click feature lookup use the same mode, row, and visible lane limit as the
renderer. Expanded mode selects only its lane. Overflow marker cells do not select a hidden feature. In aggregated modes,
the last matching row in genomic order wins, consistently with painting.

BED insert mode uses `o` and `O` to insert lanes below or above the active lane, select a new
lane, and force expanded display. With no selection, the first lane is the anchor. Counts
insert multiple lanes in one undo step. The cursor and editing operations are limited to the
selected lane. `j` and `k` move the selection among existing lanes, with counts clamped to
the first and last lanes. Lane navigation preserves the genomic position and creates no
edit revision or undo step. Without a selection, existing all-lane editing behavior remains
available.

`PlainBed` owns per-contig lane counts and optional selections alongside its assigned lane
column. The undo snapshots include this metadata, so lane creation, painting, and erasing
undo without losing empty lanes or changing feature positions. The loaded `BedTable` carries
this layout for rendering. A bounded lane viewport follows the active lane past the row cap;
painting, hover, and the cursor use the same viewport offset. Writes keep standard BED fields
and extra columns; reopening repacks because BED does not encode editor lanes.

## Implementation order

1. Add shared packing, lane schema metadata, and VCF span handling at table construction.
2. Add per-slot modes, dynamic layout heights, and track menu actions.
3. Share interval rendering for the two terminal densities, labels, and overflow markers.
4. Match hover and context-menu picking to displayed lanes; extend the BED insert cursor.
5. Update `doc/src/usage.md` and `doc/dev/server/query.md`, remove fully shipped design
   notes, and retain or trim notes containing unfinished work.
6. Run existing unit and integration tests with `cargo nextest run`, plus formatting and
   compilation checks. Do not add tests without the user's explicit request.
