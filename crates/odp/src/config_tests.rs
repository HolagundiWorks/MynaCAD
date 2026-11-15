//! Tests of the configuration contract. The JSON fixtures in `tests/fixtures/config` are committed identically in MynaOps and read by
//! `Office.Shared.ProjectConfig` there; this file runs them through the Rust reader.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::*;
use crate::Odp;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("config")
}

fn load_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The key texts of a fixture: an object is the key's JSON text, a string is the raw text.
fn texts(doc: &Value) -> BTreeMap<String, String> {
    doc["keys"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().map_or_else(|| v.to_string(), str::to_string))).collect()
}

fn all_fixtures() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for dir in ["valid", "newer", "hostile"] {
        let mut names: Vec<_> = std::fs::read_dir(fixtures().join(dir)).unwrap().map(|e| e.unwrap().path()).collect();
        names.sort();
        for p in names.into_iter().filter(|p| p.extension().is_some_and(|e| e == "json")) {
            out.push((format!("{dir}/{}", p.file_name().unwrap().to_string_lossy()), load_json(&p)));
        }
    }
    out
}

fn check_fixture(name: &str, doc: &Value) {
    let got = ProjectConfig::from_texts(&texts(doc));
    match doc["expect"].as_str().unwrap() {
        "ok" => {
            let r = got.unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(r.config.levels.as_ref().map_or(0, |l| l.items.len()) as u64, doc["levels"].as_u64().unwrap(), "{name}: levels");
            assert_eq!(r.config.tabs.as_ref().map_or(0, |t| t.items.len()) as u64, doc["tabs"].as_u64().unwrap(), "{name}: tabs");
            assert_eq!(r.config.revision(), doc["rev"].as_u64().unwrap(), "{name}: rev");
            assert_eq!(r.notes.len() as u64, doc["notes"].as_u64().unwrap(), "{name}: notes {:?}", r.notes);
        }
        "too_new" => assert!(matches!(got, Err(ConfigError::TooNew(2))), "{name}: {got:?}"),
        "error" => {
            let key = doc["error_key"].as_str().unwrap();
            match got {
                Err(ConfigError::Invalid { key: k, .. }) | Err(ConfigError::TooLarge { key: k, .. }) => {
                    assert!(k.starts_with(key), "{name}: the error names {k}, expected {key}")
                }
                other => panic!("{name}: expected an error for {key}, got {other:?}"),
            }
        }
        other => panic!("{name}: unknown expectation {other}"),
    }
}

#[test]
fn the_shared_fixtures_read_as_they_say() {
    let all = all_fixtures();
    assert!(all.len() >= 30, "the fixture set is missing files: {}", all.len());
    for (name, doc) in &all {
        check_fixture(name, doc);
    }
}

fn full() -> ProjectConfig {
    ProjectConfig::from_texts(&texts(&load_json(&fixtures().join("valid").join("full.json")))).unwrap().config
}

fn temp_file(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    (dir, path)
}

#[test]
fn the_configuration_round_trips_through_its_json_forms() {
    let c = full();
    let again = ProjectConfig::from_json(&c.to_json()).unwrap();
    assert_eq!(again.config, c);
    assert_eq!(ProjectConfig::from_texts(&c.to_texts().unwrap()).unwrap().config, c);
    c.validate().unwrap();
}

