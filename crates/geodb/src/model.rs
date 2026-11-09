//! The in-memory tables. Everything is millimetres; `x1..y2` are the two end points of a segment.
//!
//! Rows are grouped by the top-level entity they came from (`Extracted`), so an edit replaces one
//! group. A block's expanded geometry belongs to the insert that placed it: the `handle` of every
//! row is the top-level entity, which is what can be moved to a layer.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Quantised twin of a coordinate: 0.1 mm as an integer, for exact joins and hashes.
pub fn q(v: f64) -> i64 {
    if v.is_finite() { (v * 10.0).round().clamp(-9.0e15, 9.0e15) as i64 } else { 0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrimKind {
    Line,
    Arc,
    Circle,
    Poly,
    Hatch,
    Point,
    Insert,
    Text,
}

impl PrimKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PrimKind::Line => "line",
            PrimKind::Arc => "arc",
            PrimKind::Circle => "circle",
            PrimKind::Poly => "poly",
            PrimKind::Hatch => "hatch",
            PrimKind::Point => "point",
            PrimKind::Insert => "insert",
            PrimKind::Text => "text",
        }
    }
}

/// One source entity (or one entity inside an expanded block).
#[derive(Clone, Debug)]
pub struct Prim {
    pub id: u64,
    /// The top-level entity this came from (the insert, for block contents).
    pub handle: u64,
    pub kind: PrimKind,
    pub layer: Arc<str>,
    /// ACI colour number, or -1 for by-layer / true colour.
    pub color: i32,
    pub linetype: Arc<str>,
    pub lineweight: i32,
    pub closed: bool,
    /// Nested block names joined by `/`, empty for top-level geometry.
    pub block_path: Arc<str>,
    pub bbox: [f64; 4],
}

impl Prim {
    pub fn in_block(&self) -> bool {
        !self.block_path.is_empty()
    }
}

/// Every straight piece as a vector.
#[derive(Clone, Debug)]
pub struct Seg {
    pub id: u64,
    pub prim: u64,
    pub seq: u32,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub len: f64,
    /// Direction folded to 0..180 degrees.
    pub angle: f64,
    pub nx: f64,
    pub ny: f64,
}

impl Seg {
    pub fn new(id: u64, prim: u64, seq: u32, a: [f64; 2], b: [f64; 2]) -> Seg {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy);
        let mut ang = dy.atan2(dx).to_degrees();
        if ang < 0.0 {
            ang += 180.0;
        }
        if ang >= 180.0 {
            ang -= 180.0;
        }
        let (ux, uy) = if len > 0.0 { (dx / len, dy / len) } else { (1.0, 0.0) };
        // The unit normal of the folded direction (rotated +90 degrees).
        let (fx, fy) = if ang.to_radians().cos() * ux + ang.to_radians().sin() * uy >= 0.0 { (ux, uy) } else { (-ux, -uy) };
        Seg { id, prim, seq, x1: a[0], y1: a[1], x2: b[0], y2: b[1], len, angle: ang, nx: -fy, ny: fx }
    }
    pub fn bbox(&self) -> [f64; 4] {
        [self.x1.min(self.x2), self.y1.min(self.y2), self.x1.max(self.x2), self.y1.max(self.y2)]
    }
}

/// An arc or circle (a door swing, a round column, a bend). Angles in degrees, counter-clockwise from `a0` to `a1`.
#[derive(Clone, Debug)]
pub struct ArcRow {
    pub id: u64,
    pub prim: u64,
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
    pub a0: f64,
    pub a1: f64,
}

impl ArcRow {
    pub fn sweep(&self) -> f64 {
        let s = (self.a1 - self.a0).rem_euclid(360.0);
        if s == 0.0 && (self.a1 - self.a0).abs() > 1.0 { 360.0 } else { s }
    }
    pub fn point_at(&self, deg: f64) -> [f64; 2] {
        let r = deg.to_radians();
        [self.cx + self.r * r.cos(), self.cy + self.r * r.sin()]
    }
}

