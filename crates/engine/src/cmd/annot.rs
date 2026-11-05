//! Units, plot configuration and annotation layout commands (see `crate::annot`).
//!
//! * `drawing.units`: change the drawing unit, as metadata only or by scaling geometry (the mode
//!   must be chosen; nothing is ever scaled silently).
//! * `display.units`: how dimension text shows measurements; no geometry changes.
//! * `plot.config` / `plot.validate`: paper size and plot scale, annotation sizes from the printed
//!   size, and a check of the sheet.
//! * `annotation.collisions` / `annotation.layout` / `annotation.lock`: find and resolve text
//!   conflicts; only text moves, as one undo step.

use aad_doc::{Group, Handle};
use serde_json::{Value, json};

use super::*;
use crate::annot::{self, LayoutParams, Paper, PlotScale, PrintedSizes, Unit};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("drawing.units", "Drawing Units", run_drawing_units)
            .params("{units: mm|cm|m|in|ft, mode: \"metadata\" (change only the unit setting) | \"scale\" (rescale every model-space object)} → {units, mode, scaled?, factor?, warning?}"),
        CommandSpec::new("display.units", "Display Units", run_display_units)
            .params("{units: mm|cm|m|in|ft|ftin} → {display, drawing, factor} (dimension text only; geometry is not changed)"),
        CommandSpec::new("plot.config", "Plot Configuration", run_plot_config)
            .params("{scale?: \"1:100\" | 100 (kept when omitted and already set), paper: A0..A4 | [width, height] mm, textMm?: (default: ISO medium for the paper), arrowMm?: (default: ISO arrow for the paper), apply?: bool (default true)} → {sizes, printedMm}"),
        CommandSpec::new("plot.validate", "Validate Plot Layout", run_plot_validate)
            .params("{marginMm?: 10, minTextMm?: 1.8, maxTextMm?: 12} → {fits, extentMm, printableMm, warnings}")
            .noundo(),
        CommandSpec::new("annotation.collisions", "Annotation Collisions", run_collisions)
            .params("{clearance?: drawing units (default 0.5 mm printed)} → {count, conflicts: [{text, other, kind}]}")
            .noundo(),
        CommandSpec::new("annotation.layout", "Annotation Layout", run_layout)
            .params("{mode: \"preview\" | \"apply\", clearance?, maxSteps?: 12, includeManual?: bool} → {moves, unresolved, skippedManual}"),
        CommandSpec::new("annotation.lock", "Lock Annotation Position", run_lock)
            .params("{handles?, manual?: bool (default true)} (marks text as placed by hand: automatic layout leaves it alone)"),
    ]
}

fn drawn_unit(s: &Session, cmd: &str) -> Result<Unit> {
    annot::drawing_unit(s.doc()?).ok_or_else(|| bad(cmd, "the drawing's units are not set: set them first with `drawing.units` (mode metadata)"))
}

pub(super) fn run_drawing_units(s: &mut Session, p: &Value) -> Result<Value> {
    let new = str_param(p, "units")
        .and_then(Unit::parse)
        .and_then(|u| u.insunits().map(|_| u))
        .ok_or_else(|| bad("drawing.units", "`units` must be mm, cm, m, in or ft"))?;
    let mode = str_param(p, "mode").ok_or_else(|| {
        bad("drawing.units", "`mode` is required: \"metadata\" changes only the unit setting, \"scale\" rescales every model-space object")
    })?;
    let old = annot::drawing_unit(s.doc()?);
    let mut r = json!({ "units": new.name(), "mode": mode });
    match mode {
        "metadata" => {
            // Same unit already set: skip copy-on-write (shell chrome re-applies this often).
            if old == Some(new) {
                return Ok(r);
            }
            if old.is_some() && !s.doc()?.model.is_empty() {
                r["warning"] = json!("the unit setting changed but no geometry was scaled: existing objects now read as the new unit");
            }
        }
        "scale" => {
            let old = old.ok_or_else(|| {
                bad("drawing.units", "the current units are not set, so there is nothing to convert from: use mode \"metadata\" first")
            })?;
            let factor = old.metres() / new.metres();
            let hs: Vec<String> = s.doc()?.model.iter().map(|e| e.handle.hex()).collect();
            if !hs.is_empty() && (factor - 1.0).abs() > 1e-12 {
                let spec = find_command("scale").ok_or_else(|| bad("drawing.units", "no scale command"))?;
                (spec.run)(s, &json!({ "handles": hs, "base": [0, 0], "factor": factor }))?;
            }
            r["scaled"] = json!(hs.len());
            r["factor"] = json!(factor);
        }
        _ => return Err(bad("drawing.units", "`mode` must be \"metadata\" or \"scale\"")),
    }
    let d = s.doc_mut()?;
    if let Some(code) = new.insunits() {
        d.header.set_i64("INSUNITS", code);
    }
    // Dimension text follows the display unit, which is measured against the new drawing unit.
    if let Some(shown) = annot::display_unit(d).filter(|_| !d.header.str("AADDISPUNITS", "").is_empty()) {
        annot::apply_display_unit(d, new, shown);
    }
    Ok(r)
}

