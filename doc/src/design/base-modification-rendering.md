# base modification rendering

## igv behavior summary

IGV renders base modifications as a separate overlay on top of the normal alignment glyphs rather than folding them into the core alignment paint step.

At the per-read level:

- IGV parses MM into structured base-modification groups and keeps ML likelihoods alongside them.
- For each aligned base position, it considers all modifications present at that read position.
- It chooses the modification with the highest likelihood at that position.
- In its two-color mode, it can also visualize the residual "no modification" likelihood.
- It uses a modification-specific color palette and varies intensity with likelihood.
- It can optionally split the overlay by strand, drawing one strand in the upper half of the read row and the other in the lower half.
- It also supports filtering by modification type and thresholding by likelihood.

At the coverage level:

- IGV aggregates modification calls across reads.
- It renders stacked colored bars by modification type, scaled by informative read counts and average likelihood.

## current tgv state

`tgv` already has an initial base-modification path:

- `AlignedRead` stores `base_modifications` keyed by 1-based reference position.
- The alignment renderer can color aligned cells from that map when `ShowBaseModifications` is enabled.
- The current renderer chooses a modification by hardcoded priority, not by maximum likelihood.
- The current renderer uses coarse buckets, not a continuous likelihood-driven visual scale.
- Paired rendering does not yet support modifications.
- There is no threshold, no filter, no strand distinction, and no explicit no-mod state.

## staged implementation plan

### stage 1: switch parsing to noodles

Replace the custom MM parsing logic with noodles base-modification parsing in `crates/gv-core/src/alignment/read.rs`.

Goals:

- Keep the current render-facing `BaseModification` and `HashMap<u64, Vec<BaseModification>>` shape stable.
- Use noodles to parse MM positions and modification metadata.
- Continue consuming ML in SAM order so multiple modifications per site remain correct.
- Convert noodles sequence positions to 1-based reference coordinates using the CIGAR.

This is the smallest change that removes the handwritten MM parser without forcing the renderer to change immediately.

### stage 2: move modification attachment into rendering contexts

Extend rendering contexts or their modifiers so read-local modification annotations travel with the read rendering model.

Goals:

- Avoid a parallel rendering-only side map where possible.
- Keep mismatch and modification rendering composable.
- Make paired rendering easier by associating annotations with the displayed segments.

This stage should still preserve the current user-visible output unless that output is already incorrect.

### stage 3: make per-read rendering IGV-like

Refactor the terminal renderer so modification display becomes a distinct overlay step.

Goals:

- Render the read normally first.
- Choose the displayed modification by maximum likelihood at each position.
- Use a stable color per modification type or code.
- Map likelihood to terminal-friendly intensity.
- Preserve mismatch readability when a base is both mismatched and modified.

This is where the main IGV-inspired behavior starts to show up.

### stage 4: add user controls

Add the minimum controls needed to make the feature usable:

- show or hide modifications.
- threshold by likelihood.
- filter by modification type.
- optionally distinguish strands.

These should live in alignment display options rather than as renderer-only flags.

### stage 5: support paired rendering

Add base-modification rendering for paired view without duplicating the source of truth.

Goals:

- Keep the source data on `AlignedRead`.
- Preserve pair layout and stacking.
- Render modifications from both constituent reads in pair mode.

### stage 6: add coverage summaries

After per-read rendering is correct, add an IGV-like modification summary in the coverage track.

Goals:

- Aggregate modification counts by position and modification type.
- Render stacked summary bars in a way that fits the terminal UI.
- Reuse the same filtering and thresholding model as the read-level renderer.

## design constraints

- Correctness matters more than convenience. Multi-modification MM groups and reverse-complemented reads must remain correct.
- The type system should carry the distinction between parsed modification data, render-ready annotations, and final display options.
- The renderer should not assume only `5mC`, `5hmC`, and `6mA` exist, even if those are the first types we style explicitly.
- Tests should cover reverse reads, multi-modification groups, CIGAR edge cases, and threshold behavior.
