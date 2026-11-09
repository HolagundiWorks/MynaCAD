//! Where a drawing came from: Tren stamps every drawing it saves, and a drawing without the stamp
//! (and with layers that are not the app's standard set) is foreign, so the layer mapper runs
//! before anything is classified.

use aad_doc::Drawing;
use serde::Serialize;
use serde_json::{Value, json};

/// Project key (drawing project data and `.osp` `meta`) holding the Tren origin stamp.
pub const ORIGIN_KEY: &str = "trenOrigin";

#[derive(Clone, Debug, Serialize)]
pub struct OriginReport {
    /// The drawing carries a Tren stamp.
    pub stamped: bool,
    /// No stamp and layer names that are not the standard set: run the layer mapper first.
    pub foreign: bool,
    pub reasons: Vec<String>,
    /// Layers carrying objects that are not in the standard set.
    pub nonstandard_layers: Vec<String>,
    pub acad_version: Option<String>,
    pub codepage: Option<String>,
}

/// The stamp Tren writes: application, version and the format of the stamp.
pub fn stamp_value(app_version: &str) -> Value {
    json!({ "app": "Tren", "version": app_version, "stamp": 1 })
}

/// Write the stamp into the drawing's project data.
pub fn stamp(d: &mut Drawing, app_version: &str) {
    d.project.insert(ORIGIN_KEY.into(), stamp_value(app_version).to_string());
}

pub fn is_stamped(d: &Drawing) -> bool {
    d.project
        .get(ORIGIN_KEY)
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| v.get("app").and_then(Value::as_str).map(|a| a == "Tren"))
        .unwrap_or(false)
}

/// Decide the origin. `standard` is the app's fixed layer set (case-insensitive); layer `0` and
/// `Defpoints` are always allowed.
pub fn detect(d: &Drawing, standard: &[&str]) -> OriginReport {
    let stamped = is_stamped(d);
    let mut used: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for e in d.model.iter() {
        used.insert(e.common.layer.as_str());
    }
    let ok = |n: &str| n == "0" || n.eq_ignore_ascii_case("Defpoints") || standard.iter().any(|s| s.eq_ignore_ascii_case(n));
    let nonstandard: Vec<String> = used.iter().filter(|n| !ok(n)).map(|n| n.to_string()).collect();
    let mut reasons = Vec::new();
    if !stamped {
        reasons.push("no Tren stamp in the drawing".to_string());
    }
    if !nonstandard.is_empty() {
        reasons.push(format!("{} layer(s) with objects are not in the standard set", nonstandard.len()));
    }
    let only_zero = !used.is_empty() && used.iter().all(|n| *n == "0");
    if only_zero {
        reasons.push("every object is on layer 0".to_string());
    }
    let acad = d.header.get("ACADVER").and_then(|v| v.as_str()).map(str::to_string);
    let codepage = d.header.get("DWGCODEPAGE").and_then(|v| v.as_str()).map(str::to_string);
    let foreign = !stamped && (!nonstandard.is_empty() || only_zero);
    OriginReport { stamped, foreign, reasons, nonstandard_layers: nonstandard, acad_version: acad, codepage }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamped_drawing_is_never_foreign_and_an_unstamped_one_with_odd_layers_is() {
        let mut d = Drawing::new_metric();
        let mut e = aad_doc::Entity::new(aad_doc::Handle(1), aad_doc::EntityKind::Point(aad_doc::Point { p: aad_geom::Vec3::ZERO, angle: 0.0 }));
        e.common.layer = "WALL-EXT".into();
        d.model.push(e);
        let r = detect(&d, &["A-WALL"]);
        assert!(r.foreign && !r.stamped && r.nonstandard_layers == vec!["WALL-EXT".to_string()]);
        stamp(&mut d, "0.6.0");
        let r = detect(&d, &["A-WALL"]);
        assert!(r.stamped && !r.foreign);
    }

    #[test]
    fn all_on_layer_zero_without_a_stamp_is_foreign() {
        let mut d = Drawing::new_metric();
        d.model.push(aad_doc::Entity::new(aad_doc::Handle(1), aad_doc::EntityKind::Point(aad_doc::Point { p: aad_geom::Vec3::ZERO, angle: 0.0 })));
        assert!(detect(&d, &[]).foreign);
        assert!(!detect(&Drawing::new_metric(), &[]).foreign, "an empty drawing has nothing to map");
    }
}
