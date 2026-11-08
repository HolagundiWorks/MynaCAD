//! `takeoff.annotate`: the annotated drawing, a PDF of the drawing with what the take-off measured drawn over it.
//!
//! Each measured item is drawn again on the layer `TAKEOFF-ANNOT` in the colour of its kind (walls red, openings
//! green, columns blue, lintels magenta, rooms cyan, slabs orange) with its mark and its measure beside it, and a
//! legend and a title line (drawing, revision, date) stand below the drawing. The work happens on a copy of the
//! drawing, so the drawing itself is untouched. `takeoff.record` stores the PDF in the project file in chunks
//! Annotated PDF chunks in the project file (see Falcon `OspTakeoff::ReadAnnotatedPdf`).

use aad_color::Color;
use aad_doc::{Common, Drawing, EntityKind, HAlign, Handle, Lineweight, Space, Text, VAlign};
use aad_geom::Vec3;
use serde_json::{Value, json};

use super::bridge::{
    Collected, Item, KIND_COLUMN, KIND_ELEC_BOARD, KIND_ELEC_CIRCUIT, KIND_LINTEL, KIND_OPENING, KIND_PLUMB_FIXTURE, KIND_PLUMB_STACK, KIND_ROOM,
    KIND_SLAB, KIND_WALL,
};
use super::*;
use crate::{Result, Session};

/// The layer the measured items are drawn on.
pub(crate) const LAYER: &str = "TAKEOFF-ANNOT";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("takeoff.annotate", "Annotated Take-off Drawing", run_annotate)
            .menu(&["Architecture", "Take-off", "Annotated Drawing (PDF)"])
            .params("{path?: PDF file (default: <drawing>.takeoff.pdf beside the saved drawing), paper?: \"A3\", level?, bearing?} → {path, bytes, pages, sha256, drawn}")
            .noundo(),
    ]
}

/// The legend: kind, label, ACI colour.
const KINDS: [(u8, &str, u8); 10] = [
    (KIND_WALL, "Walls", 1),
    (KIND_OPENING, "Openings", 3),
    (KIND_COLUMN, "Columns", 5),
    (KIND_LINTEL, "Lintels", 6),
    (KIND_ROOM, "Rooms", 4),
    (KIND_SLAB, "Slabs", 30),
    (KIND_ELEC_BOARD, "Boards", 2),
    (KIND_ELEC_CIRCUIT, "Circuits", 140),
    (KIND_PLUMB_STACK, "Plumbing sources", 150),
    (KIND_PLUMB_FIXTURE, "Fixtures", 160),
];

fn colour_of(kind: u8) -> u8 {
    KINDS.iter().find(|k| k.0 == kind).map_or(7, |k| k.2)
}

/// What an item's row measures, for its label.
fn measure_text(row: &Value) -> String {
    let num = |k: &str| row.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
    if let Some(l) = num("length_mm").filter(|_| row.get("breadth_mm").is_none()) {
        return format!("{:.2} m", l / 1000.0);
    }
    if let Some(a) = num("area_m2") {
        return format!("{a:.2} m2");
    }
    match (num("width_mm"), num("depth_mm").or_else(|| num("height_mm"))) {
        (Some(w), Some(h)) => format!("{w:.0} x {h:.0}"),
        (Some(w), None) => format!("{w:.0}"),
        _ => String::new(),
    }
}

fn text(at: aad_geom::Vec2, height: f64, value: String, aci: u8, layer: &str) -> (Common, EntityKind) {
    let common = Common { layer: layer.into(), color: Color::Index(aci), ..Common::default() };
    let t = Text {
        insert: Vec3::new(at.x, at.y, 0.0),
        align_pt: None,
        height,
        value,
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    };
    (common, EntityKind::Text(t))
}

