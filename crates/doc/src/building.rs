//! Building-element classification for 2D plan objects (structural, finish, shop).
//!
//! Spec references are **code ids** from our open Indian index (and IFC type names), not
//! copyrighted BIS/CPWD/IS body text. Rates stay outside this crate (O-CoreOp).

use serde::{Deserialize, Serialize};

/// Family of a building element on the plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BuildingFamily {
    Structural,
    Finish,
    Shop,
    Other,
}

impl BuildingFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            BuildingFamily::Structural => "structural",
            BuildingFamily::Finish => "finish",
            BuildingFamily::Shop => "shop",
            BuildingFamily::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "structural" => Some(BuildingFamily::Structural),
            "finish" => Some(BuildingFamily::Finish),
            "shop" => Some(BuildingFamily::Shop),
            "other" => Some(BuildingFamily::Other),
            _ => None,
        }
    }
}

/// Kind of building element (2D plan semantics).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BuildingKind {
    Column,
    Beam,
    Footing,
    Wall,
    Slab,
    Plaster,
    Paint,
    Flooring,
    Door,
    Window,
    Grill,
    Other,
}

impl BuildingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BuildingKind::Column => "column",
            BuildingKind::Beam => "beam",
            BuildingKind::Footing => "footing",
            BuildingKind::Wall => "wall",
            BuildingKind::Slab => "slab",
            BuildingKind::Plaster => "plaster",
            BuildingKind::Paint => "paint",
            BuildingKind::Flooring => "flooring",
            BuildingKind::Door => "door",
            BuildingKind::Window => "window",
            BuildingKind::Grill => "grill",
            BuildingKind::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "column" => Some(BuildingKind::Column),
            "beam" => Some(BuildingKind::Beam),
            "footing" => Some(BuildingKind::Footing),
            "wall" => Some(BuildingKind::Wall),
            "slab" => Some(BuildingKind::Slab),
            "plaster" => Some(BuildingKind::Plaster),
            "paint" => Some(BuildingKind::Paint),
            "flooring" => Some(BuildingKind::Flooring),
            "door" => Some(BuildingKind::Door),
            "window" => Some(BuildingKind::Window),
            "grill" => Some(BuildingKind::Grill),
            "other" => Some(BuildingKind::Other),
            _ => None,
        }
    }

    pub fn default_family(self) -> BuildingFamily {
        match self {
            BuildingKind::Column | BuildingKind::Beam | BuildingKind::Footing | BuildingKind::Wall | BuildingKind::Slab => BuildingFamily::Structural,
            BuildingKind::Plaster | BuildingKind::Paint | BuildingKind::Flooring => BuildingFamily::Finish,
            BuildingKind::Door | BuildingKind::Window | BuildingKind::Grill => BuildingFamily::Shop,
            BuildingKind::Other => BuildingFamily::Other,
        }
    }

    /// Default IFC class name for this plan kind (semantic label only; no 3D export).
    pub fn default_ifc_type(self) -> &'static str {
        match self {
            BuildingKind::Column => "IfcColumn",
            BuildingKind::Beam => "IfcBeam",
            BuildingKind::Footing => "IfcFooting",
            BuildingKind::Wall => "IfcWall",
            BuildingKind::Slab => "IfcSlab",
            BuildingKind::Plaster => "IfcCovering",
            BuildingKind::Paint => "IfcCovering",
            BuildingKind::Flooring => "IfcCovering",
            BuildingKind::Door => "IfcDoor",
            BuildingKind::Window => "IfcWindow",
            BuildingKind::Grill => "IfcDiscreteAccessory",
            BuildingKind::Other => "IfcBuildingElementProxy",
        }
    }
}

/// Which standards system a code id belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpecSystem {
    Is,
    Bis,
    Cpwd,
    Ifc,
}

impl SpecSystem {
    pub fn as_str(self) -> &'static str {
        match self {
            SpecSystem::Is => "IS",
            SpecSystem::Bis => "BIS",
            SpecSystem::Cpwd => "CPWD",
            SpecSystem::Ifc => "IFC",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "IS" => Some(SpecSystem::Is),
            "BIS" => Some(SpecSystem::Bis),
            "CPWD" => Some(SpecSystem::Cpwd),
            "IFC" => Some(SpecSystem::Ifc),
            _ => None,
        }
    }
}

/// One code reference (open index id, not standard body text).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecRef {
    pub system: SpecSystem,
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Classification carried by an entity or a component (block) definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildingElement {
    pub family: BuildingFamily,
    pub kind: BuildingKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spec_refs: Vec<SpecRef>,
    /// IFC class name (e.g. `IfcWall`); empty means use [`BuildingKind::default_ifc_type`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ifc_type: Option<String>,
    /// Measurement / bridge section id when known (e.g. `hcw-aqc-bridge/1#walls`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measurement_rule: Option<String>,
}

impl BuildingElement {
    pub fn new(kind: BuildingKind) -> Self {
        BuildingElement {
            family: kind.default_family(),
            kind,
            spec_refs: Vec::new(),
            ifc_type: Some(kind.default_ifc_type().into()),
            measurement_rule: None,
        }
    }

    pub fn ifc_type_or_default(&self) -> &str {
        self.ifc_type.as_deref().filter(|s| !s.is_empty()).unwrap_or_else(|| self.kind.default_ifc_type())
    }
}
