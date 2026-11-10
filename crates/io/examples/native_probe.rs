//! Time the native model codec on a real drawing: `cargo run --release -p aad-io --example native_probe -- FILE.(dwg|dxf)`.
//! Prints the parse time, the encode and decode times, the part count and sizes, and checks the decoded drawing equals the parsed one.

use std::time::Instant;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: native_probe FILE");
        return;
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{path}: {e}");
            return;
        }
    };
    let t = Instant::now();
    let d = match aad_io::read(&bytes, &path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{path}: {e}");
            return;
        }
    };
    println!(
        "parse: {:.0} ms, {} entities, {} blocks ({} block entities)",
        t.elapsed().as_secs_f64() * 1e3,
        d.entity_count(),
        d.blocks.len(),
        d.blocks.values().map(|b| b.entities.len()).sum::<usize>()
    );
    let mut big: Vec<(usize, String)> = d
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            aad_doc::EntityKind::Unknown(u) => {
                Some((u.tags.iter().map(|t| t.value.len() + 8).sum::<usize>(), format!("{} ({} tags)", u.dxf_type, u.tags.len())))
            }
            _ => None,
        })
        .collect();
    big.sort_unstable_by(|a, b| b.cmp(a));
    println!("unknown entities: {}, largest: {:?}", big.len(), big.iter().take(8).collect::<Vec<_>>());
    for round in 0..3 {
        let t = Instant::now();
        let parts = match aad_io::native::encode(&d) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("encode: {e}");
                return;
            }
        };
        let enc = t.elapsed().as_secs_f64() * 1e3;
        let size: usize = parts.iter().map(|p| p.bytes.len()).sum();
        if round == 0 {
            let mut sizes: Vec<(usize, usize, usize)> = parts.iter().enumerate().map(|(i, p)| (p.bytes.len(), i, p.entities)).collect();
            sizes.sort_unstable_by(|a, b| b.cmp(a));
            println!(
                "head: {:.2} MB; largest parts (bytes, index, entities): {:?}",
                parts.first().map_or(0, |p| p.bytes.len()) as f64 / 1048576.0,
                sizes.iter().take(6).collect::<Vec<_>>()
            );
        }
        let t = Instant::now();
        let back = aad_io::native::decode(&parts.iter().map(|p| p.bytes.as_slice()).collect::<Vec<_>>());
        let dec = t.elapsed().as_secs_f64() * 1e3;
        match back {
            Ok(b) => println!(
                "round {round}: encode {enc:.0} ms, decode {dec:.0} ms, {} parts, {:.1} MB, equal: {}",
                parts.len(),
                size as f64 / 1048576.0,
                b == d
            ),
            Err(e) => println!("round {round}: encode {enc:.0} ms, decode failed: {e}"),
        }
    }
}