#[test]
fn one_ops_write_is_one_revision_and_a_second_identical_write_is_none() {
    let (_d, path) = temp_file("p.osp");
    let mut odp = Odp::create(&path, "House").unwrap();
    assert!(!read(&odp).unwrap().configured(), "a new file is not configured");
    let mut c = full();
    c.rev = None;
    let w = write(&mut odp, &c, "mynaops").unwrap();
    assert_eq!((w.revision, w.rev), (1, 1), "every key and config.rev in one revision");
    assert!(w.changed.contains(&"rev".to_string()) && w.changed.contains(&"levels".to_string()));
    assert_eq!(odp.revision().unwrap(), 1);
    // The change log names the keys, all under revision 1.
    let changes = odp.changes_since(0).unwrap();
    assert!(changes.iter().all(|(rev, table, _, _)| *rev == 1 && table == "meta"), "{changes:?}");
    assert!(changes.iter().any(|(_, _, key, _)| key == "config.rev"));

    let loaded = read(&odp).unwrap();
    assert!(loaded.configured());
    assert_eq!(loaded.read.config.levels, c.levels);
    assert_eq!(loaded.read.config.revision(), 1);
    assert_eq!(loaded.revision, 1);

    // Nothing differs: no revision, no rev bump.
    let again = write(&mut odp, &c, "mynaops").unwrap();
    assert_eq!((again.revision, again.rev, again.changed.len()), (1, 1, 0));

    // One part changes: one more revision, one rev bump, only that key written.
    let mut c2 = c.clone();
    c2.units = Some(Units { v: 1, drawing_unit: "cm".into(), locked: true });
    let w2 = write(&mut odp, &c2, "mynaops").unwrap();
    assert_eq!((w2.revision, w2.rev), (2, 2));
    assert_eq!(w2.changed, vec!["units".to_string(), "rev".to_string()]);
    assert_eq!(probe(&path).unwrap().read.config.units.unwrap().drawing_unit, "cm");
}

#[test]
fn the_local_report_keys_are_not_revisions() {
    let (_d, path) = temp_file("p.osp");
    let mut odp = Odp::create(&path, "House").unwrap();
    let mut c = full();
    c.rev = None;
    write(&mut odp, &c, "mynaops").unwrap();
    let applied = Applied {
        v: 1,
        rev: 1,
        time: "2026-11-15T10:00:00Z".into(),
        result: "partial".into(),
        has_content: true,
        items: vec![AppliedItem { key: "levels".into(), status: "orphans".into(), detail: "L3 (120 objects)".into() }],
    };
    write_applied(&mut odp, &applied).unwrap();
    assert_eq!(odp.revision().unwrap(), 1, "MynaCAD's report does not raise the revision");
    assert_eq!(read(&odp).unwrap().applied, Some(applied));
    // A configured project gets no proposal.
    assert!(!write_proposal(&mut odp, &Proposal { v: 1, ..Proposal::default() }, false).unwrap());
}

#[test]
fn a_proposal_is_written_once_for_a_legacy_file() {
    let (_d, path) = temp_file("p.osp");
    let mut odp = Odp::create(&path, "House").unwrap();
    let p = Proposal {
        v: 1,
        time: "2026-11-15T10:00:00Z".into(),
        has_content: true,
        drawing_unit: Some("mm".into()),
        levels: vec![ProposalLevel { cid: 3, name: "Ground".into(), elevation_mm: 0.0, height_mm: 3000.0 }],
        layer_sets: vec!["standard".into()],
        ..Proposal::default()
    };
    assert!(write_proposal(&mut odp, &p, false).unwrap());
    assert!(!write_proposal(&mut odp, &p, false).unwrap(), "one time");
    assert!(write_proposal(&mut odp, &p, true).unwrap(), "unless replaced");
    assert_eq!(odp.revision().unwrap(), 0);
    assert_eq!(read(&odp).unwrap().proposal, Some(p));
}

