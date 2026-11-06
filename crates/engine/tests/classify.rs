//! Building-element classification and the open Indian spec index.

use aad_engine::Session;
use serde_json::json;

#[test]
fn spec_index_lists_and_searches() {
    let mut s = Session::new();
    let list = s.execute("spec.list", &json!({})).unwrap();
    assert_eq!(list["format"], "osas-indian-spec-index");
    assert!(list["entries"].as_array().unwrap().len() >= 10, "{list}");
    let walls = s.execute("spec.list", &json!({ "applies": "wall" })).unwrap();
    assert!(walls["entries"].as_array().unwrap().iter().all(|e| e["applies"].as_array().unwrap().iter().any(|a| a == "wall")));
    let hit = s.execute("spec.search", &json!({ "q": "456" })).unwrap();
    assert!(hit["hits"].as_array().unwrap().iter().any(|h| h["id"] == "IS:456"), "{hit}");
    let one = s.execute("spec.get", &json!({ "id": "IS:456" })).unwrap();
    assert_eq!(one["code"], "456");
    assert!(s.execute("spec.get", &json!({ "id": "no-such" })).is_err());
}

#[test]
fn classify_set_get_clear_on_entity_and_block() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let h = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    let r = s
        .execute(
            "classify.set",
            &json!({
                "handles": [h],
                "kind": "wall",
                "specIds": ["IS:1905", "IFC:IfcWall"],
            }),
        )
        .unwrap();
    assert_eq!(r["classified"], 1);
    let g = s.execute("classify.get", &json!({ "handles": [h] })).unwrap();
    let b = &g["items"][0]["building"];
    assert_eq!(b["kind"], "wall");
    assert_eq!(b["family"], "structural");
    assert_eq!(b["ifcType"], "IfcWall");
    assert!(b["specRefs"].as_array().unwrap().len() >= 2, "{b}");

    // Persist into project meta the way a DXF save would.
    let meta = s.doc().unwrap().project.get("buildingMeta").expect("buildingMeta");
    assert!(meta.contains(&h) && meta.contains("wall"), "{meta}");

    s.execute("block", &json!({ "name": "DoorA", "handles": [h] })).unwrap();
    let r = s
        .execute(
            "classify.set",
            &json!({
                "block": "DoorA",
                "kind": "door",
                "specIds": ["IS:1003"],
            }),
        )
        .unwrap();
    assert_eq!(r["classified"], 1);
    let g = s.execute("classify.get", &json!({ "block": "DoorA" })).unwrap();
    assert_eq!(g["block"]["building"]["kind"], "door");
    assert_eq!(g["block"]["building"]["ifcType"], "IfcDoor");
    let bmeta = s.doc().unwrap().project.get("buildingBlockMeta").expect("buildingBlockMeta");
    assert!(bmeta.contains("DoorA") && bmeta.contains("door"), "{bmeta}");

    let c = s.execute("classify.clear", &json!({ "block": "DoorA" })).unwrap();
    assert_eq!(c["cleared"], 1);
    let g = s.execute("classify.get", &json!({ "block": "DoorA" })).unwrap();
    assert!(g["block"]["building"].is_null());
}

#[test]
fn properties_set_stamps_building_kind() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [4, 0]] })).unwrap();
    let h = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    s.execute("properties.set", &json!({ "handles": [h], "buildingKind": "column", "ifcType": "IfcColumn" })).unwrap();
    let g = s.execute("classify.get", &json!({ "handles": [h] })).unwrap();
    assert_eq!(g["items"][0]["building"]["kind"], "column");
    assert_eq!(g["items"][0]["building"]["ifcType"], "IfcColumn");
}

#[test]
fn classify_unknown_spec_id_warns() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap();
    let h = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    let r = s.execute("classify.set", &json!({ "handles": [h], "kind": "column", "specIds": ["NOPE:1"] })).unwrap();
    assert_eq!(r["classified"], 1);
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("unknown spec id")), "{r}");
}
