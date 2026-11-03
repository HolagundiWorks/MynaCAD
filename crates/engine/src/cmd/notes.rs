//! Standard numbered drawing notes: named sets stored in project data, placed as MTEXT.
//!
//! ```json
//! { "sets": { "General": { "items": ["All dimensions…", "…"] }, "…": {…} }, "current": "General" }
//! ```

use aad_doc::{EntityKind, MText, Space};
use aad_geom::Vec2;
use serde_json::{Map, Value, json};

use super::hcw::{add_on, ensure_layer};
use super::*;
use crate::annot::{self as ua};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

const SLOT: &str = "notes";
const LAYER: &str = "AN-NOTES";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("notes", "Notes", run_edit)
            .menu(&["Format", "Notes", "Edit Notes…"])
            .alias(&["stdnotes", "drawingnotes"])
            .params("{name?: set name, items?: [string], current?: name, delete?: name} → {sets, current} (set replaces / selects a named notes set; delete removes one)"),
        CommandSpec::new("notes.manager", "General Notes", run_manager)
            .menu(&["Format", "Notes", "General Notes..."])
            .alias(&["generalnotes"])
            .params("{} (opens the General Notes window in the shell: create, edit and place note sets)")
            .noundo(),
        CommandSpec::new("notes.std", "Standard Notes", run_std)
            .menu(&["Format", "Notes", "Standard Notes"])
            .alias(&["notesstd"])
            .params("{} → {name, items} (seeds the General set with typical architectural notes)"),
        CommandSpec::new("notes.list", "List Notes", run_list)
            .menu(&["Format", "Notes", "List Notes"])
            .alias(&["notesls"])
            .params("{} → {sets, current}")
            .noundo(),
        CommandSpec::new("notes.apply", "Apply Notes", run_apply)
            .menu(&["Format", "Notes", "Apply Notes"])
            .alias(&["notesplace", "placenotes"])
            .params("{name?: set name, at?: [x,y], title?: string} (places a numbered MTEXT list on AN-NOTES)")
            .interactive(|_| Ok(Box::new(PlaceNotesM))),
    ]
}

fn store(d: &aad_doc::Drawing) -> Map<String, Value> {
    match project::get(d, SLOT) {
        Value::Object(o) => o,
        _ => Map::new(),
    }
}

fn sets(o: &Map<String, Value>) -> Map<String, Value> {
    o.get("sets").and_then(Value::as_object).cloned().unwrap_or_default()
}

fn items_of(set: &Value) -> Vec<String> {
    set.get("items")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

fn default_notes() -> Vec<String> {
    vec![
        "All dimensions are in millimetres unless noted otherwise.".into(),
        "Do not scale the drawing. Use figured dimensions only.".into(),
        "Verify all dimensions on site before construction.".into(),
        "Any discrepancy between drawings and site conditions shall be reported to the architect.".into(),
        "This drawing is to be read in conjunction with all related architectural and engineering drawings.".into(),
        "Levels are in metres relative to the project datum unless noted otherwise.".into(),
        "Contractor shall coordinate openings with structure and services before work.".into(),
    ]
}

fn run_std(s: &mut Session, _p: &Value) -> Result<Value> {
    let mut o = store(s.doc()?);
    let mut all = sets(&o);
    let items = default_notes();
    all.insert("General".into(), json!({ "items": items.clone() }));
    o.insert("sets".into(), Value::Object(all));
    o.insert("current".into(), json!("General"));
    project::set(s.doc_mut()?, SLOT, &Value::Object(o));
    Ok(json!({ "name": "General", "items": items }))
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let o = store(s.doc()?);
    Ok(json!({
        "sets": sets(&o),
        "current": o.get("current").and_then(Value::as_str).unwrap_or("General"),
    }))
}

/// The General Notes window is a window of the shell, which opens it when this command is chosen.
fn run_manager(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({}))
}

