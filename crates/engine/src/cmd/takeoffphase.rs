//! Take-off phases: the estimation phase and the running-bill phase a measured dimension belongs to.
//!
//! The user picks them here, in the drawing, as they measure; Falcon shows them as columns of its tables. A
//! phase is a name from a list the project keeps (`takeoff.phases`). The *current* phase is stamped on every
//! dimension drawn or tagged afterwards, and `takeoff.phase` changes it on dimensions already measured. The
//! phases travel on the take-off mark (`phase`, `billPhase`) and on every exported row (`phase`, `bill_phase`);
//! Phase lists on take-off rows; Falcon may show them as columns when imported.

use aad_doc::{Drawing, Handle};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

/// Project entry that holds the lists and the current choice.
const KEY: &str = "takeoffPhases";
const MAX_PHASES: usize = 50;
const MAX_NAME: usize = 60;

const DEFAULT_ESTIMATION: [&str; 4] = ["Foundation", "Plinth", "Superstructure", "Finishing"];
const DEFAULT_BILLING: [&str; 3] = ["Phase 1", "Phase 2", "Phase 3"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("takeoff.phases", "Take-off Phases", run_phases).menu(&["Architecture", "Take-off", "Phases"]).alias(&["mphases"]).params(
            "{estimation?: [name], billing?: [name], current?: {estimation?: name|null, billing?: name|null}} \
                 → {estimation, billing, current}. Without parameters it only reads. The lists name the estimation phases \
                 (default Foundation, Plinth, Superstructure, Finishing) and the running-bill phases (default Phase 1 to 3). \
                 The current phases are stamped on every dimension drawn or tagged next.",
        ),
        CommandSpec::new("takeoff.phase", "Set Take-off Phase", run_phase)
            .menu(&["Architecture", "Take-off", "Set Phase of Selection"])
            .alias(&["mphase"])
            .params(
                "{estimation?: name|null, billing?: name|null, handles? (default the selection)} → {changed, skipped}. \
                 Sets the phase on dimensions that carry a take-off mark; null clears it. A name must be in the list \
                 (`takeoff.phases`). Unmarked objects are skipped: tag them first (`takeoff.tag`, `takeoff.maplayers`).",
            ),
    ]
}

/// What the project keeps: the two lists and the phases new dimensions get.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Phases {
    pub estimation: Vec<String>,
    pub billing: Vec<String>,
    pub current_estimation: Option<String>,
    pub current_billing: Option<String>,
}

impl Phases {
    fn to_json(&self) -> Value {
        json!({
            "estimation": self.estimation, "billing": self.billing,
            "current": { "estimation": self.current_estimation, "billing": self.current_billing },
        })
    }

    fn has(list: &[String], name: &str) -> bool {
        list.iter().any(|n| n.eq_ignore_ascii_case(name))
    }

    /// The list's own spelling of `name`.
    fn canonical<'a>(list: &'a [String], name: &str) -> Option<&'a str> {
        list.iter().find(|n| n.eq_ignore_ascii_case(name)).map(String::as_str)
    }
}

fn names(v: Option<&Value>, fallback: &[&str]) -> Vec<String> {
    let Some(a) = v.and_then(Value::as_array) else { return fallback.iter().map(|s| (*s).to_string()).collect() };
    clean(a.iter().filter_map(Value::as_str))
}

fn clean<'a>(it: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in it.map(str::trim).filter(|n| !n.is_empty()).map(|n| n.chars().take(MAX_NAME).collect::<String>()) {
        if !Phases::has(&out, &n) && out.len() < MAX_PHASES {
            out.push(n);
        }
    }
    out
}

/// The project's phases. A drawing that never set any has the defaults; a current phase that is no longer in its
/// list is dropped.
pub(crate) fn phases_of(d: &Drawing) -> Phases {
    let v = project::get(d, KEY);
    let estimation = names(v.get("estimation"), &DEFAULT_ESTIMATION);
    let billing = names(v.get("billing"), &DEFAULT_BILLING);
    let pick = |list: &[String], key: &str| {
        v.pointer(&format!("/current/{key}")).and_then(Value::as_str).and_then(|n| Phases::canonical(list, n)).map(str::to_string)
    };
    Phases { current_estimation: pick(&estimation, "estimation"), current_billing: pick(&billing, "billing"), estimation, billing }
}

fn store(d: &mut Drawing, p: &Phases) {
    project::set(d, KEY, &p.to_json());
}

/// A phase name in a command's parameters: absent means "not given", null means "clear".
enum Given {
    Absent,
    Clear,
    Name(String),
}

fn given(cmd: &str, p: &Value, key: &str, list: &[String]) -> Result<Given> {
    match p.get(key) {
        None => Ok(Given::Absent),
        Some(Value::Null) => Ok(Given::Clear),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(Given::Clear),
        Some(Value::String(s)) => Phases::canonical(list, s.trim()).map(|n| Given::Name(n.to_string())).ok_or_else(|| {
            bad(cmd, format!("`{key}`: `{}` is not a phase of this project; add it with `takeoff.phases` ({})", s.trim(), list.join(", ")))
        }),
        Some(_) => Err(bad(cmd, format!("`{key}` must be a phase name or null"))),
    }
}

