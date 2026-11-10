//! Phase 2: columns, rooms, beams, page regions, hatches and the slab.

use aad_doc::EntityKind;

use super::*;
use crate::tests::{Draw, count, dump, room};

fn of_kind<'a>(m: &'a GeoModel, kind: &str) -> Vec<&'a Feature> {
    m.features.iter().filter(|f| f.kind == kind).collect()
}

fn region_kinds(m: &GeoModel) -> Vec<String> {
    m.regions.iter().filter(|r| r.kind != "empty").map(|r| r.kind.clone()).collect()
}

fn keys(m: &GeoModel) -> Vec<String> {
    let mut v: Vec<String> = m.features.iter().map(|f| format!("{}:{}", f.kind, f.key)).collect();
    v.sort();
    v
}

#[test]
fn small_closed_rectangles_and_circles_are_columns_and_leave_the_wall_candidates() {
    let mut g = Draw::new(4);
    g.rect(0.0, 0.0, 400.0, 400.0);
    g.solid(150.0, 150.0, 250.0, 250.0); // a fill inside the first one
    // Four separate lines that meet at square corners.
    for (a, b) in
        [((2000.0, 0.0), (2400.0, 0.0)), ((2400.0, 0.0), (2400.0, 400.0)), ((2400.0, 400.0), (2000.0, 400.0)), ((2000.0, 400.0), (2000.0, 0.0))]
    {
        g.line_on("A-COL", a, b);
    }
    g.circle("A-COL", (4000.0, 200.0), 200.0);
    // A real wall pair elsewhere.
    g.line((0.0, 3000.0), (5000.0, 3000.0));
    g.line((0.0, 3230.0), (5000.0, 3230.0));
    let m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    let cols = of_kind(&m, "column");
    assert_eq!(cols.len(), 3, "{cols:?}");
    assert_eq!(cols.iter().filter(|c| c.subkind == "rect").count(), 2);
    assert_eq!(cols.iter().filter(|c| c.subkind == "round").count(), 1);
    assert!(count(&m, "column") >= 2, "the filled one and the one on a column layer pass the threshold: {}", dump(&m));
    assert!(cols.iter().any(|c| c.props.get("filled").and_then(|v| v.as_bool()) == Some(true)), "a fill inside the section is evidence");
    assert_eq!(count(&m, "wall"), 1, "{}", dump(&m));
    // A 230 x 5000 closed rectangle is a wall piece, not a column.
    let mut g2 = Draw::new(4);
    g2.rect(0.0, 0.0, 230.0, 5000.0);
    let m2 = GeoModel::build(&g2.d, &BuildOptions::default()).unwrap();
    assert_eq!(of_kind(&m2, "column").len(), 0, "too long and thin for a column");
}

