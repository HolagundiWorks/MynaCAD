//! IS 456:2000 checks for the reinforced concrete smart blocks (`assets/smartblocks/rcc.json`).
//!
//! The blocks are drafting aids, so a check never refuses a value: it reads the block's parameters
//! (millimetres, grades as numbers) and reports the quantities the code limits (steel area and
//! percentage, cover, spacing, tie and stirrup rules) and every limit the parameters break, each
//! with its clause. The Properties panel shows the result under the block's parameters
//! (`cmd::smartblock::property_fields`) and `smartblock.check` returns it as data.
//!
//! Clauses used (IS 456:2000): 26.3.2 (clear spacing of bars), 26.3.3 (maximum bar spacing in
//! slabs), 26.4.2 and Table 16 (nominal cover), 26.5.1 (beams), 26.5.2 (slabs), 26.5.3
//! (columns), 34.1 (footings). Table 16 gives the cover for the exposure of Table 3, with the
//! 40 mm minimum for columns and 50 mm for footings of 26.4.2.1 and 26.4.2.2.

use std::collections::BTreeMap;
use std::f64::consts::PI;

/// A quantity the code limits, as a label and a text value.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub label: String,
    pub value: String,
}

/// A limit the parameters break.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub clause: &'static str,
    pub message: String,
}

/// The outcome of checking one block.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub lines: Vec<Line>,
    pub issues: Vec<Issue>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }

    fn line(&mut self, label: &str, value: String) {
        self.lines.push(Line { label: label.to_string(), value });
    }

    fn issue(&mut self, clause: &'static str, message: String) {
        self.issues.push(Issue { clause, message });
    }
}

/// Whether `id` is a block this module checks.
pub fn applies(id: &str) -> bool {
    matches!(id, "rcc.column.rect" | "rcc.column.circular" | "rcc.beam.section" | "rcc.slab.section" | "rcc.footing.section" | "rcc.lintel.section")
}

/// Check the block `id` with its parameter values (`None` for a block this module does not check).
pub fn check(id: &str, v: &BTreeMap<String, f64>) -> Option<Report> {
    let mut r = Report::default();
    match id {
        "rcc.column.rect" => column_rect(v, &mut r),
        "rcc.column.circular" => column_circular(v, &mut r),
        "rcc.beam.section" => beam(v, &mut r),
        "rcc.slab.section" => slab(v, &mut r),
        "rcc.footing.section" => footing(v, &mut r),
        "rcc.lintel.section" => lintel(v, &mut r),
        _ => return None,
    }
    Some(r)
}

/// Nominal cover for the exposure (IS 456 Table 16): 1 mild 20, 2 moderate 30, 3 severe 45,
/// 4 very severe 50, 5 extreme 75 mm.
pub fn exposure_cover(exposure: f64) -> f64 {
    match exposure.round() as i64 {
        i64::MIN..=1 => 20.0,
        2 => 30.0,
        3 => 45.0,
        4 => 50.0,
        _ => 75.0,
    }
}

fn exposure_name(exposure: f64) -> &'static str {
    match exposure.round() as i64 {
        i64::MIN..=1 => "mild",
        2 => "moderate",
        3 => "severe",
        4 => "very severe",
        _ => "extreme",
    }
}

/// Area of one bar of diameter `d`, mm².
pub fn bar_area(d: f64) -> f64 {
    PI / 4.0 * d * d
}

fn get(v: &BTreeMap<String, f64>, k: &str) -> f64 {
    v.get(k).copied().unwrap_or(0.0)
}

fn n(x: f64) -> String {
    let r = (x * 100.0).round() / 100.0;
    if r.fract() == 0.0 { format!("{}", r as i64) } else { format!("{r}") }
}

/// Minimum percentage of steel in a slab or footing of the grade (cl. 26.5.2.1): 0.15 % for mild
/// steel (Fe250), 0.12 % for high-strength bars.
fn slab_min_pct(fy: f64) -> f64 {
    if fy < 415.0 { 0.15 } else { 0.12 }
}

