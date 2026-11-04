//! BUG-0015: an array of zero rows, columns or copies is refused, not reported as "created: 0".

use aad_engine::Session;
use serde_json::json;

#[test]
fn zero_rows_columns_or_copies_are_refused() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap();
    for (cmd, p) in [
        ("arrayrect", json!({ "rows": 0, "cols": 2 })),
        ("arrayrect", json!({ "rows": 2, "cols": 0 })),
        ("arraypolar", json!({ "center": [0, 0], "count": 0 })),
    ] {
        let err = s.execute(cmd, &p).unwrap_err().to_string();
        assert!(err.contains("at least 1"), "{cmd}: {err}");
    }
    assert_eq!(s.doc().unwrap().model.len(), 1);
}

#[test]
fn an_ordinary_array_still_works() {
    let mut s = Session::new();
    let made = s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap();
    s.execute("arrayrect", &json!({ "handles": made["handles"], "rows": 2, "cols": 2 })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 4);
}
