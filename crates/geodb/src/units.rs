//! Drafting units: what one drawing unit is in millimetres, and where that knowledge came from.
//!
//! All geometry tables are stored in millimetres whatever the drawing was drafted in. The factor
//! comes from (in order) an explicit override, the drawing's own record (`draftingUnits` project
//! key, written by Tren), `INSUNITS`, and last the scale check. A drawing with no units at all is
//! never guessed: the scale check proposes a unit and the build stays unconfirmed until a person
//! confirms it.

use aad_doc::Drawing;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Project key (drawing project data and `.osp` `meta`) holding the drafting units.
pub const UNITS_KEY: &str = "draftingUnits";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitSystem {
    Metric,
    Imperial,
}

impl UnitSystem {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitSystem::Metric => "metric",
            UnitSystem::Imperial => "imperial",
        }
    }
}

/// Where the unit came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitSource {
    /// The caller said so (`geodb.build {units}`), which also confirms it.
    Override,
    /// Tren recorded it with the drawing (`draftingUnits`).
    Project,
    /// The file's `INSUNITS`.
    Insunits,
    /// No units in the file; the scale check proposed this one.
    ScaleCheck,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Units {
    pub system: UnitSystem,
    /// `mm`, `cm`, `m`, `in` or `ft`.
    pub unit: String,
    /// Millimetres in one drawing unit.
    pub to_mm: f64,
    pub source: UnitSource,
    /// False while the unit is only a proposal (a unitless drawing): `geodb.apply` refuses.
    pub confirmed: bool,
}

/// The units a name stands for: (system, canonical name, millimetres per unit).
pub fn unit_by_name(name: &str) -> Option<(UnitSystem, &'static str, f64)> {
    match name.trim().to_ascii_lowercase().as_str() {
        "mm" | "millimetre" | "millimeter" | "millimetres" | "millimeters" => Some((UnitSystem::Metric, "mm", 1.0)),
        "cm" | "centimetre" | "centimeter" => Some((UnitSystem::Metric, "cm", 10.0)),
        "m" | "metre" | "meter" | "metres" | "meters" => Some((UnitSystem::Metric, "m", 1000.0)),
        "in" | "inch" | "inches" | "\"" => Some((UnitSystem::Imperial, "in", 25.4)),
        "ft" | "foot" | "feet" | "'" => Some((UnitSystem::Imperial, "ft", 304.8)),
        _ => None,
    }
}

/// Units from an `INSUNITS` code, when it is one a plan is drawn in.
pub fn unit_from_insunits(code: i64) -> Option<(UnitSystem, &'static str, f64)> {
    match code {
        1 => unit_by_name("in"),
        2 => unit_by_name("ft"),
        4 => unit_by_name("mm"),
        5 => unit_by_name("cm"),
        6 => unit_by_name("m"),
        _ => None,
    }
}

impl Units {
    pub fn new(name: &str, source: UnitSource, confirmed: bool) -> Option<Units> {
        let (system, unit, to_mm) = unit_by_name(name)?;
        Some(Units { system, unit: unit.to_string(), to_mm, source, confirmed })
    }

    /// What Tren writes with a drawing: `{"system": "metric", "unit": "mm"}`.
    pub fn stamp(&self) -> Value {
        json!({ "system": self.system.as_str(), "unit": self.unit })
    }
}

/// The units the drawing declares, if any. `None` means unitless: run the scale check.
pub fn declared_units(d: &Drawing) -> Option<Units> {
    if let Some(raw) = d.project.get(UNITS_KEY)
        && let Ok(v) = serde_json::from_str::<Value>(raw)
        && let Some(name) = v.get("unit").and_then(Value::as_str)
        && let Some(u) = Units::new(name, UnitSource::Project, true)
    {
        return Some(u);
    }
    let code = d.header.i64("INSUNITS", 0);
    unit_from_insunits(code).map(|(system, unit, to_mm)| Units {
        system,
        unit: unit.to_string(),
        to_mm,
        source: UnitSource::Insunits,
        confirmed: true,
    })
}

/// The measurement system a unitless drawing leans to: `MEASUREMENT` 1 is metric, 0 imperial.
pub fn leaning_system(d: &Drawing) -> UnitSystem {
    if d.header.i64("MEASUREMENT", 1) == 0 { UnitSystem::Imperial } else { UnitSystem::Metric }
}

/// The candidate units the scale check tries, in the order they win ties.
pub const CANDIDATES: [&str; 5] = ["mm", "in", "m", "cm", "ft"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_codes_map_to_millimetres() {
        assert_eq!(unit_by_name("MM").map(|u| u.2), Some(1.0));
        assert_eq!(unit_by_name("in").map(|u| u.2), Some(25.4));
        assert_eq!(unit_from_insunits(6).map(|u| u.2), Some(1000.0));
        assert!(unit_from_insunits(0).is_none() && unit_from_insunits(14).is_none());
        assert!(unit_by_name("furlong").is_none());
    }

    #[test]
    fn a_drawing_without_units_is_undeclared() {
        let mut d = Drawing::new_metric();
        d.header.set_i64("INSUNITS", 0);
        assert!(declared_units(&d).is_none());
        d.header.set_i64("INSUNITS", 4);
        assert_eq!(declared_units(&d).map(|u| (u.unit, u.source)), Some(("mm".to_string(), UnitSource::Insunits)));
        d.project.insert(UNITS_KEY.into(), r#"{"system":"imperial","unit":"in"}"#.into());
        assert_eq!(declared_units(&d).map(|u| (u.unit, u.source)), Some(("in".to_string(), UnitSource::Project)), "the drawing's own record wins");
    }
}
