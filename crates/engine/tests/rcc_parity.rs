//! The RCC smart blocks are drawn by two programs: this engine (MynaCAD) and the C# reader in MynaOps (`Office.Shared.SmartBlocks`), from the same
//! `rcc.json`. This pins what the engine makes (how many lines, polylines, dots, circles, arcs, texts and hatches each block has, for defaults and
//! for changed parameters) in `fixtures/rcc_counts.json`; MynaOps's `SmartBlockTests` holds a copy and checks its own evaluation against it.
//! Run with `UPDATE_FIXTURES=1` to write the file again after a block changes, then copy it to MynaOps.

use std::collections::BTreeMap;

use aad_engine::Session;
use aad_engine::doc::EntityKind;
use serde_json::{Value, json};

/// (block id, parameters) pairs: every block with its defaults, and some with other values.
const CASES: &[(&str, &str)] = &[
    ("rcc.column.rect", ""),
    ("rcc.column.rect", "nx=3, ny=4, bar=20, hatch=0"),
    ("rcc.column.rect", "nx=4, ny=3, bar=20, bar2=16, ftin=0"),
    ("rcc.column.circular", ""),
    ("rcc.column.circular", "dia=600, bars=12, label=0"),
    ("rcc.beam.section", ""),
    ("rcc.beam.section", "ntop=3, nbot=5, slab=150, hatch=0"),
    ("rcc.beam.elevation", ""),
    ("rcc.beam.elevation", "span=6000, nxtop=0, nxbot=0, endn=3, dims=0"),
    ("rcc.stirrup.detail", ""),
    ("rcc.footing.plan", ""),
    ("rcc.footing.plan", "sloped=0, pcc=0"),
    ("rcc.footing.section", ""),
    ("rcc.footing.combined", ""),
    ("rcc.footing.combined", "pcc=0, dims=0, ftin=0"),
    ("rcc.raft.lift", ""),
    ("rcc.footing.section", "sloped=0, length=2400, spacing=100"),
    ("rcc.lintel.section", ""),
    ("rcc.lintel.section", "jamb=0, span=2000"),
    ("rcc.slab.section", ""),
    ("rcc.slab.section", "span=1200, spacing=100, hatch=0"),
];

fn counts(s: &mut Session, id: &str, params: &str) -> BTreeMap<&'static str, usize> {
    let r = s.execute("smartblock.insert", &json!({ "id": id, "at": [0, 0], "params": params })).unwrap();
    let h = aad_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let doc = s.doc().unwrap();
    let EntityKind::Insert(i) = &doc.entity(h).unwrap().kind else { panic!("not a block reference") };
    let mut out = BTreeMap::new();
    for e in doc.block(&i.block).unwrap().entities.iter() {
        let k = match &e.kind {
            EntityKind::Line(_) => "line",
            EntityKind::LwPolyline(p) if p.const_width > 0.0 => "dot",
            EntityKind::LwPolyline(_) => "poly",
            EntityKind::Circle(_) => "circle",
            EntityKind::Arc(_) => "arc",
            EntityKind::Text(_) => "text",
            EntityKind::Hatch(_) => "hatch",
            other => panic!("{id}: an object this check does not count: {other:?}"),
        };
        *out.entry(k).or_insert(0) += 1;
    }
    out
}

#[test]
fn the_engine_makes_what_the_pinned_counts_say() {
    let mut s = Session::empty();
    s.new_drawing(true);
    let mut now = Vec::new();
    for (id, params) in CASES {
        now.push(json!({ "id": id, "params": params, "counts": counts(&mut s, id, params) }));
    }
    let now = Value::Array(now);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rcc_counts.json");
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&now).unwrap() + "\n").unwrap();
    }
    let pinned: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("tests/fixtures/rcc_counts.json (UPDATE_FIXTURES=1 writes it)")).unwrap();
    assert_eq!(now, pinned, "a block changed: run with UPDATE_FIXTURES=1 and copy the file to MynaOps's test fixtures");
}
