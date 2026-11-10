use std::sync::Arc;

use aad_doc::{Arc as DocArc, Block, Circle, Entity, EntityKind, Handle, Insert, Line, LwPolyline, Solid};
use aad_geom::{PolyVertex, Vec2, Vec3};

use super::*;

/// A drawing builder with sequential handles.
pub(crate) struct Draw {
    pub d: Drawing,
    pub next: u64,
}

impl Draw {
    pub fn new(insunits: i64) -> Draw {
        let mut d = Drawing::new_metric();
        d.header.set_i64("INSUNITS", insunits);
        Draw { d, next: 0x100 }
    }
    pub fn line(&mut self, a: (f64, f64), b: (f64, f64)) -> Handle {
        self.line_on("0", a, b)
    }
    pub fn line_on(&mut self, layer: &str, a: (f64, f64), b: (f64, f64)) -> Handle {
        let h = Handle(self.next);
        self.next += 1;
        let mut e = Entity::new(h, EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) }));
        e.common.layer = layer.into();
        self.d.model.push(e);
        h
    }
    pub fn arc(&mut self, c: (f64, f64), r: f64, start_deg: f64, end_deg: f64) -> Handle {
        let h = Handle(self.next);
        self.next += 1;
        self.d.model.push(Entity::new(
            h,
            EntityKind::Arc(DocArc { center: Vec3::new(c.0, c.1, 0.0), radius: r, start: start_deg.to_radians(), end: end_deg.to_radians() }),
        ));
        h
    }
    pub fn circle(&mut self, layer: &str, c: (f64, f64), r: f64) -> Handle {
        let h = Handle(self.next);
        self.next += 1;
        let mut e = Entity::new(h, EntityKind::Circle(Circle { center: Vec3::new(c.0, c.1, 0.0), radius: r }));
        e.common.layer = layer.into();
        self.d.model.push(e);
        h
    }
    /// A filled rectangle (a SOLID), which the tables keep as a hatch row.
    pub fn solid(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) -> Handle {
        let h = Handle(self.next);
        self.next += 1;
        let c = [Vec3::new(x0, y0, 0.0), Vec3::new(x1, y0, 0.0), Vec3::new(x0, y1, 0.0), Vec3::new(x1, y1, 0.0)];
        self.d.model.push(Entity::new(h, EntityKind::Solid(Solid { corners: c })));
        h
    }
    pub fn dash(&mut self, h: Handle) {
        let _ = self.d.modify_entity(h, |e| e.common.linetype = "DASHED".into());
    }
    pub fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) -> Handle {
        let h = Handle(self.next);
        self.next += 1;
        let pl = LwPolyline {
            vertices: [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                .iter()
                .map(|p| PolyVertex { p: Vec2::new(p.0, p.1), bulge: 0.0, start_width: 0.0, end_width: 0.0 })
                .collect(),
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        };
        self.d.model.push(Entity::new(h, EntityKind::LwPolyline(pl)));
        h
    }
}

/// A 6 m x 4 m room with 230 mm walls, a 900 mm door in the bottom wall (hinge at its left jamb) and
/// a 1200 mm window in the top wall drawn as three thin lines. `k` scales every length (1 for mm).
pub(crate) fn room(insunits: i64, k: f64, t: f64) -> Draw {
    let mut g = Draw::new(insunits);
    let p = |x: f64, y: f64| (x * k, y * k);
    // Bottom wall, broken for the door at x 1000..1900.
    g.line(p(0.0, 0.0), p(1000.0, 0.0));
    g.line(p(1900.0, 0.0), p(6000.0, 0.0));
    g.line(p(t, t), p(1000.0, t));
    g.line(p(1900.0, t), p(6000.0 - t, t));
    // Left and right walls.
    g.line(p(0.0, 0.0), p(0.0, 4000.0));
    g.line(p(t, t), p(t, 4000.0 - t));
    g.line(p(6000.0, 0.0), p(6000.0, 4000.0));
    g.line(p(6000.0 - t, t), p(6000.0 - t, 4000.0 - t));
    // Top wall, broken for the window at x 2000..3200.
    g.line(p(0.0, 4000.0), p(2000.0, 4000.0));
    g.line(p(3200.0, 4000.0), p(6000.0, 4000.0));
    g.line(p(t, 4000.0 - t), p(2000.0, 4000.0 - t));
    g.line(p(3200.0, 4000.0 - t), p(6000.0 - t, 4000.0 - t));
    // Window: three thin lines across the wall thickness through the gap.
    for y in [4000.0 - t, 4000.0 - t / 2.0, 4000.0] {
        g.line(p(2000.0, y), p(3200.0, y));
    }
    // Door: hinge on the left jamb, a quarter-circle swing and the open leaf.
    g.arc(p(1000.0, t), 900.0 * k, 0.0, 90.0);
    g.line(p(1000.0, t), p(1000.0, t + 900.0));
    g
}