fn cover_check(r: &mut Report, cover: f64, required: f64, exposure: f64, why: &str) {
    r.line("Cover", format!("{} mm (required {} mm, {} exposure)", n(cover), n(required), exposure_name(exposure)));
    if cover + 1e-9 < required {
        r.issue("26.4.2, Table 16", format!("Clear cover {} mm is below the {} mm required {}", n(cover), n(required), why));
    }
}

/// Clear spacing between `count` bars of diameter `bar` laid across `room` mm (the width between the
/// inside faces of the links); `None` for a single bar.
fn clear_gap(room: f64, count: f64, bar: f64) -> Option<f64> {
    if count < 2.0 { None } else { Some((room - count * bar) / (count - 1.0)) }
}

fn gap_check(r: &mut Report, what: &str, gap: Option<f64>, bar: f64) {
    let Some(gap) = gap else { return };
    let least = bar.max(25.0);
    if gap + 1e-9 < least {
        r.issue("26.3.2", format!("Clear spacing of {what} is {} mm, below the {} mm minimum (the bar diameter or 25 mm)", n(gap), n(least)));
    }
}

fn column_rect(v: &BTreeMap<String, f64>, r: &mut Report) {
    let (b, d, cover) = (get(v, "width"), get(v, "depth"), get(v, "cover"));
    let (nx, ny) = (get(v, "nx"), get(v, "ny"));
    let (bar, tie, tiesp) = (get(v, "bar"), get(v, "tie"), get(v, "tiesp"));
    // The four corner bars are `bar`; the others are `bar2` (the same size when it is not given).
    let bar2 = if get(v, "bar2") > 0.0 { get(v, "bar2") } else { bar };
    let (largest, smallest) = (bar.max(bar2), bar.min(bar2));
    let nbars = 2.0 * nx + 2.0 * (ny - 2.0);
    let ast = 4.0 * bar_area(bar) + (nbars - 4.0).max(0.0) * bar_area(bar2);
    let ag = b * d;
    let pct = if ag > 0.0 { ast / ag * 100.0 } else { 0.0 };
    r.line(
        "Longitudinal bars",
        if bar2 == bar {
            format!("{}-{} dia, Ast {} mm² ({} % of the section)", n(nbars), n(bar), n(ast.round()), n(pct))
        } else {
            format!(
                "4-{} dia at the corners and {}-{} dia, Ast {} mm² ({} % of the section)",
                n(bar),
                n(nbars - 4.0),
                n(bar2),
                n(ast.round()),
                n(pct)
            )
        },
    );
    r.line("Steel limits", "0.8 % to 6 % of the gross section".to_string());
    if pct < 0.8 {
        r.issue("26.5.3.1(a)", format!("Steel is {} %, below the 0.8 % minimum; add bars or a larger diameter", n(pct)));
    } else if pct > 6.0 {
        r.issue("26.5.3.1(a)", format!("Steel is {} %, above the 6 % maximum", n(pct)));
    }
    if smallest < 12.0 {
        r.issue("26.5.3.1(c)", format!("Longitudinal bars are {} mm; the minimum is 12 mm", n(smallest)));
    }
    if nbars < 4.0 {
        r.issue("26.5.3.1(b)", "A rectangular column needs at least 4 longitudinal bars".to_string());
    }
    cover_check(
        r,
        cover,
        exposure_cover(get(v, "exposure")).max(40.0).max(largest),
        get(v, "exposure"),
        "(40 mm or the bar diameter, and the exposure)",
    );
    let tie_min = (largest / 4.0).max(6.0);
    r.line(
        "Ties",
        format!(
            "{} dia at {} mm c/c (minimum dia {}; maximum spacing {})",
            n(tie),
            n(tiesp),
            n(tie_min),
            n(b.min(d).min(16.0 * smallest).min(300.0))
        ),
    );
    if tie + 1e-9 < tie_min {
        r.issue("26.5.3.2(c)(2)", format!("Ties are {} mm; the minimum is a quarter of the largest bar or 6 mm, {} mm", n(tie), n(tie_min)));
    }
    let tie_max = b.min(d).min(16.0 * smallest).min(300.0);
    if tiesp > tie_max + 1e-9 {
        r.issue(
            "26.5.3.2(c)(1)",
            format!("Tie spacing {} mm exceeds {} mm (least of the smaller side, 16 bar diameters and 300 mm)", n(tiesp), n(tie_max)),
        );
    }
    let inner = cover + tie;
    let centres_x = if nx > 1.0 { (b - 2.0 * inner - bar) / (nx - 1.0) } else { 0.0 };
    let centres_y = if ny > 1.0 { (d - 2.0 * inner - bar) / (ny - 1.0) } else { 0.0 };
    gap_check(r, "the bars on the width face", clear_gap(b - 2.0 * inner, nx, largest), largest);
    gap_check(r, "the bars on the depth face", clear_gap(d - 2.0 * inner, ny, largest), largest);
    if centres_x.max(centres_y) > 300.0 + 1e-9 {
        r.issue("26.5.3.1(h)", format!("Bars are {} mm apart along a face; the maximum is 300 mm centre to centre", n(centres_x.max(centres_y))));
    }
}

