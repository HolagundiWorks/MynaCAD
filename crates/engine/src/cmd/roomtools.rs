//! Room-label tools carried over from the Holgundi Consulting Works AutoLISP toolkit
//! (`hcwtools.lsp`, section 11: MRELABEL, MAUDIT, MCHECK, MSCHEDULE, MTOTAL). They read what the
//! `room` command draws: three centred texts on `ROOM-LABELS` (name, size, area) and, when the
//! box is on, a closed polyline on `ROOM-RECT`.
//!
//! The toolkit paired texts by reading order; here the three lines of a label are paired by their
//! geometry (the size line sits 1.5 heights, the area line 2.8 heights, below the name), so the
//! result does not depend on the order objects were drawn in. The floor prefix of MFLOOR is the
//! active floor of the project (`floors`), which `room` already uses.

use aad_doc::{Drawing, EntityKind, Handle};
use aad_geom::Vec2;
use serde_json::{Value, json};

use super::hcw::{LABEL_LAYER, RECT_LAYER, room_name};
use super::*;
use crate::{Result, Session};

const MAX_TEXTS: usize = 500_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("room.relabel", "Rename Room Label", run_relabel)
            .menu(&["Architecture", "Rooms and Areas", "Rename Room Label"])
            .params("{handle? (the room name text), name: a room name or two-letter code, floorPrefix? (default the active floor's)} → {old, new}"),
        CommandSpec::new("room.log", "Room Log", run_log)
            .menu(&["Architecture", "Rooms and Areas", "Room Log (CSV)"])
            .params("{path?} → {count, rooms: [{name, size, area, unit, x, y}], csv?, path?} (every room label on ROOM-LABELS, sorted by name)")
            .noundo(),
        CommandSpec::new("room.total", "Room Area Total", run_total)
            .menu(&["Architecture", "Rooms and Areas", "Room Area Total"])
            .params("{filter?: text the room name contains, case-insensitive; blank for all} → {count, total, unit}")
            .noundo(),
        CommandSpec::new("room.audit", "Room Label Audit", run_audit)
            .menu(&["Architecture", "Rooms and Areas", "Room Label Audit"])
            .params("{} → {labels, rectangles, labelsWithoutRectangle: [handle], rectanglesWithoutLabel: [handle]} (a label belongs to the rectangle that contains its centre)")
            .noundo(),
        CommandSpec::new("room.check", "Room Rectangle Check", run_check)
            .menu(&["Architecture", "Rooms and Areas", "Room Rectangle Check"])
            .params("{} → {result: PASS|WARN, closed, open, lines, other} (everything on ROOM-RECT should be a closed polyline)")
            .noundo(),
        CommandSpec::new("room.lighting", "Room Lighting", run_lighting)
            .menu(&["Architecture", "Rooms and Areas", "Room Lighting"])
            .alias(&["roomlux"])
            .params(
                "{} → {count, note, rooms: [{name, declared, lighting_matched, area_m2, area_ft2, lm_per_m2, lm_per_ft2, flux_lm}]} (maintained illuminance for each declared room label)",
            )
            .noundo(),
    ]
}

/// One room label found on the label layer.
struct Room {
    name_text: Handle,
    name: String,
    size: String,
    area: f64,
    unit: String,
    at: Vec2,
}

struct TextRef {
    handle: Handle,
    value: String,
    at: Vec2,
    height: f64,
}

fn label_texts(d: &Drawing, space: &aad_doc::Space) -> Vec<TextRef> {
    let Some(store) = d.space(space) else { return Vec::new() };
    store
        .iter()
        .filter(|e| e.common.layer.eq_ignore_ascii_case(LABEL_LAYER.0))
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) if t.height > 0.0 && t.height.is_finite() => {
                Some(TextRef { handle: e.handle, value: t.value.clone(), at: t.justify_point(), height: t.height })
            }
            _ => None,
        })
        .take(MAX_TEXTS)
        .collect()
}

/// "Area: 12.34 m2" -> (12.34, "m2").
fn parse_area(text: &str) -> Option<(f64, String)> {
    let rest = text.strip_prefix("Area:")?.trim();
    let (number, unit) = rest.split_once(' ').unwrap_or((rest, ""));
    let value: f64 = number.parse().ok()?;
    value.is_finite().then(|| (value, unit.trim().to_string()))
}

