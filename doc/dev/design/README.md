# Design notes

These notes hold designs for features that are proposed or partly built. When a design is fully
implemented, its note is removed: the code and `doc/src` describe the feature from then on, and
git history keeps the note. Leftover follow-ups move to [Small features](./small-features.md).

| Note | Status |
| --- | --- |
| [BED display: lanes for overlapping features](./bed-display.md) | Proposed |
| [A GUI viewer](./gui-viewer.md) | First version implemented. Animated zooms, IME input, an app bundle, signing, and Linux and Windows remain. |
| [HGVS support](./hgvs.md) | Core implemented: parsing, resolution, annotation, and `tgv download`. TUI and GUI integration remain. |
| [Signing and notarizing the macOS binary](./macos-signing.md) | Proposed |
| [Remaining MCP query improvements](./mcp-query-improvements.md) | Proposed |
| [Faster panning in the TUI](./render-cache.md) | Proposed, pending render-time measurements |
| [Small features](./small-features.md) | Ideas and follow-ups: amino acid annotation, further BED editing, and background track opening |
| [More track file formats](./track-formats.md) | Indexed VCF, BCF, indexed BED, and bigBed implemented. bigWig and GFF3 proposed. |
| [Safer viewer connections for agents](./viewer-connection.md) | Partly implemented |
