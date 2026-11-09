//! The rule table: every threshold the classifier uses, as editable parameters.
//!
//! Lengths are millimetres. Rules are stored as rows (`geo_rule`): one per section, `params_json`
//! holds the section. Editing a section bumps `version`, which marks the features stale.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::units::UnitSystem;
use crate::{Error, Result};

/// One wall type. Walls are always drawn as two lines, so a type is just its overall thickness.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WallType {
    /// `half`, `full` or `cavity`.
    pub id: String,
    pub name: String,
    /// Overall thickness in a metric drawing.
    pub metric_mm: f64,
    /// Overall thickness in an imperial drawing.
    pub imperial_mm: f64,
}

impl WallType {
    pub fn thickness(&self, system: UnitSystem) -> f64 {
        match system {
            UnitSystem::Metric => self.metric_mm,
            UnitSystem::Imperial => self.imperial_mm,
        }
    }
}

/// How two parallel segments become a wall pair.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PairRule {
    /// Allowed difference of thickness from the nominal.
    pub tolerance_mm: f64,
    /// Largest angle between the two faces.
    pub angle_tol_deg: f64,
    /// Smallest shared length of the two faces (walls are drawn in pieces; pieces chain into a run).
    pub min_overlap_mm: f64,
    /// Shortest wall run after the pieces are chained. Shorter runs are columns, returns or noise.
    pub min_run_mm: f64,
    /// Shared length over the shorter face.
    pub min_overlap_ratio: f64,
    /// Segments shorter than this are ignored.
    pub min_segment_mm: f64,
    /// Collinear pieces closer than this join into one wall run.
    pub join_gap_mm: f64,
    /// Distance within which two wall ends meet (corner or T).
    pub node_tol_mm: f64,
}

impl Default for PairRule {
    fn default() -> Self {
        PairRule {
            tolerance_mm: 8.0,
            angle_tol_deg: 1.0,
            min_overlap_mm: 100.0,
            min_run_mm: 600.0,
            min_overlap_ratio: 0.6,
            min_segment_mm: 100.0,
            join_gap_mm: 25.0,
            node_tol_mm: 30.0,
        }
    }
}

/// The cavity wall composition. The overall default of 300 mm (115 + 70 + 115) is an assumption
/// about local practice, not a code value; change `cavity.metric_mm` to suit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CavityParts {
    pub leaf_mm: f64,
    pub cavity_mm: f64,
}