/// A copy of `drawing` with the measured items, their labels, a legend and a title drawn over it. Returns the copy and
/// how many items were drawn.
pub(crate) fn annotated(drawing: &Drawing, c: &Collected, revision: &str) -> Result<(Drawing, usize)> {
    let mut d = drawing.clone();
    let before = d.extents(&Space::Model);
    let size = if before.is_empty() { 1000.0 } else { (before.max.x - before.min.x).max(before.max.y - before.min.y).max(1.0) };
    let h = size / 150.0;
    let mut drawn = 0usize;
    let mut counts = std::collections::BTreeMap::new();
    for Item { kind, n, row, .. } in &c.items {
        let Some(e) = d.entity(Handle(*n)).cloned() else { continue };
        let b = aad_doc::entity_bounds(drawing, &e, 0);
        if b.is_empty() {
            continue;
        }
        let common = Common { layer: LAYER.into(), color: Color::Index(colour_of(*kind)), lineweight: Lineweight::Mm100(50), ..Common::default() };
        d.add(&Space::Model, common, e.kind.clone()).map_err(|x| bad("takeoff.annotate", x.to_string()))?;
        let label = format!("{} {}", row.get("ref").and_then(Value::as_str).unwrap_or(""), measure_text(row));
        let at = aad_geom::Vec2::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0 + h * 0.6);
        let (cm, k) = text(at, h, label.trim().to_string(), colour_of(*kind), LAYER);
        d.add(&Space::Model, cm, k).map_err(|x| bad("takeoff.annotate", x.to_string()))?;
        *counts.entry(*kind).or_insert(0usize) += 1;
        drawn += 1;
    }
    // Legend and title below the drawing.
    let x = if before.is_empty() { 0.0 } else { before.min.x };
    let mut y = if before.is_empty() { 0.0 } else { before.min.y } - h * 3.0;
    let (cm, k) = text(
        aad_geom::Vec2::new(x, y),
        h * 1.4,
        format!(
            "{}   rev {}   {}",
            c.root.get("source").and_then(|s| s.get("drawing")).and_then(Value::as_str).unwrap_or("drawing"),
            revision,
            super::bridge::iso_time().unwrap_or_default()
        ),
        7,
        LAYER,
    );
    d.add(&Space::Model, cm, k).map_err(|x| bad("takeoff.annotate", x.to_string()))?;
    for (kind, name, aci) in KINDS {
        let n = counts.get(&kind).copied().unwrap_or(0);
        if n == 0 {
            continue;
        }
        y -= h * 2.0;
        let (cm, k) = text(aad_geom::Vec2::new(x, y), h, format!("{name}: {n}"), aci, LAYER);
        d.add(&Space::Model, cm, k).map_err(|x| bad("takeoff.annotate", x.to_string()))?;
    }
    Ok((d, drawn))
}

/// The pages named by the PDF's page tree (the plotter writes one).
pub(crate) fn page_count(pdf: &[u8]) -> u32 {
    let text = String::from_utf8_lossy(pdf);
    text.find("/Type /Pages")
        .and_then(|i| text.get(i..))
        .and_then(|t| t.split("/Count ").nth(1))
        .and_then(|t| t.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1)
}

/// Build the PDF for a collection. Fails when this build cannot plot.
pub(crate) fn build_pdf(s: &Session, c: &Collected, paper: &str) -> Result<(Vec<u8>, usize)> {
    let hook = super::file::io().and_then(|h| h.plot).ok_or_else(|| bad("takeoff.annotate", "plotting is not available in this build"))?;
    let revision = c
        .root
        .get("source")
        .and_then(|x| x.get("revision"))
        .and_then(|r| r.get("sha256"))
        .and_then(Value::as_str)
        .map_or("unsaved".to_string(), |h| h.chars().take(12).collect());
    let (d, drawn) = annotated(&s.state()?.doc, c, &revision)?;
    let opts = json!({ "paper": paper, "landscape": true, "fit": true, "compress": true, "title": "Take-off" });
    let bytes = hook(&d, &Space::Model, &opts).map_err(|e| bad("takeoff.annotate", e))?;
    Ok((bytes, drawn))
}

fn run_annotate(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err(bad("takeoff.annotate", "files cannot be written on the web"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let c = super::bridge::collect(s, p)?;
        let paper = str_param(p, "paper").unwrap_or("A3").to_string();
        let path = match str_param(p, "path") {
            Some(t) => std::path::PathBuf::from(t),
            None => match s.state()?.path.as_ref() {
                Some(drawing) => std::path::Path::new(drawing).with_extension("takeoff.pdf"),
                None => return Err(bad("takeoff.annotate", "save the drawing first, or give `path`")),
            },
        };
        let (bytes, drawn) = build_pdf(s, &c, &paper)?;
        let tmp = format!("{}.aad-tmp", path.display());
        std::fs::write(&tmp, &bytes).map_err(|e| bad("takeoff.annotate", format!("{}: {e}", path.display())))?;
        std::fs::rename(&tmp, &path).map_err(|e| bad("takeoff.annotate", format!("{}: {e}", path.display())))?;
        s.echo(format!("Annotated drawing written: {} items drawn.", drawn));
        Ok(
            json!({ "path": path.display().to_string(), "bytes": bytes.len(), "pages": page_count(&bytes), "sha256": crate::revision::sha256_hex(&bytes), "drawn": drawn, "warnings": c.warnings }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_count_reads_the_page_tree() {
        assert_eq!(page_count(b"%PDF-1.4\n2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 3 >>"), 3);
        assert_eq!(page_count(b"not a pdf"), 1);
    }

    #[test]
    fn labels_show_the_measure() {
        assert_eq!(measure_text(&json!({"length_mm": 9000.0})), "9.00 m");
        assert_eq!(measure_text(&json!({"area_m2": 54.0})), "54.00 m2");
        assert_eq!(measure_text(&json!({"width_mm": 230.0, "depth_mm": 450.0})), "230 x 450");
    }
}
