# Levels, sheets and plotting

Detail lives in `docs/levels-and-sheet-sets.md`, `docs/layer-mapper.md` and `docs/plot-styles.md`.

## Implemented in the engine
- **Levels:** named level model spaces in one drawing (`level.*`), traces, move and copy to a level, DXF/DWG round trip.
- **Sheet sets and drawing tabs:** discipline annotation per level (`sheetset.*`); standard tabs Working drawing, Electrical, Plumbing, Interior, False ceiling.
- **Layout viewports by level**, Compose Views, Master Layouts and Publisher Sets with a Navigator, PDF and DWG publish.
- **Layer mapper (`layermap`):** puts the layers of a foreign drawing on built-in layers, with a preview and an explicit apply; source names are kept.

## Planned
Sections and elevations as view spaces and tabs (Phase 6, a design), a BIM-style numbered sheet index with title-block fields and revisions, take-off level ids in the shared `.osp`.

The WinUI parts were built; some were not yet driven by hand, as `docs/status.md` states.