fn run_edit(s: &mut Session, p: &Value) -> Result<Value> {
    let mut o = store(s.doc()?);
    let mut all = sets(&o);
    if let Some(gone) = p.get("delete").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
        if all.remove(gone).is_none() {
            return Err(bad("notes", format!("unknown notes set `{gone}`")));
        }
        if o.get("current").and_then(Value::as_str) == Some(gone) {
            match all.keys().next().cloned() {
                Some(first) => o.insert("current".into(), json!(first)),
                None => o.remove("current"),
            };
        }
    }
    if let Some(items) = p.get("items").and_then(Value::as_array) {
        let name = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("General").to_string();
        if name.len() > 64 {
            return Err(bad("notes", "set name is at most 64 characters"));
        }
        if items.len() > 200 {
            return Err(bad("notes", "at most 200 notes per set"));
        }
        let clean: Vec<String> = items
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .or_else(|| v.get("text").and_then(Value::as_str))
                    .or_else(|| v.get("note").and_then(Value::as_str))
                    .or_else(|| v.get("item").and_then(Value::as_str))
                    .map(|s| s.trim().to_string())
            })
            .filter(|s| !s.is_empty() && s.len() <= 500)
            .collect();
        all.insert(name.clone(), json!({ "items": clean }));
        o.insert("current".into(), json!(name));
    }
    if let Some(cur) = p.get("current").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
        if !all.contains_key(cur) {
            return Err(bad("notes", format!("unknown notes set `{cur}`")));
        }
        o.insert("current".into(), json!(cur));
    }
    o.insert("sets".into(), Value::Object(all.clone()));
    project::set(s.doc_mut()?, SLOT, &Value::Object(o.clone()));
    Ok(json!({
        "sets": all,
        "current": o.get("current").and_then(Value::as_str).unwrap_or("General"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_is_created_placed_and_deleted() {
        let mut s = Session::new();
        s.execute("notes", &json!({"name": "Electrical", "items": ["Wire in copper.", "Earth every circuit."]})).unwrap();
        s.execute("notes", &json!({"name": "Plumbing", "items": ["Test at 1.5 times the working pressure."]})).unwrap();
        let r = s.execute("notes.apply", &json!({"name": "Electrical", "at": [0.0, 0.0]})).unwrap();
        assert_eq!(r["count"], json!(2));
        let after = s.execute("notes", &json!({"delete": "Electrical"})).unwrap();
        assert!(after["sets"].get("Electrical").is_none());
        assert_eq!(after["current"], json!("Plumbing"), "the current set moves to one that is left");
        assert!(s.execute("notes", &json!({"delete": "Electrical"})).is_err());
        assert!(s.execute("notes.apply", &json!({"name": "Electrical"})).is_err());
    }
}

fn numbered_mtext(items: &[String], title: &str) -> String {
    let mut text = String::new();
    if !title.is_empty() {
        text.push_str(title);
        text.push_str("\\P\\P");
    }
    for (i, line) in items.iter().enumerate() {
        text.push_str(&format!("{}. {}\\P", i + 1, line.replace('\n', " ")));
    }
    text
}

fn place_notes(s: &mut Session, name: &str, at: Vec2, title: &str) -> Result<Value> {
    let o = store(s.doc()?);
    let all = sets(&o);
    let set = all.get(name).ok_or_else(|| bad("notes.apply", format!("unknown notes set `{name}` — use `notes.std` or `notes`")))?;
    let items = items_of(set);
    if items.is_empty() {
        return Err(bad("notes.apply", format!("notes set `{name}` has no items")));
    }
    let th = ua::drawing_unit(s.doc()?).map_or(0.18, |u| 0.18 / u.metres());
    let width = th * 48.0;
    ensure_layer(s.doc_mut()?, LAYER, 7, "Continuous", 18, "Drawing notes");
    let style = s.doc()?.header.str("TEXTSTYLE", "Standard");
    let heading = if title.is_empty() { format!("NOTES — {name}") } else { title.to_string() };
    let contents = numbered_mtext(&items, &heading);
    let k = EntityKind::MText(MText {
        insert: aad_geom::Vec3::new(at.x, at.y, 0.0),
        height: th,
        width,
        attach: 1,
        rotation: 0.0,
        style,
        contents,
        line_spacing: 1.15,
        line_spacing_exact: false,
    });
    let h = add_on(s, LAYER, k)?;
    Ok(json!({ "name": name, "count": items.len(), "handle": h.hex(), "at": [at.x, at.y] }))
}

fn run_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let o = store(s.doc()?);
    let name = p
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| o.get("current").and_then(Value::as_str).unwrap_or("General"))
        .to_string();
    let title = p.get("title").and_then(Value::as_str).unwrap_or("").to_string();
    let at = match point_param(p, "at") {
        Some(p) => p,
        None => {
            let ext = s.doc()?.extents(&Space::Model);
            if ext.is_empty() {
                Vec2::ZERO
            } else {
                let th = ua::drawing_unit(s.doc()?).map_or(0.18, |u| 0.18 / u.metres());
                Vec2::new(ext.min.x, ext.min.y - 8.0 * th)
            }
        }
    };
    place_notes(s, &name, at, &title)
}

#[derive(Default)]
struct PlaceNotesM;

impl Interactive for PlaceNotesM {
    fn name(&self) -> &'static str {
        "notes.apply"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Specify insertion point for notes", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        match input {
            Input::Point(p) => {
                let o = store(s.doc()?);
                let name = o.get("current").and_then(Value::as_str).unwrap_or("General").to_string();
                let _ = place_notes(s, &name, p, "")?;
                Ok(Step::Done)
            }
            Input::Cancel => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
}
