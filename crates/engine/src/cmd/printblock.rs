//! A print block names a region of the model by a boundary, the way a block is named.
//!
//! The boundary is a closed polyline on layer `PRINT-BLOCK`. That layer is not frozen, not locked
//! and not frozen in new viewports. Objects inside the boundary stay where they are: they are not
//! copied into a block and no layer is frozen. The description is not stored. It is the current
//! name of the level and the tab (sheet set, or Design) the boundary was drawn on.

use aad_doc::{EntityKind, Handle, LevelId, LwPolyline, SheetSetId, Space};
use aad_geom::{PolyVertex, Vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

const KEY: &str = "printblocks1";
const LAYER: &str = "PRINT-BLOCK";
const MAX_BLOCKS: usize = 256;
const MAX_POINTS: usize = 64;
const MAX_NAME: usize = 64;
const MAX_SCAN: usize = 20_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Book {
    #[serde(default)]
    v: u32,
    #[serde(default)]
    next: u32,
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Item {
    id: u32,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    level: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sheet: Option<u32>,
    handle: String,
    /// Level and tab names at the last save, so another app can list the block without the level tables.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("printblock", "Print Block...", run_make)
            .menu(&["Draw", "Block", "Print Block..."])
            .alias(&["printblk", "pblock"])
            .params("{name, window?: [x0,y0,x1,y1] | points?: [[x,y],…] (≥3)} → {name, description, handle, level, sheet, contents}. Draws a boundary. The description is the level and the tab. Nothing is frozen and objects are not converted into a block.")
            .interactive(|_| Ok(Box::new(PrintBlockM::default()))),
        CommandSpec::new("printblock.list", "List Print Blocks", run_list)
            .menu(&["Draw", "Block", "List Print Blocks"])
            .params("{} → {printBlocks: [{name, description, handle, level, sheet, contents}]}")
            .noundo(),
        CommandSpec::new("printblock.set", "Rename Print Block", run_set)
            .params("{name, newName} → {name, description}"),
        CommandSpec::new("printblock.delete", "Delete Print Block", run_delete)
            .params("{name} → {deleted}. Removes the name. The boundary polyline stays."),
    ]
}

fn run_make(s: &mut Session, p: &Value) -> Result<Value> {
    require_model(s, "printblock")?;
    let name = str_param(p, "name").unwrap_or("").trim().to_string();
    check_name(&name)?;
    let points = boundary_points(p)?;
    make(s, &name, &points)
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let mut book = {
        let d = s.doc()?;
        load(d)?
    };
    let texts = {
        let d = s.doc()?;
        book.items.iter().map(|item| describe(d, item.level, item.sheet)).collect::<Vec<_>>()
    };
    for (item, text) in book.items.iter_mut().zip(texts) {
        item.description = text;
    }
    save(s.doc_mut()?, &book)?;
    let d = s.doc()?;
    let rows: Vec<Value> = book.items.iter().filter_map(|item| row(d, item).ok()).collect();
    Ok(json!({ "printBlocks": rows }))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").unwrap_or("").trim().to_string();
    let new_name = str_param(p, "newName").unwrap_or("").trim().to_string();
    check_name(&new_name)?;
    let d = s.doc()?;
    let mut book = load(d)?;
    if book.items.iter().any(|item| item.name.eq_ignore_ascii_case(&new_name) && !item.name.eq_ignore_ascii_case(&name)) {
        return Err(bad("printblock.set", format!("a print block named \"{new_name}\" already exists")));
    }
    let item = book.items.iter_mut().find(|item| item.name.eq_ignore_ascii_case(&name)).ok_or_else(|| bad("printblock.set", format!("no print block named \"{name}\"")))?;
    item.name = new_name;
    let out_name = item.name.clone();
    let level = item.level;
    let sheet = item.sheet;
    save(s.doc_mut()?, &book)?;
    let d = s.doc()?;
    Ok(json!({ "name": out_name, "description": describe(d, level, sheet) }))
}

fn run_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").unwrap_or("").trim().to_string();
    let d = s.doc()?;
    let mut book = load(d)?;
    let before = book.items.len();
    book.items.retain(|item| !item.name.eq_ignore_ascii_case(&name));
    if book.items.len() == before {
        return Err(bad("printblock.delete", format!("no print block named \"{name}\"")));
    }
    save(s.doc_mut()?, &book)?;
    Ok(json!({ "deleted": name }))
}