#[test]
fn a_closed_loop_of_walls_is_a_room_with_an_area_a_perimeter_and_a_slab() {
    let g = room(4, 1.0, 230.0);
    let m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert_eq!(of_kind(&m, "room").len(), 1, "{}", dump(&m));
    let r = m.rooms.iter().find(|r| r.kind == "room").unwrap();
    let want = 5540.0 * 3540.0;
    assert!((r.area_mm2 - want).abs() / want < 0.03, "area {} vs {want}", r.area_mm2);
    let want_p = 2.0 * (5540.0 + 3540.0);
    assert!((r.perimeter_mm - want_p).abs() / want_p < 0.05, "perimeter {} vs {want_p}", r.perimeter_mm);
    assert!(r.outline.len() >= 4 && r.outline.len() <= 12, "{:?}", r.outline);
    assert_eq!(of_kind(&m, "slab").len(), 1);
    let sa = 6000.0 * 4000.0;
    let slab = m.rooms.iter().find(|r| r.kind == "slab").unwrap();
    assert!((slab.area_mm2 - sa).abs() / sa < 0.03, "slab {} vs {sa}", slab.area_mm2);
    assert_eq!(region_kinds(&m), vec!["plan".to_string()]);
    // Walls that bound the room gained confidence over what the pair alone gives.
    let mut plain = m.rules.clone();
    plain.set_section("room", &serde_json::json!({"room_bonus": 0.0, "wall_hatch_bonus": 0.0})).unwrap();
    let mut m2 = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    m2.reclassify(plain).unwrap();
    let sum = |m: &GeoModel| -> f64 { m.features.iter().filter(|f| f.kind == "wall").map(|f| f.confidence).sum() };
    assert!(sum(&m) > sum(&m2) + 0.3, "{} vs {}", sum(&m), sum(&m2));
    // A side of the room taken away (both faces) leaves no loop: no room, no slab.
    let mut open = room(4, 1.0, 230.0);
    for x in [6000.0, 6000.0 - 230.0] {
        let victim = open
            .d
            .model
            .iter()
            .find(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.x == x && l.b.x == x && l.b.y > 3000.0))
            .map(|e| e.handle)
            .unwrap();
        let _ = open.d.remove_entity(victim);
    }
    let m3 = GeoModel::build(&open.d, &BuildOptions::default()).unwrap();
    assert_eq!(of_kind(&m3, "room").len(), 0, "{}", dump(&m3));
}

#[test]
fn an_elevation_beside_the_plan_does_not_produce_walls() {
    let mut g = room(4, 1.0, 230.0);
    // Two long horizontal pairs 20 m below the plan: a front view with a plinth and a roof line. No loop, no door swing.
    for y in [-20000.0, -19770.0, -16000.0, -15770.0] {
        g.line((0.0, y), (6000.0, y));
    }
    let with_plan = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert_eq!(count(&with_plan, "wall"), 5, "the elevation's pairs are not walls: {}", dump(&with_plan));
    let kinds = region_kinds(&with_plan);
    assert!(kinds.contains(&"plan".to_string()) && kinds.contains(&"other".to_string()), "{kinds:?}");
    // The same lines on a sheet with no plan are judged on their own and stay walls.
    let mut alone = Draw::new(4);
    for y in [-20000.0, -19770.0, -16000.0, -15770.0] {
        alone.line((0.0, y), (6000.0, y));
    }
    let m = GeoModel::build(&alone.d, &BuildOptions::default()).unwrap();
    assert_eq!(count(&m, "wall"), 2, "{}", dump(&m));
    // The penalty is a rule parameter.
    let mut r = with_plan.rules.clone();
    r.set_section("region", &serde_json::json!({"non_plan_penalty": 0.0})).unwrap();
    let mut again = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    again.reclassify(r).unwrap();
    assert_eq!(count(&again, "wall"), 7);
}

#[test]
fn dashed_and_floating_pairs_are_beams_not_walls() {
    let mut g = room(4, 1.0, 230.0);
    // A dashed pair across the room (a beam above), and a solid pair floating in the middle (not bounding anything).
    let a = g.line((230.0, 1500.0), (5770.0, 1500.0));
    let b = g.line((230.0, 1730.0), (5770.0, 1730.0));
    g.dash(a);
    g.dash(b);
    g.line((2000.0, 2600.0), (4000.0, 2600.0));
    g.line((2000.0, 2830.0), (4000.0, 2830.0));
    let m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert_eq!(count(&m, "wall"), 5, "{}", dump(&m));
    let beams = of_kind(&m, "beam");
    assert_eq!(beams.len(), 2, "{}", dump(&m));
    let ev = |f: &&Feature| f.props.get("beam_evidence").map(|v| v.to_string()).unwrap_or_default();
    assert!(
        beams.iter().any(|f| ev(f).contains("dashed")) && beams.iter().any(|f| ev(f).contains("floating")),
        "{:?}",
        beams.iter().map(ev).collect::<Vec<_>>()
    );
    assert!(beams.iter().all(|f| f.rule == "beam.pair" && f.confidence >= 0.5));
    // The room is still found: the beams do not cut it.
    assert_eq!(of_kind(&m, "room").len(), 1);
    // A threshold nothing reaches turns the beam rule off.
    let mut r = m.rules.clone();
    r.set_section("beam", &serde_json::json!({"min_score": 5.0})).unwrap();
    let mut off = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    off.reclassify(r).unwrap();
    assert_eq!(of_kind(&off, "beam").len(), 0);
}

