//! Node types: their ports, configuration rules and evaluation. Every type has a stable id and an
//! implementation version; a graph that names a version this program does not implement is
//! refused with [`Code::OutdatedSchema`].

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Value as Json, json};

use crate::diag::{Code, Diagnostic};
use crate::formula;
use crate::graph::Node;
use crate::schema::{self, ComponentType};
use crate::types::{DataType, Value, WallValue};
use crate::units::{self, Dim};
use crate::{Context, MAX_PATH_POINTS};

/// What evaluating a node does besides computing its outputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Effect {
    Pure,
    /// Produces geometry the host commits through its own command and undo system.
    Geometry,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortSpec {
    pub id: String,
    pub label: String,
    pub data_type: DataType,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSpec {
    pub node_type: &'static str,
    pub node_version: u32,
    pub label: &'static str,
    pub category: &'static str,
    pub effect: Effect,
    /// Reads the project context (levels): part of its cache key.
    pub reads_context: bool,
    pub inputs: Vec<PortSpec>,
    pub outputs: Vec<PortSpec>,
}

impl NodeSpec {
    pub fn input(&self, id: &str) -> Option<&PortSpec> {
        self.inputs.iter().find(|p| p.id == id)
    }
    pub fn output(&self, id: &str) -> Option<&PortSpec> {
        self.outputs.iter().find(|p| p.id == id)
    }
}

/// Where a value came from, in precedence order (later wins).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Source {
    /// The component schema's default.
    Default,
    /// A project rule, such as the level's height.
    Project { rule: String },
    /// An override stored on the instance.
    Override,
    /// A graph connection.
    Graph { node_id: String, port_id: String },
    /// Computed by this node.
    Computed,
    /// Entered on this node (a parameter node).
    Input,
}

fn port(id: &str, label: &str, data_type: DataType, required: bool) -> PortSpec {
    PortSpec { id: id.into(), label: label.into(), data_type, required }
}

pub const NUMBER: &str = "param.number";
pub const TOGGLE: &str = "param.toggle";
pub const CHOICE: &str = "param.choice";
pub const FORMULA: &str = "math.formula";
pub const PATH: &str = "geom.path";
pub const WALL: &str = "component.wall";
pub const MEASURE_WALL: &str = "measure.wall";

/// An entry of the node library: the type, its version and a starting configuration.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub node_type: &'static str,
    pub node_version: u32,
    pub label: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub config: Json,
}

pub fn library() -> Vec<LibraryEntry> {
    vec![
        LibraryEntry {
            node_type: NUMBER,
            node_version: 1,
            label: "Number",
            category: "Inputs",
            description: "A number with a unit, shown as a slider or a number box",
            config: json!({ "value": 3.0, "unit": "m", "min": 2.4, "max": 6.0, "step": 0.05, "control": "slider" }),
        },
        LibraryEntry {
            node_type: TOGGLE,
            node_version: 1,
            label: "Toggle",
            category: "Inputs",
            description: "True or false",
            config: json!({ "value": false }),
        },
        LibraryEntry {
            node_type: CHOICE,
            node_version: 1,
            label: "Choice",
            category: "Inputs",
            description: "One of a list of stable ids",
            config: json!({ "options": [{ "id": "a", "label": "A" }], "value": "a" }),
        },
        LibraryEntry {
            node_type: FORMULA,
            node_version: 1,
            label: "Formula",
            category: "Math",
            description: "A unit-checked formula over named inputs",
            config: json!({ "expr": "a + 150 mm", "inputs": [{ "name": "a", "dimension": "length" }] }),
        },
        LibraryEntry {
            node_type: PATH,
            node_version: 1,
            label: "Path",
            category: "Geometry",
            description: "A centre line: points, or a line or polyline in the drawing",
            config: json!({ "points": [[0.0, 0.0], [5.0, 0.0]], "closed": false }),
        },
        LibraryEntry {
            node_type: WALL,
            node_version: 1,
            label: "Wall",
            category: "Components",
            description: "A wall component (osas.wall, schema 1): path, height, thickness, material, finish",
            config: json!({ "componentType": schema::WALL_TYPE, "schemaVersion": 1, "instanceId": "wall-1", "overrides": {}, "locked": [] }),
        },
        LibraryEntry {
            node_type: MEASURE_WALL,
            node_version: 1,
            label: "Wall measurements",
            category: "Takeoff",
            description: "Length, gross face area and gross volume from the wall geometry (not net of openings; Falcon measures the BOQ)",
            config: json!({}),
        },
    ]
}

