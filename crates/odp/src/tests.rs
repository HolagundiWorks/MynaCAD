use serde_json::json;

use super::*;

fn temp() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("house.osp");
    (dir, path)
}

fn row(slot: u32, kind: u8, n: u32) -> TakeoffRow {
    TakeoffRow { slot, kind, n, json: json!({ "ref": format!("W{n}"), "length_mm": 9000 }) }
}

#[test]
fn a_new_file_has_its_facts_and_revision_zero() {
    let (_d, path) = temp();
    let odp = Odp::create(&path, "House").unwrap();
    let m = odp.meta().unwrap();
    assert_eq!((m.name.as_str(), m.revision), ("House", 0));
    assert_eq!(m.project_id.len(), 32);
    assert!(m.created.ends_with('Z'));
    assert!(Odp::create(&path, "again").is_err(), "an existing file is not overwritten");
}

#[test]
fn a_drawing_is_kept_byte_for_byte_with_its_hash() {
    let (_d, path) = temp();
    let mut odp = Odp::create(&path, "House").unwrap();
    let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let mut e = odp.edit().unwrap();
    e.put_drawing("house.dwg", ".DWG", &bytes).unwrap();
    assert_eq!(e.commit().unwrap(), 1);
    drop(odp);
    let odp = Odp::open(&path).unwrap();
    let d = odp.drawing("house.dwg").unwrap().unwrap();
    assert_eq!((d.ext.as_str(), d.bytes.len()), ("dwg", 200_000));
    assert_eq!(d.bytes, bytes);
    assert_eq!(d.sha256, sha256_hex(&bytes));
    assert_eq!(odp.drawing_names().unwrap(), ["house.dwg"]);
}

#[test]
fn each_commit_is_one_revision_and_an_empty_one_is_none() {
    let (_d, path) = temp();
    let mut odp = Odp::create(&path, "House").unwrap();
    let mut e = odp.edit().unwrap();
    e.put_takeoff_row(&row(7, 3, 1)).unwrap();
    e.put_takeoff_row(&row(7, 3, 2)).unwrap();
    assert_eq!(e.commit().unwrap(), 1);
    assert_eq!(odp.edit().unwrap().commit().unwrap(), 1, "nothing changed: the revision stays");
    let mut e = odp.edit().unwrap();
    e.put_project_doc("{\"project\":{}}").unwrap();
    assert_eq!(e.commit().unwrap(), 2);
    assert_eq!(odp.revision().unwrap(), 2);
}

#[test]
fn an_edit_dropped_without_commit_changes_nothing() {
    let (_d, path) = temp();
    let mut odp = Odp::create(&path, "House").unwrap();
    {
        let mut e = odp.edit().unwrap();
        e.put_takeoff_row(&row(1, 3, 1)).unwrap();
    }
    assert_eq!(odp.revision().unwrap(), 0);
    assert!(odp.takeoff_rows(1).unwrap().is_empty());
}

#[test]
fn take_off_rows_replace_and_retain_and_the_change_log_says_what_happened() {
    let (_d, path) = temp();
    let mut odp = Odp::create(&path, "House").unwrap();
    let mut e = odp.edit().unwrap();
    e.put_takeoff_header(&TakeoffHeader { slot: 7, state: "complete".into(), json: json!({ "state": "complete" }) }).unwrap();
    for n in 1..=3 {
        e.put_takeoff_row(&row(7, 3, n)).unwrap();
    }
    e.commit().unwrap();
    let mut e = odp.edit().unwrap();
    assert_eq!(e.retain_takeoff_rows(7, &[(3, 1), (3, 3)]).unwrap(), 1);
    e.commit().unwrap();
    let rows = odp.takeoff_rows(7).unwrap();
    assert_eq!(rows.iter().map(|r| r.n).collect::<Vec<_>>(), [1, 3]);
    let log = odp.changes_since(1).unwrap();
    assert_eq!(log, [(2, "takeoff_row".to_string(), "7/3/2".to_string(), "delete".to_string())]);
    assert_eq!(odp.takeoff_headers().unwrap().len(), 1);
}

#[test]
fn project_doc_assets_and_markups_round_trip() {
    let (_d, path) = temp();
    let mut odp = Odp::create(&path, "House").unwrap();
    let mut e = odp.edit().unwrap();
    e.put_project_doc("{\"project\":{\"name\":\"House \u{2014} \u{c4}\"}}").unwrap();
    e.put_asset(&Asset {
        id: "a1".into(),
        slot: Some(7),
        name: "annotated.pdf".into(),
        mime: "application/pdf".into(),
        sha256: String::new(),
        bytes: vec![1, 2, 3],
    })
    .unwrap();
    e.put_markup(&Markup {
        id: "m1".into(),
        drawing: "house.dwg".into(),
        author: "tablet".into(),
        created: "2026-10-30T07:16:41Z".into(),
        kind: "cloud".into(),
        json: json!({ "pts": [[0, 0], [10, 10]] }),
        text: "check this beam".into(),
        resolved: false,
    })
    .unwrap();
    e.commit().unwrap();
    assert!(odp.project_doc().unwrap().unwrap().contains("House \u{2014} \u{c4}"));
    let a = odp.asset("a1").unwrap().unwrap();
    assert_eq!((a.bytes, a.sha256), (vec![1, 2, 3], sha256_hex(&[1, 2, 3])));
    let m = odp.markups("house.dwg").unwrap();
    assert_eq!((m.len(), m[0].text.as_str(), m[0].resolved), (1, "check this beam", false));
    assert!(Odp::create(&path.with_extension("x"), "x").unwrap().project_doc().unwrap().is_none());
}