/// The rooms of a space: a text with an area text 2.8 name-heights below it (the size text 1.5
/// below). The area text carries 0.7 of the name height, the size text 0.8.
fn find_rooms(d: &Drawing, space: &aad_doc::Space) -> Vec<Room> {
    let texts = label_texts(d, space);
    let mut areas: Vec<&TextRef> = texts.iter().filter(|t| parse_area(&t.value).is_some()).collect();
    areas.sort_by(|a, b| a.at.x.total_cmp(&b.at.x));
    let mut rooms = Vec::new();
    for name in texts.iter().filter(|t| parse_area(&t.value).is_none() && !t.value.contains(" x ")) {
        let tol = name.height * 0.02;
        let first = areas.partition_point(|a| a.at.x < name.at.x - tol);
        let want_y = name.at.y - name.height * 2.8;
        let area = areas[first..]
            .iter()
            .take_while(|a| a.at.x <= name.at.x + tol)
            .find(|a| (a.at.y - want_y).abs() <= tol && (a.height - name.height * 0.7).abs() <= tol);
        let Some(area) = area else { continue };
        let Some((value, unit)) = parse_area(&area.value) else { continue };
        let size_y = name.at.y - name.height * 1.5;
        let size =
            texts.iter().find(|t| (t.at.x - name.at.x).abs() <= tol && (t.at.y - size_y).abs() <= tol).map(|t| t.value.clone()).unwrap_or_default();
        rooms.push(Room { name_text: name.handle, name: name.value.clone(), size, area: value, unit, at: name.at });
    }
    rooms
}

/// A room as the AQC hand-off lists it: the row, the name text that identifies it (its handle is the
/// row's stable id) and the room's box when one is drawn (the geometry for the bounding box).
pub(crate) struct RoomRow {
    pub row: Value,
    pub handle: Handle,
    pub outline: Option<Handle>,
}

/// The rooms of a space as the AQC hand-off lists them: a stable `ref` from the name text's handle,
/// name, size text and area in square metres. Areas drawn in square feet are converted.
pub(crate) fn rooms_for_export(d: &Drawing, space: &aad_doc::Space) -> Vec<RoomRow> {
    let mut rooms = find_rooms(d, space);
    rooms.sort_by(|a, b| a.name.cmp(&b.name).then(a.at.y.total_cmp(&b.at.y)).then(a.at.x.total_cmp(&b.at.x)));
    let rects: Vec<(Handle, aad_geom::Bounds2)> = d
        .space(space)
        .map(|st| {
            st.iter()
                .filter(|e| e.common.layer.eq_ignore_ascii_case(RECT_LAYER.0))
                .filter_map(|e| match &e.kind {
                    EntityKind::LwPolyline(pl) if pl.closed => {
                        Some((e.handle, aad_geom::Polyline { vertices: pl.vertices.clone(), closed: true }.bounds()))
                    }
                    _ => None,
                })
                .take(MAX_TEXTS)
                .collect()
        })
        .unwrap_or_default();
    rooms
        .iter()
        .map(|r| {
            let m2 = if r.unit.starts_with("sq ft") { r.area * 0.092_903_04 } else { r.area };
            let outline =
                rects.iter().find(|(_, b)| r.at.x >= b.min.x && r.at.x <= b.max.x && r.at.y >= b.min.y && r.at.y <= b.max.y).map(|(h, _)| *h);
            let mut row = json!({ "ref": format!("R{}", r.name_text.hex()), "name": r.name, "size": r.size, "area_m2": (m2 * 1000.0).round() / 1000.0 });
            merge_lighting(&mut row, &r.name, r.area, &r.unit);
            RoomRow { row, handle: r.name_text, outline }
        })
        .collect()
}

fn run_relabel(s: &mut Session, p: &Value) -> Result<Value> {
    let h = *targets(s, p)?.first().ok_or_else(|| bad("room.relabel", "select the room name text"))?;
    let typed = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("room.relabel", "`name` is required"))?;
    let prefix = match str_param(p, "floorPrefix") {
        Some(v) => v.to_string(),
        None => project::active_floor_prefix(s.doc()?),
    };
    let name = room_name(typed);
    let full = if prefix.trim().is_empty() { name } else { format!("{} {}", prefix.trim(), name) }.to_uppercase();
    if locked_text(s, h)? {
        return Err(bad("room.relabel", "that text is on a locked layer"));
    }
    let mut old = String::new();
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Text(t) = &mut e.kind {
            old = std::mem::replace(&mut t.value, full.clone());
        }
    })?;
    if old.is_empty() {
        return Err(bad("room.relabel", "select a single-line text"));
    }
    Ok(json!({ "old": old, "new": full }))
}