fn cfg_err(node: &Node, msg: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Code::InvalidConfig, Some(&node.node_id), None, msg)
}

/// A number in the node's configuration: `{value, unit}` or a quantity text such as `"3.2 m"`.
fn cfg_quantity(v: &Json, unit_name: &str) -> Option<(f64, Dim)> {
    match v {
        Json::Number(n) => units::to_si(n.as_f64()?, unit_name),
        Json::String(s) => units::parse_quantity(s),
        _ => None,
    }
}

fn number_unit(node: &Node) -> Result<(String, Dim), Diagnostic> {
    let u = node.config.get("unit").and_then(Json::as_str).unwrap_or("");
    let (d, _) = units::unit(u).ok_or_else(|| cfg_err(node, format!("unknown unit `{u}`")))?;
    Ok((u.to_string(), d))
}

fn formula_inputs(node: &Node) -> Result<Vec<(String, Dim)>, Diagnostic> {
    let mut out = Vec::new();
    for i in node.config.get("inputs").and_then(Json::as_array).into_iter().flatten() {
        let name = i.get("name").and_then(Json::as_str).unwrap_or("");
        if !formula::valid_input_name(name) {
            return Err(cfg_err(node, format!("`{name}` cannot name a formula input (letters, digits and `_`, not a unit or function)")));
        }
        if out.iter().any(|(n, _)| n == name) {
            return Err(cfg_err(node, format!("input `{name}` is listed twice")));
        }
        let dname = i.get("dimension").and_then(Json::as_str).unwrap_or("number");
        let d = Dim::from_name(dname).ok_or_else(|| cfg_err(node, format!("unknown dimension `{dname}`")))?;
        out.push((name.to_string(), d));
        if out.len() > 32 {
            return Err(cfg_err(node, "a formula has at most 32 inputs"));
        }
    }
    Ok(out)
}

fn formula_parts(node: &Node) -> Result<(formula::Expr, Vec<(String, Dim)>, Dim), Diagnostic> {
    let inputs = formula_inputs(node)?;
    let text = node.config.get("expr").and_then(Json::as_str).unwrap_or("");
    let bad = |e: formula::FormulaError| {
        let code = if matches!(e, formula::FormulaError::UnitMismatch(_)) { Code::UnitMismatch } else { Code::InvalidFormula };
        Diagnostic::error(code, Some(&node.node_id), None, format!("formula `{text}`: {e}"))
    };
    let expr = formula::parse(text).map_err(bad)?;
    let dims: BTreeMap<String, Dim> = inputs.iter().cloned().collect();
    let out = formula::infer(&expr, &dims).map_err(bad)?;
    Ok((expr, inputs, out))
}

fn wall_type(node: &Node) -> Result<ComponentType, Diagnostic> {
    let id = node.config.get("componentType").and_then(Json::as_str).unwrap_or(schema::WALL_TYPE);
    let v = node.config.get("schemaVersion").and_then(Json::as_u64).unwrap_or(0);
    if id != schema::WALL_TYPE {
        return Err(cfg_err(node, format!("a wall node needs component type `{}`, not `{id}`", schema::WALL_TYPE)));
    }
    let v = u32::try_from(v).unwrap_or(u32::MAX);
    schema::component_type(id, v).ok_or_else(|| {
        Diagnostic::error(
            Code::OutdatedSchema,
            Some(&node.node_id),
            None,
            format!("component type `{id}` schema version {v} is not known to this program (it knows 1)"),
        )
    })
}