fn run_display_units(s: &mut Session, p: &Value) -> Result<Value> {
    let shown = str_param(p, "units").and_then(Unit::parse).ok_or_else(|| bad("display.units", "`units` must be mm, cm, m, in, ft or ftin"))?;
    let drawn = drawn_unit(s, "display.units")?;
    annot::apply_display_unit(s.doc_mut()?, drawn, shown);
    Ok(json!({ "display": shown.name(), "drawing": drawn.name(), "factor": drawn.metres() / shown.metres() }))
}

fn paper_of(p: &Value, cmd: &str) -> Result<Paper> {
    match p.get("paper") {
        Some(Value::String(n)) => Paper::named(n).ok_or_else(|| bad(cmd, "`paper` must be A0, A1, A2, A3, A4 or [width, height] in mm")),
        Some(Value::Array(a)) if a.len() == 2 => {
            let (w, h) = (a.first().and_then(Value::as_f64), a.get(1).and_then(Value::as_f64));
            w.zip(h)
                .and_then(|(w, h)| Paper::custom(w, h))
                .ok_or_else(|| bad(cmd, "custom paper must be [width, height] in mm, between 50 and 20000"))
        }
        _ => Err(bad(cmd, "`paper` is required")),
    }
}

fn run_plot_config(s: &mut Session, p: &Value) -> Result<Value> {
    let scale = match p.get("scale") {
        Some(Value::String(t)) => PlotScale::parse(t),
        Some(v) => v.as_f64().map(PlotScale).filter(|c| c.0 > 0.0 && c.0.is_finite()),
        None => configured(s).map(|(sc, _)| sc),
    }
    .ok_or_else(|| bad("plot.config", "`scale` must be like \"1:100\" or a denominator"))?;
    let paper = paper_of(p, "plot.config")?;
    let iso = annot::paper_iso_name(paper).unwrap_or("");
    let rank = annot::text_rank_for_paper(if iso.is_empty() { "A1" } else { iso });
    let printed = PrintedSizes { text_mm: f64_or(p, "textMm", rank.medium), arrow_mm: f64_or(p, "arrowMm", rank.arrow) };
    if !(printed.text_mm.is_finite() && printed.text_mm > 0.0 && printed.arrow_mm.is_finite() && printed.arrow_mm > 0.0) {
        return Err(bad("plot.config", "`textMm` and `arrowMm` must be positive"));
    }
    let drawn = drawn_unit(s, "plot.config")?;
    let sizes = annot::sizes(printed, scale, drawn);
    if bool_or(p, "apply", true) {
        let d = s.doc_mut()?;
        annot::apply_sizes(d, sizes);
        d.header.set_f64("AADPLOTSCALE", scale.0);
        d.header.set_f64("AADPAPERW", paper.width);
        d.header.set_f64("AADPAPERH", paper.height);
        if !iso.is_empty() {
            d.header.set_str("AADOBER", iso);
        }
        d.header.set_f64("AADTEXTS", rank.small);
        d.header.set_f64("AADTEXTM", rank.medium);
        d.header.set_f64("AADTEXTL", rank.large);
    }
    Ok(json!({
        "scale": scale.label(),
        "paper": if iso.is_empty() { Value::Null } else { json!(iso) },
        "paperMm": [paper.width, paper.height],
        "drawingUnit": drawn.name(),
        "printedMm": { "small": rank.small, "medium": rank.medium, "large": rank.large, "arrow": rank.arrow, "text": printed.text_mm },
        "sizes": {
            "textHeight": sizes.text_height, "arrowSize": sizes.arrow_size, "extOffset": sizes.ext_offset,
            "extExtend": sizes.ext_extend, "textGap": sizes.text_gap, "baselineSpacing": sizes.baseline_spacing,
        },
        "applied": bool_or(p, "apply", true),
    }))
}

