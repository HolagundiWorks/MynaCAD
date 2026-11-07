# Suite integration: what Tren has, what the contracts need

This page maps Tren in the Kivo suite (Tren drawing, Kivo office
management, Falcon quantities and costing) with one
shared project identity in the **company file**. It describes Tren's side
only. Where it names Kivo / Falcon behaviour beyond what
Tren code proves, the statement is marked.

Nothing here is a claim that a full cross-application click-through has been
proven on a clean Windows machine. Section 5 lists what is built, what is
missing and what the first milestone needs.

## 1. Ownership

| Data | System of record | Tren's part |
|---|---|---|
| Drawing geometry, layers, blocks, layouts, undo, DXF/DWG | Tren (`aad-doc`, `aad-engine`) | Owns it. Other applications read it through commands, never through the document model |
| Company details (name, address, registrations) | Kivo company file | Reads them (`companyfile`), prints them in the title block; never writes; when linked, refuses free typing of firm identity via `project` / wizard |
| Project name, site, client | Studio / Kivo, in the company file | Reads them (`companyfile`); when `project_ref` is set, those detail keys are refresh-only in Draft |
| Quantities and costs | Falcon | Produces measured quantities (`takeoff.*`) and a hand-off file (`bridge.export`); does not price |
| Reusable details and spatial records | (planned) | Component library in code today; not a separate spatial server |
| AI conversation, tool permissions, audit | Shared AI runtime (not built) | Tren has its own tool registry and audit log (`aad-agent`) for its own tools |

Rules Tren already follows and the suite should keep: a hand-off file never changes the drawing it
came from; imported text and tool results are data, not instructions ([ai-agent.md](../ai-agent.md)).

OSS suite apps do **not** ship an AORMS / portal connector. Company file is SoR.

## 2. What exists in Tren

| Capability | Where | State |
|---|---|---|
| Every action is a command with a JSON form, usable without a window | `crates/engine/src/cmd/*`, `Session::execute` | Implemented. This is the seam the suite should call |
| Control channel (loopback JSON lines) | `aad-host`, [control-protocol.md](../control-protocol.md) | Implemented: `engine.execute`, `cmdline.*`, `drawing.inspect`, `app.open`, `app.save` |
| MCP server over the same commands | `crates/mcp`, [mcp.md](../mcp.md) | Implemented |
| Typed agent tools with schema validation, risk classes, confirmation and an audit log | `crates/agent` | Implemented for Tren's own tools. Quantity take-off tools are reserved but not registered (`agent::PLANNED`) |
| Company / project details from the Studio company file | `crates/office`, `companyfile`, `project` suite lock | Implemented (read-only + refresh-only lock on linked detail keys; ticket / `context` only, no admin password UI; local `CompanyProfile` write gated when `OSAS_COMPANY` is set). Interactive project-picker UI still open |
| Quantity take-off from drawn geometry, rates, bill, opening schedule | `takeoff.*`, `schedule`, `openings.*` ([hcw-kit-port.md](../hcw-kit-port.md)) | Implemented; reads `MEASURE-*` layers |
| Parametric wall graph → measured facts | `aad-param`, `param.*` ([parametric.md](../parametric.md)) | Implemented (Rust evaluate/commit/undo; no node editor UI). Committed walls feed `bridge.export` / `takeoff.record` |
| Hand-off to and from Falcon | `bridge.export`, `bridge.import`; file `hcw-aqc-bridge` version 1, millimetres, beside the drawing as `name.aqc.json` | Implemented. Import previews first and never changes geometry. Wall rows may come from brick take-off and/or parametric commit. CoreOp parses `source.project_ref` when present |
| Take-off in project file | `takeoff.record` / `takeoff.annotate` | Implemented; header carries company-file `project_ref` when linked ([PROJECT-FILE.md](../PROJECT-FILE.md)) |
| Spatial AI tools (overview, window, nearest, describe) | `crates/agent` | Implemented |
| Drawing-level change record (revision, checksum) | `revision.rs`, `drawing.revision`, `<file>.aadrev.json` | Implemented |
| Stable cross-application project and artifact references | `project_ref` from the company file in take-off / bridge headers | Partial: OSAS `ProjectRef` when linked via `companyfile`; no portal office-id key |

## 3. Contracts the suite needs from Tren

