//! Entities: the graphical objects of a drawing.

use aad_color::Color;
use aad_geom::{
    Arc as GArc, Bounds2, Circle as GCircle, Ellipse as GEllipse, Line as GLine, Mat3, PolyVertex, Polyline, Segment, Spline as GSpline, Vec2, Vec3,
};
use serde::{Deserialize, Serialize};

/// A database handle (DXF group 5), unique per drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Handle(pub u64);

impl Handle {
    pub fn hex(self) -> String {
        format!("{:X}", self.0)
    }
    pub fn parse_hex(s: &str) -> Option<Handle> {
        u64::from_str_radix(s.trim(), 16).ok().map(Handle)
    }
}

/// Lineweight in hundredths of a millimetre, or a logical value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lineweight {
    #[default]
    ByLayer,
    ByBlock,
    Default,
    /// 0..=211 (hundredths of mm).
    Mm100(u16),
}

impl Lineweight {
    /// The standard lineweight values (mm × 100).
    pub const STANDARD: [u16; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];
    pub fn to_dxf(self) -> i16 {
        match self {
            Lineweight::ByLayer => -1,
            Lineweight::ByBlock => -2,
            Lineweight::Default => -3,
            Lineweight::Mm100(v) => v.min(211) as i16,
        }
    }
    pub fn from_dxf(v: i16) -> Lineweight {
        match v {
            -2 => Lineweight::ByBlock,
            -3 => Lineweight::Default,
            0..=211 => Lineweight::Mm100(v as u16),
            _ => Lineweight::ByLayer,
        }
    }
    pub fn name(self) -> String {
        match self {
            Lineweight::ByLayer => "ByLayer".into(),
            Lineweight::ByBlock => "ByBlock".into(),
            Lineweight::Default => "Default".into(),
            Lineweight::Mm100(v) => format!("{:.2} mm", f64::from(v) / 100.0),
        }
    }
}

/// Transparency: ByLayer, ByBlock, or 0..=90 percent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Transparency {
    #[default]
    ByLayer,
    ByBlock,
    Percent(u8),
}

impl Transparency {
    /// "ByLayer", "ByBlock" or the percentage ("50").
    pub fn name(&self) -> String {
        match self {
            Transparency::ByLayer => "ByLayer".into(),
            Transparency::ByBlock => "ByBlock".into(),
            Transparency::Percent(p) => p.to_string(),
        }
    }
}

/// What a take-off line or outline measures. Stored on the entity; `MEASURE-*` layers are only a
/// display and a legacy source for drawings that have no mark yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TakeoffKind {
    ExtWall,
    IntWall,
    Beam,
    Lintel,
    Linear,
    FloorArea,
    CeilingArea,
    Area,
    Column,
    Count,
    Deduct,
    Slab,
    SlabDeduct,
}

impl TakeoffKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TakeoffKind::ExtWall => "extWall",
            TakeoffKind::IntWall => "intWall",
            TakeoffKind::Beam => "beam",
            TakeoffKind::Lintel => "lintel",
            TakeoffKind::Linear => "linear",
            TakeoffKind::FloorArea => "floorArea",
            TakeoffKind::CeilingArea => "ceilingArea",
            TakeoffKind::Area => "area",
            TakeoffKind::Column => "column",
            TakeoffKind::Count => "count",
            TakeoffKind::Deduct => "deduct",
            TakeoffKind::Slab => "slab",
            TakeoffKind::SlabDeduct => "slabDeduct",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "extwall" | "external" | "fullbrick" | "ext" => Some(TakeoffKind::ExtWall),
            "intwall" | "internal" | "halfbrick" | "int" => Some(TakeoffKind::IntWall),
            "beam" => Some(TakeoffKind::Beam),
            "lintel" => Some(TakeoffKind::Lintel),
            "linear" => Some(TakeoffKind::Linear),
            "floorarea" | "floor" => Some(TakeoffKind::FloorArea),
            "ceilingarea" | "ceiling" => Some(TakeoffKind::CeilingArea),
            "area" => Some(TakeoffKind::Area),
            "column" => Some(TakeoffKind::Column),
            "count" => Some(TakeoffKind::Count),
            "deduct" => Some(TakeoffKind::Deduct),
            "slab" => Some(TakeoffKind::Slab),
            "slabdeduct" | "slab-deduct" => Some(TakeoffKind::SlabDeduct),
            _ => None,
        }
    }

    /// Closed outline (area, column, slab) rather than a wall centre-line.
    pub fn is_outline(self) -> bool {
        matches!(
            self,
            TakeoffKind::FloorArea
                | TakeoffKind::CeilingArea
                | TakeoffKind::Area
                | TakeoffKind::Column
                | TakeoffKind::Count
                | TakeoffKind::Slab
                | TakeoffKind::SlabDeduct
        )
    }

    /// `external` or `internal` for plaster and paint. A linear mark of 200 mm or more is external.
    pub fn wall_type(self, thickness_mm: Option<f64>) -> &'static str {
        match self {
            TakeoffKind::ExtWall => "external",
            TakeoffKind::Linear if thickness_mm.is_some_and(|t| t >= 200.0) => "external",
            _ => "internal",
        }
    }

    /// Nominal thickness when the kind implies one and the user did not give one.
    pub fn default_thickness_mm(self) -> Option<f64> {
        match self {
            TakeoffKind::ExtWall => Some(230.0),
            TakeoffKind::IntWall => Some(115.0),
            _ => None,
        }
    }
}