fn column_circular(v: &BTreeMap<String, f64>, r: &mut Report) {
    let (dia, cover, bars) = (get(v, "dia"), get(v, "cover"), get(v, "bars"));
    let (bar, tie, tiesp) = (get(v, "bar"), get(v, "tie"), get(v, "tiesp"));
    let ast = bars * bar_area(bar);
    let ag = PI / 4.0 * dia * dia;
    let pct = if ag > 0.0 { ast / ag * 100.0 } else { 0.0 };
    r.line("Longitudinal bars", format!("{}-{} dia, Ast {} mm² ({} % of the section)", n(bars), n(bar), n(ast.round()), n(pct)));
    r.line("Steel limits", "0.8 % to 6 % of the gross section".to_string());
    if pct < 0.8 {
        r.issue("26.5.3.1(a)", format!("Steel is {} %, below the 0.8 % minimum; add bars or a larger diameter", n(pct)));
    } else if pct > 6.0 {
        r.issue("26.5.3.1(a)", format!("Steel is {} %, above the 6 % maximum", n(pct)));
    }
    if bar < 12.0 {
        r.issue("26.5.3.1(c)", format!("Longitudinal bars are {} mm; the minimum is 12 mm", n(bar)));
    }
    if bars < 6.0 {
        r.issue("26.5.3.1(b)", "A circular column needs at least 6 longitudinal bars".to_string());
    }
    cover_check(r, cover, exposure_cover(get(v, "exposure")).max(40.0).max(bar), get(v, "exposure"), "(40 mm or the bar diameter, and the exposure)");
    let tie_min = (bar / 4.0).max(6.0);
    let tie_max = dia.min(16.0 * bar).min(300.0);
    r.line("Ties", format!("{} dia at {} mm c/c (minimum dia {}; maximum spacing {})", n(tie), n(tiesp), n(tie_min), n(tie_max)));
    if tie + 1e-9 < tie_min {
        r.issue("26.5.3.2(c)(2)", format!("Ties are {} mm; the minimum is a quarter of the largest bar or 6 mm, {} mm", n(tie), n(tie_min)));
    }
    if tiesp > tie_max + 1e-9 {
        r.issue(
            "26.5.3.2(c)(1)",
            format!("Tie spacing {} mm exceeds {} mm (least of the diameter, 16 bar diameters and 300 mm)", n(tiesp), n(tie_max)),
        );
    }
    if bars >= 1.0 {
        let ring = dia - 2.0 * (cover + tie) - bar;
        let pitch = PI * ring / bars;
        gap_check(r, "the bars around the section", Some(pitch - bar), bar);
        if pitch > 300.0 + 1e-9 {
            r.issue("26.5.3.1(h)", format!("Bars are {} mm apart round the section; the maximum is 300 mm centre to centre", n(pitch)));
        }
    }
}