These are proposals. Each is a thin layer over commands that exist; none requires the shared
runtime to touch the document model.

| Contract | Proposed content | Built on |
|---|---|---|
| `DrawingRef` | project id, drawing id, path, DXF/DWG format, byte size | `Session` document state, `companyfile` / project details |
| `DrawingRevision` | `DrawingRef` + SHA-256 of the saved bytes + saved time + entity count | **new**: computed at save in `file.rs` (`commit_save`) and stored beside the file, not in the DXF |
| `QuantityTakeoff` | the saved `takeoff.*` results: item, group, length or area or count, unit, source layer, handles, rounding rule | `takeoff.list`, `bridge.export`, `takeoff.record` |
| `AuditEvent` | request number, tool, arguments, outcome, drawing revision before and after | `agent::AuditEntry`, extended with the revision |

An estimate built from a take-off must record the `DrawingRevision` it used, the measurement rules
(metric rounding to 1 cm, imperial to 1/8 in, deduction layers) and the rate schedule. Tren can
supply the first two; the third belongs to Falcon.

## 4. The AI boundary

Tren's rule is that the model proposes and deterministic commands calculate ([ai-agent.md](../ai-agent.md)).
For a shared runtime that means:

- Tren registers its tools under a `cad.*` namespace with the schemas it already validates. The
  registry already declares a risk class and whether a call needs confirmation; a shared runtime
  should read those, not replace them.
- Write tools stay behind `Session::execute`, so undo, the locked-layer rules and the
  panic guard (`guard.rs`) apply to every caller.
- Offline mode: Tren's assistant runs a local model (`aad-llm`, llama.cpp) and sends no drawing
  content anywhere. A hosted model is not wired in and nothing in Tren requires one.
- The runtime must not get raw document access, arbitrary paths or SQL. The control channel and MCP
  already limit callers to named commands; the suite should use them as they are.

## 5. First milestone against Tren

Milestone 1 is "drawing to draft estimate". Tren's side, step by step:

| Step | Tren today | Gap |
|---|---|---|
| Open the project's linked drawing | `app.open` with paths from Myna | Myna supplies the paths; Tren does not look up drawing files by project id |
| Resolve the same project in Falcon | OSAS `project_ref` when the drawing is linked via `companyfile` | CoreOp Import / Take-off consume the ref; Windows GUI E2E still open |
| Extract measurements | `takeoff.start` and `takeoff.*` over `MEASURE-*` layers | Needs a stated list of supported geometry; unsupported objects must be reported, not skipped silently (today `takeoff` reads the measure layers only) |
| Pass them on | `bridge.export` / `takeoff.record` | Done on Draft side; CoreOp reader built; product E2E open |
| Calculate quantities and cost | Falcon | Not Tren's part (Studio surfaces estimate + RA totals on fee/invoice) |
| Draft fee / invoice in Kivo | Kivo | Not Tren's part |
| Reject unauthorised writes, keep an audit trail | agent confirmation policy and audit log | Done: the audit is persisted beside the drawing (section 6, step 4); sharing it across applications is not built |

## 6. Order of work in Tren

1. `DrawingRevision` at save, written beside the file; checked by `drawing.revision`. **Done.**
2. Rooms and the revision in `bridge.export`, as additive keys of format version 1. **Done.** CoreOp accepts extra keys / `project_ref` on import.
3. Register the existing take-off, room and revision commands as agent tools (read-only
   ones first), with schemas, in the existing registry. **Done** for the read-only ones (`get_takeoff_result`, `get_room_log`, `get_room_area_total`, `get_drawing_revision`, `check_permit_layers`). Commands that draw into the drawing (`schedule`, running a take-off) stay planned.
4. Persist the audit log next to the drawing revision it refers to. **Done**: `<drawing>.aadaudit.jsonl`, one line per proposed call.
5. **Next:** interactive company-file project picker UI; product E2E with CoreOp/Studio; scanned-PDF underlay polish.

## 7. Not decided here

The shared repository layout, the IPC protocol between applications, the shared identity and role
model, the indexing service and the installer coordination belong to a specification that covers all
suite repositories. Signing certs, release pins, and counsel review of dual licences
on AGPL apps are suite ops — not Tren licence changes (Tren stays MIT OR Apache-2.0).
