//! The allow-listed tool registry. Each tool is one existing engine command with a strict
//! argument schema; the model never reaches anything outside this list.

use aad_engine::Session;
use serde_json::{Map, Value, json};

/// How much a call can change, which decides whether the user must confirm it first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    /// Reads the drawing only.
    Read,
    /// Opens another drawing (a repaired copy of a file); nothing existing is changed.
    Open,
    /// Changes the open drawing (one undo step).
    Mutating,
    /// Removes definitions from the open drawing (one undo step).
    Destructive,
}

impl Risk {
    /// Whether the application must get the user's confirmation before running the call.
    pub fn needs_confirmation(self) -> bool {
        self >= Risk::Mutating
    }
}

type RiskFn = fn(&Map<String, Value>) -> Risk;
type RunFn = fn(&mut Session, &Map<String, Value>) -> Result<Value, String>;

/// A registered tool.
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema (draft 2020-12 subset, see [`crate::validate`]) of the arguments.
    pub schema: fn() -> Value,
    pub risk: RiskFn,
    /// A short description of what a confirmation would allow, shown to the user.
    pub summary: fn(&Map<String, Value>) -> String,
    pub run: RunFn,
}

/// Tools that are planned but not built. They are not offered to the model and a call to one is
/// refused as not implemented.
pub const PLANNED: &[&str] = &["find_openings", "generate_schedule", "run_quantity_takeoff", "check_missing_attributes", "export_table"];

fn exec(s: &mut Session, id: &str, args: Value) -> Result<Value, String> {
    s.execute(id, &args).map_err(|e| e.to_string())
}

fn empty_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn purge_types() -> Vec<&'static str> {
    vec!["all", "blocks", "dimstyles", "groups", "layers", "linetypes", "mleaderstyles", "tablestyles", "textstyles", "zerolength", "emptytext"]
}