/// The ports of `node`, which can depend on its configuration.
pub fn spec(node: &Node) -> Result<NodeSpec, Diagnostic> {
    let known = |v: u32| {
        if node.node_version == v {
            Ok(())
        } else {
            Err(Diagnostic::error(
                Code::OutdatedSchema,
                Some(&node.node_id),
                None,
                format!("`{}` version {} is not implemented (this program has version {v})", node.node_type, node.node_version),
            ))
        }
    };
    let base = |t: &'static str, label: &'static str, category: &'static str| NodeSpec {
        node_type: t,
        node_version: 1,
        label,
        category,
        effect: Effect::Pure,
        reads_context: false,
        inputs: vec![],
        outputs: vec![],
    };
    let length = DataType::number(Dim::LENGTH);
    match node.node_type.as_str() {
        NUMBER => {
            known(1)?;
            let (_, d) = number_unit(node)?;
            Ok(NodeSpec { outputs: vec![port("value", "Value", DataType::number(d), false)], ..base(NUMBER, "Number", "Inputs") })
        }
        TOGGLE => {
            known(1)?;
            Ok(NodeSpec { outputs: vec![port("value", "Value", DataType::Boolean, false)], ..base(TOGGLE, "Toggle", "Inputs") })
        }
        CHOICE => {
            known(1)?;
            Ok(NodeSpec { outputs: vec![port("value", "Value", DataType::Enum, false)], ..base(CHOICE, "Choice", "Inputs") })
        }
        FORMULA => {
            known(1)?;
            let (_, inputs, out) = formula_parts(node)?;
            Ok(NodeSpec {
                inputs: inputs.iter().map(|(n, d)| port(n, n, DataType::number(*d), true)).collect(),
                outputs: vec![port("result", "Result", DataType::number(out), false)],
                ..base(FORMULA, "Formula", "Math")
            })
        }
        PATH => {
            known(1)?;
            Ok(NodeSpec { outputs: vec![port("path", "Path", DataType::Curve, false)], ..base(PATH, "Path", "Geometry") })
        }
        WALL => {
            known(1)?;
            let t = wall_type(node)?;
            Ok(NodeSpec {
                effect: Effect::Geometry,
                reads_context: true,
                inputs: t.params.iter().map(|p| port(p.id, p.label, p.data_type.clone(), p.required)).collect(),
                outputs: vec![
                    port("wall", "Wall", DataType::Wall, false),
                    port("length", "Length", length.clone(), false),
                    port("height", "Height", length.clone(), false),
                    port("thickness", "Thickness", length, false),
                    port("material", "Material", DataType::Enum, false),
                    port("finish", "Finish", DataType::Enum, false),
                ],
                ..base(WALL, "Wall", "Components")
            })
        }
        MEASURE_WALL => {
            known(1)?;
            Ok(NodeSpec {
                inputs: vec![port("wall", "Wall", DataType::Wall, true)],
                outputs: vec![
                    port("length", "Length", DataType::number(Dim::LENGTH), false),
                    port("grossFaceArea", "Gross face area", DataType::number(Dim::AREA), false),
                    port("grossVolume", "Gross volume", DataType::number(Dim::VOLUME), false),
                ],
                ..base(MEASURE_WALL, "Wall measurements", "Takeoff")
            })
        }
        other => Err(Diagnostic::error(Code::UnknownNodeType, Some(&node.node_id), None, format!("unknown node type `{other}`"))),
    }
}

/// What a node produced: outputs with their sources, and warnings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Produced {
    pub outputs: BTreeMap<String, (Value, Source)>,
    pub warnings: Vec<Diagnostic>,
}