/// Take-off identity of one entity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeoffMark {
    pub kind: TakeoffKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness_mm: Option<f64>,
    /// The estimation phase this dimension belongs to (a name from the project's list).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// The running-bill phase this dimension belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bill_phase: Option<String>,
}

impl TakeoffMark {
    /// A mark with no phases.
    pub fn new(kind: TakeoffKind, thickness_mm: Option<f64>) -> Self {
        TakeoffMark { kind, thickness_mm, phase: None, bill_phase: None }
    }

    /// The same mark in the given estimation and running-bill phases.
    pub fn in_phases(mut self, phase: Option<String>, bill_phase: Option<String>) -> Self {
        self.phase = phase;
        self.bill_phase = bill_phase;
        self
    }
}

/// Properties every entity has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Common {
    pub layer: String,
    pub color: Color,
    pub linetype: String,
    pub lineweight: Lineweight,
    pub ltscale: f64,
    pub transparency: Transparency,
    pub visible: bool,
    pub thickness: f64,
    pub extrusion: Vec3,
    /// Named plot style: `ByLayer`, `ByBlock` or a style of the layout's named (`.stb`) plot
    /// style table.
    pub plot_style: String,
    /// What this object is for quantity take-off. Absent on ordinary geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takeoff: Option<TakeoffMark>,
    /// Building-element classification (structural / finish / shop). Absent on ordinary geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub building: Option<crate::BuildingElement>,
}