#[test]
fn a_project_that_is_not_json_is_refused() {
    let (_d, path) = temp();
    let mut odp = Odp::create(&path, "House").unwrap();
    let mut e = odp.edit().unwrap();
    assert!(e.put_project_doc("not json").is_err());
}

#[test]
fn files_that_are_not_projects_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("a.osp");
    std::fs::write(&text, b"this is not sqlite").unwrap();
    assert!(matches!(Odp::open(&text), Err(Error::NotAProject(_))));
    let other = dir.path().join("b.db");
    rusqlite::Connection::open(&other)
        .unwrap()
        .execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT); INSERT INTO meta VALUES ('format','something-else');")
        .unwrap();
    assert!(matches!(Odp::open(&other), Err(Error::NotAProject(_))));
    assert!(Odp::open(&dir.path().join("missing.osp")).is_err());
}

#[test]
fn a_newer_version_is_refused_not_misread() {
    let (_d, path) = temp();
    drop(Odp::create(&path, "House").unwrap());
    rusqlite::Connection::open(&path).unwrap().execute("UPDATE meta SET value = '99' WHERE key = 'version'", []).unwrap();
    assert!(matches!(Odp::open(&path), Err(Error::TooNew(99))));
}

#[test]
fn two_handles_on_one_file_see_each_others_commits() {
    let (_d, path) = temp();
    let mut writer = Odp::create(&path, "House").unwrap();
    let reader = Odp::open(&path).unwrap();
    assert_eq!(reader.revision().unwrap(), 0);
    let mut e = writer.edit().unwrap();
    e.put_takeoff_row(&row(1, 3, 1)).unwrap();
    e.commit().unwrap();
    assert_eq!(reader.revision().unwrap(), 1);
    assert_eq!(reader.takeoff_rows(1).unwrap().len(), 1);
}

#[test]
fn take_off_records_round_trip_and_only_what_changed_is_written() {
    let (_d, path) = temp();
    let mut odp = Odp::create(&path, "House").unwrap();
    let id = |kind: u64, slot: u64, n: u64| (kind << 56) | (slot << 32) | n;
    let pdf: Vec<u8> = (0..(PDF_CHUNK * 2 + 10)).map(|i| (i % 251) as u8).collect();
    let mut records = vec![
        (id(1, 5, 0), br#"{"format":"hcw-takeoff-shilpi","state":"complete"}"#.to_vec()),
        (id(3, 5, 1), br#"{"ref":"W1"}"#.to_vec()),
        (id(3, 5, 2), br#"{"ref":"W2"}"#.to_vec()),
    ];
    for (n, c) in pdf.chunks(PDF_CHUNK).enumerate() {
        records.push((id(9, 5, n as u64), c.to_vec()));
    }
    assert_eq!(odp.replace_takeoff(&records).unwrap(), 1);
    let mut back = odp.takeoff_as_records().unwrap();
    back.sort();
    let mut want = records.clone();
    want.sort();
    assert_eq!(back, want, "what was written is what comes back, PDF chunks included");

    // the same again: no change, no new revision
    assert_eq!(odp.replace_takeoff(&records).unwrap(), 1);
    // one row edited, one removed, the PDF gone
    let next = vec![
        (id(1, 5, 0), br#"{"format":"hcw-takeoff-shilpi","state":"complete"}"#.to_vec()),
        (id(3, 5, 1), br#"{"ref":"W1","length_mm":5}"#.to_vec()),
    ];
    assert_eq!(odp.replace_takeoff(&next).unwrap(), 2);
    let log: Vec<_> = odp.changes_since(1).unwrap().into_iter().map(|(_, t, k, o)| format!("{t} {k} {o}")).collect();
    assert_eq!(log, ["takeoff_row 5/3/1 put", "takeoff_row 5/3/2 delete", "asset annotated-5.pdf delete"]);
    assert!(odp.asset("annotated-5.pdf").unwrap().is_none());
    // a drawing's header removed takes its rows with it
    assert_eq!(odp.replace_takeoff(&[]).unwrap(), 3);
    assert!(odp.takeoff_headers().unwrap().is_empty() && odp.takeoff_rows(5).unwrap().is_empty());
}