fn make(s: &mut Session, name: &str, points: &[Vec2]) -> Result<Value> {
    if !matches!(s.space(), Space::Model) {
        return Err(bad("printblock", "print blocks are drawn on the model, not in paper space"));
    }
    let d = s.doc()?;
    let mut book = load(d)?;
    if book.items.len() >= MAX_BLOCKS {
        return Err(bad("printblock", format!("a drawing holds at most {MAX_BLOCKS} print blocks")));
    }
    if book.items.iter().any(|item| item.name.eq_ignore_ascii_case(name)) {
        return Err(bad("printblock", format!("a print block named \"{name}\" already exists")));
    }
    let level = d.active_level.map(|id| id.0);
    let sheet = d.active_sheet.map(|id| id.0);
    let vertices: Vec<PolyVertex> = points.iter().copied().map(PolyVertex::new).collect();
    let kind = EntityKind::LwPolyline(LwPolyline { vertices, closed: true, const_width: 0.0, elevation: 0.0, plinegen: false });
    let mut common = s.current_common();
    common.layer = LAYER.into();
    let created = s.doc_mut()?.ensure_layer(LAYER);
    if created {
        if let Some(layer) = s.doc_mut()?.layers.iter_mut().find(|layer| layer.name == LAYER) {
            // The boundary stays editable and is not a frozen paper-space viewport layer.
            layer.frozen = false;
            layer.locked = false;
            layer.vp_freeze_new = false;
            layer.plot = false;
        }
    }
    let handle = s.doc_mut()?.add(&Space::Model, common, kind)?;
    let id = book.next.max(1);
    book.next = id.saturating_add(1);
    book.items.push(Item { id, name: name.to_string(), level, sheet, handle: handle.hex(), description: String::new() });
    let texts = {
        let d = s.doc()?;
        book.items.iter().map(|item| describe(d, item.level, item.sheet)).collect::<Vec<_>>()
    };
    for (item, text) in book.items.iter_mut().zip(texts) {
        item.description = text;
    }
    save(s.doc_mut()?, &book)?;
    let d = s.doc()?;
    let item = book.items.last().ok_or_else(|| bad("printblock", "the print block was not stored"))?;
    row(d, item)
}

fn row(d: &aad_doc::Drawing, item: &Item) -> Result<Value> {
    let contents = contents(d, item);
    Ok(json!({
        "name": item.name,
        "description": describe(d, item.level, item.sheet),
        "handle": item.handle,
        "level": item.level,
        "sheet": item.sheet,
        "contents": contents,
    }))
}

fn describe(d: &aad_doc::Drawing, level: Option<u32>, sheet: Option<u32>) -> String {
    let level_name = match level {
        Some(id) => d.level(LevelId(id)).map(|level| level.name.clone()).unwrap_or_else(|| "Missing level".into()),
        None => "Level 1".into(),
    };
    let tab = match sheet {
        Some(id) => d.sheet_sets.iter().find(|set| set.id == SheetSetId(id)).map(|set| set.name.clone()).unwrap_or_else(|| "Missing tab".into()),
        None => "Design".into(),
    };
    format!("{level_name} / {tab}")
}

fn contents(d: &aad_doc::Drawing, item: &Item) -> Vec<String> {
    let Some(boundary) = Handle::parse_hex(&item.handle) else { return Vec::new() };
    let Some(poly) = boundary_of(d, boundary) else { return Vec::new() };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut scan = |store: &aad_doc::EntityStore| {
        for entity in store.iter().take(MAX_SCAN) {
            if entity.handle == boundary || !seen.insert(entity.handle) {
                continue;
            }
            let bounds = aad_doc::entity_bounds(d, entity, 0);
            if bounds.is_empty() {
                continue;
            }
            let center = bounds.center();
            if center.is_finite() && aad_geom::point_in_polygon(&poly, center) {
                out.push(entity.handle.hex());
            }
        }
    };
    if let Some(id) = item.level {
        if let Some(store) = d.level_store(LevelId(id)) {
            scan(store);
        }
    } else {
        scan(&d.model);
    }
    if let (Some(level), Some(sheet)) = (item.level, item.sheet) {
        if let Some(store) = d.annotation_store(SheetSetId(sheet), LevelId(level)) {
            scan(store);
        }
    }
    out
}

fn boundary_of(d: &aad_doc::Drawing, handle: Handle) -> Option<Vec<Vec2>> {
    let entity = d.entity(handle)?;
    let EntityKind::LwPolyline(poly) = &entity.kind else { return None };
    let pts: Vec<Vec2> = poly.vertices.iter().map(|vertex| vertex.p).filter(|p| p.is_finite()).collect();
    (pts.len() >= 3).then_some(pts)
}

fn boundary_points(p: &Value) -> Result<Vec<Vec2>> {
    if let Some(pts) = points_param(p, "points") {
        return checked_ring(&pts);
    }
    let Some(window) = p.get("window") else {
        return Err(bad("printblock", "`window` ([x0, y0, x1, y1]) or `points` is required"));
    };
    let nums: Vec<f64> = window.as_array().map(|items| items.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    match nums[..] {
        [x0, y0, x1, y1] => checked_ring(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]),
        _ => Err(bad("printblock", "`window` must be [x0, y0, x1, y1]")),
    }
}