fn configured(s: &Session) -> Option<(PlotScale, Paper)> {
    let h = &s.doc().ok()?.header;
    let (sc, w, hh) = (h.f64("AADPLOTSCALE", 0.0), h.f64("AADPAPERW", 0.0), h.f64("AADPAPERH", 0.0));
    (sc > 0.0 && w > 0.0 && hh > 0.0).then_some((PlotScale(sc), Paper { width: w, height: hh }))
}

fn run_plot_validate(s: &mut Session, p: &Value) -> Result<Value> {
    let (scale, paper) = configured(s).ok_or_else(|| bad("plot.validate", "no plot configuration: run `plot.config` first"))?;
    let drawn = drawn_unit(s, "plot.validate")?;
    let margin = f64_or(p, "marginMm", 10.0);
    let (min_t, max_t) = (f64_or(p, "minTextMm", 1.8), f64_or(p, "maxTextMm", 12.0));
    let k = scale.du_per_paper_mm(drawn);
    let d = s.doc()?;
    let ext = d.extents(&aad_doc::Space::Model);
    let (ew, eh) = if ext.is_empty() { (0.0, 0.0) } else { (ext.width() / k, ext.height() / k) };
    let (pw, ph) = paper.printable(margin);
    let mut warnings = Vec::new();
    let fits = ew <= pw + 1e-9 && eh <= ph + 1e-9;
    if !fits {
        let need = (ew / pw.max(1e-9)).max(eh / ph.max(1e-9)) * scale.0;
        let nearest = annot::nearest_plot_scale(1.0 / need.max(1e-12));
        warnings.push(format!(
            "the drawing is {ew:.0} x {eh:.0} mm at {}, larger than the printable {pw:.0} x {ph:.0} mm; nearest scale that fits is {}",
            scale.label(),
            nearest.nearest
        ));
    }
    let mut small = Vec::new();
    let mut large = Vec::new();
    for e in d.model.iter() {
        let h = match &e.kind {
            aad_doc::EntityKind::Text(t) => t.height,
            aad_doc::EntityKind::MText(t) => t.height,
            _ => continue,
        };
        let mm = h / k;
        if mm < min_t - 1e-9 {
            small.push(e.handle.hex());
        } else if mm > max_t + 1e-9 {
            large.push(e.handle.hex());
        }
    }
    if !small.is_empty() {
        warnings.push(format!("{} text object(s) print smaller than {min_t} mm", small.len()));
    }
    if !large.is_empty() {
        warnings.push(format!("{} text object(s) print larger than {max_t} mm", large.len()));
    }
    let conflicts = annot::conflicts(d, &aad_doc::Space::Model, 0.0).len();
    if conflicts > 0 {
        warnings.push(format!("{conflicts} annotation conflict(s): run `annotation.collisions`"));
    }
    small.truncate(50);
    large.truncate(50);
    Ok(json!({
        "scale": scale.label(), "paperMm": [paper.width, paper.height], "extentMm": [ew, eh], "printableMm": [pw, ph],
        "fits": fits, "smallText": small, "largeText": large, "conflicts": conflicts, "warnings": warnings,
    }))
}