fn beam(v: &BTreeMap<String, f64>, r: &mut Report) {
    let (b, depth, cover) = (get(v, "width"), get(v, "depth"), get(v, "cover"));
    let (ntop, nbot) = (get(v, "ntop"), get(v, "nbot"));
    let (bartop, barbot) = (get(v, "bartop"), get(v, "barbot"));
    let (stir, stsp, fy) = (get(v, "stir"), get(v, "stsp"), get(v, "fy"));
    let d = depth - cover - stir - barbot / 2.0;
    let ast = nbot * bar_area(barbot);
    let asc = ntop * bar_area(bartop);
    let ast_min = 0.85 * b * d / fy.max(1.0);
    let ast_max = 0.04 * b * depth;
    r.line("Effective depth", format!("{} mm", n(d.round())));
    r.line(
        "Tension steel",
        format!("{}-{} dia, Ast {} mm² (minimum {}, maximum {})", n(nbot), n(barbot), n(ast.round()), n(ast_min.round()), n(ast_max.round())),
    );
    r.line("Top steel", format!("{}-{} dia, {} mm²", n(ntop), n(bartop), n(asc.round())));
    if ast + 1e-9 < ast_min {
        r.issue("26.5.1.1(a)", format!("Tension steel {} mm² is below the minimum 0.85 b d / fy = {} mm²", n(ast.round()), n(ast_min.round())));
    }
    if ast > ast_max + 1e-9 || asc > ast_max + 1e-9 {
        r.issue("26.5.1.1(b)", format!("Tension or compression steel exceeds 4 % of the section ({} mm²)", n(ast_max.round())));
    }
    let inner = cover + stir;
    gap_check(r, "the bottom bars", clear_gap(b - 2.0 * inner, nbot, barbot), barbot);
    gap_check(r, "the top bars", clear_gap(b - 2.0 * inner, ntop, bartop), bartop);
    cover_check(r, cover, exposure_cover(get(v, "exposure")).max(barbot).max(bartop), get(v, "exposure"), "(the exposure, or the bar diameter)");
    let sv_max_shear = if b > 0.0 { 2.0 * bar_area(stir) * 0.87 * fy / (0.4 * b) } else { f64::MAX };
    let sv_max = (0.75 * d).min(300.0).min(sv_max_shear);
    r.line("Stirrups", format!("2-legged {} dia at {} mm c/c (maximum spacing {})", n(stir), n(stsp), n(sv_max.floor())));
    if stsp > (0.75 * d).min(300.0) + 1e-9 {
        r.issue("26.5.1.5", format!("Stirrup spacing {} mm exceeds {} mm (0.75 d or 300 mm)", n(stsp), n((0.75 * d).min(300.0).floor())));
    }
    if stsp > sv_max_shear + 1e-9 {
        r.issue(
            "26.5.1.6",
            format!(
                "Stirrup spacing {} mm is above {} mm, the most that gives the minimum shear steel Asv/(b sv) = 0.4/(0.87 fy)",
                n(stsp),
                n(sv_max_shear.floor())
            ),
        );
    }
    if depth > 750.0 {
        r.issue("26.5.1.3", "Beams deeper than 750 mm need side-face reinforcement of 0.1 % of the web area, spaced at most 300 mm".to_string());
    }
}

fn slab(v: &BTreeMap<String, f64>, r: &mut Report) {
    let (thick, cover, bar, dist) = (get(v, "thick"), get(v, "cover"), get(v, "bar"), get(v, "dist"));
    let (spacing, distsp, fy) = (get(v, "spacing"), get(v, "distsp"), get(v, "fy"));
    let d = thick - cover - bar / 2.0;
    let pct = slab_min_pct(fy);
    let ast_min = pct / 100.0 * 1000.0 * thick;
    let ast = if spacing > 0.0 { bar_area(bar) * 1000.0 / spacing } else { 0.0 };
    let ast_dist = if distsp > 0.0 { bar_area(dist) * 1000.0 / distsp } else { 0.0 };
    r.line("Effective depth", format!("{} mm", n(d.round())));
    r.line("Main steel", format!("{} dia at {} mm c/c, {} mm² per metre (minimum {})", n(bar), n(spacing), n(ast.round()), n(ast_min.round())));
    r.line("Distribution steel", format!("{} dia at {} mm c/c, {} mm² per metre", n(dist), n(distsp), n(ast_dist.round())));
    if ast + 1e-9 < ast_min {
        r.issue(
            "26.5.2.1",
            format!("Main steel {} mm² per metre is below the minimum {} % of the section, {} mm²", n(ast.round()), n(pct), n(ast_min.round())),
        );
    }
    if ast_dist + 1e-9 < ast_min {
        r.issue(
            "26.5.2.1",
            format!(
                "Distribution steel {} mm² per metre is below the minimum {} % of the section, {} mm²",
                n(ast_dist.round()),
                n(pct),
                n(ast_min.round())
            ),
        );
    }
    let main_max = (3.0 * d).min(300.0);
    let dist_max = (5.0 * d).min(450.0);
    if spacing > main_max + 1e-9 {
        r.issue("26.3.3(b)", format!("Main bar spacing {} mm exceeds {} mm (3 d or 300 mm)", n(spacing), n(main_max.floor())));
    }
    if distsp > dist_max + 1e-9 {
        r.issue("26.3.3(b)", format!("Distribution bar spacing {} mm exceeds {} mm (5 d or 450 mm)", n(distsp), n(dist_max.floor())));
    }
    if bar > thick / 8.0 + 1e-9 {
        r.issue(
            "26.5.2.2",
            format!("Bar diameter {} mm exceeds an eighth of the slab thickness ({} mm)", n(bar), n((thick / 8.0 * 10.0).floor() / 10.0)),
        );
    }
    cover_check(r, cover, exposure_cover(get(v, "exposure")).max(bar), get(v, "exposure"), "(the exposure, or the bar diameter)");
}