pub(crate) fn dump(m: &GeoModel) -> String {
    let mut rows: Vec<String> = m
        .features
        .iter()
        .filter(|f| f.confidence >= m.rules.evidence.classify_min)
        .map(|f| {
            format!(
                "{}.{} ({:.0},{:.0})-({:.0},{:.0}) len={:.0} w={:.0} conf={:.2} members={}",
                f.kind,
                f.subkind,
                f.x1.min(f.x2),
                f.y1.min(f.y2),
                f.x1.max(f.x2),
                f.y1.max(f.y2),
                f.length,
                f.width,
                f.confidence,
                f.members.len()
            )
        })
        .collect();
    rows.sort();
    rows.join("\n") + "\n"
}

fn walls(m: &GeoModel) -> Vec<&Feature> {
    m.features.iter().filter(|f| f.kind == "wall" && f.confidence >= m.rules.evidence.classify_min).collect()
}

pub(crate) fn count(m: &GeoModel, kind: &str) -> usize {
    m.features.iter().filter(|f| f.kind == kind && f.confidence >= m.rules.evidence.classify_min).count()
}

fn golden(name: &str, got: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden").join(name);
    if std::env::var("GEODB_BLESS").is_ok() {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(&path, got).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(got, want, "golden file {name} differs; run with GEODB_BLESS=1 to rewrite it after checking the change");
}

#[test]
fn the_room_has_five_wall_runs_a_door_and_a_window_and_matches_its_golden_file() {
    let g = room(4, 1.0, 230.0);
    let m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert_eq!(m.units.unit, "mm");
    assert!(m.units.confirmed);
    assert_eq!(count(&m, "wall"), 5, "{}", dump(&m));
    assert_eq!(count(&m, "door"), 1, "{}", dump(&m));
    assert_eq!(count(&m, "window"), 1, "{}", dump(&m));
    assert!(walls(&m).iter().all(|f| f.subkind == "full"));
    golden("room.txt", &dump(&m));
}

#[test]
fn wall_types_follow_the_thickness_and_the_drawing_units() {
    // Half brick, full brick and cavity in a metric drawing.
    for (t, want) in [(115.0, "half"), (230.0, "full"), (300.0, "cavity")] {
        let m = GeoModel::build(&room(4, 1.0, t).d, &BuildOptions::default()).unwrap();
        assert!(count(&m, "wall") >= 4, "{t}: {}", dump(&m));
        assert!(walls(&m).iter().all(|f| f.subkind == want), "{t}: {}", dump(&m));
    }
    // The same room drafted in inches: 9 in = 228.6 mm is a full brick wall.
    let inch = 1.0 / 25.4;
    let m = GeoModel::build(&room(1, inch, 9.0 / inch).d, &BuildOptions::default()).unwrap();
    assert_eq!(m.units.unit, "in");
    assert_eq!(m.units.system, UnitSystem::Imperial);
    assert_eq!(count(&m, "wall"), 5, "{}", dump(&m));
    assert!(walls(&m).iter().all(|f| f.subkind == "full" && (f.width - 228.6).abs() < 1e-6));
    // Rows are millimetres whatever the drawing unit.
    let longest = m.owners.values().flat_map(|e| e.segs.iter()).map(|s| s.len).fold(0.0, f64::max);
    assert!((longest - 4100.0).abs() < 1e-6, "{longest}");
}

#[test]
fn a_unitless_drawing_gets_a_proposal_and_waits_for_confirmation() {
    // The room drawn in metres with no INSUNITS.
    let g = room(0, 0.001, 230.0);
    let m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert!(!m.units.confirmed, "a guess is never confirmed");
    assert_eq!(m.units.unit, "m", "{:?}", m.scale_check);
    assert_eq!(count(&m, "wall"), 5, "{}", dump(&m));
    assert!(m.warnings.iter().any(|w| w.contains("no units")), "{:?}", m.warnings);
    // Naming the unit confirms it.
    let m = GeoModel::build(&g.d, &BuildOptions { units: Some("m".into()), ..BuildOptions::default() }).unwrap();
    assert!(m.units.confirmed && m.units.source == UnitSource::Override);
    // A wrong declared unit is flagged, not silently trusted.
    let mut wrong = room(0, 0.001, 230.0);
    wrong.d.header.set_i64("INSUNITS", 4);
    let m = GeoModel::build(&wrong.d, &BuildOptions::default()).unwrap();
    assert!(m.warnings.iter().any(|w| w.contains("fit m much better")), "{:?}", m.warnings);
}

#[test]
fn the_drawing_stamp_beats_insunits() {
    let mut g = room(4, 1.0, 230.0);
    g.d.project.insert(units::UNITS_KEY.into(), r#"{"system":"metric","unit":"mm"}"#.into());
    g.d.header.set_i64("INSUNITS", 0);
    let m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert!(m.units.confirmed && m.units.source == UnitSource::Project);
}

#[test]
fn layers_do_not_change_what_is_found_only_how_sure_it_is() {
    let mut g = room(4, 1.0, 230.0);
    let plain = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    let hs = g.d.model.handles();
    for h in hs {
        let _ = g.d.modify_entity(h, |e| e.common.layer = "A-WALL".into());
    }
    let named = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert_eq!(
        count(&plain, "wall"),
        count(&named, "wall"),
        "{}
---
{}",
        dump(&plain),
        dump(&named)
    );
    let (a, b): (f64, f64) = (
        plain.features.iter().filter(|f| f.kind == "wall").map(|f| f.confidence).sum(),
        named.features.iter().filter(|f| f.kind == "wall").map(|f| f.confidence).sum(),
    );
    assert!(b > a, "a layer called A-WALL adds evidence: {a} vs {b}");
}

#[test]
fn stair_treads_are_not_walls() {
    let mut g = Draw::new(4);
    for i in 0..12 {
        g.line((0.0, 300.0 * i as f64), (1200.0, 300.0 * i as f64));
    }
    let m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert_eq!(count(&m, "wall"), 0, "{}", dump(&m));
}

#[test]
fn a_single_thick_pair_that_is_too_short_is_not_a_wall() {
    let mut g = Draw::new(4);
    g.rect(0.0, 0.0, 500.0, 230.0);
    let m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert_eq!(count(&m, "wall"), 0, "a 230 x 500 column section is not a wall");
}

#[test]
fn door_and_window_blocks_are_found_by_name_and_by_attribute() {
    let mut g = room(4, 1.0, 230.0);
    let mut b = Block::new("AADOPEN-1");
    b.entities.push(Entity::new(Handle(0x1), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(900.0, 0.0, 0.0) })));
    g.d.blocks.insert("AADOPEN-1".into(), Arc::new(b));
    let mut ins = Insert { block: "AADOPEN-1".into(), insert: Vec3::new(8000.0, 0.0, 0.0), ..Insert::default() };
    ins.set_attrib("OKIND", "window");
    ins.set_attrib("OWIDTH", "900");
    g.d.model.push(Entity::new(Handle(0x900), EntityKind::Insert(ins)));
    let m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    let w: Vec<_> = m.features.iter().filter(|f| f.kind == "window" && f.subkind == "block").collect();
    assert_eq!(w.len(), 1);
    assert!((w[0].width - 900.0).abs() < 1e-6 && w[0].confidence > 0.9);
    assert_eq!(w[0].members.first().map(|m| m.handle), Some(0x900));
}

