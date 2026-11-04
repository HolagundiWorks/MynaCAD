//! BUG-0012: a point far outside any drawing is refused, not drawn.

use aad_engine::Session;
use serde_json::json;

#[test]
fn a_line_with_a_huge_coordinate_is_refused() {
    let mut s = Session::new();
    for far in [1e308, -1e308, 1e12] {
        let err = s.execute("line", &json!({ "points": [[0, 0], [far, 0]] })).unwrap_err().to_string();
        assert!(err.contains("points"), "{far}: {err}");
    }
    assert_eq!(s.doc().unwrap().model.len(), 0);
}

#[test]
fn ordinary_and_large_but_sane_coordinates_still_draw() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [1e9, -1e9]] })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
}