impl Default for CavityParts {
    fn default() -> Self {
        CavityParts { leaf_mm: 115.0, cavity_mm: 70.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoorRule {
    pub radius_min_mm: f64,
    pub radius_max_mm: f64,
    /// A swing is a quarter circle: sweep within this many degrees of 90.
    pub sweep_tol_deg: f64,
    /// How far the hinge may be outside the wall rectangle.
    pub hinge_tol_mm: f64,
    /// Block names (`*` and `?` wildcards, case-insensitive) that are door symbols.
    pub block_patterns: Vec<String>,
}

impl Default for DoorRule {
    fn default() -> Self {
        DoorRule {
            radius_min_mm: 500.0,
            radius_max_mm: 1300.0,
            sweep_tol_deg: 12.0,
            hinge_tol_mm: 25.0,
            block_patterns: vec!["*DOOR*".into(), "DR-*".into()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowRule {
    pub width_min_mm: f64,
    pub width_max_mm: f64,
    /// Fewest and most thin lines across the wall in the opening.
    pub lines_min: usize,
    pub lines_max: usize,
    /// The lines must span at least this share of the wall thickness.
    pub span_ratio: f64,
    pub block_patterns: Vec<String>,
}

impl Default for WindowRule {
    fn default() -> Self {
        WindowRule {
            width_min_mm: 450.0,
            width_max_mm: 3000.0,
            lines_min: 2,
            lines_max: 5,
            span_ratio: 0.5,
            block_patterns: vec!["*WINDOW*".into(), "WIN*".into(), "*GLAZ*".into()],
        }
    }
}

/// Evidence weights added to or taken from a feature's confidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Evidence {
    pub base_wall: f64,
    /// The layer name matches `layer_wall` (only helps when the layer carries a name).
    pub layer_match: f64,
    pub layer_wall: Vec<String>,
    pub layer_door: Vec<String>,
    pub layer_window: Vec<String>,
    /// A run at least this many thicknesses long.
    pub long_run: f64,
    pub long_run_factor: f64,
    /// Both faces share a corner or T with another wall run.
    pub joined: f64,
    /// A ladder of evenly spaced parallel lines (stair treads, hatching) rather than two faces.
    pub ladder_penalty: f64,
    /// A short run.
    pub short_penalty: f64,
    /// The faces are drawn dashed, hidden or centre-line: a beam above or a centre line, not a cut wall.
    pub dashed_penalty: f64,
    /// A run lying wholly inside a thicker wall (window sill lines look like a thin wall).
    pub nested_penalty: f64,
    /// A run whose two faces are the sides of one closed rectangle shorter than `column_max_mm`: a column or pier section.
    pub column_penalty: f64,
    pub column_max_mm: f64,
    /// A face is part of a block (furniture, a fixture): a wall is drawn at the top level far more often.
    pub block_penalty: f64,
    /// Features below this confidence are listed as ambiguous and are not classified.
    pub classify_min: f64,
}

impl Default for Evidence {
    fn default() -> Self {
        Evidence {
            base_wall: 0.55,
            layer_match: 0.2,
            layer_wall: vec!["*WALL*".into()],
            layer_door: vec!["*DOOR*".into()],
            layer_window: vec!["*WIND*".into(), "*WINDOW*".into(), "*GLAZ*".into()],
            long_run: 3.0,
            long_run_factor: 0.1,
            joined: 0.1,
            ladder_penalty: 0.35,
            short_penalty: 0.15,
            dashed_penalty: 0.3,
            nested_penalty: 0.5,
            column_penalty: 0.4,
            column_max_mm: 1200.0,
            block_penalty: 0.0,
            classify_min: 0.5,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    pub version: u32,
    pub wall_types: Vec<WallType>,
    pub cavity_parts: CavityParts,
    pub pair: PairRule,
    pub door: DoorRule,
    pub window: WindowRule,
    pub evidence: Evidence,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            version: 1,
            wall_types: vec![
                WallType { id: "half".into(), name: "Half brick".into(), metric_mm: 115.0, imperial_mm: 114.3 },
                WallType { id: "full".into(), name: "Full brick".into(), metric_mm: 230.0, imperial_mm: 228.6 },
                WallType { id: "cavity".into(), name: "Cavity wall".into(), metric_mm: 300.0, imperial_mm: 304.8 },
            ],
            cavity_parts: CavityParts::default(),
            pair: PairRule::default(),
            door: DoorRule::default(),
            window: WindowRule::default(),
            evidence: Evidence::default(),
        }
    }
}

/// The rule row ids, one per section.
pub const SECTIONS: [&str; 6] = ["wall_types", "cavity_parts", "pair", "door", "window", "evidence"];

impl Rules {
    /// The `[lo, hi]` gap band of each wall type for a drawing system.
    pub fn bands(&self, system: UnitSystem) -> Vec<(f64, f64)> {
        self.wall_types.iter().map(|t| (t.thickness(system) - self.pair.tolerance_mm, t.thickness(system) + self.pair.tolerance_mm)).collect()
    }

    /// One section as JSON (what a `geo_rule` row stores).
    pub fn section(&self, kind: &str) -> Option<Value> {
        let v = serde_json::to_value(self).ok()?;
        v.get(kind).cloned()
    }

    /// Replace (merge into) one section. Unknown kinds and malformed values are errors; the rules are validated after.
    pub fn set_section(&mut self, kind: &str, params: &Value) -> Result<()> {
        if !SECTIONS.contains(&kind) {
            return Err(Error::Invalid(format!("no rule section `{kind}` (one of {})", SECTIONS.join(", "))));
        }
        let mut whole = serde_json::to_value(&*self).map_err(|e| Error::Invalid(e.to_string()))?;
        let cur = whole.get_mut(kind).ok_or_else(|| Error::Invalid(format!("no rule section `{kind}`")))?;
        match (cur.as_object_mut(), params.as_object()) {
            (Some(c), Some(p)) => {
                for (k, v) in p {
                    c.insert(k.clone(), v.clone());
                }
            }
            _ => *cur = params.clone(),
        }
        let next: Rules = serde_json::from_value(whole).map_err(|e| Error::Invalid(format!("rule `{kind}`: {e}")))?;
        next.validate()?;
        let version = self.version + 1;
        *self = Rules { version, ..next };
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let bad = |m: &str| Err(Error::Invalid(m.to_string()));
        let fin = |v: f64| v.is_finite() && v >= 0.0;
        if self.wall_types.is_empty() || self.wall_types.len() > 12 {
            return bad("wall_types needs 1 to 12 entries");
        }
        if self.wall_types.iter().any(|t| !(t.metric_mm > 1.0 && t.metric_mm < 2000.0 && t.imperial_mm > 1.0 && t.imperial_mm < 2000.0)) {
            return bad("wall thickness must be between 1 and 2000 mm");
        }
        let p = &self.pair;
        if !(fin(p.tolerance_mm) && p.tolerance_mm < 100.0) {
            return bad("pair.tolerance_mm must be 0 to 100");
        }
        if !(p.angle_tol_deg.is_finite() && p.angle_tol_deg > 0.0 && p.angle_tol_deg <= 10.0) {
            return bad("pair.angle_tol_deg must be above 0 and at most 10");
        }
        if !(fin(p.min_overlap_mm) && fin(p.min_run_mm) && fin(p.min_segment_mm) && fin(p.join_gap_mm) && fin(p.node_tol_mm)) {
            return bad("pair lengths must be finite and not negative");
        }
        if !(p.min_overlap_ratio.is_finite() && (0.0..=1.0).contains(&p.min_overlap_ratio)) {
            return bad("pair.min_overlap_ratio must be 0 to 1");
        }
        // Bands must not overlap, or a gap would belong to two types.
        for system in [UnitSystem::Metric, UnitSystem::Imperial] {
            let mut b = self.bands(system);
            b.sort_by(|a, c| a.0.total_cmp(&c.0));
            if b.windows(2).any(|w| w[0].1 >= w[1].0) {
                return bad("wall type thicknesses overlap within the tolerance; lower the tolerance or change a thickness");
            }
        }
        let (d, w) = (&self.door, &self.window);
        if !(fin(d.radius_min_mm) && fin(d.radius_max_mm) && d.radius_min_mm <= d.radius_max_mm) {
            return bad("door radius range is invalid");
        }
        if !(fin(w.width_min_mm) && fin(w.width_max_mm) && w.width_min_mm <= w.width_max_mm && w.lines_min >= 1 && w.lines_min <= w.lines_max) {
            return bad("window range is invalid");
        }
        if self.evidence.classify_min.is_nan() {
            return bad("evidence.classify_min is not a number");
        }
        Ok(())
    }
}

/// Case-insensitive wildcard match (`*` any run, `?` one character).
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_ascii_uppercase().chars().collect();
    let t: Vec<char> = text.to_ascii_uppercase().chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ti < t.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some(pi);
                mark = ti;
                pi += 1;
            }
            Some(c) if *c == '?' || Some(c) == t.get(ti) => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some(s) => {
                    pi = s + 1;
                    mark += 1;
                    ti = mark;
                }
                None => return false,
            },
        }
    }
    while p.get(pi) == Some(&'*') {
        pi += 1;
    }
    pi == p.len()
}

pub fn glob_any(patterns: &[String], text: &str) -> bool {
    patterns.iter().any(|p| glob(p, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_validate_and_bands_do_not_overlap() {
        let r = Rules::default();
        r.validate().unwrap();
        assert_eq!(r.bands(UnitSystem::Metric)[1], (222.0, 238.0));
        assert!((r.bands(UnitSystem::Imperial)[1].0 - 220.6).abs() < 1e-9);
    }

    #[test]
    fn editing_bumps_the_version_and_rejects_nonsense() {
        let mut r = Rules::default();
        r.set_section("pair", &json!({"tolerance_mm": 5.0})).unwrap();
        assert_eq!(r.version, 2);
        assert_eq!(r.pair.tolerance_mm, 5.0);
        assert!(r.set_section("pair", &json!({"tolerance_mm": -1.0})).is_err());
        assert!(r.set_section("nope", &json!({})).is_err());
        assert!(r.set_section("pair", &json!({"tolerance_mm": 90.0})).is_err(), "bands would overlap");
        assert_eq!(r.version, 2, "a rejected edit changes nothing");
        r.set_section("wall_types", &json!([{"id":"full","name":"Full","metric_mm":225.0,"imperial_mm":228.6}])).unwrap();
        assert_eq!(r.wall_types.len(), 1);
    }

    #[test]
    fn glob_matches() {
        assert!(glob("*WALL*", "a-wall-new") && glob("AADOPEN-*", "aadopen-1F") && glob("D?", "d1"));
        assert!(!glob("D?", "d12") && !glob("*DOOR*", "A-WALL") && glob("*", ""));
    }
}
