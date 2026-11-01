//! The PDF underlay with the real PDF writer and reader: a drawing is plotted to PDF, the PDF is attached to a
//! fresh drawing as snappable lines, calibrated from two points and a real distance, and the take-off tools measure over it.

use std::sync::Once;

use aad_engine::Session;
use aad_engine::doc::EntityKind;
use serde_json::json;

fn install_io() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        aad_engine::cmd::file::set_io(aad_engine::cmd::file::IoHooks {
            read: |b, name| aad_io::read(b, name).map_err(|e| e.to_string()),
            write: |d, name| aad_io::write(d, name).map_err(|e| e.to_string()),
            plot: Some(|d, space, opts| aad_io::plot(d, space, opts).map_err(|e| e.to_string())),
        });
        aad_engine::cmd::file::set_pdf_reader(|bytes, page| aad_io::pdfread::read_page(bytes, page).map_err(|e| e.to_string()));
        aad_engine::cmd::file::set_raster_reader(|bytes| {
            aad_io::raster::read_gray(bytes).map(|g| aad_engine::cmd::file::Raster { width: g.width, height: g.height, gray: g.gray })
        });
    });
}

fn session() -> Session {
    install_io();
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s
}

/// A plan drawn at 1:1 in millimetres, plotted to a PDF file: a 9 m x 6 m outline.
fn plan_pdf(dir: &std::path::Path) -> std::path::PathBuf {
    let mut s = session();
    s.execute("line", &json!({"points": [[0, 0], [9000, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[9000, 0], [9000, 6000]]})).unwrap();
    s.execute("line", &json!({"points": [[9000, 6000], [0, 6000]]})).unwrap();
    s.execute("line", &json!({"points": [[0, 6000], [0, 0]]})).unwrap();
    let path = dir.join("plan.pdf");
    s.execute("plot", &json!({"path": path.to_string_lossy(), "paper": "A3", "landscape": true})).unwrap();
    path
}

/// The box of everything on a layer, in drawing units.
fn layer_box(s: &Session, layer: &str) -> (f64, f64, f64, f64) {
    let d = s.doc().unwrap();
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for e in d.model.iter().filter(|e| e.common.layer == layer) {
        if let EntityKind::LwPolyline(pl) = &e.kind {
            for v in &pl.vertices {
                b = (b.0.min(v.p.x), b.1.min(v.p.y), b.2.max(v.p.x), b.3.max(v.p.y));
            }
        }
    }
    b
}

fn count(s: &Session, layer: &str) -> usize {
    s.doc().unwrap().model.iter().filter(|e| e.common.layer == layer).count()
}

#[test]
fn a_plotted_plan_is_attached_calibrated_and_measured() {
    let dir = std::env::temp_dir().join(format!("aad-underlay-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = plan_pdf(&dir);
    let mut s = session();
    let r = s.execute("underlay.attach", &json!({"path": pdf.to_string_lossy()})).unwrap();
    assert_eq!(r["layer"], "PDF-UNDERLAY");
    assert!(r["paths"].as_u64().unwrap() >= 4, "the outline lines: {r}");
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("calibrate")), "the scale is said to be unknown");
    assert_eq!(count(&s, "PDF-UNDERLAY") as u64, r["paths"].as_u64().unwrap());

    // The outline width on the page, before calibration (the page holds the plan fitted to A3, at paper millimetres).
    let (x0, y0, x1, _y1) = layer_box(&s, "PDF-UNDERLAY");
    let page_width = x1 - x0;
    assert!(page_width > 100.0 && page_width < 420.0, "an A3 sheet drawing: {page_width}");

    // Calibrate from the bottom edge, which is 9000 mm long in the real building.
    let c = s.execute("underlay.calibrate", &json!({"p1": [x0, y0], "p2": [x1, y0], "length": 9, "unit": "m"})).unwrap();
    assert!(c["factor"].as_f64().unwrap() > 1.0);
    let (a0, b0, a1, b1) = layer_box(&s, "PDF-UNDERLAY");
    assert!((a1 - a0 - 9000.0).abs() < 0.01, "9 m wide after calibration: {}", a1 - a0);
    assert!((b1 - b0 - 6000.0).abs() < 1.0, "the 6 m side follows, the plot kept its proportions: {}", b1 - b0);
    assert!((a0 - x0).abs() < 1e-6 && (b0 - y0).abs() < 1e-6, "scaled about the first point");
    let list = s.execute("underlay.list", &json!({})).unwrap();
    assert_eq!(list["underlays"][0]["calibrated"], true);

    // Measure over it with the take-off tools: a line on the measure layer along the bottom edge reads 9 m.
    s.execute("takeoff.start", &json!({})).unwrap();
    s.execute("layer.current", &json!({"name": "MEASURE-FULLBRICK"})).unwrap();
    s.execute("line", &json!({"points": [[a0, b0], [a1, b0]]})).unwrap();
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    let exported = s.execute("bridge.export", &json!({"write": false})).unwrap();
    let rows: serde_json::Value = serde_json::from_str(exported["json"].as_str().unwrap()).unwrap();
    let length = rows["walls"][0]["length_mm"].as_f64().unwrap();
    assert!((length - 9000.0).abs() < 1.0, "the measured wall reads 9 m: {length}");

    // Undo takes a calibration back; removing deletes the layer lines and the record.
    let before = layer_box(&s, "PDF-UNDERLAY");
    s.execute("underlay.calibrate", &json!({"p1": [a0, b0], "p2": [a1, b0], "length": 18, "unit": "m"})).unwrap();
    assert_ne!(layer_box(&s, "PDF-UNDERLAY"), before);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(layer_box(&s, "PDF-UNDERLAY"), before, "undo restores the scale");
    let rm = s.execute("underlay.remove", &json!({})).unwrap();
    assert_eq!(rm["removed"].as_u64(), r["paths"].as_u64());
    assert_eq!(count(&s, "PDF-UNDERLAY"), 0);
    assert!(s.execute("underlay.list", &json!({})).unwrap()["underlays"].as_array().unwrap().is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn bad_requests_are_errors_and_change_nothing() {
    let dir = std::env::temp_dir().join(format!("aad-underlay-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = plan_pdf(&dir);
    let p = pdf.to_string_lossy().to_string();
    let mut s = session();
    let before = s.doc().unwrap().entity_count();
    for bad in [
        json!({}),
        json!({"path": "C:/no/such/file.pdf"}),
        json!({"path": p, "page": 0}),
        json!({"path": p, "page": 2}),
        json!({"path": p, "page": "x"}),
        json!({"path": p, "scale": -1}),
        json!({"path": p, "scale": 1e30}),
        json!({"path": p, "at": "here"}),
    ] {
        assert!(s.execute("underlay.attach", &bad).is_err(), "{bad}");
    }
    // Not a PDF at all.
    let junk = dir.join("junk.pdf");
    std::fs::write(&junk, b"this is not a pdf").unwrap();
    assert!(s.execute("underlay.attach", &json!({"path": junk.to_string_lossy()})).is_err());
    assert_eq!(s.doc().unwrap().entity_count(), before, "nothing was drawn");

    // Attached once, a second attach on the same layer is refused; a different layer is fine.
    s.execute("underlay.attach", &json!({"path": p})).unwrap();
    assert!(s.execute("underlay.attach", &json!({"path": p})).is_err());
    s.execute("underlay.attach", &json!({"path": p, "layer": "PDF-UNDERLAY-2", "at": [1000, 1000], "scale": 100})).unwrap();
    let (x0, y0, x1, _) = layer_box(&s, "PDF-UNDERLAY-2");
    assert!(x0 >= 1000.0 && y0 >= 1000.0 && x1 - x0 > 1000.0, "placed at [1000, 1000] and shown at 1:100");

    // Calibration refuses what makes no sense, and leaves the lines alone.
    let box1 = layer_box(&s, "PDF-UNDERLAY");
    for bad in [
        json!({"p1": [0, 0], "p2": [0, 0], "length": 5}),
        json!({"p1": [0, 0], "p2": [10, 0], "length": 0}),
        json!({"p1": [0, 0], "p2": [10, 0], "length": -3}),
        json!({"p1": [0, 0], "p2": [10, 0], "length": 5, "unit": "furlongs"}),
        json!({"p1": [0, 0], "p2": [1e-9, 0], "length": 1e9}),
        json!({"p1": [0, 0], "length": 5}),
        json!({"p1": [0, 0], "p2": [10, 0], "length": 5, "layer": "NOPE"}),
    ] {
        assert!(s.execute("underlay.calibrate", &bad).is_err(), "{bad}");
    }
    assert_eq!(layer_box(&s, "PDF-UNDERLAY"), box1);
    assert!(s.execute("underlay.remove", &json!({"layer": "NOPE"})).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

/// A page with only text (the shape of a scanned or text-only PDF) has nothing to draw and is refused with a reason.
#[test]
fn a_page_without_vector_lines_is_refused_with_a_reason() {
    let content = "BT (scan) Tj ET";
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 150] /Contents 4 0 R /Resources << >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len() + 1),
    ];
    for (i, o) in objs.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1));
    for o in &offsets {
        pdf.push_str(&format!("{o:010} 00000 n \n"));
    }
    pdf.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1));
    let path = std::env::temp_dir().join(format!("aad-underlay-scan-{}.pdf", std::process::id()));
    std::fs::write(&path, pdf).unwrap();
    let mut s = session();
    let err = s.execute("underlay.attach", &json!({"path": path.to_string_lossy()})).unwrap_err().to_string();
    assert!(err.contains("no vector lines") && err.contains("scanned"), "{err}");
    assert_eq!(s.doc().unwrap().model.iter().count(), 0);
    std::fs::remove_file(&path).ok();
}