impl Default for Common {
    fn default() -> Self {
        Common {
            layer: "0".into(),
            color: Color::ByLayer,
            linetype: "ByLayer".into(),
            lineweight: Lineweight::ByLayer,
            ltscale: 1.0,
            transparency: Transparency::ByLayer,
            visible: true,
            thickness: 0.0,
            extrusion: Vec3::Z,
            plot_style: "ByLayer".into(),
            takeoff: None,
            building: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub handle: Handle,
    #[serde(default)]
    pub common: Common,
    pub kind: EntityKind,
}

impl Entity {
    pub fn new(handle: Handle, kind: EntityKind) -> Self {
        Entity { handle, common: Common::default(), kind }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub a: Vec3,
    pub b: Vec3,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub p: Vec3,
    #[serde(default)]
    pub angle: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Circle {
    pub center: Vec3,
    pub radius: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Arc {
    pub center: Vec3,
    pub radius: f64,
    /// Radians, CCW.
    pub start: f64,
    pub end: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ellipse {
    pub center: Vec3,
    pub major: Vec3,
    pub ratio: f64,
    pub start: f64,
    pub end: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LwPolyline {
    pub vertices: Vec<PolyVertex>,
    pub closed: bool,
    #[serde(default)]
    pub const_width: f64,
    #[serde(default)]
    pub elevation: f64,
    /// Polyline linetype generation (PLINEGEN).
    #[serde(default)]
    pub plinegen: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
    Aligned,
    Middle,
    Fit,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VAlign {
    #[default]
    Baseline,
    Bottom,
    Middle,
    Top,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    pub insert: Vec3,
    #[serde(default)]
    pub align_pt: Option<Vec3>,
    pub height: f64,
    pub value: String,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default = "one")]
    pub width_factor: f64,
    #[serde(default)]
    pub oblique: f64,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default)]
    pub halign: HAlign,
    #[serde(default)]
    pub valign: VAlign,
}
impl Text {
    /// The point the text is placed by, as the Insertion snap and the grip show it: the alignment
    /// point (DXF group 11) of justified text, the start point (group 10) of left-baseline,
    /// aligned and fit text.
    pub fn justify_point(&self) -> Vec2 {
        match (self.halign, self.valign) {
            (HAlign::Left, VAlign::Baseline) | (HAlign::Aligned | HAlign::Fit, _) => self.insert.xy(),
            _ => self.align_pt.unwrap_or(self.insert).xy(),
        }
    }
}
fn one() -> f64 {
    1.0
}
fn standard() -> String {
    "Standard".into()
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MText {
    pub insert: Vec3,
    pub height: f64,
    /// Reference rectangle width (0 = no wrapping).
    #[serde(default)]
    pub width: f64,
    /// 1..=9: TL TC TR ML MC MR BL BC BR.
    #[serde(default = "attach_tl")]
    pub attach: u8,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default = "standard")]
    pub style: String,
    /// Raw MTEXT contents with inline codes (\P, {\fArial|b1;...}, \S1/2; …).
    pub contents: String,
    #[serde(default = "one")]
    pub line_spacing: f64,
    /// Line spacing style (DXF 73): Exactly (`true`) keeps `line_spacing` whatever the character
    /// heights; At least (`false`, the default) lets taller characters push lines apart.
    #[serde(default)]
    pub line_spacing_exact: bool,
}
fn attach_tl() -> u8 {
    1
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attrib {
    pub tag: String,
    pub text: Text,
    #[serde(default)]
    pub invisible: bool,
    #[serde(default)]
    pub constant: bool,
    #[serde(default)]
    pub prompt: String,
    /// An attribute's own properties; each one left unset follows the attribute definition.
    #[serde(default, skip_serializing_if = "AttribProps::is_empty")]
    pub props: AttribProps,
}

/// Properties set on one attribute of a block reference (an ATTRIB's layer, colour, linetype and
/// lineweight), overriding those of its definition.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AttribProps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linetype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lineweight: Option<Lineweight>,
}

impl AttribProps {
    /// Nothing set: the attribute is drawn with its definition's properties.
    pub fn is_empty(&self) -> bool {
        *self == AttribProps::default()
    }
    /// `base` (the definition's properties) with the ones set here in place.
    pub fn over(&self, base: &Common) -> Common {
        Common {
            layer: self.layer.clone().unwrap_or_else(|| base.layer.clone()),
            color: self.color.unwrap_or(base.color),
            linetype: self.linetype.clone().unwrap_or_else(|| base.linetype.clone()),
            lineweight: self.lineweight.unwrap_or(base.lineweight),
            ..base.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Insert {
    pub block: String,
    pub insert: Vec3,
    #[serde(default = "unit_scale")]
    pub scale: Vec3,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub attribs: Vec<Attrib>,
    #[serde(default = "one_u")]
    pub cols: u32,
    #[serde(default = "one_u")]
    pub rows: u32,
    #[serde(default)]
    pub col_spacing: f64,
    #[serde(default)]
    pub row_spacing: f64,
    /// XCLIP boundary in world coordinates (two opposite corners = AABB, or a polygon). Empty: no clip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clip: Vec<Vec2>,
    /// The clip boundary is applied when drawing this insert.
    #[serde(default)]
    pub clipping: bool,
}
fn unit_scale() -> Vec3 {
    Vec3::new(1.0, 1.0, 1.0)
}
fn one_u() -> u32 {
    1
}
impl Default for Insert {
    fn default() -> Self {
        Insert {
            block: String::new(),
            insert: Vec3::ZERO,
            scale: unit_scale(),
            rotation: 0.0,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
            clip: Vec::new(),
            clipping: false,
        }
    }
}

impl Insert {
    /// Block-to-world transform (2D part).
    pub fn transform(&self, base: Vec2) -> Mat3 {
        Mat3::translate(self.insert.xy())
            .then_before(Mat3::rotate(self.rotation))
            .then_before(Mat3::scale(self.scale.x, self.scale.y))
            .then_before(Mat3::translate(-base))
    }

    /// Attribute text for `tag`, if this insert carries one.
    pub fn attrib_value(&self, tag: &str) -> Option<&str> {
        self.attribs.iter().find(|a| a.tag.eq_ignore_ascii_case(tag)).map(|a| a.text.value.as_str())
    }

    /// A door or window placed by `wall.door` / `wall.window` (parameters live on invisible attributes).
    pub fn is_opening(&self) -> bool {
        matches!(self.attrib_value("OKIND"), Some("door") | Some("window"))
    }

    /// Plan width in drawing units, when this insert is an opening.
    pub fn opening_width(&self) -> Option<f64> {
        if !self.is_opening() {
            return None;
        }
        self.attrib_value("OWIDTH").and_then(|v| v.parse().ok()).filter(|w: &f64| w.is_finite() && *w > 0.0)
    }

    /// Set an invisible attribute, adding it when the tag is new.
    pub fn set_attrib(&mut self, tag: &str, value: &str) {
        if let Some(a) = self.attribs.iter_mut().find(|a| a.tag.eq_ignore_ascii_case(tag)) {
            a.text.value = value.to_string();
            a.invisible = true;
            return;
        }
        self.attribs.push(Attrib {
            tag: tag.to_string(),
            text: Text {
                insert: Vec3::ZERO,
                align_pt: None,
                height: 1.0,
                value: value.to_string(),
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                style: "Standard".into(),
                halign: HAlign::Left,
                valign: VAlign::Baseline,
            },
            invisible: true,
            constant: true,
            prompt: String::new(),
            props: AttribProps::default(),
        });
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DimKind {
    /// Rotated linear dimension (horizontal = 0, vertical = 90°).
    Linear {
        rotation: f64,
    },
    Aligned,
    Angular,
    Angular3P,
    Diameter,
    Radius,
    Ordinate {
        x_type: bool,
    },
    ArcLength,
}
/// A dimension. Definition points follow DXF: `defpt` (10: dimension line point),
/// `p13`/`p14` (extension origins), `p15`/`p16` (arc/angle points), `text_mid` (11).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dimension {
    pub kind: DimKind,
    pub defpt: Vec3,
    pub text_mid: Vec3,
    #[serde(default)]
    pub p13: Vec3,
    #[serde(default)]
    pub p14: Vec3,
    #[serde(default)]
    pub p15: Vec3,
    #[serde(default)]
    pub p16: Vec3,
    /// "" = measured value, "<>" inside = measurement placeholder, " " = suppressed.
    #[serde(default)]
    pub text: String,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default)]
    pub measurement: f64,
    #[serde(default)]
    pub text_rotation: f64,
    /// User moved the text (text_mid is authoritative).
    #[serde(default)]
    pub user_text_pos: bool,
    /// Anonymous block with the rendered geometry (from files); regenerated when edited.
    #[serde(default)]
    pub block: Option<String>,
    /// Per-dimension style overrides (DIMOVERRIDE): DimStyle field names (camelCase) → values,
    /// merged over the named style.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub overrides: serde_json::Map<String, serde_json::Value>,
    /// Associativity: which definition points follow which objects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assoc: Vec<DimAssoc>,
}

/// A dimension definition point that follows an object (associative dimensions).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DimAssoc {
    /// Definition point: "defpt", "p13", "p14", "p15" or "p16".
    pub point: String,
    /// The object it is attached to.
    pub handle: Handle,
    pub snap: AssocSnap,
}

/// Where on the object an associative definition point sits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssocSnap {
    /// Start point of a line/arc (or polyline vertex 0).
    Start,
    /// End point of a line/arc.
    End,
    Mid,
    /// Centre of a circle/arc.
    Center,
    /// On a circle/arc at a fixed angle (radians) from its centre.
    OnCircle {
        angle: f64,
    },
    /// Intersection of this (line) object with another line.
    Intersection {
        other: Handle,
    },
    /// Vertex `index` of a polyline.
    Vertex {
        index: usize,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leader {
    pub vertices: Vec<Vec3>,
    #[serde(default = "yes")]
    pub arrow: bool,
    #[serde(default)]
    pub spline: bool,
    #[serde(default = "standard")]
    pub style: String,
}
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MLeader {
    pub leaders: Vec<Vec<Vec3>>,
    pub landing: Vec3,
    pub dogleg: f64,
    pub text: Option<MText>,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default = "one")]
    pub arrow_size: f64,
    /// Leader lines are smooth curves through their points (leader type Spline). An empty
    /// `leaders` is a multileader without a leader line (leader type None).
    #[serde(default)]
    pub spline: bool,
    /// Block content in place of the text: a block reference placed at the landing.
    #[serde(default)]
    pub block: Option<Insert>,
}

impl MLeader {
    /// Each leader line as drawn: through its points to the landing, smoothed for spline leaders.
    pub fn leader_paths(&self) -> Vec<Vec<Vec2>> {
        self.leaders
            .iter()
            .map(|l| {
                let pts: Vec<Vec2> = l.iter().map(|v| v.xy()).chain(std::iter::once(self.landing.xy())).collect();
                if self.spline && pts.len() > 2 { GSpline::from_fit_points(&pts).tessellate(1e-3) } else { pts }
            })
            .collect()
    }
}
/// A hatch boundary loop: closed polyline with bulges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HatchLoop {
    pub vertices: Vec<PolyVertex>,
    #[serde(default)]
    pub outer: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hatch {
    pub pattern: String,
    pub solid: bool,
    pub loops: Vec<HatchLoop>,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub associative: bool,
    /// 0 normal (odd parity), 1 outer, 2 ignore.
    #[serde(default)]
    pub style: u8,
    #[serde(default)]
    pub elevation: f64,
    /// Gradient fill: name + second colour.
    #[serde(default)]
    pub gradient: Option<Gradient>,
    #[serde(default)]
    pub origin: Vec2,
    #[serde(default)]
    pub background: Option<Color>,
    /// The pattern's line families as a file defined them, for a pattern that isn't in the
    /// standard library (unit scale and angle, like the library's; empty otherwise).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pattern_lines: Vec<crate::library::PatternLine>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gradient {
    pub name: String,
    pub color1: Color,
    pub color2: Color,
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub centered: bool,
}
/// SOLID / TRACE: up to four corners (DXF order 1,2,4,3 when drawn).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    pub corners: [Vec3; 4],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RayLine {
    pub base: Vec3,
    pub dir: Vec3,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub center: Vec3,
    pub width: f64,
    pub height: f64,
    pub view_center: Vec2,
    pub view_height: f64,
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub frozen_layers: Vec<String>,
    /// Per-viewport layer colour overrides (VP Color in the Layer Properties Manager).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layer_colors: Vec<(String, Color)>,
    /// Optional VPCLIP polygon in paper-space coordinates. When three or more points are set,
    /// contents are clipped to the polygon (even-odd); the border still uses width/height.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clip: Vec<Vec2>,
    /// The level this viewport shows; `None` is the first level (every older file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<crate::LevelId>,
    /// The sheet set whose annotation of that level is drawn over it; `None` is plain geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet_set: Option<crate::SheetSetId>,
    /// Other levels shown faded under or over this viewport's level (at most [`crate::MAX_TRACES`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traces: Vec<crate::Trace>,
    /// Optional named compose view this viewport follows; when set, level / sheet set / traces /
    /// scale are taken from that view when applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compose_view: Option<crate::ComposeViewId>,
}
impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            center: Vec3::ZERO,
            width: 1.0,
            height: 1.0,
            view_center: Vec2::ZERO,
            view_height: 1.0,
            id: 0,
            locked: false,
            frozen_layers: Vec::new(),
            layer_colors: Vec::new(),
            clip: Vec::new(),
            level: None,
            sheet_set: None,
            traces: Vec::new(),
            compose_view: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub insert: Vec3,
    /// One pixel's width (U) and height (V) vectors in drawing units.
    pub u: Vec3,
    pub v: Vec3,
    /// Image size in pixels.
    pub size: Vec2,
    /// The image file (its IMAGEDEF object's path).
    pub path: String,
    /// The image definition's name (its key in the drawing's image dictionary).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Clip boundary in pixel coordinates (origin at the top-left corner of the image, y down):
    /// two opposite corners of a rectangle, or a polygon's vertices. Empty: the whole image.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clip: Vec<Vec2>,
    /// The clip boundary is applied.
    #[serde(default)]
    pub clipping: bool,
    /// Display flags: 1 show the image, 2 show it when not aligned with the screen, 4 use the
    /// clip boundary, 8 transparency on.
    #[serde(default = "image_display")]
    pub display: u16,
    /// Brightness, contrast and fade, 0..=100.
    #[serde(default = "image_fifty")]
    pub brightness: u8,
    #[serde(default = "image_fifty")]
    pub contrast: u8,
    #[serde(default)]
    pub fade: u8,
    /// The definition's default size of one pixel and its resolution unit (0 none, 2 cm, 5 inch).
    #[serde(default = "image_pixel")]
    pub pixel_size: Vec2,
    #[serde(default)]
    pub resolution_units: u8,
}
fn image_display() -> u16 {
    7
}
fn image_fifty() -> u8 {
    50
}
fn image_pixel() -> Vec2 {
    Vec2::new(1.0, 1.0)
}
impl Default for Image {
    fn default() -> Self {
        Image {
            insert: Vec3::ZERO,
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
            size: Vec2::new(1.0, 1.0),
            path: String::new(),
            name: String::new(),
            clip: Vec::new(),
            clipping: false,
            display: image_display(),
            brightness: image_fifty(),
            contrast: image_fifty(),
            fade: 0,
            pixel_size: image_pixel(),
            resolution_units: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wipeout {
    pub boundary: Vec<Vec2>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCell {
    pub text: String,
    #[serde(default)]
    pub merged: Option<(u32, u32)>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub insert: Vec3,
    pub col_widths: Vec<f64>,
    pub row_heights: Vec<f64>,
    pub cells: Vec<Vec<TableCell>>,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default = "default_text_h")]
    pub text_height: f64,
    /// First row is a title (merged across, larger text).
    #[serde(default)]
    pub title: bool,
    /// The row after the title holds column headers.
    #[serde(default)]
    pub header: bool,
}
fn default_text_h() -> f64 {
    0.18
}
/// A raw DXF group: kept for round-tripping entities we do not model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawTag {
    pub code: i32,
    pub value: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unknown {
    pub dxf_type: String,
    pub tags: Vec<RawTag>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EntityKind {
    Line(Line),
    Point(Point),
    Circle(Circle),
    Arc(Arc),
    Ellipse(Ellipse),
    LwPolyline(LwPolyline),
    Spline(GSpline),
    Ray(RayLine),
    XLine(RayLine),
    Text(Text),
    MText(MText),
    AttDef(Attrib),
    Insert(Insert),
    Dimension(Dimension),
    Leader(Leader),
    MLeader(MLeader),
    Hatch(Hatch),
    Solid(Solid),
    Trace(Solid),
    Viewport(Viewport),
    Image(Image),
    Wipeout(Wipeout),
    Table(Table),
    Unknown(Unknown),
}

/// A primitive the entity decomposes into, for snapping, selection, trimming and rendering.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Seg(Segment),
    Circle(GCircle),
    Ellipse(GEllipse),
    Spline(GSpline),
    Point(Vec2),
    /// An infinite line (`ray == false`) or a ray from `base` along `dir`.
    Infinite {
        base: Vec2,
        dir: Vec2,
        ray: bool,
    },
    /// A filled polygon (solids, wide polylines, hatch fill) — the outline is also selectable.
    Fill(Vec<Vec2>),
}

impl EntityKind {
    /// AutoCAD's object type name as shown in the Properties palette.
    pub fn type_name(&self) -> &'static str {
        match self {
            EntityKind::Line(_) => "Line",
            EntityKind::Point(_) => "Point",
            EntityKind::Circle(_) => "Circle",
            EntityKind::Arc(_) => "Arc",
            EntityKind::Ellipse(_) => "Ellipse",
            EntityKind::LwPolyline(_) => "Polyline",
            EntityKind::Spline(_) => "Spline",
            EntityKind::Ray(_) => "Ray",
            EntityKind::XLine(_) => "Xline",
            EntityKind::Text(_) => "Text",
            EntityKind::MText(_) => "MText",
            EntityKind::AttDef(_) => "Attribute Definition",
            EntityKind::Insert(_) => "Block Reference",
            EntityKind::Dimension(_) => "Dimension",
            EntityKind::Leader(_) => "Leader",
            EntityKind::MLeader(_) => "Multileader",
            EntityKind::Hatch(_) => "Hatch",
            EntityKind::Solid(_) => "2D Solid",
            EntityKind::Trace(_) => "Trace",
            EntityKind::Viewport(_) => "Viewport",
            EntityKind::Image(_) => "Raster Image",
            EntityKind::Wipeout(_) => "Wipeout",
            EntityKind::Table(_) => "Table",
            EntityKind::Unknown(_) => "Proxy",
        }
    }
    /// DXF entity type name.
    pub fn dxf_name(&self) -> &str {
        match self {
            EntityKind::Line(_) => "LINE",
            EntityKind::Point(_) => "POINT",
            EntityKind::Circle(_) => "CIRCLE",
            EntityKind::Arc(_) => "ARC",
            EntityKind::Ellipse(_) => "ELLIPSE",
            EntityKind::LwPolyline(_) => "LWPOLYLINE",
            EntityKind::Spline(_) => "SPLINE",
            EntityKind::Ray(_) => "RAY",
            EntityKind::XLine(_) => "XLINE",
            EntityKind::Text(_) => "TEXT",
            EntityKind::MText(_) => "MTEXT",
            EntityKind::AttDef(_) => "ATTDEF",
            EntityKind::Insert(_) => "INSERT",
            EntityKind::Dimension(_) => "DIMENSION",
            EntityKind::Leader(_) => "LEADER",
            EntityKind::MLeader(_) => "MULTILEADER",
            EntityKind::Hatch(_) => "HATCH",
            EntityKind::Solid(_) => "SOLID",
            EntityKind::Trace(_) => "TRACE",
            EntityKind::Viewport(_) => "VIEWPORT",
            EntityKind::Image(_) => "IMAGE",
            EntityKind::Wipeout(_) => "WIPEOUT",
            EntityKind::Table(_) => "ACAD_TABLE",
            EntityKind::Unknown(u) => u.dxf_type.as_str(),
        }
    }

    /// Geometric primitives in WCS (2D projection). Text/blocks/dimensions return their
    /// insertion points only; their full geometry comes from the renderer's expansion.
    pub fn prims(&self) -> Vec<Prim> {
        match self {
            EntityKind::Line(l) => vec![Prim::Seg(Segment::Line(GLine::new(l.a.xy(), l.b.xy())))],
            EntityKind::Point(p) => vec![Prim::Point(p.p.xy())],
            EntityKind::Circle(c) => vec![Prim::Circle(GCircle::new(c.center.xy(), c.radius))],
            EntityKind::Arc(a) => vec![Prim::Seg(Segment::Arc { arc: GArc::new(a.center.xy(), a.radius, a.start, a.end), ccw: true })],
            EntityKind::Ellipse(e) => {
                vec![Prim::Ellipse(GEllipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end })]
            }
            EntityKind::LwPolyline(p) => {
                let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
                pl.segments().into_iter().map(Prim::Seg).collect()
            }
            EntityKind::Spline(s) => vec![Prim::Spline(s.clone())],
            EntityKind::Ray(r) => vec![Prim::Infinite { base: r.base.xy(), dir: r.dir.xy().normalized(), ray: true }],
            EntityKind::XLine(r) => vec![Prim::Infinite { base: r.base.xy(), dir: r.dir.xy().normalized(), ray: false }],
            EntityKind::Solid(s) | EntityKind::Trace(s) => {
                let c = &s.corners;
                vec![Prim::Fill(vec![c[0].xy(), c[1].xy(), c[3].xy(), c[2].xy()])]
            }
            EntityKind::Hatch(h) => {
                h.loops.iter().flat_map(|l| Polyline { vertices: l.vertices.clone(), closed: true }.segments()).map(Prim::Seg).collect()
            }
            EntityKind::Wipeout(w) => {
                let mut pts = w.boundary.clone();
                if let Some(f) = pts.first().copied() {
                    pts.push(f);
                }
                pts.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(*w.first()?, *w.get(1)?))))).collect()
            }
            EntityKind::Viewport(v) => {
                let c = v.center.xy();
                let h = Vec2::new(v.width / 2.0, v.height / 2.0);
                let b = Bounds2::new(c - h, c + h).corners();
                (0..4).map(|i| Prim::Seg(Segment::Line(GLine::new(b[i], b[(i + 1) % 4])))).collect()
            }
            EntityKind::Leader(l) => {
                l.vertices.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(w.first()?.xy(), w.get(1)?.xy()))))).collect()
            }
            EntityKind::Text(t) => vec![Prim::Point(t.insert.xy())],
            EntityKind::MText(t) => vec![Prim::Point(t.insert.xy())],
            EntityKind::AttDef(a) => vec![Prim::Point(a.text.insert.xy())],
            EntityKind::Insert(i) => vec![Prim::Point(i.insert.xy())],
            EntityKind::Dimension(d) => vec![Prim::Point(d.defpt.xy())],
            EntityKind::MLeader(m) => m
                .leaders
                .iter()
                .flat_map(|l| l.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(w.first()?.xy(), w.get(1)?.xy()))))))
                .chain(m.block.iter().map(|b| Prim::Point(b.insert.xy())))
                .collect(),
            EntityKind::Image(i) => vec![Prim::Point(i.insert.xy())],
            EntityKind::Table(t) => vec![Prim::Point(t.insert.xy())],
            EntityKind::Unknown(_) => Vec::new(),
        }
    }

    /// Apply a 2D affine transform (move/rotate/uniform scale/mirror). Non-uniform scale turns
    /// circles into ellipses only where the kind supports it; otherwise radii use the mean scale.
    pub fn transform(&mut self, m: &Mat3) {
        let t3 = |p: &mut Vec3| {
            let q = m.apply(p.xy());
            p.x = q.x;
            p.y = q.y;
        };
        let s = m.scale_factor();
        let rot = m.rotation();
        let mirror = m.is_mirroring();
        match self {
            EntityKind::Line(l) => {
                t3(&mut l.a);
                t3(&mut l.b);
            }
            EntityKind::Point(p) => t3(&mut p.p),
            EntityKind::Circle(c) => {
                t3(&mut c.center);
                c.radius *= s;
            }
            EntityKind::Arc(a) => {
                let sp = m.apply(Vec2::polar(a.center.xy(), a.radius, a.start));
                let ep = m.apply(Vec2::polar(a.center.xy(), a.radius, a.end));
                t3(&mut a.center);
                a.radius *= s;
                let c = a.center.xy();
                if mirror {
                    a.start = c.angle_to(ep);
                    a.end = c.angle_to(sp);
                } else {
                    a.start = c.angle_to(sp);
                    a.end = c.angle_to(ep);
                }
            }
            EntityKind::Ellipse(e) => {
                let sp = m.apply(e.center.xy() + e.major.xy() * e.start.cos() + e.major.xy().perp() * (e.ratio * e.start.sin()));
                let ep = m.apply(e.center.xy() + e.major.xy() * e.end.cos() + e.major.xy().perp() * (e.ratio * e.end.sin()));
                t3(&mut e.center);
                let mj = m.apply_vec(e.major.xy());
                e.major = mj.to3(0.0);
                let full = (aad_geom::ccw_sweep(e.start, e.end) - aad_geom::TAU).abs() < 1e-9;
                if !full {
                    let ge = GEllipse { center: e.center.xy(), major: mj, ratio: e.ratio, start: 0.0, end: aad_geom::TAU };
                    let (ps, pe) = (ge.param_of(sp), ge.param_of(ep));
                    if mirror {
                        e.start = pe;
                        e.end = ps;
                    } else {
                        e.start = ps;
                        e.end = pe;
                    }
                }
            }
            EntityKind::LwPolyline(p) => {
                for v in &mut p.vertices {
                    v.p = m.apply(v.p);
                    v.start_width *= s;
                    v.end_width *= s;
                    if mirror {
                        v.bulge = -v.bulge;
                    }
                }
                p.const_width *= s;
            }
            EntityKind::Spline(sp) => sp.transform(m),
            EntityKind::Ray(r) | EntityKind::XLine(r) => {
                t3(&mut r.base);
                r.dir = m.apply_vec(r.dir.xy()).normalized().to3(0.0);
            }
            EntityKind::Text(t) => transform_text(t, m, s, rot, mirror),
            EntityKind::AttDef(a) => transform_text(&mut a.text, m, s, rot, mirror),
            EntityKind::MText(t) => {
                t3(&mut t.insert);
                t.height *= s;
                t.width *= s;
                t.rotation = aad_geom::norm_angle(t.rotation + rot);
            }
            EntityKind::Insert(i) => {
                t3(&mut i.insert);
                i.scale.x *= s;
                i.scale.y *= s;
                i.scale.z *= s;
                i.rotation = aad_geom::norm_angle(i.rotation + rot);
                if mirror {
                    i.scale.y = -i.scale.y;
                    i.rotation = aad_geom::norm_angle(rot - (i.rotation - rot));
                }
                for a in &mut i.attribs {
                    transform_text(&mut a.text, m, s, rot, false);
                }
                i.col_spacing *= s;
                i.row_spacing *= s;
            }
            EntityKind::Dimension(d) => {
                for p in [&mut d.defpt, &mut d.text_mid, &mut d.p13, &mut d.p14, &mut d.p15, &mut d.p16] {
                    t3(p);
                }
                if let DimKind::Linear { rotation } = &mut d.kind {
                    *rotation = aad_geom::norm_angle(*rotation + rot);
                }
                d.block = None;
            }
            EntityKind::Leader(l) => l.vertices.iter_mut().for_each(t3),
            EntityKind::MLeader(ml) => {
                for l in &mut ml.leaders {
                    l.iter_mut().for_each(t3);
                }
                t3(&mut ml.landing);
                if let Some(t) = &mut ml.text {
                    t3(&mut t.insert);
                    t.height *= s;
                }
                if let Some(b) = &mut ml.block {
                    // Transformed exactly like a block reference.
                    let mut k = EntityKind::Insert(b.clone());
                    k.transform(m);
                    if let EntityKind::Insert(nb) = k {
                        *b = nb;
                    }
                }
            }
            EntityKind::Hatch(h) => {
                for l in &mut h.loops {
                    for v in &mut l.vertices {
                        v.p = m.apply(v.p);
                        if mirror {
                            v.bulge = -v.bulge;
                        }
                    }
                }
                h.scale *= s;
                h.angle = aad_geom::norm_angle(h.angle + rot);
                h.origin = m.apply(h.origin);
            }
            EntityKind::Solid(so) | EntityKind::Trace(so) => so.corners.iter_mut().for_each(t3),
            EntityKind::Viewport(v) => {
                t3(&mut v.center);
                v.width *= s;
                v.height *= s;
            }
            EntityKind::Image(i) => {
                t3(&mut i.insert);
                i.u = m.apply_vec(i.u.xy()).to3(0.0);
                i.v = m.apply_vec(i.v.xy()).to3(0.0);
            }
            EntityKind::Wipeout(w) => w.boundary.iter_mut().for_each(|p| *p = m.apply(*p)),
            EntityKind::Table(t) => {
                t3(&mut t.insert);
                t.col_widths.iter_mut().for_each(|w| *w *= s);
                t.row_heights.iter_mut().for_each(|h| *h *= s);
                t.text_height *= s;
            }
            EntityKind::Unknown(_) => {}
        }
    }

    /// Grip points (editable defining points), in display order.
    pub fn grips(&self) -> Vec<Vec2> {
        match self {
            EntityKind::Line(l) => vec![l.a.xy(), l.a.xy().mid(l.b.xy()), l.b.xy()],
            EntityKind::Point(p) => vec![p.p.xy()],
            EntityKind::Circle(c) => {
                let o = c.center.xy();
                vec![o, o + Vec2::X * c.radius, o + Vec2::Y * c.radius, o - Vec2::X * c.radius, o - Vec2::Y * c.radius]
            }
            EntityKind::Arc(a) => {
                let g = GArc::new(a.center.xy(), a.radius, a.start, a.end);
                vec![g.start_point(), g.mid_point(), g.end_point(), a.center.xy()]
            }
            EntityKind::Ellipse(e) => {
                let c = e.center.xy();
                let mj = e.major.xy();
                let mn = mj.perp() * e.ratio;
                vec![c, c + mj, c + mn, c - mj, c - mn]
            }
            EntityKind::LwPolyline(p) => {
                let mut g: Vec<Vec2> = Vec::new();
                let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
                for (i, v) in p.vertices.iter().enumerate() {
                    g.push(v.p);
                    if let Some(s) = pl.segments().get(i) {
                        g.push(s.mid());
                    }
                }
                g
            }
            EntityKind::Spline(s) => {
                if s.fit.is_empty() {
                    s.control.clone()
                } else {
                    s.fit.clone()
                }
            }
            EntityKind::Ray(r) | EntityKind::XLine(r) => vec![r.base.xy(), r.base.xy() + r.dir.xy()],
            EntityKind::Text(t) => vec![t.justify_point()],
            EntityKind::MText(t) => vec![t.insert.xy()],
            EntityKind::AttDef(a) => vec![a.text.justify_point()],
            EntityKind::Insert(i) => {
                if let Some(w) = i.opening_width() {
                    let sign = if i.scale.x < 0.0 { -1.0 } else { 1.0 };
                    let u = Vec2::new(i.rotation.cos(), i.rotation.sin()) * sign;
                    let half = w / 2.0;
                    vec![i.insert.xy(), i.insert.xy() - u * half, i.insert.xy() + u * half]
                } else {
                    std::iter::once(i.insert.xy()).chain(i.attribs.iter().map(|a| a.text.justify_point())).collect()
                }
            }
            EntityKind::Dimension(d) => vec![d.p13.xy(), d.p14.xy(), d.defpt.xy(), d.text_mid.xy()],
            EntityKind::Leader(l) => l.vertices.iter().map(|v| v.xy()).collect(),
            EntityKind::MLeader(m) => m.leaders.iter().flat_map(|l| l.iter().map(|v| v.xy())).chain(std::iter::once(m.landing.xy())).collect(),
            EntityKind::Hatch(h) => {
                let b = Bounds2::from_points(h.loops.iter().flat_map(|l| l.vertices.iter().map(|v| v.p)));
                vec![b.center()]
            }
            EntityKind::Solid(s) | EntityKind::Trace(s) => s.corners.iter().map(|c| c.xy()).collect(),
            EntityKind::Viewport(v) => {
                let c = v.center.xy();
                let h = Vec2::new(v.width / 2.0, v.height / 2.0);
                Bounds2::new(c - h, c + h).corners().to_vec()
            }
            EntityKind::Image(i) => vec![i.insert.xy()],
            EntityKind::Wipeout(w) => w.boundary.clone(),
            EntityKind::Table(t) => vec![t.insert.xy()],
            EntityKind::Unknown(_) => Vec::new(),
        }
    }
}

fn transform_text(t: &mut Text, m: &Mat3, s: f64, rot: f64, mirror: bool) {
    let q = m.apply(t.insert.xy());
    t.insert.x = q.x;
    t.insert.y = q.y;
    if let Some(a) = &mut t.align_pt {
        let q = m.apply(a.xy());
        a.x = q.x;
        a.y = q.y;
    }
    t.height *= s;
    // MIRRTEXT = 0 (the default): mirrored text keeps reading left to right.
    t.rotation = aad_geom::norm_angle(t.rotation + if mirror { 0.0 } else { rot });
}