#[test]
fn hostile_numbers_never_panic() {
    let mut g = Draw::new(4);
    g.line((f64::NAN, 0.0), (10.0, 10.0));
    g.line((0.0, 0.0), (f64::INFINITY, 5.0));
    g.line((1e300, 1e300), (-1e300, 5.0));
    g.line((0.0, 0.0), (0.0, 0.0));
    g.arc((0.0, 0.0), f64::NAN, 0.0, 90.0);
    g.arc((0.0, 0.0), -5.0, 0.0, 90.0);
    g.rect(0.0, 0.0, f64::MAX, f64::MAX);
    let mut d = g.d;
    d.header.set_f64("INSUNITS", f64::NAN);
    let m = GeoModel::build(&d, &BuildOptions::default());
    assert!(m.is_ok());
    // A NaN unit name and absurd rules are errors, not panics.
    assert!(GeoModel::build(&d, &BuildOptions { units: Some("parsec".into()), ..BuildOptions::default() }).is_err());
    let mut r = Rules::default();
    assert!(r.set_section("pair", &serde_json::json!({"angle_tol_deg": f64::NAN})).is_err());
}

#[test]
fn an_incremental_update_equals_a_full_rebuild() {
    let g = room(4, 1.0, 230.0);
    let mut d = g.d;
    let mut m = GeoModel::build(&d, &BuildOptions::default()).unwrap();
    let before = dump(&m);
    // Edit 1: stretch the right wall's outer face 1 m (it is one line).
    let target = d
        .model
        .iter()
        .find(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.x == 6000.0 && l.b.x == 6000.0 && l.b.y == 4000.0))
        .map(|e| e.handle)
        .unwrap();
    d.modify_entity(target, |e| {
        if let EntityKind::Line(l) = &mut e.kind {
            l.b.y = 4000.0 - 500.0;
        }
    })
    .unwrap();
    let (stats, delta) = m.update(&d);
    assert_eq!(stats.changed, 1);
    assert!(!delta.added_features.is_empty() || !delta.dropped_features.is_empty());
    let fresh = GeoModel::build(&d, &BuildOptions::default()).unwrap();
    assert_eq!(dump(&m), dump(&fresh), "incremental result differs from a rebuild\nbefore:\n{before}");
    assert_eq!(m.sha256, fresh.sha256);
    // Edit 2: erase the door's swing arc; the door goes.
    let arc = d.model.iter().find(|e| matches!(e.kind, EntityKind::Arc(_))).map(|e| e.handle).unwrap();
    assert!(d.remove_entity(arc).is_some());
    let (stats, _) = m.update(&d);
    assert_eq!(stats.removed, 1);
    assert_eq!(count(&m, "door"), 0);
    assert_eq!(dump(&m), dump(&GeoModel::build(&d, &BuildOptions::default()).unwrap()));
    // Edit 3: add a new wall pair far away.
    let mut g2 = Draw { d, next: 0x5000 };
    g2.line((10000.0, 0.0), (14000.0, 0.0));
    g2.line((10000.0, 230.0), (14000.0, 230.0));
    let (stats, _) = m.update(&g2.d);
    assert_eq!(stats.changed, 2);
    assert_eq!(dump(&m), dump(&GeoModel::build(&g2.d, &BuildOptions::default()).unwrap()));
    // Nothing changed: nothing to do.
    let (stats, _) = m.update(&g2.d);
    assert_eq!(stats.changed + stats.removed, 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_tables_are_written_in_bulk_updated_incrementally_and_never_logged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.osp");
    let mut conn = store::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE change (revision INTEGER NOT NULL, tbl TEXT NOT NULL, key TEXT NOT NULL, op TEXT NOT NULL);").unwrap();
    let g = room(4, 1.0, 230.0);
    let mut d = g.d;
    let mut m = GeoModel::build(&d, &BuildOptions::default()).unwrap();
    let w = store::write_full(&mut conn, &m, "room", 3).unwrap();
    assert!(w.rtree && w.rows > 0);
    let count_of = |c: &rusqlite::Connection, t: &str| -> i64 { c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).unwrap() };
    let segs: usize = m.owners.values().map(|e| e.segs.len()).sum();
    assert_eq!(count_of(&conn, "geo_seg") as usize, segs, "the tables match the entity counts");
    assert_eq!(count_of(&conn, "geo_prim") as usize, m.owners.values().map(|e| e.prims.len()).sum::<usize>());
    assert_eq!(count_of(&conn, "geo_rtree") as usize, count_of(&conn, "geo_prim") as usize);
    assert_eq!(count_of(&conn, "geo_feature") as usize, m.features.len());
    assert!(!m.rooms.is_empty(), "the room and the slab");
    assert_eq!(count_of(&conn, "geo_room") as usize, m.rooms.len());
    assert_eq!(count_of(&conn, "geo_region") as usize, m.regions.len());
    let sha: String = conn.query_row("SELECT drawing_sha256 FROM geo_run", [], |r| r.get(0)).unwrap();
    assert_eq!(sha, m.sha256);
    let q1: i64 = conn.query_row("SELECT qx2 FROM geo_seg WHERE x2 = 1000 LIMIT 1", [], |r| r.get(0)).unwrap();
    assert_eq!(q1, 10_000, "quantised twin is 0.1 mm");
    // Edit and apply the delta; the stored tables match a fresh write.
    let target = d.model.iter().find(|e| matches!(e.kind, EntityKind::Arc(_))).map(|e| e.handle).unwrap();
    d.remove_entity(target).unwrap();
    let (_, delta) = m.update(&d);
    store::write_update(&mut conn, &m, &delta, "room", 4).unwrap();
    assert_eq!(count_of(&conn, "geo_feature") as usize, m.features.len());
    assert_eq!(count_of(&conn, "geo_seg") as usize, m.owners.values().map(|e| e.segs.len()).sum::<usize>());
    assert_eq!(count_of(&conn, "geo_pair") as usize, m.pair_rows.len());
    assert_eq!(count_of(&conn, "geo_room") as usize, m.rooms.len());
    assert_eq!(count_of(&conn, "geo_hatch_link") as usize, m.hatch_links.len());
    assert_eq!(count_of(&conn, "geo_feature_member") as i64, m.features.iter().map(|f| f.members.len() as i64).sum::<i64>());
    // Nothing reached the change log.
    assert_eq!(count_of(&conn, "change"), 0);
    let s = store::read_summary(&conn).unwrap();
    assert_eq!(s["rows"]["features"], serde_json::json!(m.features.len()));
}

#[test]
fn rules_can_be_edited_and_reclassify_without_extraction() {
    let g = room(4, 1.0, 230.0);
    let mut m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert_eq!(count(&m, "wall"), 5);
    let mut r = m.rules.clone();
    // Ask for 2 m of shared face: the short pieces beside the door and window drop out.
    r.set_section("pair", &serde_json::json!({"min_run_mm": 2500.0})).unwrap();
    m.reclassify(r).unwrap();
    assert_eq!(m.rules.version, 2);
    assert_eq!(count(&m, "wall"), 4, "{}", dump(&m));
    assert!(m.features.iter().all(|f| f.rules_version == 2));
}

#[test]
fn feature_keys_survive_a_rebuild_and_change_with_the_geometry() {
    let g = room(4, 1.0, 230.0);
    let a = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    let b = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    let ka: std::collections::BTreeSet<_> = a.features.iter().map(|f| f.key.clone()).collect();
    let kb: std::collections::BTreeSet<_> = b.features.iter().map(|f| f.key.clone()).collect();
    assert_eq!(ka, kb);
    assert_eq!(ka.len(), a.features.len(), "keys are unique");
}