impl Produced {
    fn put(&mut self, port: &str, v: Value, src: Source) {
        self.outputs.insert(port.to_string(), (v, src));
    }
}

/// An input as it arrives: the value and where it came from.
pub type Inputs = BTreeMap<String, (Value, Source)>;

fn path_length(points: &[[f64; 2]], closed: bool) -> f64 {
    let seg = |a: &[f64; 2], b: &[f64; 2]| ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    let mut l: f64 = points.windows(2).map(|w| seg(&w[0], &w[1])).sum();
    if closed
        && points.len() > 2
        && let (Some(a), Some(b)) = (points.last(), points.first())
    {
        l += seg(a, b);
    }
    l
}

fn read_points(node: &Node) -> Result<Vec<[f64; 2]>, Diagnostic> {
    let arr = node.config.get("points").and_then(Json::as_array).ok_or_else(|| cfg_err(node, "`points` ([[x, y], ...] in metres) is required"))?;
    if arr.len() < 2 || arr.len() > MAX_PATH_POINTS {
        return Err(cfg_err(node, format!("a path has 2 to {MAX_PATH_POINTS} points")));
    }
    arr.iter()
        .map(|p| {
            let x = p.get(0).and_then(Json::as_f64).filter(|v| v.is_finite());
            let y = p.get(1).and_then(Json::as_f64).filter(|v| v.is_finite());
            x.zip(y).map(|(x, y)| [x, y]).ok_or_else(|| cfg_err(node, "every point is two finite numbers"))
        })
        .collect()
}

fn range_check(node: &Node, port: Option<&str>, what: &str, v: f64, min: Option<f64>, max: Option<f64>, unit_name: &str) -> Result<(), Diagnostic> {
    let show = |x: f64| {
        let d = units::unit(unit_name).map_or(Dim::NONE, |(d, _)| d);
        let shown = units::from_si(x, d, unit_name).unwrap_or(x);
        format!("{shown} {unit_name}").trim().to_string()
    };
    if min.is_some_and(|m| v < m - 1e-12) || max.is_some_and(|m| v > m + 1e-12) {
        let range = format!("{} to {}", min.map_or("-".into(), show), max.map_or("-".into(), show));
        return Err(Diagnostic::error(Code::OutOfRange, Some(&node.node_id), port, format!("{what} {} is outside {range}", show(v))));
    }
    Ok(())
}