/// The registered tools, in a stable order.
pub fn registry() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "get_drawing_summary",
            description: "Report the open drawing's units, layers, blocks, object counts and what flattening did when it was opened.",
            schema: empty_schema,
            risk: |_| Risk::Read,
            summary: |_| "Read the drawing summary".into(),
            run: |s, _| {
                let mut r = exec(s, "drawing.inspect", json!({}))?;
                if let (Some(o), Some(f)) = (r.as_object_mut(), s.last_flatten.as_ref()) {
                    o.insert("flattenOnImport".into(), f.to_json());
                }
                Ok(r)
            },
        },
        ToolDef {
            name: "get_takeoff_result",
            description: "List the saved quantity take-offs, or read one by name (the measured items with their lengths, areas and counts, as the take-off commands saved them). It reports what was measured; it never measures or estimates.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "name": { "type": "string", "maxLength": 64, "description": "the take-off to read, e.g. Bricks, Columns, Beams, Slab; omit to list the saved ones" } },
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Read,
            summary: |_| "Read a saved take-off".into(),
            run: |s, a| exec(s, "takeoff.list", Value::Object(a.clone())),
        },
        ToolDef {
            name: "get_room_log",
            description: "List the room labels in the current space: name, size text, area and position. Rooms are paired from the labels the ROOM command drew; nothing is calculated here.",
            schema: empty_schema,
            risk: |_| Risk::Read,
            summary: |_| "Read the room log".into(),
            run: |s, _| exec(s, "room.log", json!({})),
        },
        ToolDef {
            name: "get_room_area_total",
            description: "Add up the areas of the room labels in the current space, optionally only rooms whose name contains a word (for example BEDROOM).",
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "filter": { "type": "string", "maxLength": 64, "description": "only rooms whose name contains this text; omit for all rooms" } },
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Read,
            summary: |_| "Add up room areas".into(),
            run: |s, a| exec(s, "room.total", Value::Object(a.clone())),
        },
        ToolDef {
            name: "get_drawing_revision",
            description: "Say whether the drawing file on disk is the one its last save recorded: current, changed since, or never recorded, with its SHA-256. Use it to name exactly which drawing a result came from.",
            schema: empty_schema,
            risk: |_| Risk::Read,
            summary: |_| "Check the drawing file's revision".into(),
            run: |s, _| exec(s, "drawing.revision", json!({})),
        },
        ToolDef {
            name: "check_permit_layers",
            description: "Check the building-permission AP- layers before an AutoPlan submission: which required layers have a closed polyline, and which have open ones. Reports PASS, WARN or FAIL.",
            schema: empty_schema,
            risk: |_| Risk::Read,
            summary: |_| "Check the AP- permit layers".into(),
            run: |s, _| exec(s, "bpltcheck", json!({})),
        },
        ToolDef {
            name: "get_drawing_digest",
            description: "Compact drawing digest for AI: object counts by type/layer, extents, room tally, schedule opening count. Prefer this over dumping the whole drawing into the prompt.",
            schema: empty_schema,
            risk: |_| Risk::Read,
            summary: |_| "Read the drawing digest".into(),
            run: |s, _| exec(s, "ai.digest", json!({})),
        },
        ToolDef {
            name: "query_context_cards",
            description: "Capped ContextCards (id, layer, type, bboxMm, lengthMm/geom) for a window of the open drawing. Default window is the current view. Filter with layers/types (e.g. A-DOOR, A-WALL, Insert).",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "window": { "type": "array", "items": { "type": "number" }, "maxItems": 4, "description": "[x0,y0,x1,y1]; omit for current view" },
                        "layers": { "type": "array", "items": { "type": "string", "maxLength": 64 }, "maxItems": 20 },
                        "types": { "type": "array", "items": { "type": "string", "maxLength": 32 }, "maxItems": 10 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 24 }
                    },
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Read,
            summary: |_| "Query context cards in a window".into(),
            run: |s, a| exec(s, "ai.cards", Value::Object(a.clone())),
        },
        ToolDef {
            name: "get_spatial_overview",
            description: "Where things are: the drawing's extents, the part on screen, and a grid showing how many objects of which types and layers lie in each part. Row 0 is the bottom (smallest Y), column 0 the left (smallest X). Use it first to find out where to look.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "cols": { "type": "integer", "minimum": 1, "maximum": 12, "description": "grid columns (default 6)" },
                        "rows": { "type": "integer", "minimum": 1, "maximum": 12, "description": "grid rows (default 4)" }
                    },
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Read,
            summary: |_| "Read where objects are in the drawing".into(),
            run: |s, a| exec(s, "spatial.overview", Value::Object(a.clone())),
        },
        ToolDef {
            name: "find_objects_in_window",
            description: "List the objects in a rectangle of the drawing (drawing units, Y up): handle, type, layer, bounds, centre and key geometry. Use it to answer what is at, near or inside a place.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "window": { "type": "array", "items": { "type": "number" }, "maxItems": 4, "description": "[x0, y0, x1, y1]: two opposite corners" },
                        "mode": { "type": "string", "enum": ["crossing", "inside"], "description": "crossing: touching the window (default); inside: wholly within it" },
                        "layers": { "type": "array", "items": { "type": "string", "maxLength": 64 }, "maxItems": 20, "description": "only these layers" },
                        "types": { "type": "array", "items": { "type": "string", "maxLength": 32 }, "maxItems": 10, "description": "only these object types, e.g. Line, Circle, Text, Block Reference" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "most objects to list (default 25)" }
                    },
                    "required": ["window"],
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Read,
            summary: |_| "Read the objects in a window of the drawing".into(),
            run: |s, a| exec(s, "spatial.query", Value::Object(a.clone())),
        },
        ToolDef {
            name: "find_nearest_objects",
            description: "The objects nearest a point (drawing units, Y up), closest first, with their distance to the point.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "point": { "type": "array", "items": { "type": "number" }, "maxItems": 2, "description": "[x, y]" },
                        "count": { "type": "integer", "minimum": 1, "maximum": 20, "description": "how many (default 5)" },
                        "layers": { "type": "array", "items": { "type": "string", "maxLength": 64 }, "maxItems": 20 },
                        "types": { "type": "array", "items": { "type": "string", "maxLength": 32 }, "maxItems": 10 }
                    },
                    "required": ["point"],
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Read,
            summary: |_| "Read the objects nearest a point".into(),
            run: |s, a| exec(s, "spatial.nearest", Value::Object(a.clone())),
        },
        ToolDef {
            name: "describe_objects",
            description: "Details of objects by handle: type, layer, bounds, centre, and their geometry (ends of a line, radius of a circle, words of a text, name of a block).",
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "handles": { "type": "array", "items": { "type": "string", "maxLength": 16 }, "maxItems": 20, "description": "handles as returned by the other tools" } },
                    "required": ["handles"],
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Read,
            summary: |_| "Read details of objects".into(),
            run: |s, a| exec(s, "spatial.describe", Value::Object(a.clone())),
        },
        ToolDef {
            name: "flatten_drawing",
            description: "Flatten the whole open drawing to 2D (elevations, thickness and z values removed). Reports 3D objects that cannot be flattened. One undo step.",
            schema: empty_schema,
            risk: |_| Risk::Mutating,
            summary: |_| "Flatten every object of the open drawing to 2D".into(),
            run: |s, _| exec(s, "flatten", json!({ "all": true })),
        },
        ToolDef {
            name: "audit_drawing",
            description: "Check the open drawing for database errors. With fix=true, repair them (one undo step); without it, only report. Auditor v0 detect path uses fix=false.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "fix": { "type": "boolean", "description": "repair the errors found (default false: report only)" } },
                    "additionalProperties": false
                })
            },
            risk: |a| if a.get("fix") == Some(&Value::Bool(true)) { Risk::Mutating } else { Risk::Read },
            summary: |a| {
                if a.get("fix") == Some(&Value::Bool(true)) {
                    "Audit the drawing and repair the errors found".into()
                } else {
                    "Audit the drawing (report only)".into()
                }
            },
            run: |s, a| {
                if a.get("fix") == Some(&Value::Bool(true)) { exec(s, "audit", json!({ "fix": true })) } else { aad_auditor::audit_drawing(s) }
            },
        },
        ToolDef {
            name: "validate_drawing",
            description: "Validate the open drawing: audit plus entity-type counts. With fix=true, repair audit issues (one undo step); without it, detect-only (Auditor v0).",
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "fix": { "type": "boolean", "description": "repair audit errors found (default false: report only)" } },
                    "additionalProperties": false
                })
            },
            risk: |a| if a.get("fix") == Some(&Value::Bool(true)) { Risk::Mutating } else { Risk::Read },
            summary: |a| {
                if a.get("fix") == Some(&Value::Bool(true)) {
                    "Validate the drawing and repair the errors found".into()
                } else {
                    "Validate the drawing (report only)".into()
                }
            },
            run: |s, a| {
                if a.get("fix") == Some(&Value::Bool(true)) { exec(s, "validate", json!({ "fix": true })) } else { aad_auditor::validate_drawing(s) }
            },
        },
        ToolDef {
            name: "check_command_parity",
            description: "Detect-only: compare the reference menu catalog to registered commands (same breadth metric as cargo xtask parity). Needs the Tren source tree (AAD_REPO_ROOT or cwd). Does not rewrite docs/parity-checklist.md.",
            schema: empty_schema,
            risk: |_| Risk::Read,
            summary: |_| "Check command/menu parity (detect only)".into(),
            run: |_s, _| {
                let root = aad_auditor::repo_root_or_err()?;
                aad_auditor::check_command_parity(&root)
            },
        },
        ToolDef {
            name: "check_command_menu_coverage",
            description: "Detect-only: same as check_command_parity — report which reference menu items lack a matching registered command.",
            schema: empty_schema,
            risk: |_| Risk::Read,
            summary: |_| "Check command/menu coverage (detect only)".into(),
            run: |_s, _| {
                let root = aad_auditor::repo_root_or_err()?;
                aad_auditor::check_command_menu_coverage(&root)
            },
        },
        ToolDef {
            name: "check_ui_tokens",
            description: "Detect-only: scan apps/aad-winui for window-template and hard-coded dimension problems (same rules as cargo xtask ui). Needs the Tren source tree.",
            schema: empty_schema,
            risk: |_| Risk::Read,
            summary: |_| "Check WinUI design-system tokens (detect only)".into(),
            run: |_s, _| {
                let root = aad_auditor::repo_root_or_err()?;
                aad_auditor::check_ui_tokens(&root)
            },
        },
        ToolDef {
            name: "auditor_report",
            description: "Auditor v0: one JSON detect report (audit, validate, command/menu parity, UI tokens). Never repairs. Screenshot capture is documented in the report; use the live Windows control ui.screenshot / MCP screenshot separately.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "checks": {
                            "type": "string",
                            "maxLength": 64,
                            "description": "comma list: audit,validate,parity,ui (default all)"
                        }
                    },
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Read,
            summary: |_| "Run Auditor v0 detect report".into(),
            run: |s, a| {
                let checks = aad_auditor::DetectChecks::parse(a.get("checks").and_then(Value::as_str).unwrap_or(""))?;
                let root = if checks.parity || checks.ui_tokens { Some(aad_auditor::repo_root_or_err()?) } else { None };
                let report = aad_auditor::run_detect(s, root.as_deref(), checks)?;
                Ok(report.value)
            },
        },
        ToolDef {
            name: "purge_unused",
            description: "Remove unused named definitions (blocks, layers, linetypes, styles, groups) from the open drawing. One undo step.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "enum": purge_types(), "description": "kind of definition to purge (default all)" },
                        "names": { "type": "string", "maxLength": 256, "description": "name pattern such as \"A*,B\" (default *)" }
                    },
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Destructive,
            summary: |a| {
                format!(
                    "Remove unused {} definitions matching `{}`",
                    a.get("type").and_then(Value::as_str).unwrap_or("all"),
                    a.get("names").and_then(Value::as_str).unwrap_or("*")
                )
            },
            run: |s, a| exec(s, "purge", Value::Object(a.clone())),
        },
        ToolDef {
            name: "recover_file",
            description: "Open a damaged DXF or DWG file as a repaired copy in a new tab. The file on disk is not changed.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "path": { "type": "string", "minLength": 1, "maxLength": 1024, "description": "path of the file to recover" } },
                    "required": ["path"],
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Open,
            summary: |a| format!("Recover {}", a.get("path").and_then(Value::as_str).unwrap_or("a file")),
            run: |s, a| exec(s, "recover", Value::Object(a.clone())),
        },
        ToolDef {
            name: "set_drawing_units",
            description: "Set the drawing unit. mode=metadata changes only the unit setting (geometry is not scaled); mode=scale rescales every model-space object. The mode is required.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "units": { "type": "string", "enum": ["mm", "cm", "m", "in", "ft"] },
                        "mode": { "type": "string", "enum": ["metadata", "scale"] }
                    },
                    "required": ["units", "mode"],
                    "additionalProperties": false
                })
            },
            risk: |a| if a.get("mode") == Some(&Value::String("scale".into())) { Risk::Destructive } else { Risk::Mutating },
            summary: |a| {
                let u = a.get("units").and_then(Value::as_str).unwrap_or("?");
                if a.get("mode") == Some(&Value::String("scale".into())) {
                    format!("Change the drawing unit to {u} AND rescale every model-space object")
                } else {
                    format!("Change only the drawing unit setting to {u} (geometry is not scaled)")
                }
            },
            run: |s, a| exec(s, "drawing.units", Value::Object(a.clone())),
        },
        ToolDef {
            name: "set_display_units",
            description: "Choose the unit dimension text shows measurements in. Geometry is not changed.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "units": { "type": "string", "enum": ["mm", "cm", "m", "in", "ft", "ftin"] } },
                    "required": ["units"],
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Mutating,
            summary: |a| format!("Show dimensions in {}", a.get("units").and_then(Value::as_str).unwrap_or("?")),
            run: |s, a| exec(s, "display.units", Value::Object(a.clone())),
        },
        ToolDef {
            name: "set_plot_configuration",
            description: "Set the plot scale and paper size and size dimension and text annotation from the printed size (default 2.5 mm). Geometry is not changed.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "scale": { "type": "string", "maxLength": 12, "description": "for example 1:100" },
                        "paper": { "type": "string", "enum": ["A0", "A1", "A2", "A3", "A4"] },
                        "textMm": { "type": "number", "minimum": 1, "maximum": 20 },
                        "arrowMm": { "type": "number", "minimum": 1, "maximum": 20 }
                    },
                    "required": ["scale", "paper"],
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Mutating,
            summary: |a| {
                format!(
                    "Set the plot to {} on {} and resize annotation",
                    a.get("scale").and_then(Value::as_str).unwrap_or("?"),
                    a.get("paper").and_then(Value::as_str).unwrap_or("?")
                )
            },
            run: |s, a| exec(s, "plot.config", Value::Object(a.clone())),
        },
        ToolDef {
            name: "validate_plot_layout",
            description: "Check the drawing against the configured plot: does it fit the sheet, text that prints too small or large, annotation conflicts.",
            schema: empty_schema,
            risk: |_| Risk::Read,
            summary: |_| "Validate the plot layout".into(),
            run: |s, _| exec(s, "plot.validate", json!({})),
        },
        ToolDef {
            name: "analyze_annotation_collisions",
            description: "List text that overlaps other text, dimensions, blocks, leaders or drawing geometry. Changes nothing.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "clearance": { "type": "number", "minimum": 0 } },
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Read,
            summary: |_| "Analyze annotation collisions".into(),
            run: |s, a| exec(s, "annotation.collisions", Value::Object(a.clone())),
        },
        ToolDef {
            name: "preview_layout_changes",
            description: "Calculate the text moves that would resolve annotation conflicts, without applying them. Only text moves; geometry never does.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "clearance": { "type": "number", "minimum": 0 },
                        "includeManual": { "type": "boolean" }
                    },
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Read,
            summary: |_| "Preview annotation layout changes".into(),
            run: |s, a| {
                let mut args = a.clone();
                args.insert("mode".into(), json!("preview"));
                exec(s, "annotation.layout", Value::Object(args))
            },
        },
        ToolDef {
            name: "calculate_annotation_layout",
            description: "Same as preview_layout_changes: calculate where conflicting text would move, without applying it.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "clearance": { "type": "number", "minimum": 0 }, "includeManual": { "type": "boolean" } },
                    "additionalProperties": false
                })
            },
            risk: |_| Risk::Read,
            summary: |_| "Calculate annotation layout".into(),
            run: |s, a| {
                let mut args = a.clone();
                args.insert("mode".into(), json!("preview"));
                exec(s, "annotation.layout", Value::Object(args))
            },
        },
        ToolDef {
            name: "apply_layout_changes",
            description: "Move conflicting text into clear space. Text placed by hand is skipped unless includeManual is true. One undo step.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "clearance": { "type": "number", "minimum": 0 }, "includeManual": { "type": "boolean" } },
                    "additionalProperties": false
                })
            },
            risk: |a| if a.get("includeManual") == Some(&Value::Bool(true)) { Risk::Destructive } else { Risk::Mutating },
            summary: |a| {
                if a.get("includeManual") == Some(&Value::Bool(true)) {
                    "Move conflicting text, including text placed by hand".into()
                } else {
                    "Move conflicting text (text placed by hand is left alone)".into()
                }
            },
            run: |s, a| {
                let mut args = a.clone();
                args.insert("mode".into(), json!("apply"));
                exec(s, "annotation.layout", Value::Object(args))
            },
        },
    ]
}

/// The tool named `name`.
pub fn find(name: &str) -> Option<ToolDef> {
    registry().into_iter().find(|t| t.name == name)
}

/// The tools in the function-calling format of the model's chat template.
pub fn function_schemas() -> Value {
    Value::Array(
        registry()
            .iter()
            .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": (t.schema)() } }))
            .collect(),
    )
}
