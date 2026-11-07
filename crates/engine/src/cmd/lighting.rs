//! Maintained illuminance for a declared room.
//!
//! The figure is lumens per square metre (lux) and the same quantity per square foot.
//! Circulation, toilets, offices and stores use the maintained levels published for those
//! uses. Residential rooms use one general level in that same range. This table is ours.
//! It is not a certified copy of EN 12464-1, the IES Lighting Handbook, or IS 3646.
//!
//! `flux_lm` is that illuminance times the room area: lumens incident on the area, not the
//! lamp catalog, which also depends on the fitting and the maintenance factor.

use serde_json::{Value, json};

/// Square metres in one square foot. The room tools already convert areas with this factor.
const M2_PER_FT2: f64 = 0.092_903_04;

/// `(declared name, maintained lm/m²)`. Longer names are matched first.
const LEVELS: &[(&str, u32)] = &[
    ("Master Bedroom", 100),
    ("Guest Room", 100),
    ("Dressing Room", 100),
    ("Bedroom", 100),
    ("Living Room", 150),
    ("Dining Room", 150),
    ("Pooja Room", 150),
    ("Kitchen", 300),
    ("Bathroom", 150),
    ("Attached Toilet", 200),
    ("Common Toilet", 200),
    ("Study Room", 500),
    ("Store Room", 100),
    ("Home Theater", 50),
    ("Wardrobe", 100),
    ("Staircase", 150),
    ("Corridor", 100),
    ("Entrance", 200),
    ("Lobby", 300),
    ("Utility", 200),
    ("Laundry", 200),
    ("Pantry", 200),
    ("Garage", 150),
    ("Garden", 20),
    ("Balcony", 50),
    ("Terrace", 50),
    ("Courtyard", 50),
    ("Office", 500),
    ("Gym", 300),
];

const GENERAL_LUX: u32 = 150;

struct Level {
    declared: String,
    lux: u32,
    matched: bool,
}

fn level_for(label: &str) -> Level {
    let up = label.to_uppercase();
    let mut best: Option<(&str, u32)> = None;
    for (name, lux) in LEVELS {
        if up.contains(&name.to_uppercase()) && best.is_none_or(|(kept, _)| name.len() > kept.len()) {
            best = Some((*name, *lux));
        }
    }
    match best {
        Some((name, lux)) => Level { declared: name.to_string(), lux, matched: true },
        None => Level { declared: "General".into(), lux: GENERAL_LUX, matched: false },
    }
}

fn round_to(value: f64, places: i32) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let scale = 10_f64.powi(places);
    (value * scale).round() / scale
}

/// Lighting row for one room label. `area` is in `unit` (`m2` or `sq ft`).
pub(crate) fn lighting_for(name: &str, area: f64, unit: &str) -> Value {
    let level = level_for(name);
    let area_ok = area.is_finite() && area >= 0.0;
    let area_m2 = if !area_ok {
        0.0
    } else if unit.starts_with("sq ft") {
        area * M2_PER_FT2
    } else {
        area
    };
    let area_ft2 = if M2_PER_FT2 > 0.0 { area_m2 / M2_PER_FT2 } else { 0.0 };
    let lm_per_ft2 = level.lux as f64 * M2_PER_FT2;
    json!({
        "declared": level.declared,
        "lighting_matched": level.matched,
        "lm_per_m2": level.lux,
        "lm_per_ft2": round_to(lm_per_ft2, 2),
        "area_m2": round_to(area_m2, 3),
        "area_ft2": round_to(area_ft2, 2),
        "flux_lm": round_to(area_m2 * level.lux as f64, 1),
    })
}

pub(crate) const NOTE: &str = "Maintained illuminance for general lighting of the declared room. lm_per_m2 and lm_per_ft2 are that level. flux_lm is the level times the room area (lumens on the area), not the lamp catalog.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_master_bedroom_is_not_read_as_a_bedroom() {
        let row = lighting_for("GF MASTER BEDROOM", 12.0, "m2");
        assert_eq!(row["declared"], "Master Bedroom");
        assert_eq!(row["lighting_matched"], true);
        assert_eq!(row["lm_per_m2"], 100);
    }

    #[test]
    fn an_undeclared_name_uses_the_general_level() {
        let row = lighting_for("PLANT", 10.0, "m2");
        assert_eq!(row["declared"], "General");
        assert_eq!(row["lighting_matched"], false);
        assert_eq!(row["lm_per_m2"], 150);
        assert_eq!(row["flux_lm"], 1500.0);
    }

    #[test]
    fn square_feet_use_the_same_level_per_area() {
        let row = lighting_for("BEDROOM", 100.0, "sq ft");
        assert_eq!(row["lm_per_m2"], 100);
        assert_eq!(row["lm_per_ft2"], 9.29);
        assert_eq!(row["flux_lm"], 929.0);
    }
}