/// Evaluate one node. `inputs` holds the connected inputs that have values.
pub fn evaluate(node: &Node, ctx: &Context, inputs: &Inputs) -> Result<Produced, Diagnostic> {
    let mut out = Produced::default();
    match node.node_type.as_str() {
        NUMBER => {
            let (u, _) = number_unit(node)?;
            let raw = node.config.get("value").ok_or_else(|| cfg_err(node, "`value` is required"))?;
            let (v, d) = cfg_quantity(raw, &u).ok_or_else(|| cfg_err(node, format!("`value` must be a number in `{u}`")))?;
            let (ud, _) = units::unit(&u).unwrap_or((Dim::NONE, 1.0));
            if d != ud {
                return Err(Diagnostic::error(
                    Code::UnitMismatch,
                    Some(&node.node_id),
                    None,
                    format!("`value` is a {}, the node gives a {}", d.name(), ud.name()),
                ));
            }
            let bound = |k: &str| node.config.get(k).and_then(|b| cfg_quantity(b, &u)).map(|(x, _)| x);
            range_check(node, None, "value", v, bound("min"), bound("max"), &u)?;
            out.put("value", Value::Number { value: v, dim: d }, Source::Input);
        }
        TOGGLE => {
            let v = node.config.get("value").and_then(Json::as_bool).ok_or_else(|| cfg_err(node, "`value` must be true or false"))?;
            out.put("value", Value::Boolean { value: v }, Source::Input);
        }
        CHOICE => {
            let v = node.config.get("value").and_then(Json::as_str).ok_or_else(|| cfg_err(node, "`value` (an option id) is required"))?;
            let ok = node.config.get("options").and_then(Json::as_array).into_iter().flatten().any(|o| o.get("id").and_then(Json::as_str) == Some(v));
            if !ok {
                return Err(cfg_err(node, format!("`{v}` is not one of the options")));
            }
            out.put("value", Value::Enum { value: v.to_string() }, Source::Input);
        }
        FORMULA => {
            let (expr, ins, dim) = formula_parts(node)?;
            let mut vars = BTreeMap::new();
            for (name, _) in &ins {
                let (v, _) = inputs
                    .get(name)
                    .and_then(|(v, _)| v.number())
                    .ok_or_else(|| Diagnostic::error(Code::MissingInput, Some(&node.node_id), Some(name), format!("input `{name}` has no value")))?;
                vars.insert(name.clone(), v);
            }
            let v = formula::eval(&expr, &vars).map_err(|e| {
                let code = if e == formula::FormulaError::DivisionByZero { Code::DivisionByZero } else { Code::InvalidFormula };
                Diagnostic::error(code, Some(&node.node_id), None, e.to_string())
            })?;
            out.put("result", Value::Number { value: v, dim }, Source::Computed);
        }
        PATH => {
            let points = read_points(node)?;
            let closed = node.config.get("closed").and_then(Json::as_bool).unwrap_or(false);
            if path_length(&points, closed) <= 1e-9 {
                return Err(Diagnostic::error(Code::GeometryFailure, Some(&node.node_id), None, "the path has no length"));
            }
            out.put("path", Value::Curve { points, closed }, Source::Input);
        }
        WALL => evaluate_wall(node, ctx, inputs, &mut out)?,
        MEASURE_WALL => {
            let Some((Value::Wall { wall }, _)) = inputs.get("wall") else {
                return Err(Diagnostic::error(Code::MissingInput, Some(&node.node_id), Some("wall"), "`wall` has no value"));
            };
            let area = wall.length * wall.height;
            out.put("length", Value::Number { value: wall.length, dim: Dim::LENGTH }, Source::Computed);
            out.put("grossFaceArea", Value::Number { value: area, dim: Dim::AREA }, Source::Computed);
            out.put("grossVolume", Value::Number { value: area * wall.thickness, dim: Dim::VOLUME }, Source::Computed);
        }
        other => return Err(Diagnostic::error(Code::UnknownNodeType, Some(&node.node_id), None, format!("unknown node type `{other}`"))),
    }
    Ok(out)
}