fn checked_ring(points: &[Vec2]) -> Result<Vec<Vec2>> {
    if points.len() < 3 || points.len() > MAX_POINTS {
        return Err(bad("printblock", format!("a boundary needs 3 to {MAX_POINTS} points")));
    }
    if points.iter().any(|p| !p.is_finite() || p.x.abs() > 1e12 || p.y.abs() > 1e12) {
        return Err(bad("printblock", "boundary points must be finite"));
    }
    let mut area = 0.0;
    let n = points.len();
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        area += a.x * b.y - b.x * a.y;
    }
    if area.abs() < 1e-8 {
        return Err(bad("printblock", "the boundary has no area"));
    }
    Ok(points.to_vec())
}

fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_NAME || name.chars().any(|c| c.is_control() || "<>/\\\":;?*|,=`".contains(c)) {
        return Err(bad("printblock", "invalid print block name"));
    }
    Ok(())
}

fn require_model(s: &Session, cmd: &str) -> Result<()> {
    if matches!(s.space(), Space::Model) { Ok(()) } else { Err(bad(cmd, "print blocks are drawn on the model, not in paper space")) }
}

fn load(d: &aad_doc::Drawing) -> Result<Book> {
    let Some(text) = d.project.get(KEY) else { return Ok(Book { v: 1, ..Book::default() }) };
    serde_json::from_str(text).map_err(|_| bad("printblock", "print block data is damaged"))
}

fn save(d: &mut aad_doc::Drawing, book: &Book) -> Result<()> {
    if book.items.is_empty() {
        d.project.remove(KEY);
        return Ok(());
    }
    let text = serde_json::to_string(book).map_err(|_| bad("printblock", "could not store the print block"))?;
    d.project.insert(KEY.into(), text);
    Ok(())
}

#[derive(Default)]
struct PrintBlockM {
    existing: Option<String>,
    name: Option<String>,
    polygon: bool,
    points: Vec<Vec2>,
}

