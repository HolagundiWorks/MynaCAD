//! SVG export of a space (vector, for the web and documents).

use std::fmt::Write as _;

use aad_color::Rgb;
use aad_doc::{Drawing, Space};
use aad_geom::Bounds2;
use aad_render::Kind;

/// Export with a white background (colour 7 prints black), fitted to the extents.
pub fn export(d: &Drawing, space: &Space) -> String {
    export_window(d, space, None)
}

/// Export showing exactly `window` (the `viewBox`; geometry outside it is clipped by the viewer),
/// or the extents with a 2% margin when `window` is `None`.
pub fn export_window(d: &Drawing, space: &Space, window: Option<Bounds2>) -> String {
    export_layers(d, space, window, true)
}

/// Like [`export_window`]; with `layers` each drawing layer is a `<g>` group marked as an Inkscape
/// layer (`inkscape:groupmode="layer"`, `inkscape:label` the layer name), so editors list and hide
/// them. The groups are in the order each layer first draws; with `layers` off everything is drawn in
/// the display list's exact order.
pub fn export_layers(d: &Drawing, space: &Space, window: Option<Bounds2>, layers: bool) -> String {
    let list = aad_render::build(d, space, &aad_render::Options::default());
    let window = window.filter(|w| !w.is_empty());
    // A corrupted coordinate far outside any drawing would stretch the view and print hundreds of digits per point: fit to what is within range.
    let in_range = |q: &aad_geom::Vec2| q.x.abs() < 1e12 && q.y.abs() < 1e12;
    let fitted = if in_range(&list.bounds.min) && in_range(&list.bounds.max) {
        list.bounds
    } else {
        Bounds2::from_points(list.prims.iter().flat_map(|p| list.points(p).iter().copied()).filter(|q| in_range(q)))
    };
    let b = window.unwrap_or(fitted);
    let (w, h) = if b.is_empty() { (1.0, 1.0) } else { (b.width().max(1e-9), b.height().max(1e-9)) };
    let m = if window.is_some() { 0.0 } else { (w.max(h)) * 0.02 };
    let (x0, y1) = if b.is_empty() { (0.0, 1.0) } else { (b.min.x - m, b.max.y + m) };
    let vw = w + 2.0 * m;
    let vh = h + 2.0 * m;
    let stroke = vw.max(vh) / 1500.0;
    let index = layers.then(|| crate::layers::index(d, &list));
    let mut bodies: Vec<String> = vec![String::new(); index.as_ref().map_or(1, |i| i.names.len().max(1))];
    let mut head = String::new();
    let _ = write!(
        head,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" viewBox="0 0 {vw:.6} {vh:.6}" width="{:.0}" height="{:.0}"><rect width="100%" height="100%" fill="white"/>"#,
        1600.0,
        1600.0 * vh / vw
    );
    let white = Rgb(255, 255, 255);
    let tx = |x: f64| x - x0;
    let ty = |y: f64| y1 - y;
    for (pi, p) in list.prims.iter().enumerate() {
        let g = index.as_ref().and_then(|i| i.of_prim.get(pi).copied()).unwrap_or(0).min(bodies.len() - 1);
        let s = &mut bodies[g];
        let c = p.display_rgb(white).hex();
        let pts = list.points(p);
        // A corrupted coordinate far outside any drawing would print hundreds of digits for every point of the shape.
        if pts.iter().any(|q| !(q.x.abs() < 1e12 && q.y.abs() < 1e12)) {
            continue;
        }
        match p.kind {
            Kind::Polyline => {
                let _ = write!(
                    s,
                    r#"<polyline fill="none" stroke="{c}" stroke-width="{stroke:.6}" stroke-linecap="round" stroke-linejoin="round" points=""#
                );
                for q in pts {
                    let _ = write!(s, "{:.6},{:.6} ", tx(q.x), ty(q.y));
                }
                s.push_str(r#""/>"#);
            }
            Kind::Arc { sweep } => {
                let (Some(center), Some(rim)) = (pts.first(), pts.get(1)) else { continue };
                let radius = center.dist(*rim);
                let start = center.angle_to(*rim);
                let mut tess = Vec::new();
                aad_geom::Arc::new(*center, radius, start, start + aad_render::sweep_radians(sweep)).tessellate((radius * 1e-3).max(1e-6), &mut tess);
                let _ = write!(
                    s,
                    r#"<polyline fill="none" stroke="{c}" stroke-width="{stroke:.6}" stroke-linecap="round" stroke-linejoin="round" points=""#
                );
                for q in &tess {
                    let _ = write!(s, "{:.6},{:.6} ", tx(q.x), ty(q.y));
                }
                s.push_str(r#""/>"#);
            }
            Kind::Ellipse { start, sweep } => {
                let (Some(center), Some(u_tip), Some(v_tip)) = (pts.first(), pts.get(1), pts.get(2)) else { continue };
                let u = *u_tip - *center;
                let v = *v_tip - *center;
                let sw = aad_render::sweep_radians(sweep);
                let a0 = f64::from(start);
                let n = 32usize.max(((sw.abs() / 0.05) as usize).min(512));
                let _ = write!(
                    s,
                    r#"<polyline fill="none" stroke="{c}" stroke-width="{stroke:.6}" stroke-linecap="round" stroke-linejoin="round" points=""#
                );
                for i in 0..=n {
                    let t = a0 + sw * i as f64 / n as f64;
                    let q = *center + u * t.cos() + v * t.sin();
                    let _ = write!(s, "{:.6},{:.6} ", tx(q.x), ty(q.y));
                }
                s.push_str(r#""/>"#);
            }
            Kind::Tris | Kind::Mask => {
                // A wipeout's mask hides what lies under it with the white background.
                let c = if p.kind == Kind::Mask { white.hex() } else { c };
                let _ = write!(s, r#"<path fill="{c}" d=""#);
                for t in pts.chunks(3) {
                    if let [a, b2, cc] = t {
                        let _ = write!(s, "M{:.6} {:.6}L{:.6} {:.6}L{:.6} {:.6}Z", tx(a.x), ty(a.y), tx(b2.x), ty(b2.y), tx(cc.x), ty(cc.y));
                    }
                }
                s.push_str(r#""/>"#);
            }
            Kind::Point => {
                if let Some(q) = pts.first() {
                    let _ = write!(s, r#"<circle cx="{:.6}" cy="{:.6}" r="{:.6}" fill="{c}"/>"#, tx(q.x), ty(q.y), stroke);
                }
            }
            Kind::Infinite { ray } => {
                if let (Some(bp), Some(dir)) = (pts.first(), pts.get(1)) {
                    // A window can lie far from the base point: reach past it.
                    let far = (vw + vh) * 2.0 + if window.is_some() { bp.dist(b.center()) } else { 0.0 };
                    let a = if ray { *bp } else { *bp - *dir * far };
                    let e = *bp + *dir * far;
                    let _ = write!(
                        s,
                        r#"<line x1="{:.6}" y1="{:.6}" x2="{:.6}" y2="{:.6}" stroke="{c}" stroke-width="{stroke:.6}"/>"#,
                        tx(a.x),
                        ty(a.y),
                        tx(e.x),
                        ty(e.y)
                    );
                }
            }
        }
    }
    assemble(head, &bodies, index.as_ref().map(|i| i.names.as_slice()))
}

/// The document: the header, then either one group per layer or the bodies as one run.
fn assemble(head: String, bodies: &[String], names: Option<&[String]>) -> String {
    let mut s = head;
    match names {
        Some(names) => {
            for (i, (name, body)) in names.iter().zip(bodies).enumerate() {
                if body.is_empty() {
                    continue;
                }
                let _ = write!(
                    s,
                    r#"<g id="{}" inkscape:groupmode="layer" inkscape:label="{}">{body}</g>"#,
                    crate::layers::xml_id(name, i),
                    crate::layers::xml_attr(name)
                );
            }
        }
        None => {
            for b in bodies {
                s.push_str(b);
            }
        }
    }
    s.push_str("</svg>");
    s
}

/// An SVG of a plot of `space` as the PDF writer would lay it out: the sheet in points
/// (`viewBox`, with `width` and `height` in millimetres), the drawing fitted, scaled and clipped to
/// the printable area as set by `o`, and the layers as groups unless `o.layers` is `Some(false)`.
pub fn plot(d: &Drawing, space: &Space, o: &crate::PdfOptions) -> crate::Result<String> {
    let pr = crate::pdf::prepare(d, space, o)?;
    let index = o.layers.unwrap_or(true).then(|| crate::layers::index(d, &pr.list));
    let mut bodies: Vec<String> = vec![String::new(); index.as_ref().map_or(1, |i| i.names.len().max(1))];
    let (w, h) = (pr.media.x, pr.media.y);
    let fy = |y: f64| h - y;
    for (pi, p) in pr.list.prims.iter().enumerate() {
        let Some(shape) = crate::pdf::shape(p, &pr.list, &pr.map, &pr.clip_pt) else { continue };
        let g = index.as_ref().and_then(|i| i.of_prim.get(pi).copied()).unwrap_or(0).min(bodies.len() - 1);
        let s = &mut bodies[g];
        match shape {
            crate::pdf::Shape::Stroke { pts, rgb, width_pt } => {
                let _ = write!(
                    s,
                    r#"<polyline fill="none" stroke="{}" stroke-width="{:.3}" stroke-linecap="round" stroke-linejoin="round" points=""#,
                    rgb.hex(),
                    width_pt.max(0.25)
                );
                for q in &pts {
                    let _ = write!(s, "{:.3},{:.3} ", q.x, fy(q.y));
                }
                s.push_str(r#""/>"#);
            }
            crate::pdf::Shape::Fill { pts, rgb } => {
                let _ = write!(s, r#"<path fill="{}" d=""#, rgb.hex());
                for t in pts.chunks(3) {
                    if let [a, b, c] = t {
                        let _ = write!(s, "M{:.3} {:.3}L{:.3} {:.3}L{:.3} {:.3}Z", a.x, fy(a.y), b.x, fy(b.y), c.x, fy(c.y));
                    }
                }
                s.push_str(r#""/>"#);
            }
            crate::pdf::Shape::Dot { at, size, rgb } => {
                let _ = write!(s, r#"<circle cx="{:.3}" cy="{:.3}" r="{:.3}" fill="{}"/>"#, at.x, fy(at.y), size / 2.0, rgb.hex());
            }
        }
    }
    let mm = 25.4 / 72.0;
    let mut head = String::new();
    let _ = write!(
        head,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" viewBox="0 0 {w:.3} {h:.3}" width="{:.3}mm" height="{:.3}mm"><title>{}</title><defs><clipPath id="printable"><rect x="{:.3}" y="{:.3}" width="{:.3}" height="{:.3}"/></clipPath></defs><rect width="{w:.3}" height="{h:.3}" fill="white"/><g clip-path="url(#printable)">"#,
        w * mm,
        h * mm,
        crate::layers::xml_attr(&pr.title),
        pr.clip_pt.min.x,
        fy(pr.clip_pt.max.y),
        pr.clip_pt.width(),
        pr.clip_pt.height()
    );
    let mut s = assemble(head, &bodies, index.as_ref().map(|i| i.names.as_slice()));
    // The clip group opened in the header is closed before the root.
    if let Some(at) = s.rfind("</svg>") {
        s.insert_str(at, "</g>");
    }
    Ok(s)
}
