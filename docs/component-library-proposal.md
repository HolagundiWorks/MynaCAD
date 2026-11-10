# Component library (hosted store): proposal

Status: **option B chosen by the owner, 2026-10-19. The store (`library.publish`, `library.list`) is built; the bridge from `aad-param`'s component types and the MynaCAD merge on load (section 5) are not.** It needs the owner's decision (section 6) because it adds
record kinds to the shared take-off store, and the kind numbers belong to the contract in the MynaOps
repository (`docs/PROJECT-FILE.md` / bridge docs: kinds 0, 2 and 10 to 255 are reserved; "adding a kind or an optional field is
not a new version").

It answers one gap in the parametric specification (section 6, milestone M3): component types, parameter
schemas, materials and finishes are today **code** in `crates/param/src/schema.rs` (`osas.wall` schema 1, four
materials and three finishes hard-coded). The spec wants them as reusable, versioned definitions in a hosted library that
MynaCAD instances reference.

## 1. What exists

| Item | State |
|---|---|
| `ComponentType` (id, label, `schema_version`, `spec_version`, params, `measurement_rule`) | In code, `osas.wall` v1 only |
| `ParamDef` (type, required, default, min, max, unit, choices, `affects` geometry / takeoff / specification) | In code |
| A graph pins `type@schema_version`; an unknown version is refused, never reinterpreted | Built |
| Take-off in the project file (kinds 1, 3 to 9 with a drawing slot) | Built (`takeoff.record`) |
| A hosted library, materials and finishes as data, presets | **Not built** |

## 2. Principles

1. **Definitions are immutable once published.** A change is a new version; instances and graphs pin the version
   they were made with (spec 6.4). Old versions stay readable so old graphs keep their meaning.
2. **The code stays the fallback.** The built-in definitions remain compiled in, so MynaCAD works with no
   database. A database definition with the same id and version must be identical or is reported as a conflict;
   it never silently replaces the built-in one.
3. **Same style as the take-off records:** one UTF-8 JSON object per record, `format` and `version` in the
   payload, a reader skips a kind it does not know and says so.
4. **Library and drawings are separate.** The take-off database holds per-drawing slots; a component library is
   shared across drawings, so it has its own namespace and, preferably, its own database file or server (section 3).
5. **Measured facts only.** The library holds specifications and rule identifiers, never rates or costs. Rate
   schedules are MynaOps's.

## 3. Where it lives

| Option | For | Against |
|---|---|---|
| A. Same shared `takeoff.vdb`, new kinds | One server to run; MynaOps already reads it | Mixes a global library with per-drawing slots; a take-off reader must skip more kinds; a corrupt library write sits next to take-off data |
| **B. A second database, `%LocalAppData%\OSAS\library\library.vdb`, served on 7422 (gateway 7423)** | Clean separation; can be versioned, backed up and shared on its own; no change to the take-off contract | One more process to start; MynaOps needs to know it only if it reads definitions |
| C. Files in the project | Simplest | Not shared across projects; no cross-app reuse |

Recommendation: **B**, with the same writer rules as the take-off database. Option A is workable if the owner prefers
one server; the id layout below works for both.

## 4. Records (draft)

Ids reuse the layout `(kind << 56) | (hi << 32) | lo` so the same tools work. In the library database the kinds are
free to be numbered from 1; in option A they would take kinds from the reserved range, to be assigned by the owner.
Numbers below are for option B.

| Kind | Record | `hi` | `lo` |
|---|---|---|---|
| 1 | Library header: `format` `osas-library`, `version` 1, `state`, counts | 0 | 0 |
| 2 | Component type version | 24-bit hash of the type id (probed like a drawing slot, section 4.1 of the take-off contract) | schema version |
| 3 | Material | hash of the material id | version |
| 4 | Finish | hash of the finish id | version |
| 5 | Preset (a named set of parameter values for a type version) | hash of the preset id | version |
| 6 | Graph template (a saved sub-graph, for example "wall with height slider") | hash of the template id | version |

### Component type version (kind 2)

```json
{
  "format": "osas-component-type", "version": 1,
  "id": "osas.wall", "label": "Wall",
  "schema_version": 1, "spec_version": 1,
  "measurement_rule": "hcw-aqc-bridge/1#walls",
  "params": [
    { "id": "height", "label": "Height", "type": "length", "required": false,
      "default": 3.0, "min": 0.1, "max": 20.0, "unit": "m",
      "affects": { "geometry": false, "takeoff": true, "specification": true } },
    { "id": "material", "label": "Material", "type": "enum", "default_choice": "brick.full",
      "choices": [ { "id": "brick.full", "label": "Brick, full" } ],
      "ref": "material", "affects": { "specification": true, "takeoff": true } }
  ],
  "replaces": null
}
```

The fields are exactly those of `ParamDef` today, so the existing serialiser produces them. `type` is the
`aad-param` data type name (unit dimension included). A parameter that names `ref: "material"` or `"finish"` takes
its choices from the material or finish records (compatibility listed in the material), instead of an inline list.

### Material and finish (kinds 3 and 4)

```json
{ "format": "osas-material", "version": 1, "id": "brick.full", "label": "Brick, full",
  "applies_to": ["osas.wall"], "spec": { "nominal_thickness_mm": 230 }, "spec_version": 1 }
{ "format": "osas-finish", "version": 1, "id": "plaster.internal", "label": "Internal plaster",
  "applies_to": ["osas.wall"], "layers": [ { "name": "plaster", "thickness_mm": 12 } ], "spec_version": 1 }
```

No rate, cost or waste factor. A finish names what is applied; **how much** is MynaOps's measurement rule.

### Immutability

A record id fixes `(type, version)`. Writing a record whose id exists with different bytes is refused by the
writer (it reads the existing payload first). Retiring a version sets `"deprecated": true` through a new version,
never by editing.

## 5. MynaCAD behaviour

1. **Load.** At startup and on `param.library`, read the library database if reachable; merge with the built-ins
   (built-in wins on an identical id and version; a mismatch is a warning naming both).
2. **Pick.** The component selector lists types, their versions and compatible materials and finishes from the
   merged set.
3. **Bind.** A graph still names `type@schema_version`; the project also records the library revision it read
   (`library_revision`: the header's counts and a hash of the ids), so an issued take-off can say which library
   produced it.
4. **Missing definition.** A graph that names a version no source has is refused with a diagnostic (existing
   behaviour), not guessed.
5. **Publish.** A command `library.publish` (proposed) writes built-in and user-defined types to the library, using
   the same recording, complete protocol as `takeoff.record`.
6. **Never from graph evaluation.** Evaluating a graph does not write the library (spec 3.2: no database writes
   from arbitrary nodes).

## 6. Decisions for the owner

1. Option A, B or C for where the library lives.
2. For option A: the kind numbers (the contract reserves 10 to 255) and whether MynaOps wants to read them.
3. Who owns material and finish definitions: MynaCAD publishes them, or MynaOps does (its rate schedules refer to
   them by id, so the ids must be agreed with it).
4. Whether presets and graph templates are in the first version or later.
5. The measurement rule identifier format: today `hcw-aqc-bridge/1#walls`. The spec's
   `measure(component, rule, revision)` call needs a MynaOps contract that does not exist yet.

## 7. First build step, once decided

1. Serialise today's `component_types()` to the kind 2 payload, with a round-trip test (the payload parses back to
   an equal `ComponentType`).
2. `library.publish` and `library.list` commands, file and server targets, same sink code as `takeoff.record`.
3. Merge on load with the conflict warning; tests for identical, conflicting and missing definitions.
4. Materials and finishes move from the enum lists in `schema.rs` to records, with the built-in lists as the
   fallback.

## 8. Built (2026-10-19)

- `library.publish {definitions, server?, path?}` and `library.list {kind?, server?, path?}` in
  `crates/engine/src/cmd/library.rs`, reachable like every command. Target: the library server on
  `127.0.0.1:7422` (gateway 7423, data `%LocalAppData%\OSAS\library\library.vdb`), started when nothing answers; a
  named `server`; or a `.vdb` file. A file that is not a library database (an entity store, a take-off database)
  is refused and left untouched.
- Records as in section 4: kinds 1 header, 2 component type, 3 material, 4 finish, 5 preset, 6 graph template; id
  `(kind << 56) | (hash24(id) << 32) | revision`, probed past a hash another definition holds. The header is
  written `recording` and then `complete`, as in the take-off protocol.
- Definitions are immutable: the same bytes again are `unchanged`, different bytes under the same id and
  revision are reported as `conflicts` and not written, a change is a new revision. Input is validated before any
  write (kind, id shape, revision of at least 1, payload version 1, size limits); a bad request writes nothing.
- Tests: `crates/engine/tests/library.rs` (publish, list, immutability, new revision, refusal of other databases,
  hostile input, nothing written on a bad request) and unit tests in `cmd/library.rs` (validation, immutability,
  the probe). The former `shilpid` integration test was removed with ShilpiDB; library merge tests remain in `cargo test -p aad-engine --test library`.
- Not built: serialising `aad-param`'s `component_types()` to kind 2 records (`aad-param` is not yet committed to
  the main line), MynaCAD reading the library on load and merging it with the built-ins, materials and finishes
  taken from records by the component selector, and MynaOps reading definitions.