/// A block as placed.
#[derive(Clone, Debug)]
pub struct InsertRow {
    pub id: u64,
    pub prim: u64,
    pub block: Arc<str>,
    pub x: f64,
    pub y: f64,
    pub rot: f64,
    pub sx: f64,
    pub sy: f64,
    /// Footprint of the block's geometry as placed.
    pub width: f64,
    pub depth: f64,
    /// Hash of the block's own geometry (name independent) for matching symbols.
    pub shape_hash: u64,
    /// Attributes as a JSON object (`OKIND` marks doors and windows placed by `wall.door`).
    pub attrs: BTreeMap<String, String>,
    /// The insert is inside another block.
    pub nested: bool,
}

#[derive(Clone, Debug)]
pub struct TextRow {
    pub id: u64,
    pub prim: u64,
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub height: f64,
    pub rot: f64,
}

#[derive(Clone, Debug)]
pub struct HatchRow {
    pub id: u64,
    pub prim: u64,
    pub pattern: String,
    pub solid: bool,
    pub area: f64,
    pub loops: Vec<Vec<[f64; 2]>>,
}

/// Everything one top-level entity contributed.
#[derive(Clone, Debug, Default)]
pub struct Extracted {
    pub prims: Vec<Prim>,
    pub segs: Vec<Seg>,
    pub arcs: Vec<ArcRow>,
    pub inserts: Vec<InsertRow>,
    pub texts: Vec<TextRow>,
    pub hatches: Vec<HatchRow>,
}

impl Extracted {
    pub fn bbox(&self) -> Option<[f64; 4]> {
        let mut it = self.prims.iter().map(|p| p.bbox).filter(|b| b[0] <= b[2]);
        let first = it.next()?;
        Some(it.fold(first, |a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]))
    }
}

/// A feature the rules found.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feature {
    pub id: u64,
    /// `wall`, `door` or `window`.
    pub kind: String,
    /// Wall: `half`, `full`, `cavity`. Door: `swing` or `block`. Window: `lines` or `block`.
    pub subkind: String,
    pub confidence: f64,
    /// Rule section that produced it, and its version.
    pub rule: String,
    pub rules_version: u32,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    /// Opening width, or wall thickness for a wall.
    pub width: f64,
    pub depth: f64,
    pub length: f64,
    pub props: serde_json::Value,
    pub members: Vec<Member>,
    /// Hash of the geometry; the key a review decision attaches to.
    pub key: String,
}

impl Feature {
    pub fn bbox(&self) -> [f64; 4] {
        let r = self.depth.max(self.width) / 2.0;
        [self.x1.min(self.x2) - r, self.y1.min(self.y2) - r, self.x1.max(self.x2) + r, self.y1.max(self.y2) + r]
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub prim: u64,
    pub handle: u64,
    /// `face_a`, `face_b`, `swing`, `leaf`, `frame`, `block`, `sill`.
    pub role: String,
}

/// String interner so a layer name is stored once however many rows use it.
#[derive(Default)]
pub struct Interner {
    map: HashMap<String, Arc<str>>,
}

impl Interner {
    pub fn get(&mut self, s: &str) -> Arc<str> {
        if let Some(a) = self.map.get(s) {
            return a.clone();
        }
        let a: Arc<str> = Arc::from(s);
        self.map.insert(s.to_string(), a.clone());
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_is_a_vector_with_a_folded_angle_and_a_unit_normal() {
        let s = Seg::new(1, 1, 0, [0.0, 0.0], [0.0, -10.0]);
        assert!((s.len - 10.0).abs() < 1e-12);
        assert!((s.angle - 90.0).abs() < 1e-9, "direction folds to 0..180");
        assert!((s.nx.hypot(s.ny) - 1.0).abs() < 1e-12);
        assert!(s.nx.abs() > 0.99, "normal of a vertical line is horizontal");
        let h = Seg::new(2, 1, 0, [10.0, 5.0], [0.0, 5.0]);
        assert!(h.angle.abs() < 1e-9 && h.ny.abs() > 0.99);
        assert_eq!(q(1.234), 12);
        assert_eq!(q(f64::NAN), 0);
    }
}
