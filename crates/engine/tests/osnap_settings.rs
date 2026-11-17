//! `osnap.settings`: the Osnap settings pane's command (docs/osnap-settings.md).

use aad_engine::Session;
use aad_engine::snap::mode;
use serde_json::{Value, json};

fn known() -> u32 {
    mode::ALL.iter().fold(0, |a, (b, _)| a | b)
}

#[test]
fn reports_defaults_and_every_mode() {
    let mut s = Session::new();
    let r = s.execute("osnap.settings", &json!({})).unwrap();
    assert_eq!(r["osmode"], 4133);
    assert_eq!(r["aperture"], 10.0);
    assert_eq!(r["on"], true);
    assert_eq!(r["hatch"], false);
    let modes = r["modes"].as_array().unwrap();
    assert_eq!(modes.len(), 14);
    let on: Vec<&str> = modes.iter().filter(|m| m["enabled"] == true).filter_map(|m| m["id"].as_str()).collect();
    assert_eq!(on, ["endpoint", "center", "intersection", "extension"]);
}

#[test]
fn alias_and_all_none() {
    let mut s = Session::new();
    // The alias reaches the command from the command line (programmatic calls use the id).
    s.script("OSNAPSETTINGS\n").unwrap();
    assert!(s.running.is_none());
    let r = s.execute("osnap.settings", &json!({ "all": true })).unwrap();
    assert_eq!(r["osmode"].as_u64().unwrap() as u32, known());
    assert_eq!(s.settings.osmode & known(), known());
    let r = s.execute("osnap.settings", &json!({ "none": true })).unwrap();
    assert_eq!(r["osmode"], 0);
    assert!(r["modes"].as_array().unwrap().iter().all(|m| m["enabled"] == false));
}

#[test]
fn modes_osmode_and_master_switch() {
    let mut s = Session::new();
    s.execute("osnap.settings", &json!({ "modes": ["mid", "Quadrant"] })).unwrap();
    assert_eq!(s.settings.osmode, mode::MID | mode::QUA);
    s.execute("osnap.settings", &json!({ "on": false })).unwrap();
    assert_eq!(s.settings.osmode, mode::MID | mode::QUA | mode::OFF);
    // Editing modes keeps the master switch.
    s.execute("osnap.settings", &json!({ "osmode": 1 })).unwrap();
    assert_eq!(s.settings.osmode, mode::END | mode::OFF);
    s.execute("osnap.settings", &json!({ "on": true })).unwrap();
    assert_eq!(s.settings.osmode, mode::END);
    assert_eq!(s.execute("getvar", &json!({ "name": "OSMODE" })).unwrap()["value"], 1);
}

#[test]
fn aperture_hatch_tracking_apply_live() {
    let mut s = Session::new();
    let r = s.execute("osnap.settings", &json!({ "aperture": 18.5, "hatch": true, "tracking": false })).unwrap();
    assert_eq!(r["aperture"], 18.5);
    assert!(s.settings.osnaphatch && !s.settings.otrack);
    s.execute("osnap.settings", &json!({ "aperture": 500 })).unwrap();
    assert_eq!(s.settings.aperture, 50.0);
    s.execute("osnap.settings", &json!({ "aperture": -4 })).unwrap();
    assert_eq!(s.settings.aperture, 1.0);
}

#[test]
fn hostile_params_never_panic() {
    let mut s = Session::new();
    let hostile: Vec<Value> = vec![
        json!(null),
        json!([]),
        json!("x"),
        json!({ "osmode": u64::MAX }),
        json!({ "osmode": -1 }),
        json!({ "osmode": 1e300 }),
        json!({ "osmode": "7" }),
        json!({ "modes": [null, 5, "", "é", "ünï€", "\u{1F600}x", ["end"], {"a": 1}] }),
        json!({ "modes": "end" }),
        json!({ "all": "yes", "none": 3 }),
        json!({ "aperture": f64::MAX }),
        json!({ "aperture": "NaN" }),
        json!({ "aperture": null, "hatch": 1, "tracking": "no", "on": [] }),
    ];
    for p in hostile {
        let r = s.execute("osnap.settings", &p);
        assert!(r.is_ok(), "{p}: {r:?}");
        assert_eq!(s.settings.osmode & !(known() | mode::OFF), 0, "{p}");
        assert!((1.0..=50.0).contains(&s.settings.aperture), "{p}");
    }
}
