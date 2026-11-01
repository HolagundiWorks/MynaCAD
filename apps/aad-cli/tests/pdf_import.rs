//! `pdf.import` with the real PDF writer and reader: a layered drawing plotted to PDF comes back as editable
//! geometry on the same layers, text is read through its font, and the import is one undo step.

use std::sync::Once;

use aad_engine::Session;
use aad_engine::doc::EntityKind;
use serde_json::{Value, json};

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

fn temp(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aad-pdfimport-{}-{tag}", std::process::id()))
}

fn on(s: &Session, layer: &str) -> Vec<EntityKind> {
    s.doc().unwrap().model.iter().filter(|e| e.common.layer == layer).map(|e| e.kind.clone()).collect()
}

/// A one-page PDF with `content` and `extra` objects numbered from 5.
fn pdf_with(content: &str, res: &str, extra: &[String]) -> Vec<u8> {
    let mut pdf = String::from("%PDF-1.5\n");
    let mut offsets = Vec::new();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 150] /Contents 4 0 R /Resources {res} >>"),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len() + 1),
    ];
    objs.extend(extra.iter().cloned());
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
    pdf.into_bytes()
}

#[test]
fn a_plotted_drawing_comes_back_as_geometry_on_its_layers() {
    let mut a = session();
    a.execute("layer.new", &json!({"name": "A-WALL"})).unwrap();
    a.execute("layer.new", &json!({"name": "E-LITE"})).unwrap();
    a.execute("layer.current", &json!({"name": "A-WALL"})).unwrap();
    a.execute("line", &json!({"points": [[0, 0], [9000, 0]]})).unwrap();
    a.execute("circle", &json!({"center": [4000, 3000], "radius": 800})).unwrap();
    a.execute("layer.current", &json!({"name": "E-LITE"})).unwrap();
    a.execute("rectang", &json!({"p1": [1000, 1000], "p2": [3000, 2000]})).unwrap();
    let file = temp("plan.pdf");
    a.execute("plot", &json!({"path": file.to_string_lossy(), "paper": "A3", "landscape": true})).unwrap();

    let mut b = session();
    let r = b.execute("pdf.import", &json!({"path": file.to_string_lossy()})).unwrap();
    let layers: Vec<String> = r["layers"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_string()).collect();
    assert!(layers.contains(&"A-WALL".to_string()) && layers.contains(&"E-LITE".to_string()), "{layers:?}");
    assert!(on(&b, "A-WALL").iter().any(|k| matches!(k, EntityKind::Line(_))), "the wall line is a line");
    assert!(on(&b, "A-WALL").iter().any(|k| matches!(k, EntityKind::Circle(_))), "the round shape is a circle again: {r}");
    assert!(on(&b, "E-LITE").iter().any(|k| matches!(k, EntityKind::LwPolyline(p) if p.closed)), "the rectangle is a closed polyline");
    assert_eq!(r["objects"]["circles"], 1);

    // One undo step takes it all back.
    b.execute("undo", &json!({})).unwrap();
    assert_eq!(b.doc().unwrap().model.iter().count(), 0);

    // By colour, or all on one layer.
    let c = b.execute("pdf.import", &json!({"path": file.to_string_lossy(), "layers": "colour"})).unwrap();
    assert!(c["layers"].as_array().unwrap().iter().all(|l| l.as_str().unwrap().starts_with("PDF-")), "{c}");
    b.execute("undo", &json!({})).unwrap();
    let one = b.execute("pdf.import", &json!({"path": file.to_string_lossy(), "layers": "single", "layer": "SITE"})).unwrap();
    assert_eq!(one["layers"], json!(["SITE"]));
    assert_eq!(on(&b, "SITE").len(), b.doc().unwrap().model.iter().count());
    std::fs::remove_file(&file).ok();
}

#[test]
fn text_and_filled_shapes_become_text_and_solid_hatches_at_the_chosen_scale() {
    let content = "1 0 0 rg 10 10 40 20 re f /OC /L0 BDC BT /F1 12 Tf 0 0 1 rg 50 60 Td (Room 1) Tj ET EMC";
    let res = "<< /Properties << /L0 5 0 R >> /Font << /F1 6 0 R >> >>";
    let bytes = pdf_with(content, res, &["<< /Type /OCG /Name (A-TEXT) >>".into(), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into()]);
    let file = temp("text.pdf");
    std::fs::write(&file, bytes).unwrap();
    let mut s = session();
    // 1:50 drawing on paper, in millimetres, placed at (1000, 2000).
    let r = s.execute("pdf.import", &json!({"path": file.to_string_lossy(), "scale": 50, "at": [1000, 2000]})).unwrap();
    assert_eq!(r["objects"]["hatches"], 1);
    assert_eq!(r["objects"]["texts"], 1);
    let k = 25.4 / 72.0 * 50.0;
    let text = s
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| if let EntityKind::Text(t) = &e.kind { Some((t.clone(), e.common.layer.clone())) } else { None })
        .unwrap();
    assert_eq!((text.0.value.as_str(), text.1.as_str()), ("Room 1", "A-TEXT"));
    assert!((text.0.insert.x - (1000.0 + 50.0 * k)).abs() < 1e-6 && (text.0.insert.y - (2000.0 + 60.0 * k)).abs() < 1e-6);
    assert!((text.0.height - 12.0 * 0.72 * k).abs() < 1e-6, "the capital height is 0.72 of the em: {}", text.0.height);
    let hatch = s
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some((h.clone(), e.common.color)) } else { None })
        .unwrap();
    assert!(hatch.0.solid && hatch.0.loops[0].vertices.len() == 4);
    assert_eq!(hatch.1, aad_engine::doc::color::Color::True(aad_engine::doc::color::Rgb(255, 0, 0)));
    // The options leave things out.
    s.execute("undo", &json!({})).unwrap();
    let r = s.execute("pdf.import", &json!({"path": file.to_string_lossy(), "text": false, "fills": false}));
    assert!(r.is_err(), "nothing is left to import: {r:?}");
    std::fs::remove_file(&file).ok();
}

#[test]
fn bad_requests_are_refused_and_change_nothing() {
    let file = temp("small.pdf");
    std::fs::write(&file, pdf_with("0 0 m 10 10 l S", "<< >>", &[])).unwrap();
    let path = file.to_string_lossy().to_string();
    let mut s = session();
    let bad: Vec<Value> = vec![
        json!({}),
        json!({"path": ""}),
        json!({"path": "Z:/no/such/file.pdf"}),
        json!({"path": path, "page": 0}),
        json!({"path": path, "page": 2}),
        json!({"path": path, "page": "x"}),
        json!({"path": path, "scale": 0}),
        json!({"path": path, "scale": 1e12}),
        json!({"path": path, "at": [1]}),
        json!({"path": path, "layers": "all"}),
    ];
    for params in bad {
        assert!(s.execute("pdf.import", &params).is_err(), "{params}");
        assert_eq!(s.doc().unwrap().model.iter().count(), 0, "{params}");
    }
    std::fs::write(&file, b"not a pdf at all").unwrap();
    assert!(s.execute("pdf.import", &json!({"path": path})).is_err());
    std::fs::remove_file(&file).ok();
}