fn locked_text(s: &Session, h: Handle) -> Result<bool> {
    let d = s.doc()?;
    Ok(d.entity(h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked))
}

fn csv_field(v: &str) -> String {
    if v.contains([',', '"', '\n', '\r']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v.to_string() }
}

fn run_log(s: &mut Session, p: &Value) -> Result<Value> {
    let space = s.space();
    let mut rooms = find_rooms(s.doc()?, &space);
    if rooms.is_empty() {
        return Err(bad("room.log", "no room labels on ROOM-LABELS in this space"));
    }
    rooms.sort_by(|a, b| a.name.cmp(&b.name).then(a.at.y.total_cmp(&b.at.y)).then(a.at.x.total_cmp(&b.at.x)));
    let mut csv = String::from("RoomType,Size,Area,AreaUnit,InsX,InsY\n");
    for r in &rooms {
        csv.push_str(&format!("{},{},{:.2},{},{:.3},{:.3}\n", csv_field(&r.name), csv_field(&r.size), r.area, csv_field(&r.unit), r.at.x, r.at.y));
    }
    let list: Vec<Value> =
        rooms.iter().map(|r| json!({ "name": r.name, "size": r.size, "area": r.area, "unit": r.unit, "x": r.at.x, "y": r.at.y })).collect();
    match str_param(p, "path") {
        Some(_path) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                std::fs::write(_path, &csv).map_err(|e| bad("room.log", format!("{_path}: {e}")))?;
                Ok(json!({ "count": rooms.len(), "rooms": list, "path": _path }))
            }
            #[cfg(target_arch = "wasm32")]
            Err(bad("room.log", "paths are not available on the web; omit `path` to get the CSV text"))
        }
        None => Ok(json!({ "count": rooms.len(), "rooms": list, "csv": csv })),
    }
}

fn run_total(s: &mut Session, p: &Value) -> Result<Value> {
    let filter = str_param(p, "filter").unwrap_or("").trim().to_uppercase();
    let space = s.space();
    let rooms = find_rooms(s.doc()?, &space);
    let mut total = 0.0;
    let mut count = 0usize;
    let mut unit = String::new();
    for r in rooms.iter().filter(|r| filter.is_empty() || r.name.to_uppercase().contains(&filter)) {
        total += r.area;
        count += 1;
        if unit.is_empty() {
            unit = r.unit.clone();
        }
    }
    Ok(json!({ "count": count, "total": total, "unit": unit }))
}

fn run_audit(s: &mut Session, _p: &Value) -> Result<Value> {
    let space = s.space();
    let d = s.doc()?;
    let rooms = find_rooms(d, &space);
    let rects: Vec<(Handle, aad_geom::Bounds2)> = d
        .space(&space)
        .map(|st| {
            st.iter()
                .filter(|e| e.common.layer.eq_ignore_ascii_case(RECT_LAYER.0))
                .filter_map(|e| match &e.kind {
                    EntityKind::LwPolyline(pl) if pl.closed => {
                        Some((e.handle, aad_geom::Polyline { vertices: pl.vertices.clone(), closed: true }.bounds()))
                    }
                    _ => None,
                })
                .take(MAX_TEXTS)
                .collect()
        })
        .unwrap_or_default();
    let inside = |b: &aad_geom::Bounds2, p: Vec2| p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y;
    // The label's centre is the middle of its three lines, which hang below the name.
    let centre = |r: &Room| r.at;
    let no_rect: Vec<String> = rooms.iter().filter(|r| !rects.iter().any(|(_, b)| inside(b, centre(r)))).map(|r| r.name_text.hex()).collect();
    let no_label: Vec<String> = rects.iter().filter(|(_, b)| !rooms.iter().any(|r| inside(b, centre(r)))).map(|(h, _)| h.hex()).collect();
    Ok(json!({ "labels": rooms.len(), "rectangles": rects.len(), "labelsWithoutRectangle": no_rect, "rectanglesWithoutLabel": no_label }))
}

fn merge_lighting(row: &mut Value, name: &str, area: f64, unit: &str) {
    let Some(into) = row.as_object_mut() else { return };
    let Some(extra) = super::lighting::lighting_for(name, area, unit).as_object().cloned() else { return };
    for (key, value) in extra {
        if key != "area_m2" {
            into.insert(key, value);
        }
    }
}