/// The estimation and running-bill phase for something being drawn or tagged: the ones the call names, else the
/// current ones. Names outside the lists are refused.
pub(crate) fn for_new_mark(cmd: &str, d: &Drawing, p: &Value) -> Result<(Option<String>, Option<String>)> {
    let ph = phases_of(d);
    let est = match given(cmd, p, "phase", &ph.estimation)? {
        Given::Absent => ph.current_estimation.clone(),
        Given::Clear => None,
        Given::Name(n) => Some(n),
    };
    let bill = match given(cmd, p, "billPhase", &ph.billing)? {
        Given::Absent => ph.current_billing.clone(),
        Given::Clear => None,
        Given::Name(n) => Some(n),
    };
    Ok((est, bill))
}

/// The lists and the current choice as the shell shows them: `{estimation, billing, current: {estimation, billing}}`.
pub fn phases_json(d: &Drawing) -> Value {
    phases_of(d).to_json()
}

/// Write the phases of the mark on `h` (if it has one) into an exported row.
pub(crate) fn stamp_row(d: &Drawing, h: Option<Handle>, row: &mut Value) {
    let Some(mark) = h.and_then(|h| d.entity(h)).and_then(|e| e.common.takeoff.as_ref()) else { return };
    if let Some(n) = &mark.phase {
        row["phase"] = json!(n);
    }
    if let Some(n) = &mark.bill_phase {
        row["bill_phase"] = json!(n);
    }
}

fn run_phases(s: &mut Session, p: &Value) -> Result<Value> {
    let mut ph = phases_of(s.doc()?);
    let list = |key: &str| -> Result<Option<Vec<String>>> {
        match p.get(key) {
            None => Ok(None),
            Some(Value::Array(a)) => {
                if a.iter().any(|v| !v.is_string()) {
                    return Err(bad("takeoff.phases", format!("`{key}` must be a list of names")));
                }
                let out = clean(a.iter().filter_map(Value::as_str));
                if out.is_empty() {
                    return Err(bad("takeoff.phases", format!("`{key}` needs at least one name")));
                }
                Ok(Some(out))
            }
            Some(_) => Err(bad("takeoff.phases", format!("`{key}` must be a list of names"))),
        }
    };
    let (est, bill) = (list("estimation")?, list("billing")?);
    let changes = est.is_some() || bill.is_some() || p.get("current").is_some();
    if let Some(e) = est {
        ph.estimation = e;
    }
    if let Some(b) = bill {
        ph.billing = b;
    }
    // Dimensions already measured keep the name they have; a rename is a new phase.
    ph.current_estimation = ph.current_estimation.as_deref().and_then(|n| Phases::canonical(&ph.estimation, n)).map(str::to_string);
    ph.current_billing = ph.current_billing.as_deref().and_then(|n| Phases::canonical(&ph.billing, n)).map(str::to_string);
    if let Some(cur) = p.get("current") {
        if !cur.is_object() {
            return Err(bad("takeoff.phases", "`current` must be {estimation?, billing?}"));
        }
        match given("takeoff.phases", cur, "estimation", &ph.estimation)? {
            Given::Absent => {}
            Given::Clear => ph.current_estimation = None,
            Given::Name(n) => ph.current_estimation = Some(n),
        }
        match given("takeoff.phases", cur, "billing", &ph.billing)? {
            Given::Absent => {}
            Given::Clear => ph.current_billing = None,
            Given::Name(n) => ph.current_billing = Some(n),
        }
    }
    if changes {
        store(s.doc_mut()?, &ph);
    }
    Ok(ph.to_json())
}

fn run_phase(s: &mut Session, p: &Value) -> Result<Value> {
    let ph = phases_of(s.doc()?);
    let est = given("takeoff.phase", p, "estimation", &ph.estimation)?;
    let bill = given("takeoff.phase", p, "billing", &ph.billing)?;
    if matches!((&est, &bill), (Given::Absent, Given::Absent)) {
        return Err(bad("takeoff.phase", "give `estimation`, `billing` or both (null clears)"));
    }
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("takeoff.phase", "nothing selected"));
    }
    let (mut changed, mut skipped) = (0usize, 0usize);
    for h in hs {
        let marked = s.doc()?.entity(h).is_some_and(|e| e.common.takeoff.is_some());
        if !marked {
            skipped += 1;
            continue;
        }
        let r = s.doc_mut()?.modify_entity(h, |e| {
            if let Some(m) = e.common.takeoff.as_mut() {
                match &est {
                    Given::Absent => {}
                    Given::Clear => m.phase = None,
                    Given::Name(n) => m.phase = Some(n.clone()),
                }
                match &bill {
                    Given::Absent => {}
                    Given::Clear => m.bill_phase = None,
                    Given::Name(n) => m.bill_phase = Some(n.clone()),
                }
            }
        });
        if r.is_ok() {
            changed += 1;
        } else {
            skipped += 1;
        }
    }
    super::takeoff::sync_meta(s.doc_mut()?);
    Ok(json!({ "changed": changed, "skipped": skipped }))
}