#[test]
fn a_column_grid_without_doors_is_a_structural_layout() {
    let mut g = Draw::new(4);
    for (x, y) in [(0.0, 0.0), (6000.0, 0.0), (0.0, 4000.0), (6000.0, 4000.0)] {
        for (a, b) in
            [((x, y), (x + 400.0, y)), ((x + 400.0, y), (x + 400.0, y + 400.0)), ((x + 400.0, y + 400.0), (x, y + 400.0)), ((x, y + 400.0), (x, y))]
        {
            g.line_on("A-COL", a, b);
        }
    }
    // Beams between the columns, 230 wide.
    for y in [100.0, 330.0, 4100.0, 4330.0] {
        g.line((400.0, y), (6000.0, y));
    }
    for x in [100.0, 330.0, 6100.0, 6330.0] {
        g.line((x, 400.0), (x, 4000.0));
    }
    let m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert!(region_kinds(&m).contains(&"structure".to_string()), "{:?}", m.regions);
    assert_eq!(count(&m, "column"), 4);
    let walls_before = count(&m, "wall");
    assert!(walls_before >= 2, "{}", dump(&m));
    // The region alone is only part of the evidence (0.4 of the 0.5 needed); a stronger weight turns the pairs into beams.
    let mut r = m.rules.clone();
    r.set_section("beam", &serde_json::json!({"structure": 0.7})).unwrap();
    let mut b = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    b.reclassify(r).unwrap();
    assert!(count(&b, "beam") >= 2 && count(&b, "wall") < walls_before, "walls {walls_before} -> {} beams {}", count(&b, "wall"), count(&b, "beam"));
}

#[test]
fn hatches_are_kept_and_assigned_to_a_room_a_wall_or_nothing() {
    let mut g = room(4, 1.0, 230.0);
    g.solid(3000.0, 2000.0, 3400.0, 2400.0); // inside the room
    g.solid(60.0, 1500.0, 170.0, 1900.0); // inside the left wall (poche)
    g.solid(30000.0, 30000.0, 30400.0, 30400.0); // nowhere
    let m = GeoModel::build(&g.d, &BuildOptions::default()).unwrap();
    assert_eq!(m.owners.values().map(|e| e.hatches.len()).sum::<usize>(), 3, "boundaries stay in geo_hatch");
    let kinds: Vec<&str> = m.hatch_links.iter().map(|l| l.kind.as_str()).collect();
    assert_eq!(m.hatch_links.len(), 2, "{kinds:?}");
    assert!(kinds.contains(&"room") && kinds.contains(&"wall"), "{kinds:?}");
    let room_key = &of_kind(&m, "room")[0].key;
    assert!(m.hatch_links.iter().any(|l| l.kind == "room" && &l.feature_key == room_key));
    assert_eq!(m.rooms.iter().find(|r| r.kind == "room").map(|r| r.hatches), Some(1));
}

