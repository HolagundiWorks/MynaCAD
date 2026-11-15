//! Layout viewports bound to a section or elevation view space (`viewport.set {view}`).

use aad_engine::Session;
use aad_engine::doc::{EntityKind, Handle, Space, Viewport};
use aad_render::{Options, VIEWPORT_CONTENT, build};
use serde_json::{Value, json};

/// Millimetre drawing with a wall and a section "Section A-A" holding one drafted line; Layout1 is current.
fn project() -> Session {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s.execute("level.new", &json!({"name": "First", "first": "Ground", "elevation": 3150.0, "height": 3000.0})).unwrap();
    s.execute("level.set", &json!({"level": "Ground", "elevation": 0.0, "height": 3000.0})).unwrap();
    s.execute("wall.draw", &json!({"points": [[4115.0, -3000.0], [4115.0, 3000.0]], "thickness": 230.0})).unwrap();
    s.execute("section.new", &json!({"p1": [0.0, 0.0], "p2": [10000.0, 0.0], "depth": 5000.0})).unwrap();
    s.execute("view.activate", &json!({"view": "Section A-A"})).unwrap();
    s.execute("line", &json!({"points": [[100.0, 100.0], [900.0, 100.0]]})).unwrap();
    s.execute("level.activate", &json!({"level": "Ground"})).unwrap();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    s
}

fn add(s: &mut Session, extra: Value) -> Result<Handle, String> {
    let mut p = json!({"scale": "1:100"});
    if let (Some(o), Some(e)) = (p.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    let r = s.execute("viewport.add", &p).map_err(|e| e.to_string())?;
    Ok(Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap())
}

fn viewport(s: &Session, h: Handle) -> Viewport {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Viewport(v) => v.clone(),
        k => panic!("{k:?}"),
    }
}

fn counts(s: &Session) -> (usize, usize) {
    let l = build(s.doc().unwrap(), &Space::Paper("Layout1".into()), &Options::default());
    let guides = l.prims.iter().filter(|p| p.handle == VIEWPORT_CONTENT).count();
    (guides, l.prims.len() - guides)
}

#[test]
fn a_viewport_can_show_a_view_with_faded_guides_and_its_drafted_objects() {
    let mut s = project();
    let plain = add(&mut s, json!({})).unwrap();
    let (g0, o0) = counts(&s);
    let _ = o0;

    let vid = s.doc().unwrap().view_space_by_name("Section A-A").unwrap().rec.id;
    s.execute("viewport.set", &json!({"handle": plain.hex(), "view": "Section A-A", "viewHeight": 9000.0})).unwrap();
    assert_eq!(viewport(&s, plain).view, Some(vid));
    let (g1, o1) = counts(&s);
    assert!(g1 >= 7, "guides and the drafted line are drawn: g0={g0} o0={o0} g1={g1} o1={o1}");
    assert!(o1 >= 1, "the drafted line shows as a normal object");

    // Back to a level.
    s.execute("viewport.set", &json!({"handle": plain.hex(), "view": null})).unwrap();
    assert_eq!(viewport(&s, plain).view, None);
    // Binding a level takes it off the view; by id works too.
    s.execute("viewport.set", &json!({"handle": plain.hex(), "view": vid.0})).unwrap();
    s.execute("viewport.set", &json!({"handle": plain.hex(), "level": "First"})).unwrap();
    assert_eq!(viewport(&s, plain).view, None);
    // viewport.add takes it directly and the listing reports it.
    let h = add(&mut s, json!({"view": "Section A-A"})).unwrap();
    assert_eq!(viewport(&s, h).view, Some(vid));
    let l = s.execute("viewport.list", &json!({})).unwrap();
    assert!(l.to_string().contains("\"view\""), "{l}");
}

#[test]
fn deleting_the_view_unbinds_viewports_and_undo_restores_it() {
    let mut s = project();
    let h = add(&mut s, json!({"view": "Section A-A"})).unwrap();
    s.execute("view.delete", &json!({"view": "Section A-A", "force": true})).unwrap();
    assert_eq!(viewport(&s, h).view, None);
    s.undo().unwrap();
    assert!(viewport(&s, h).view.is_some());
}

#[test]
fn view_binding_round_trips_through_dxf() {
    let mut s = project();
    let h = add(&mut s, json!({"view": "Section A-A"})).unwrap();
    let want = viewport(&s, h).view;
    let bytes = aad_io::write(s.doc().unwrap(), "v.dxf").unwrap();
    let back = aad_io::read(&bytes, "v.dxf").unwrap();
    let found = back
        .layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .filter_map(|e| if let EntityKind::Viewport(v) = &e.kind { Some(v.view) } else { None })
        .any(|v| v.is_some() && v == want);
    assert!(found, "the viewport still shows the view after a DXF round trip");
    assert!(back.view_space_by_name("Section A-A").is_some());
}

#[test]
fn hostile_view_values_are_refused() {
    let mut s = project();
    let h = add(&mut s, json!({})).unwrap();
    for bad in [json!("nope"), json!(99999), json!(-1), json!(1.5), json!(true), json!([]), json!({}), json!("x".repeat(100_000)), json!(u64::MAX)] {
        assert!(s.execute("viewport.set", &json!({"handle": h.hex(), "view": bad})).is_err(), "{bad}");
        assert!(s.execute("viewport.add", &json!({"view": bad})).is_err(), "{bad}");
    }
    assert_eq!(viewport(&s, h).view, None);
}

#[test]
fn sheet_index_binds_view_sheets() {
    let mut s = project();
    let r = s.execute("sheetindex.build", &json!({"levels": false, "allViews": true})).unwrap();
    assert_eq!(r["created"].as_array().unwrap().len(), 1, "{r}");
    assert!(r["warnings"].as_array().unwrap().is_empty(), "{r}");
    let l = s.execute("sheet.list", &json!({})).unwrap();
    let layout = l["sheets"][0]["layout"].as_str().unwrap().to_string();
    let vps = s.execute("viewport.list", &json!({"layout": layout})).unwrap();
    assert!(vps.to_string().contains("\"view\":"), "{vps}");
    let vid = s.doc().unwrap().view_space_by_name("Section A-A").unwrap().rec.id;
    let vp = s
        .doc()
        .unwrap()
        .layout(&layout)
        .unwrap()
        .entities
        .iter()
        .find_map(|e| if let EntityKind::Viewport(v) = &e.kind { (v.id != 1).then(|| v.clone()) } else { None })
        .unwrap();
    assert_eq!(vp.view, Some(vid));
}
