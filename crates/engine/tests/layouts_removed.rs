//! Package H6 (docs/layout-book.md 11.1): the layout, viewport, page-setup, sheet-index, master and publisher commands are
//! gone from the catalog; what the Ops-first flow and the DXF/DWG round trip need is still there.

use aad_engine::{Session, cmd::find_command};
use serde_json::json;

const DELETED: &[&str] = &[
    "layout",
    "layout.new",
    "layout.delete",
    "layout.rename",
    "layout.copy",
    "layout.list",
    "layout.set",
    "mview",
    "viewport.set",
    "viewport.list",
    "viewport.add",
    "viewport.remove",
    "vplayer",
    "vpclip",
    "mspace",
    "pspace",
    "page.manager",
    "page.toggle",
    "pagesetup",
    "sheet.page",
    "sheet.titleblock",
    "sheet.designer",
    "sheet.fields",
    "sheet.template.save",
    "sheet.template.list",
    "sheet.template.apply",
    "sheet.template.delete",
    "sheet.index",
    "sheet.plotwindow",
    "sheet.list",
    "sheet.new",
    "sheet.set",
    "sheet.delete",
    "sheet.renumber",
    "sheet.move",
    "sheetindex.build",
    "sheetindex.fill",
    "sheetindex.table",
    "sheetindex.publish",
    "sheet.revision.add",
    "sheet.revision.list",
    "issue.create",
    "issue.delete",
    "issue.list",
    "master.list",
    "master.new",
    "master.set",
    "master.delete",
    "master.apply",
    "publisher.list",
    "publisher.new",
    "publisher.set",
    "publisher.delete",
    "publisher.check",
    "publisher.publish",
    "plot.batch",
    "floor.viewport",
    "compose.view.apply",
];

const KEPT: &[&str] = &[
    "plot",
    "plot.window",
    "plotl",
    "plotp",
    "exportpdf",
    "plotstyle",
    "stylesmanager",
    "sheet.scheme",
    "compose.view.list",
    "compose.view.save",
    "compose.view.set",
    "compose.view.delete",
    "printblock",
    "revision.clouds",
    "revision.list",
    "xclip",
    "sheetset.list",
    "level.list",
];

#[test]
fn the_deleted_commands_are_not_in_the_catalog_and_the_kept_ones_are() {
    for id in DELETED {
        assert!(find_command(id).is_none(), "`{id}` should have been removed");
    }
    for id in KEPT {
        assert!(find_command(id).is_some(), "`{id}` must stay");
    }
    // Aliases of deleted commands are gone too ("publish" was a plot.batch alias, "lo" a layout alias).
    for alias in ["publish", "batchplot", "lo", "mv", "ms", "ps", "publisher", "tilemode"] {
        assert!(find_command(alias).is_none(), "alias `{alias}` should be gone");
    }
}

#[test]
fn plot_is_model_space_only() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    // A layout other than Model is refused with a pointer to MynaOps; "Model" is accepted as a parameter.
    let e = s.execute("plot", &json!({"layout": "Layout1"})).unwrap_err().to_string();
    assert!(e.contains("MynaOps"), "{e}");
    let e = s.execute("exportpdf", &json!({"layout": "Layout1"})).unwrap_err().to_string();
    assert!(e.contains("MynaOps"), "{e}");
    // A window is validated.
    assert!(s.execute("plot.window", &json!({"window": [1, 1, 1, 5]})).is_err());
    assert_eq!(s.execute("plot.window", &json!({"window": [5, 5, 0, 0]})).unwrap()["window"], json!([0.0, 0.0, 5.0, 5.0]));
}

#[test]
fn plotl_and_plotp_pick_a_model_window_and_choose_the_orientation_without_making_a_layout() {
    for (id, landscape) in [("plotl", true), ("plotp", false)] {
        let mut s = Session::new();
        s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
        let layouts_before = s.doc().unwrap().layouts.len();
        s.start(id).unwrap();
        s.input(aad_engine::Input::Point(aad_geom::Vec2::new(0.0, 0.0))).unwrap();
        s.input(aad_engine::Input::Point(aad_geom::Vec2::new(8.0, 6.0))).unwrap();
        let req = s.take_plot_request().expect("a plot request");
        assert_eq!(req.window, Some([0.0, 0.0, 8.0, 6.0]));
        assert_eq!(req.landscape, Some(landscape));
        assert_eq!(s.doc().unwrap().layouts.len(), layouts_before, "no layout is created");
    }
}

#[test]
fn sheet_scheme_is_kept_for_the_project_configuration_and_keeps_stored_sheets() {
    let mut s = Session::new();
    let r = s.execute("sheet.scheme", &json!({})).unwrap();
    assert_eq!(r["scheme"]["pattern"], "{D}-{N}");
    let r = s.execute("sheet.scheme", &json!({"pattern": "{D}{N}", "width": 4, "step": 10})).unwrap();
    assert_eq!(r["scheme"]["width"], 4);
    // A drawing from an older MynaCAD stores sheets and issues under `sheetidx1`; changing the scheme must not drop them.
    let stored = json!({
        "v": 1, "seed": 2, "issueSeed": 1,
        "scheme": {"pattern": "{D}-{N}", "width": 3, "step": 1, "disciplines": [{"code": "A", "name": "Architectural", "start": 101}]},
        "sheets": [{"id": 1, "number": "A-101", "discipline": "A", "title": "Ground floor plan", "layout": "Layout1"}],
        "issues": [], "tables": {},
    });
    s.doc_mut().unwrap().project.insert("sheetidx1".into(), stored.to_string());
    s.execute("sheet.scheme", &json!({"width": 5})).unwrap();
    let text = s.doc().unwrap().project.get("sheetidx1").cloned().unwrap();
    let back: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(back["sheets"][0]["number"], "A-101");
    assert_eq!(back["sheets"][0]["title"], "Ground floor plan");
    assert_eq!(back["scheme"]["width"], 5);
    // Hostile input is an error, never a panic.
    for bad in [
        json!({"width": 99}),
        json!({"step": 0}),
        json!({"pattern": "no number"}),
        json!({"disciplines": []}),
        json!({"disciplines": [{"code": "!!", "start": 1}]}),
    ] {
        assert!(s.execute("sheet.scheme", &bad).is_err(), "{bad}");
    }
}