/// Resolve each wall parameter by precedence: schema default, project rule, instance override,
/// graph connection. A locked parameter keeps its override (or default) and a connection to it
/// is reported as a conflict.
fn evaluate_wall(node: &Node, ctx: &Context, inputs: &Inputs, out: &mut Produced) -> Result<(), Diagnostic> {
    let t = wall_type(node)?;
    let instance = node.config.get("instanceId").and_then(Json::as_str).unwrap_or("");
    if !crate::graph::valid_id(instance) {
        return Err(cfg_err(node, "`instanceId` must be a valid id"));
    }
    let overrides = node.config.get("overrides").cloned().unwrap_or(Json::Null);
    let locked: Vec<&str> = node.config.get("locked").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_str).collect();
    for l in &locked {
        if t.param(l).is_none() {
            return Err(cfg_err(node, format!("`locked` names `{l}`, which a wall does not have")));
        }
    }
    if let Some(o) = overrides.as_object() {
        for k in o.keys() {
            if t.param(k).is_none_or(|p| p.id == "path") {
                return Err(cfg_err(node, format!("`overrides` cannot set `{k}`")));
            }
        }
    }
    let level = node.config.get("level").and_then(Json::as_str).map(str::to_string).or_else(|| ctx.default_level.clone());

    let mut resolve_number = |id: &str| -> Result<(f64, Source), Diagnostic> {
        let def = t.param(id).ok_or_else(|| cfg_err(node, format!("no parameter `{id}`")))?;
        let mut v = (def.default.unwrap_or(0.0), Source::Default);
        if id == "height"
            && let Some(h) = level.as_ref().and_then(|l| ctx.level_heights.get(l))
        {
            v = (*h, Source::Project { rule: format!("level `{}` height", level.as_deref().unwrap_or("")) });
        }
        if let Some(o) = overrides.get(id) {
            let (x, d) = cfg_quantity(o, def.unit).ok_or_else(|| cfg_err(node, format!("override `{id}` must be a {}", def.data_type.name())))?;
            if d != Dim::LENGTH {
                return Err(Diagnostic::error(
                    Code::UnitMismatch,
                    Some(&node.node_id),
                    Some(id),
                    format!("override `{id}` is a {}, not a length", d.name()),
                ));
            }
            v = (x, Source::Override);
        }
        if let Some((val, src)) = inputs.get(id) {
            if locked.contains(&id) {
                out.warnings.push(Diagnostic::warning(
                    Code::Conflict,
                    Some(&node.node_id),
                    Some(id),
                    format!("`{id}` is locked; the connected value is not used"),
                ));
            } else if let Some((x, _)) = val.number() {
                v = (x, src.clone());
            }
        }
        range_check(node, Some(id), id, v.0, def.min, def.max, def.unit)?;
        Ok(v)
    };
    let (height, hsrc) = resolve_number("height")?;
    let (thickness, tsrc) = resolve_number("thickness")?;

    let mut choice = |id: &str| -> Result<(String, Source), Diagnostic> {
        let def = t.param(id).ok_or_else(|| cfg_err(node, format!("no parameter `{id}`")))?;
        let mut v = (def.default_choice.unwrap_or("").to_string(), Source::Default);
        if let Some(o) = overrides.get(id).and_then(Json::as_str) {
            v = (o.to_string(), Source::Override);
        }
        if let Some((Value::Enum { value }, src)) = inputs.get(id) {
            if locked.contains(&id) {
                out.warnings.push(Diagnostic::warning(
                    Code::Conflict,
                    Some(&node.node_id),
                    Some(id),
                    format!("`{id}` is locked; the connected value is not used"),
                ));
            } else {
                v = (value.clone(), src.clone());
            }
        }
        if !def.choices.iter().any(|c| c.id == v.0) {
            return Err(Diagnostic::error(Code::OutOfRange, Some(&node.node_id), Some(id), format!("`{}` is not a {id} a wall can have", v.0)));
        }
        Ok(v)
    };
    let (material, msrc) = choice("material")?;
    let (finish, fsrc) = choice("finish")?;

    let Some((Value::Curve { points, closed }, psrc)) = inputs.get("path") else {
        return Err(Diagnostic::error(Code::MissingInput, Some(&node.node_id), Some("path"), "a wall needs a path"));
    };
    let length = path_length(points, *closed);
    if length <= 1e-9 {
        return Err(Diagnostic::error(Code::GeometryFailure, Some(&node.node_id), Some("path"), "the wall path has no length"));
    }
    let wall = WallValue {
        instance_id: instance.to_string(),
        component_type: t.id.to_string(),
        schema_version: t.schema_version,
        path: points.clone(),
        closed: *closed,
        length,
        height,
        thickness,
        level,
        material: Some(material.clone()),
        finish: Some(finish.clone()),
    };
    out.put("wall", Value::Wall { wall: Box::new(wall) }, Source::Computed);
    out.put("material", Value::Enum { value: material }, msrc);
    out.put("finish", Value::Enum { value: finish }, fsrc);
    out.put("length", Value::Number { value: length, dim: Dim::LENGTH }, psrc.clone());
    out.put("height", Value::Number { value: height, dim: Dim::LENGTH }, hsrc);
    out.put("thickness", Value::Number { value: thickness, dim: Dim::LENGTH }, tsrc);
    Ok(())
}