#[test]
fn a_file_with_a_too_new_schema_or_an_oversize_value_is_an_error_not_a_crash() {
    let (_d, path) = temp_file("p.osp");
    let mut odp = Odp::create(&path, "House").unwrap();
    odp.set_local_meta(&[("config.schema", r#"{"v":1,"min_reader":7}"#)]).unwrap();
    assert!(matches!(read(&odp), Err(ConfigError::TooNew(7))));
    assert!(matches!(probe(&path), Err(ConfigError::TooNew(7))));
    let huge = format!(r#"{{"v":1,"drawing_unit":"mm","pad":"{}"}}"#, "x".repeat(MAX_VALUE_BYTES + 10));
    odp.set_local_meta(&[("config.schema", r#"{"v":1,"min_reader":1}"#), ("config.units", huge.as_str())]).unwrap();
    assert!(matches!(read(&odp), Err(ConfigError::TooLarge { .. })));
    assert!(matches!(probe(&path), Err(ConfigError::TooLarge { .. })));
}

#[test]
fn a_damaged_report_key_is_left_out_with_a_note() {
    let (_d, path) = temp_file("p.osp");
    let mut odp = Odp::create(&path, "House").unwrap();
    odp.set_local_meta(&[("config.applied.cad", "{not json"), ("config.proposal", r#"{"v":3}"#)]).unwrap();
    let l = read(&odp).unwrap();
    assert!(l.applied.is_none() && l.proposal.is_none() && l.read.notes.len() == 2, "{:?}", l.read.notes);
}

#[test]
fn the_level_binding_round_trips_and_a_damaged_one_is_empty() {
    let map: BTreeMap<u32, u32> = [(1, 5), (7, 2)].into();
    assert_eq!(levelmap_from_text(&levelmap_to_text(&map)), map);
    for bad in ["", "{", "[]", r#"{"v":2,"map":{"1":1}}"#, r#"{"v":1,"map":{"x":1,"2":-3,"3":99999999999}}"#] {
        assert!(levelmap_from_text(bad).is_empty(), "{bad}");
    }
}

/// A small deterministic generator, so the mutations are the same on every run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn hostile_config_never_panics() {
    // Oversize value, a huge count, deep nesting, absurd numbers: errors, no panic.
    let big = "x".repeat(MAX_VALUE_BYTES + 1);
    let mut parts: BTreeMap<String, String> = BTreeMap::new();
    parts.insert("units".into(), format!(r#"{{"v":1,"drawing_unit":"{big}"}}"#));
    assert!(matches!(ProjectConfig::from_texts(&parts), Err(ConfigError::TooLarge { .. })));

    let items: Vec<Value> = (0..5000).map(|i| json!({"cid": i, "name": format!("L{i}"), "elevation_mm": 0, "height_mm": 3000})).collect();
    parts.clear();
    parts.insert("levels".into(), json!({"v": 1, "next_cid": 6000, "items": items}).to_string());
    assert!(ProjectConfig::from_texts(&parts).is_err(), "a thousand levels");

    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    for name in PARTS {
        parts.clear();
        parts.insert(name.into(), deep.clone());
        assert!(ProjectConfig::from_texts(&parts).is_err());
        parts.insert(name.into(), format!(r#"{{"v":1,"x":{deep}}}"#));
        assert!(ProjectConfig::from_texts(&parts).is_err());
    }
    for n in ["1e999", "-1e999", "18446744073709551616", "-9223372036854775809", "NaN", "Infinity", "null", "true", "\"\"", "[]", "{}"] {
        for name in PARTS {
            parts.clear();
            parts.insert(name.into(), format!(r#"{{"v":1,"next_cid":{n},"cid":{n},"rev":{n},"min_reader":{n},"elevation_mm":{n},"width":{n},"items":[{{"cid":{n},"tid":{n},"name":{n},"elevation_mm":{n},"height_mm":{n}}}]}}"#));
            let _ = ProjectConfig::from_texts(&parts);
        }
    }

    // Single-byte mutations of the full fixture: every result is Ok or Err.
    let base = texts(&load_json(&fixtures().join("valid").join("full.json")));
    let mut rng = Lcg(7);
    for _ in 0..3000 {
        let mut p = base.clone();
        let names: Vec<String> = p.keys().cloned().collect();
        let name = &names[(rng.next() as usize) % names.len()];
        let mut bytes = p[name].clone().into_bytes();
        for _ in 0..1 + rng.next() % 3 {
            let at = (rng.next() as usize) % bytes.len();
            bytes[at] = b" {}[]\":,0123456789-.eE\\ntuf"[(rng.next() as usize) % 27];
        }
        p.insert(name.clone(), String::from_utf8_lossy(&bytes).into_owned());
        let _ = ProjectConfig::from_texts(&p);
        let _ = ProjectConfig::from_json(&serde_json::from_str(&format!("{{\"{name}\":{}}}", p[name])).unwrap_or(Value::Null));
    }
    // The report keys.
    for t in ["", "{", "{\"v\":1}", &deep, "{\"v\":1,\"result\":\"ok\",\"items\":5}"] {
        let _ = parse_applied(t);
        let _ = parse_proposal(t);
    }
}

#[test]
fn hostile_values_in_a_file_never_panic() {
    let (_d, path) = temp_file("p.osp");
    let mut odp = Odp::create(&path, "House").unwrap();
    for (k, v) in
        [("config.schema", "{\"v\":1}"), ("config.levels", "\u{0}\u{1}"), ("config.tabs", "{\"v\":1,\"items\":[null]}"), ("config.rev", "[]")]
    {
        odp.set_local_meta(&[(k, v)]).unwrap();
        let _ = read(&odp);
        let _ = probe(&path);
    }
    // Not a project file at all.
    let junk = path.with_file_name("junk.osp");
    std::fs::write(&junk, b"this is not sqlite").unwrap();
    assert!(probe(&junk).is_err());
    assert!(probe(&path.with_file_name("missing.osp")).is_err());
}

/// The file the Rust writer made from `valid/full.json` is committed (`config-rust.osp`) and read by C#; this one reads the C# writer's file.
#[test]
fn the_files_written_by_either_language_read_the_same() {
    let expect = full();
    for name in ["config-rust.osp", "config-csharp.osp"] {
        let src = fixtures().join(name);
        let (_d, copy) = temp_file(name);
        std::fs::copy(&src, &copy).unwrap_or_else(|e| panic!("{name}: {e}"));
        let l = probe(&copy).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(l.configured(), "{name}");
        assert_eq!(l.read.config.revision(), 1, "{name}");
        assert_eq!(l.revision, 1, "{name}: one revision for one write");
        assert_eq!(l.read.config.levels, expect.levels, "{name}");
        assert_eq!(l.read.config.tabs, expect.tabs, "{name}");
        assert_eq!(l.read.config.sheets, expect.sheets, "{name}");
        assert_eq!(l.read.config.project, expect.project, "{name}");
        assert_eq!(l.read.config.team, expect.team, "{name}");
        assert_eq!(l.read.config.units, expect.units, "{name}");
        assert_eq!(l.read.config.layer_sets, expect.layer_sets, "{name}");
        assert_eq!(l.read.config.naming, expect.naming, "{name}");
        assert_eq!(l.read.config.titleblock, expect.titleblock, "{name}");
        let odp = Odp::open(&copy).unwrap();
        assert_eq!(read(&odp).unwrap().read.config.levels, expect.levels, "{name}");
        if name == "config-rust.osp" {
            assert_eq!(l.applied, Some(fixture_applied()), "MynaCAD's report is read back");
        }
    }
}

#[test]
fn rewriting_the_same_configuration_over_a_file_the_other_language_wrote_changes_nothing() {
    let mut c = full();
    c.rev = None;
    for name in ["config-rust.osp", "config-csharp.osp"] {
        let (_d, copy) = temp_file(name);
        std::fs::copy(fixtures().join(name), &copy).unwrap();
        let mut odp = Odp::open(&copy).unwrap();
        let w = write(&mut odp, &c, "mynaops").unwrap();
        assert!(w.changed.is_empty(), "{name}: {:?}", w.changed);
        assert_eq!((w.revision, w.rev), (1, 1), "{name}");
    }
}

/// Regenerate `config-rust.osp` (`cargo test -p aad-odp regenerate_rust_fixture -- --ignored`).
#[test]
#[ignore]
fn regenerate_rust_fixture() {
    let path = fixtures().join("config-rust.osp");
    let _ = std::fs::remove_file(&path);
    let mut odp = Odp::create(&path, "Config fixture").unwrap();
    let mut c = full();
    c.rev = None;
    write(&mut odp, &c, "mynaops").unwrap();
    // MynaCAD's report, a local key: C# reads it too.
    write_applied(&mut odp, &fixture_applied()).unwrap();
    odp.checkpoint().unwrap();
}

/// The report the committed `config-rust.osp` carries.
fn fixture_applied() -> Applied {
    Applied {
        v: 1,
        rev: 1,
        time: "2026-11-15T10:00:00Z".into(),
        result: "partial".into(),
        has_content: true,
        items: vec![AppliedItem {
            key: "levels".into(),
            status: "orphans".into(),
            detail: "L3 (120 objects) is no longer configured and stays".into(),
        }],
    }
}