impl Interactive for PrintBlockM {
    fn name(&self) -> &'static str {
        "PRINTBLOCK"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if require_model(s, "printblock").is_err() {
            s.echo("Print blocks are drawn on the model, not in paper space.");
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.name.is_none() {
            return match &self.existing {
                Some(name) => Prompt::new(format!("Print block \"{name}\" already exists. Replace it?"), Accept::TEXT).kw(&["Yes", "No"]).default("N"),
                None => Prompt::new("Enter print block name", Accept::TEXT),
            };
        }
        if self.points.is_empty() {
            return Prompt::new("Specify first corner of the boundary", Accept::POINT).kw(&["Polygon"]);
        }
        if self.polygon {
            Prompt::new("Specify next boundary point", Accept::POINT).kw(&["Close"]).base(self.points[self.points.len() - 1])
        } else {
            Prompt::new("Specify other corner of the boundary", Accept::POINT).base(self.points[0])
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.name.is_none() {
            return self.name_input(s, i);
        }
        match i {
            Input::Keyword(word) if !self.polygon && self.points.is_empty() && word.eq_ignore_ascii_case("Polygon") => {
                self.polygon = true;
                Ok(Step::Continue)
            }
            Input::Keyword(word) if self.polygon && word.eq_ignore_ascii_case("Close") => self.close(s),
            Input::Enter if self.polygon => self.close(s),
            Input::Point(p) if self.points.is_empty() => {
                self.points.push(p);
                Ok(Step::Continue)
            }
            Input::Point(p) if !self.polygon => {
                let a = self.points[0];
                let ring = [a, Vec2::new(p.x, a.y), p, Vec2::new(a.x, p.y)];
                self.finish(s, &ring)
            }
            Input::Point(p) => {
                if self.points.len() >= MAX_POINTS {
                    s.echo(format!("A boundary has at most {MAX_POINTS} points."));
                    return Ok(Step::Continue);
                }
                if self.points.last().is_none_or(|last| !last.near(p, 1e-9)) {
                    self.points.push(p);
                }
                Ok(Step::Continue)
            }
            Input::Cancel => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
}

impl PrintBlockM {
    fn name_input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.existing.is_some() {
            let yes = matches!(&i, Input::Keyword(k) | Input::Text(k) if k.trim().to_ascii_lowercase().starts_with('y'));
            if yes {
                let Some(name) = self.existing.take() else { return Ok(Step::Continue) };
                if let Err(err) = drop_named(s, &name) {
                    s.echo(err.to_string());
                    return Ok(Step::Done);
                }
                self.name = Some(name);
            } else {
                self.existing = None;
            }
            return Ok(Step::Continue);
        }
        let Input::Text(text) = i else { return Ok(Step::Continue) };
        let name = text.trim().to_string();
        if check_name(&name).is_err() {
            s.echo("Invalid print block name.");
            return Ok(Step::Continue);
        }
        let exists = s.doc().ok().and_then(|d| load(d).ok()).is_some_and(|book| book.items.iter().any(|item| item.name.eq_ignore_ascii_case(&name)));
        if exists {
            self.existing = Some(name);
        } else {
            self.name = Some(name);
        }
        Ok(Step::Continue)
    }

    fn close(&mut self, s: &mut Session) -> Result<Step> {
        if self.points.len() < 3 {
            s.echo("A boundary needs at least 3 points.");
            return Ok(Step::Continue);
        }
        let points = self.points.clone();
        self.finish(s, &points)
    }

    fn finish(&mut self, s: &mut Session, points: &[Vec2]) -> Result<Step> {
        let Some(name) = self.name.clone() else { return Ok(Step::Cancel) };
        match make(s, &name, points) {
            Ok(row) => {
                let description = row.get("description").and_then(Value::as_str).unwrap_or("");
                let n = row.get("contents").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
                s.echo(format!("Print block \"{name}\" ({description}) covers {n} object(s)."));
                Ok(Step::Done)
            }
            Err(err) => {
                s.echo(err.to_string());
                Ok(Step::Done)
            }
        }
    }
}

fn drop_named(s: &mut Session, name: &str) -> Result<()> {
    let d = s.doc()?;
    let mut book = load(d)?;
    book.items.retain(|item| !item.name.eq_ignore_ascii_case(name));
    save(s.doc_mut()?, &book)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
        let made = s.execute("line", &json!({ "points": [a, b] })).unwrap();
        made["handles"][0].as_str().unwrap().to_string()
    }

    #[test]
    fn boundary_names_what_is_inside_and_freezes_nothing() {
        let mut s = Session::new();
        s.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
        s.execute("level.activate", &json!({ "level": "Ground" })).unwrap();
        let inside = line(&mut s, [1.0, 1.0], [2.0, 1.0]);
        let outside = line(&mut s, [50.0, 50.0], [60.0, 50.0]);
        let made = s.execute("printblock", &json!({ "name": "Plan", "window": [0.0, 0.0, 10.0, 10.0] })).unwrap();
        assert_eq!(made["description"], "Ground / Design");
        let contents = made["contents"].as_array().unwrap();
        assert!(contents.iter().any(|h| h == &inside), "{contents:?}");
        assert!(!contents.iter().any(|h| h == &outside));
        let d = s.doc().unwrap();
        let layer = d.layer(LAYER).unwrap();
        assert!(!layer.frozen && !layer.locked && !layer.vp_freeze_new);
        let handle = Handle::parse_hex(&inside).unwrap();
        assert!(matches!(d.entity(handle).unwrap().kind, EntityKind::Line(_)));
        s.execute("level.set", &json!({ "level": "Ground", "name": "Ground floor" })).unwrap();
        let listed = s.execute("printblock.list", &json!({})).unwrap();
        assert_eq!(listed["printBlocks"][0]["description"], "Ground floor / Design");
    }

    #[test]
    fn description_follows_the_tab_name() {
        let mut s = Session::new();
        s.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
        s.execute("level.activate", &json!({ "level": "Ground" })).unwrap();
        s.execute("sheetset.new", &json!({ "name": "Working drawing" })).unwrap();
        s.execute("sheetset.activate", &json!({ "set": "Working drawing" })).unwrap();
        line(&mut s, [1.0, 1.0], [2.0, 1.0]);
        let made = s.execute("printblock", &json!({ "name": "Sheet", "window": [0.0, 0.0, 10.0, 10.0] })).unwrap();
        assert_eq!(made["description"], "Ground / Working drawing");
        s.execute("sheetset.set", &json!({ "set": "Working drawing", "name": "WD" })).unwrap();
        let listed = s.execute("printblock.list", &json!({})).unwrap();
        assert_eq!(listed["printBlocks"][0]["description"], "Ground / WD");
        let stored = s.doc().unwrap().project.get(KEY).cloned().unwrap_or_default();
        assert!(stored.contains("Ground / WD"), "{stored}");
    }

    #[test]
    fn a_bad_name_and_a_flat_boundary_are_refused() {
        let mut s = Session::new();
        assert!(s.execute("printblock", &json!({ "name": "A<B", "window": [0.0, 0.0, 1.0, 1.0] })).is_err());
        assert!(s.execute("printblock", &json!({ "name": "Flat", "points": [[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]] })).is_err());
    }
}