#[test]
fn an_incremental_update_equals_a_full_rebuild_for_rooms_beams_and_columns() {
    let g = room(4, 1.0, 230.0);
    let d = g.d;
    let mut m = GeoModel::build(&d, &BuildOptions::default()).unwrap();
    assert_eq!(of_kind(&m, "room").len(), 1);
    // Add a dashed beam pair, a column and a hatch.
    let mut g2 = Draw { d, next: 0x6000 };
    let a = g2.line((230.0, 1500.0), (5770.0, 1500.0));
    let b = g2.line((230.0, 1730.0), (5770.0, 1730.0));
    g2.dash(a);
    g2.dash(b);
    g2.rect(2500.0, 2500.0, 2900.0, 2900.0);
    g2.solid(2600.0, 2600.0, 2800.0, 2800.0);
    let (stats, delta) = m.update(&g2.d);
    assert_eq!(stats.changed, 4);
    assert!(!delta.added_features.is_empty());
    let fresh = GeoModel::build(&g2.d, &BuildOptions::default()).unwrap();
    assert_eq!(dump(&m), dump(&fresh), "after adding");
    assert_eq!(of_kind(&m, "beam").len(), 1);
    assert_eq!(m.hatch_links.len(), fresh.hatch_links.len());
    assert_eq!(keys(&m), keys(&fresh));
    // Open the room: erase a side. The room and the slab go in the incremental result as in the rebuild.
    let mut d = g2.d;
    let victim = d
        .model
        .iter()
        .find(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.x == 6000.0 && l.b.x == 6000.0 && l.b.y == 4000.0))
        .map(|e| e.handle)
        .unwrap();
    let _ = d.remove_entity(victim);
    let (_, delta) = m.update(&d);
    assert!(!delta.dropped_features.is_empty());
    let fresh = GeoModel::build(&d, &BuildOptions::default()).unwrap();
    assert_eq!(dump(&m), dump(&fresh), "after erasing");
    assert_eq!(keys(&m), keys(&fresh));
    assert_eq!(m.sha256, fresh.sha256);
}

#[test]
fn the_phase_two_rules_are_sections_that_validate() {
    let mut r = Rules::default();
    for s in rules::SECTIONS {
        assert!(r.section(s).is_some(), "{s}");
    }
    assert!(r.set_section("room", &serde_json::json!({"cell_mm": 1.0})).is_err());
    assert!(r.set_section("region", &serde_json::json!({"gap_mm": 10.0})).is_err());
    assert!(r.set_section("column", &serde_json::json!({"min_side_mm": 5000.0})).is_err());
    r.set_section("beam", &serde_json::json!({"min_score": 0.7})).unwrap();
    assert_eq!(r.version, 2);
    assert_eq!(r.beam.min_score, 0.7);
}

#[test]
fn hostile_sheets_do_not_panic_in_the_context_stage() {
    let mut g = room(4, 1.0, 230.0);
    // Absurd extents, a huge circle, a giant solid, tiny and huge coordinates side by side.
    g.line((1.0e8, 1.0e8), (1.0e8 + 5000.0, 1.0e8));
    g.line((1.0e8, 1.0e8 + 230.0), (1.0e8 + 5000.0, 1.0e8 + 230.0));
    g.circle("0", (0.0, 0.0), 1.0e12);
    g.solid(-1.0e9, -1.0e9, 1.0e9, 1.0e9);
    g.circle("0", (f64::NAN, 0.0), 300.0);
    g.rect(0.0, 0.0, 1.0e-9, 1.0e-9);
    assert!(GeoModel::build(&g.d, &BuildOptions::default()).is_ok());
    // Extreme rule values are rejected or survive.
    let mut r = Rules::default();
    for (sec, v) in [
        ("room", serde_json::json!({"max_cells": 1000, "cell_mm": 5.0})),
        ("room", serde_json::json!({"bridge_mm": 1.0e9})),
        ("region", serde_json::json!({"gap_mm": 100.0, "frame_share": 0.0})),
        ("column", serde_json::json!({"min_side_mm": 0.0, "max_side_mm": 1.0e9, "max_aspect": 1.0e9})),
    ] {
        let _ = r.set_section(sec, &v);
    }
    assert!(GeoModel::build(&room(4, 1.0, 230.0).d, &BuildOptions { rules: Some(r), ..BuildOptions::default() }).is_ok());
}
