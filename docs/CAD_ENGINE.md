# CAD engine

What the drafting engine (`crates/geom`, `doc`, `render`, `io`, `engine`) does, limited to what is
in the code and covered by tests. How the pieces fit: [ARCHITECTURE.md](ARCHITECTURE.md). Feature
list and what is planned: [status.md](status.md).

## Entity types

`Line`, `Point`, `Circle`, `Arc`, `Ellipse`, `LwPolyline` (with bulges), `Spline`, `Ray`, `XLine`,
`Text`, `MText`, `AttDef`, `Insert` (blocks, with attributes), `Dimension`, `Leader`, `MLeader`,
`Hatch`, `Solid`, `Trace`, `Viewport`, `Image`, `Wipeout`, `Table` and `Unknown` (raw DXF data kept
so a file can round-trip; `crates/doc/src/entity.rs`). Table entities are displayed but have no
commands. Each entity has a handle, layer, colour, linetype, lineweight and transparency.

## Coordinates and units

- World coordinates are `f64`, 2D in the world coordinate system. Points keep a `z` for DXF
  round-trip only. There is no UCS; angles are in degrees at the command interface.
- The drawing unit is the `INSUNITS` header value. When it is missing, commands that need it fail
  with a message and nothing is guessed. Display units (including feet-inches) change dimension text
  only. See [units-annotation.md](units-annotation.md).
- Paper space has its own sheet size and viewports with a view centre and height; model-space
  content is drawn through a viewport and clipped to it ([sheets.md](sheets.md)).

## Geometry

`crates/geom`: vectors and 3x3 matrices, bounds, lines, arcs and bulge arcs, circles, ellipses,
B-splines, line/circle/arc intersections, closest-point queries, polygon area and point-in-polygon,
tessellation at a given tolerance, and planar region tracing for HATCH and BOUNDARY. Properties of
the transforms, intersections and areas are checked with generated inputs
(`crates/geom/tests/properties.rs`, [TESTING.md](TESTING.md)). Timings: [PERFORMANCE.md](PERFORMANCE.md).

## Snapping

Object snap modes: endpoint, midpoint, centre, geometric centre, node, quadrant, intersection,
extension, insertion, perpendicular, tangent, nearest, apparent intersection and parallel
(`crates/engine/src/snap.rs`). Tangent and perpendicular picked before a base point exists are
resolved when the other end is known (`Deferred`). Ortho, polar tracking and object snap tracking
are toggled with F8, F10 and F11. **There is no grid and no grid snap** ([workspaces.md](workspaces.md)).

## Selection and hit-testing

Click picking, selection windows (left to right: fully inside; right to left: crossing), fence,
select all, quick select, previous and last. Queries go through a per-space R-tree
(`crates/engine/src/spatial.rs`, spaces of 256 entities or more) that is patched per changed store
chunk and built in the background after a drawing opens or changes. Pick aperture follows the
pick-box setting and the zoom.

## Text, dimensions and hatches

- Text and multiline text are laid out with TrueType/OpenType fonts from the system, or the built-in
  single-stroke font for SHX styles (README, `crates/fonts`).
  SHX files are not read.
- Dimensions: linear, aligned, radius, diameter, angular, baseline, continue, quick dimension and
  multileaders, with dimension styles and associativity ([status.md](status.md)).
- Hatch: pattern, solid and gradient hatches over closed boundaries; the pattern library is our own
  (`crates/doc/src/library.rs`); island and segment counts are capped.

## Undo and redo

Whole-drawing snapshots that share unchanged entity chunks, 2 000 steps per document, redo cleared
by a new edit. An edit whose result has NaN or infinite coordinates is refused and the drawing is
restored ([ARCHITECTURE.md](ARCHITECTURE.md)). Property tests assert that undoing every edit restores
the original drawing and that a failed command changes nothing.

## File interoperability

- DXF: read ASCII and binary; write R2000 (AC1015) ASCII. AAD-only data travels in extension data
  under the application name `AAD`.
- DWG: read and write through the `acadrust` crate by converting to and from DXF in memory; native
  builds only; run in a worker process. Not every DWG version or object is supported.
- 3D content is flattened to 2D on import and reported; 3D solids, surfaces and meshes are kept as
  raw data and not converted ([maintenance.md](maintenance.md)).
- Export: PDF, SVG, PNG; plotting with plot style tables ([plot-styles.md](plot-styles.md)).
- Object types Tren does not model are kept when read and are not written back; the save reports them.

## Known precision and compatibility limits

- `f64` throughout; very large coordinates are refused rather than stored as infinity.
- DWG compatibility is as good as the third-party reader: an allocation bug in it was contained,
  not fixed (BUG-001 in [BUG_BACKLOG.md](BUG_BACKLOG.md)). No representative architectural DWG was
  available for testing ([workspaces.md](workspaces.md)).
- No UCS, no SHX fonts, no 3D, no DWG on the web ([status.md](status.md)).

## Layer protection

Objects on a locked or frozen layer cannot be changed: after every undoable command the engine
compares the drawing with what it was (`crates/engine/src/protect.rs`) and, if the command replaced
or removed such an object, or added one on such a layer, restores the drawing and reports an error
naming the layer. This holds for every command, by handle, by selection and interactively. Selecting a
locked object is not an edit and is allowed. `layer.current` refuses locked and frozen layers. Not
covered: objects inside block definitions. Layer-state configurations (`space.*`) sit on top of this:
they set the flags, the engine enforces them ([ACTIVE_DRAWING_SPACE.md](ACTIVE_DRAWING_SPACE.md)).
