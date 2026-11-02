//! Profiling aid: open a drawing and time the display list and mesh at several zoom levels, the way the host does on a zoom.
//! Run: cargo run --release -p aad-host --example profile_render -- <file.dwg> [cx cy height]
use std::time::Instant;

use aad_engine::doc::Space;
use aad_geom::Vec2;
use aad_render::{Kind, Options, mesh};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("file");
    let bytes = std::fs::read(path).unwrap();
    let t = Instant::now();
    let doc = aad_io::read(&bytes, path).unwrap();
    println!("read: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let num = |i: usize, d: f64| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (cx, cy, h0) = (num(2, 28546.7), num(3, -6219.6), num(4, 28432.5));
    let pixel_h = 606.0;
    // What costs what: the same view with text off, fill off, both off.
    {
        let height = h0 / 2.0;
        let px = height / pixel_h;
        let band = aad_render::tessellation_band(px);
        for (label, text, fill) in [("all", true, true), ("no text", false, true), ("no fill", true, false), ("neither", false, false)] {
            let opts = Options {
                tolerance: aad_render::tessellation_tolerance(band),
                min_dash: px * 2.0,
                text,
                fill,
                text_greek: false,
                fill_tint: true,
                lineweights: true,
                view_height: height,
                view_center: Some(Vec2::new(cx, cy)),
                plot_style_table: None,
                screen_px: 0.0,
            };
            let t = Instant::now();
            let list = aad_render::build(&doc, &Space::Model, &opts);
            let tri: usize = list.prims.iter().filter(|p| matches!(p.kind, Kind::Tris | Kind::Mask)).map(|p| p.len as usize).sum();
            println!(
                "band {band} {label:>8}: build {:>6.1} ms, prims {:>7}, tri verts {:>8}",
                t.elapsed().as_secs_f64() * 1000.0,
                list.prims.len(),
                tri
            );
        }
    }
    for step in 0..6 {
        let height = h0 / 2f64.powi(step);
        let px = height / pixel_h;
        let band = aad_render::tessellation_band(px);
        let lod = aad_render::lod_flags_for_view(px.max(1e-300), 1.0);
        let opts = Options {
            tolerance: aad_render::tessellation_tolerance(band),
            min_dash: px * 2.0,
            text: lod.text,
            fill: lod.fill,
            text_greek: lod.text_greek,
            fill_tint: lod.fill_tint,
            lineweights: true,
            view_height: height,
            view_center: Some(Vec2::new(cx, cy)),
            plot_style_table: None,
            screen_px: px,
        };
        let t = Instant::now();
        let list = aad_render::build(&doc, &Space::Model, &opts);
        let build_ms = t.elapsed().as_secs_f64() * 1000.0;
        let (mut poly, mut tris, mut other) = (0usize, 0usize, 0usize);
        for p in &list.prims {
            match p.kind {
                Kind::Polyline => poly += p.len as usize,
                Kind::Tris | Kind::Mask => tris += p.len as usize,
                _ => other += 1,
            }
        }
        let t = Instant::now();
        let origin = mesh::choose_origin(&list.bounds, Vec2::new(cx, cy), height);
        let m = mesh::build_mesh_mode(&list, origin, aad_color::Rgb(255, 255, 255), Some(mesh::LineweightMode::Screen));
        let mesh_ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "height {:>9.0} band {:>3} text={} fill={} tint={} | build {:>7.1} ms prims {:>7} polyverts {:>8} trisverts {:>8} other {:>5} | mesh {:>7.1} ms ({} mesh words)",
            height,
            band,
            lod.text,
            lod.fill,
            lod.fill_tint,
            build_ms,
            list.prims.len(),
            poly,
            tris,
            other,
            mesh_ms,
            m.lines.len() / 4 + m.tris.len() / 4 + m.quads.len() / 4 + m.rings.len() / 4
        );
    }
}