fn footing(v: &BTreeMap<String, f64>, r: &mut Report) {
    let (depth, edge, cover) = (get(v, "depth"), get(v, "edge"), get(v, "cover"));
    let (bar, spacing, fy) = (get(v, "bar"), get(v, "spacing"), get(v, "fy"));
    let d = depth - cover - bar;
    let pct = slab_min_pct(fy);
    let ast_min = pct / 100.0 * 1000.0 * depth;
    let ast = if spacing > 0.0 { bar_area(bar) * 1000.0 / spacing } else { 0.0 };
    r.line("Effective depth", format!("{} mm", n(d.round())));
    r.line("Mesh", format!("{} dia at {} mm c/c each way, {} mm² per metre (minimum {})", n(bar), n(spacing), n(ast.round()), n(ast_min.round())));
    r.line("Edge thickness", format!("{} mm (minimum 150 mm on soil)", n(edge)));
    if edge + 1e-9 < 150.0 {
        r.issue("34.1.2", format!("Thickness at the edge is {} mm; the minimum on soil is 150 mm", n(edge)));
    }
    if ast + 1e-9 < ast_min {
        r.issue(
            "26.5.2.1",
            format!("Mesh steel {} mm² per metre is below the minimum {} % of the section, {} mm²", n(ast.round()), n(pct), n(ast_min.round())),
        );
    }
    let max = (3.0 * d).min(300.0);
    if spacing > max + 1e-9 {
        r.issue("26.3.3(b)", format!("Bar spacing {} mm exceeds {} mm (3 d or 300 mm)", n(spacing), n(max.floor())));
    }
    cover_check(r, cover, exposure_cover(get(v, "exposure")).max(50.0).max(bar), get(v, "exposure"), "(50 mm for a footing, and the exposure)");
}