fn run_lighting(s: &mut Session, _p: &Value) -> Result<Value> {
    let space = s.space();
    let mut rooms = find_rooms(s.doc()?, &space);
    if rooms.is_empty() {
        return Err(bad("room.lighting", "no room labels on ROOM-LABELS in this space"));
    }
    rooms.sort_by(|a, b| a.name.cmp(&b.name).then(a.at.y.total_cmp(&b.at.y)).then(a.at.x.total_cmp(&b.at.x)));
    let list: Vec<Value> = rooms
        .iter()
        .map(|r| {
            let mut row = json!({ "name": r.name });
            merge_lighting(&mut row, &r.name, r.area, &r.unit);
            row
        })
        .collect();
    Ok(json!({ "count": list.len(), "note": super::lighting::NOTE, "rooms": list }))
}

fn run_check(s: &mut Session, _p: &Value) -> Result<Value> {
    let space = s.space();
    let d = s.doc()?;
    let (mut closed, mut open, mut lines, mut other) = (0usize, 0usize, 0usize, 0usize);
    if let Some(st) = d.space(&space) {
        for e in st.iter().filter(|e| e.common.layer.eq_ignore_ascii_case(RECT_LAYER.0)).take(MAX_TEXTS) {
            match &e.kind {
                EntityKind::LwPolyline(pl) if pl.closed => closed += 1,
                EntityKind::LwPolyline(_) => open += 1,
                EntityKind::Line(_) => lines += 1,
                _ => other += 1,
            }
        }
    }
    if closed + open + lines + other == 0 {
        return Err(bad("room.check", "no objects on ROOM-RECT in this space"));
    }
    let result = if open + lines + other == 0 { "PASS" } else { "WARN" };
    Ok(json!({ "result": result, "closed": closed, "open": open, "lines": lines, "other": other }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(s: &mut Session, name: &str, x0: f64, y0: f64, x1: f64, y1: f64, rectangle: bool) {
        s.execute("room", &json!({ "p1": [x0, y0], "p2": [x1, y1], "name": name, "rectangle": rectangle, "units": "m", "textHeight": 0.2 })).unwrap();
    }

    #[test]
    fn the_log_pairs_each_name_with_its_own_area_whatever_the_drawing_order() {
        let mut s = Session::new();
        room(&mut s, "br", 0.0, 0.0, 4.0, 3.0, true);
        room(&mut s, "ki", 10.0, 0.0, 13.0, 2.0, true);
        room(&mut s, "li", 20.0, 0.0, 25.0, 4.0, false);
        let r = s.execute("room.log", &json!({})).unwrap();
        assert_eq!(r["count"], json!(3));
        let rooms = r["rooms"].as_array().unwrap();
        let area = |name: &str| rooms.iter().find(|x| x["name"] == name).unwrap()["area"].as_f64().unwrap();
        assert_eq!(area("BEDROOM"), 12.0);
        assert_eq!(area("KITCHEN"), 6.0);
        assert_eq!(area("LIVING ROOM"), 20.0);
        assert!(r["csv"].as_str().unwrap().starts_with("RoomType,Size,Area,AreaUnit,InsX,InsY\nBEDROOM,"));
    }

    #[test]
    fn total_sums_areas_matching_a_filter() {
        let mut s = Session::new();
        room(&mut s, "br", 0.0, 0.0, 4.0, 3.0, true);
        room(&mut s, "mb", 10.0, 0.0, 14.0, 4.0, true);
        room(&mut s, "ki", 20.0, 0.0, 23.0, 2.0, true);
        let all = s.execute("room.total", &json!({})).unwrap();
        assert_eq!((all["count"].clone(), all["total"].clone()), (json!(3), json!(34.0)));
        let beds = s.execute("room.total", &json!({ "filter": "bedroom" })).unwrap();
        assert_eq!((beds["count"].clone(), beds["total"].clone()), (json!(2), json!(28.0)));
        assert_eq!(beds["unit"], json!("m2"));
        assert_eq!(s.execute("room.total", &json!({ "filter": "zzz" })).unwrap()["count"], json!(0));
    }

    #[test]
    fn relabel_renames_in_place_and_uses_codes() {
        let mut s = Session::new();
        room(&mut s, "br", 0.0, 0.0, 4.0, 3.0, false);
        let rooms = find_rooms(s.doc().unwrap(), &aad_doc::Space::Model);
        let h = rooms[0].name_text.hex();
        let r = s.execute("room.relabel", &json!({ "handle": h, "name": "ki" })).unwrap();
        assert_eq!(r["old"], json!("BEDROOM"));
        assert_eq!(r["new"], json!("KITCHEN"));
        assert!(s.execute("room.relabel", &json!({ "handle": h })).is_err());
        assert!(s.execute("room.relabel", &json!({ "handle": h, "name": "  " })).is_err());
        let r = s.execute("room.log", &json!({})).unwrap();
        assert_eq!(r["rooms"][0]["name"], json!("KITCHEN"));
        let none = s.execute("room.relabel", &json!({ "handles": [], "name": "ki" }));
        assert!(none.is_err());
    }

    #[test]
    fn audit_finds_labels_without_a_box_and_boxes_without_a_label() {
        let mut s = Session::new();
        room(&mut s, "br", 0.0, 0.0, 4.0, 3.0, true);
        room(&mut s, "ki", 10.0, 0.0, 13.0, 2.0, false);
        s.execute("rectang", &json!({ "p1": [30, 0], "p2": [34, 4] })).unwrap();
        let d = s.doc_mut().unwrap();
        let lone = d.model.iter().last().unwrap().handle;
        d.modify_entity(lone, |e| e.common.layer = RECT_LAYER.0.to_string()).unwrap();
        let r = s.execute("room.audit", &json!({})).unwrap();
        assert_eq!(r["labels"], json!(2));
        assert_eq!(r["rectangles"], json!(2));
        assert_eq!(r["labelsWithoutRectangle"].as_array().unwrap().len(), 1);
        assert_eq!(r["rectanglesWithoutLabel"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn check_passes_closed_rectangles_and_warns_about_lines() {
        let mut s = Session::new();
        assert!(s.execute("room.check", &json!({})).is_err(), "nothing on the layer");
        room(&mut s, "br", 0.0, 0.0, 4.0, 3.0, true);
        assert_eq!(s.execute("room.check", &json!({})).unwrap()["result"], json!("PASS"));
        let r = s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
        let h = Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
        s.doc_mut().unwrap().modify_entity(h, |e| e.common.layer = RECT_LAYER.0.to_string()).unwrap();
        let r = s.execute("room.check", &json!({})).unwrap();
        assert_eq!((r["result"].clone(), r["lines"].clone()), (json!("WARN"), json!(1)));
    }

    #[test]
    fn lighting_follows_the_declared_room() {
        let mut s = Session::new();
        room(&mut s, "br", 0.0, 0.0, 4.0, 3.0, true);
        room(&mut s, "st", 10.0, 0.0, 14.0, 4.0, false);
        let light = s.execute("room.lighting", &json!({})).unwrap();
        let rooms = light["rooms"].as_array().unwrap();
        let bed = rooms.iter().find(|r| r["name"] == "BEDROOM").unwrap();
        assert_eq!(bed["declared"], json!("Bedroom"));
        assert_eq!(bed["lm_per_m2"], json!(100));
        assert_eq!(bed["lm_per_ft2"], json!(9.29));
        assert_eq!(bed["flux_lm"], json!(1200.0));
        let study = rooms.iter().find(|r| r["name"] == "STUDY ROOM").unwrap();
        assert_eq!(study["lm_per_m2"], json!(500));
        assert_eq!(study["flux_lm"], json!(8000.0));
        assert!(light["note"].as_str().unwrap().contains("not the lamp catalog"));
        let exported = rooms_for_export(s.doc().unwrap(), &aad_doc::Space::Model);
        let bed_row = exported.iter().find(|r| r.row["name"] == "BEDROOM").unwrap();
        assert_eq!(bed_row.row["lm_per_m2"], json!(100));
    }

    #[test]
    fn an_undeclared_room_uses_the_general_level() {
        let mut s = Session::new();
        room(&mut s, "Plant", 0.0, 0.0, 2.0, 2.0, false);
        let row = &s.execute("room.lighting", &json!({})).unwrap()["rooms"][0];
        assert_eq!(row["lighting_matched"], json!(false));
        assert_eq!(row["declared"], json!("General"));
        assert_eq!(row["lm_per_m2"], json!(150));
        assert_eq!(row["flux_lm"], json!(600.0));
        assert!(Session::new().execute("room.lighting", &json!({})).is_err());
    }

    #[test]
    fn the_area_text_parses_and_hostile_text_is_ignored() {
        assert_eq!(parse_area("Area: 12.50 m2"), Some((12.5, "m2".into())));
        assert_eq!(parse_area("Area: 1.00 sq ft"), Some((1.0, "sq ft".into())));
        assert_eq!(parse_area("Area: NaN m2"), None);
        assert_eq!(parse_area("Area: inf m2"), None);
        assert_eq!(parse_area("Area:"), None);
        assert_eq!(parse_area("12 m2"), None);
    }
}
