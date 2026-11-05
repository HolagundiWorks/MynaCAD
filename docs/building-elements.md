# Building elements, Indian code index, and SketchUp-style components

2D plan objects can carry a **building-element classification** (structural, finish, or shop),
references into an open **Indian specification index**, and IFC **type names** as metadata. Rates
and full standard text stay outside O-Draft (O-CoreOp / licensed packs).

This is not a 3D BIM modeller and does not write `.ifc` files yet.

## Classification

Commands:

| Command | Role |
|---|---|
| `classify.set` | Stamp `BuildingElement` on selection / handles, or on a block definition (`block`) |
| `classify.get` | Read classification |
| `classify.clear` | Remove classification |

Fields: `family`, `kind` (column, beam, footing, wall, slab, plaster, paint, flooring, door, window, grill, other), `specRefs` / `specIds`, `ifcType`, `measurementRule`.

DXF round-trip keeps entity and block classifications in project keys `buildingMeta` and
`buildingBlockMeta` (same pattern as take-off marks).

## Spec index

Curated open index: [`assets/specs/indian-index.json`](../assets/specs/indian-index.json).

- Short titles and code ids only — **not** a certified copy of BIS, IS, or CPWD body text.
- CPWD entries carry item **code hints** only; no DSR rates.
- Commands: `spec.list`, `spec.get`, `spec.search`.

## Groups and components (SketchUp-style)

| Concept | O-Draft |
|---|---|
| Group | Named set of entities; optional nested `childGroups` (depth capped at 16). `group.explode` promotes one level of nesting. Nesting survives DXF via `groupNest`. |
| Component | Block definition + INSERT, optionally classified. `component.make` / `place` / `makeunique` / `edit` (edit opens `bedit` / `refedit`). |

## Bridge / take-off

When an entity (or the block of an INSERT) is classified, `bridge.export` / `takeoff.record` rows may
carry optional fields: `ifc_type`, `building_kind`, `building_family`, `spec_refs`,
`measurement_rule`. O-CoreOp may ignore them until it learns them.

## Not in this release

- Full IFC STEP/XML export
- Embedding copyrighted IS/BIS/CPWD prose or DSR rate books
- Parametric types beyond `osas.wall` (`osas.column`, etc.)