fn lintel(v: &BTreeMap<String, f64>, r: &mut Report) {
    let (cover, bar) = (get(v, "cover"), get(v, "bar"));
    r.line("Bars", format!("{} dia", n(bar)));
    cover_check(r, cover, exposure_cover(get(v, "exposure")).max(bar), get(v, "exposure"), "(the exposure, or the bar diameter)");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(k, x)| (k.to_string(), *x)).collect()
    }

    fn column() -> BTreeMap<String, f64> {
        vals(&[
            ("width", 230.0),
            ("depth", 450.0),
            ("cover", 40.0),
            ("nx", 2.0),
            ("ny", 3.0),
            ("bar", 16.0),
            ("tie", 8.0),
            ("tiesp", 150.0),
            ("fck", 25.0),
            ("fy", 500.0),
            ("exposure", 1.0),
        ])
    }

    fn beam_vals() -> BTreeMap<String, f64> {
        vals(&[
            ("width", 230.0),
            ("depth", 450.0),
            ("cover", 25.0),
            ("ntop", 2.0),
            ("nbot", 3.0),
            ("bartop", 12.0),
            ("barbot", 16.0),
            ("stir", 8.0),
            ("stsp", 150.0),
            ("fck", 25.0),
            ("fy", 500.0),
            ("exposure", 1.0),
        ])
    }

    fn slab_vals() -> BTreeMap<String, f64> {
        vals(&[
            ("span", 3000.0),
            ("thick", 125.0),
            ("cover", 20.0),
            ("bar", 10.0),
            ("dist", 8.0),
            ("spacing", 200.0),
            ("distsp", 250.0),
            ("fck", 25.0),
            ("fy", 500.0),
            ("exposure", 1.0),
        ])
    }

    fn footing_vals() -> BTreeMap<String, f64> {
        vals(&[
            ("length", 1500.0),
            ("depth", 450.0),
            ("edge", 200.0),
            ("cover", 50.0),
            ("bar", 12.0),
            ("spacing", 150.0),
            ("fck", 25.0),
            ("fy", 500.0),
            ("exposure", 1.0),
        ])
    }

    fn clauses(r: &Report) -> Vec<&'static str> {
        r.issues.iter().map(|i| i.clause).collect()
    }

    #[test]
    fn cover_follows_table_16() {
        assert_eq!([1.0, 2.0, 3.0, 4.0, 5.0].map(exposure_cover), [20.0, 30.0, 45.0, 50.0, 75.0]);
    }

    #[test]
    fn the_defaults_of_every_block_comply() {
        for (id, v) in [
            ("rcc.column.rect", column()),
            ("rcc.beam.section", beam_vals()),
            ("rcc.slab.section", slab_vals()),
            ("rcc.footing.section", footing_vals()),
            ("rcc.lintel.section", vals(&[("cover", 25.0), ("bar", 10.0), ("exposure", 1.0)])),
            (
                "rcc.column.circular",
                vals(&[("dia", 450.0), ("cover", 40.0), ("bars", 8.0), ("bar", 16.0), ("tie", 8.0), ("tiesp", 150.0), ("exposure", 1.0)]),
            ),
        ] {
            let r = check(id, &v).unwrap();
            assert!(r.ok(), "{id}: {:?}", r.issues);
            assert!(!r.lines.is_empty(), "{id} reports nothing");
        }
        assert!(check("rcc.footing.plan", &BTreeMap::new()).is_none());
    }

    #[test]
    fn column_steel_percentage_limits() {
        let mut v = column();
        v.insert("bar".into(), 12.0);
        v.insert("ny".into(), 2.0);
        // 4 bars of 12 mm = 452 mm² in 103 500 mm²: 0.44 %.
        let r = check("rcc.column.rect", &v).unwrap();
        assert!(clauses(&r).contains(&"26.5.3.1(a)"));
        v.insert("bar".into(), 32.0);
        v.insert("nx".into(), 5.0);
        v.insert("ny".into(), 8.0);
        v.insert("width".into(), 300.0);
        v.insert("depth".into(), 450.0);
        let r = check("rcc.column.rect", &v).unwrap();
        assert!(r.issues.iter().any(|i| i.clause == "26.5.3.1(a)" && i.message.contains("maximum")), "{:?}", r.issues);
    }

    #[test]
    fn column_ties_and_cover() {
        let mut v = column();
        v.insert("tiesp".into(), 250.0); // more than the 230 mm least side
        v.insert("tie".into(), 6.0);
        v.insert("bar".into(), 32.0); // ties must be at least 8
        v.insert("cover".into(), 30.0);
        let r = check("rcc.column.rect", &v).unwrap();
        let c = clauses(&r);
        assert!(c.contains(&"26.5.3.2(c)(1)"), "{c:?}");
        assert!(c.contains(&"26.5.3.2(c)(2)"), "{c:?}");
        assert!(c.contains(&"26.4.2, Table 16"), "{c:?}");
        // The bar diameter is the cover when it is larger than 40 mm: not for 32 mm, but 40 mm is.
        v.insert("cover".into(), 40.0);
        v.insert("bar".into(), 16.0);
        v.insert("tie".into(), 8.0);
        v.insert("tiesp".into(), 150.0);
        v.insert("exposure".into(), 3.0); // severe: 45 mm
        let r = check("rcc.column.rect", &v).unwrap();
        assert!(clauses(&r).contains(&"26.4.2, Table 16"));
    }

    #[test]
    fn column_minimum_bar_diameter() {
        let mut v = column();
        v.insert("bar".into(), 10.0);
        v.insert("ny".into(), 4.0);
        let r = check("rcc.column.rect", &v).unwrap();
        assert!(clauses(&r).contains(&"26.5.3.1(c)"));
    }

    #[test]
    fn beam_steel_and_stirrups() {
        let mut v = beam_vals();
        // d = 450 - 25 - 8 - 8 = 409; 0.85 * 230 * 409 / 500 = 160 mm².
        let r = check("rcc.beam.section", &v).unwrap();
        assert!(r.lines.iter().any(|l| l.label == "Effective depth" && l.value == "409 mm"), "{:?}", r.lines);
        v.insert("nbot".into(), 2.0);
        v.insert("barbot".into(), 8.0); // 100 mm² < 160
        let r = check("rcc.beam.section", &v).unwrap();
        assert!(clauses(&r).contains(&"26.5.1.1(a)"));
        v.insert("stsp".into(), 350.0);
        let r = check("rcc.beam.section", &v).unwrap();
        assert!(clauses(&r).contains(&"26.5.1.5"));
        v.insert("depth".into(), 800.0);
        let r = check("rcc.beam.section", &v).unwrap();
        assert!(clauses(&r).contains(&"26.5.1.3"));
    }

    #[test]
    fn beam_bars_that_do_not_fit() {
        let mut v = beam_vals();
        v.insert("nbot".into(), 6.0);
        v.insert("barbot".into(), 25.0); // 6 x 25 + 5 gaps of 25 > 230 - 66
        let r = check("rcc.beam.section", &v).unwrap();
        assert!(clauses(&r).contains(&"26.3.2"));
    }

    #[test]
    fn slab_minimum_steel_and_spacing() {
        let mut v = slab_vals();
        // 10 dia at 200: 393 mm² per metre against 0.12 % of 125 x 1000 = 150 mm².
        assert!(check("rcc.slab.section", &v).unwrap().ok());
        v.insert("spacing".into(), 450.0);
        let r = check("rcc.slab.section", &v).unwrap();
        assert!(clauses(&r).contains(&"26.3.3(b)"));
        v.insert("spacing".into(), 200.0);
        v.insert("thick".into(), 75.0);
        v.insert("bar".into(), 12.0);
        let r = check("rcc.slab.section", &v).unwrap();
        assert!(clauses(&r).contains(&"26.5.2.2"));
        // Fe250 asks for 0.15 %.
        let mut m = slab_vals();
        m.insert("fy".into(), 250.0);
        m.insert("bar".into(), 6.0);
        m.insert("spacing".into(), 250.0); // 113 mm² < 187.5
        let r = check("rcc.slab.section", &m).unwrap();
        assert!(clauses(&r).contains(&"26.5.2.1"));
    }

    #[test]
    fn footing_cover_edge_and_mesh() {
        let mut v = footing_vals();
        v.insert("cover".into(), 40.0);
        v.insert("edge".into(), 120.0);
        v.insert("bar".into(), 8.0);
        v.insert("spacing".into(), 300.0);
        let r = check("rcc.footing.section", &v).unwrap();
        let c = clauses(&r);
        assert!(c.contains(&"34.1.2"), "{c:?}");
        assert!(c.contains(&"26.4.2, Table 16"), "{c:?}");
        assert!(c.contains(&"26.5.2.1"), "{c:?}");
    }

    #[test]
    fn circular_column_needs_six_bars() {
        let v = vals(&[("dia", 450.0), ("cover", 40.0), ("bars", 5.0), ("bar", 16.0), ("tie", 8.0), ("tiesp", 150.0), ("exposure", 1.0)]);
        let r = check("rcc.column.circular", &v).unwrap();
        assert!(clauses(&r).contains(&"26.5.3.1(b)"));
    }

    #[test]
    fn a_missing_value_is_not_a_panic() {
        for id in ["rcc.column.rect", "rcc.column.circular", "rcc.beam.section", "rcc.slab.section", "rcc.footing.section", "rcc.lintel.section"] {
            let r = check(id, &BTreeMap::new()).unwrap();
            assert!(!r.lines.is_empty(), "{id}");
        }
    }
}