/// The default clearance: half a printed millimetre when a plot scale is set, else none.
fn default_clearance(s: &Session) -> f64 {
    match (configured(s), annot::drawing_unit(s.doc().ok().unwrap_or(&aad_doc::Drawing::new_imperial()))) {
        (Some((scale, _)), Some(u)) => 0.5 * scale.du_per_paper_mm(u),
        _ => 0.0,
    }
}

fn run_collisions(s: &mut Session, p: &Value) -> Result<Value> {
    let clearance = f64_or(p, "clearance", default_clearance(s));
    if !(clearance.is_finite() && clearance >= 0.0) {
        return Err(bad("annotation.collisions", "`clearance` must be zero or positive"));
    }
    let all = annot::conflicts(s.doc()?, &s.space(), clearance);
    let list: Vec<Value> = all.iter().take(500).map(|c| json!({ "text": c.text.hex(), "other": c.other.hex(), "kind": c.kind })).collect();
    Ok(json!({ "count": all.len(), "clearance": clearance, "conflicts": list, "truncated": all.len() > 500 }))
}

fn run_layout(s: &mut Session, p: &Value) -> Result<Value> {
    let apply = match str_param(p, "mode") {
        Some("preview") => false,
        Some("apply") => true,
        _ => return Err(bad("annotation.layout", "`mode` must be \"preview\" or \"apply\"")),
    };
    let clearance = f64_or(p, "clearance", default_clearance(s));
    let max_steps = p.get("maxSteps").and_then(Value::as_u64).unwrap_or(12).clamp(1, 100) as u32;
    if !(clearance.is_finite() && clearance >= 0.0) {
        return Err(bad("annotation.layout", "`clearance` must be zero or positive"));
    }
    let params = LayoutParams { clearance, max_steps, include_manual: bool_or(p, "includeManual", false) };
    let space = s.space();
    let layout = annot::propose(s.doc()?, &space, params);
    let mut moved = Vec::new();
    if apply {
        let locked: std::collections::HashSet<String> = s.doc()?.layers.iter().filter(|l| l.locked).map(|l| l.name.clone()).collect();
        let d = s.doc_mut()?;
        for m in &layout.moves {
            let mut ok = false;
            d.modify_entity(m.handle, |e| {
                if !locked.contains(&e.common.layer) {
                    annot::translate_text(e, m.dx, m.dy);
                    ok = true;
                }
            })?;
            if ok {
                moved.push(m.handle);
            }
        }
    }
    let moves: Vec<Value> = layout.moves.iter().map(|m| json!({ "handle": m.handle.hex(), "dx": m.dx, "dy": m.dy })).collect();
    Ok(json!({
        "mode": if apply { "apply" } else { "preview" },
        "moves": moves,
        "applied": moved.len(),
        "unresolved": layout.unresolved.iter().map(|h| h.hex()).collect::<Vec<_>>(),
        "skippedManual": layout.skipped_manual.iter().map(|h| h.hex()).collect::<Vec<_>>(),
    }))
}

fn run_lock(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("annotation.lock", "nothing selected"));
    }
    let manual = bool_or(p, "manual", true);
    let d = s.doc_mut()?;
    let pos = d.groups.iter().position(|g| g.name.eq_ignore_ascii_case(annot::MANUAL_GROUP));
    let i = match pos {
        Some(i) => i,
        None => {
            d.groups.push(Group {
                name: annot::MANUAL_GROUP.into(),
                description: "Annotation placed by hand".into(),
                selectable: false,
                members: Vec::new(),
                child_groups: Vec::new(),
            });
            d.groups.len() - 1
        }
    };
    let Some(group) = d.groups.get_mut(i) else { return Err(bad("annotation.lock", "group missing")) };
    let members: &mut Vec<Handle> = &mut group.members;
    for h in &hs {
        members.retain(|m| m != h);
        if manual {
            members.push(*h);
        }
    }
    Ok(json!({ "manual": manual, "count": hs.len() }))
}
