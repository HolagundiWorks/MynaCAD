//! The tables in the project file (`.osp`), all prefixed `geo_`.
//!
//! The geometry tables are derived: they can be dropped and rebuilt at any time, and a file whose
//! `geo_schema` version differs from [`SCHEMA_VERSION`] has them dropped and recreated. Two
//! tables hold a person's data and survive a rebuild: `geo_rule` (the parameters) and
//! `geo_review` (decisions keyed by the feature's geometry hash). Nothing here touches the
//! `change` log, so none of it syncs anywhere.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::{Value, json};

use crate::model::*;
use crate::rules::{Rules, SECTIONS};
use crate::{Delta, Error, GeoModel, PairRow, Result, SCHEMA_VERSION};

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Db(e.to_string())
    }
}

const DERIVED: [&str; 11] = [
    "geo_run",
    "geo_prim",
    "geo_seg",
    "geo_arc",
    "geo_insert",
    "geo_text",
    "geo_hatch",
    "geo_pair",
    "geo_feature",
    "geo_feature_member",
    "geo_rtree",
];

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS geo_schema (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS geo_rule (id TEXT PRIMARY KEY, kind TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, params_json TEXT NOT NULL, version INTEGER NOT NULL, weight REAL NOT NULL DEFAULT 1.0) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS geo_review (feature_key TEXT PRIMARY KEY, state TEXT NOT NULL, note TEXT NOT NULL DEFAULT '', by TEXT NOT NULL DEFAULT '', at TEXT NOT NULL DEFAULT '') WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS geo_run (id INTEGER PRIMARY KEY, built_utc TEXT NOT NULL, drawing TEXT NOT NULL, drawing_sha256 TEXT NOT NULL, revision INTEGER NOT NULL,
    unit_scale REAL NOT NULL, unit TEXT NOT NULL, system TEXT NOT NULL, units_confirmed INTEGER NOT NULL, rules_version INTEGER NOT NULL, info_json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS geo_prim (id INTEGER PRIMARY KEY, handle TEXT NOT NULL, kind TEXT NOT NULL, layer TEXT NOT NULL, color INTEGER NOT NULL, linetype TEXT NOT NULL,
    lineweight INTEGER NOT NULL, closed INTEGER NOT NULL, space TEXT NOT NULL, block_path TEXT NOT NULL, minx REAL, miny REAL, maxx REAL, maxy REAL);
CREATE INDEX IF NOT EXISTS geo_prim_handle ON geo_prim (handle);
CREATE TABLE IF NOT EXISTS geo_seg (id INTEGER PRIMARY KEY, prim_id INTEGER NOT NULL, seq INTEGER NOT NULL, x1 REAL, y1 REAL, x2 REAL, y2 REAL, len REAL, angle REAL, nx REAL, ny REAL,
    qx1 INTEGER, qy1 INTEGER, qx2 INTEGER, qy2 INTEGER);
CREATE INDEX IF NOT EXISTS geo_seg_prim ON geo_seg (prim_id);
CREATE TABLE IF NOT EXISTS geo_arc (id INTEGER PRIMARY KEY, prim_id INTEGER NOT NULL, cx REAL, cy REAL, r REAL, a0 REAL, a1 REAL, qcx INTEGER, qcy INTEGER, qr INTEGER);
CREATE INDEX IF NOT EXISTS geo_arc_prim ON geo_arc (prim_id);
CREATE TABLE IF NOT EXISTS geo_insert (id INTEGER PRIMARY KEY, prim_id INTEGER NOT NULL, block TEXT NOT NULL, x REAL, y REAL, rot REAL, sx REAL, sy REAL, width REAL, depth REAL,
    shape_hash TEXT NOT NULL, attrs_json TEXT NOT NULL, nested INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS geo_insert_prim ON geo_insert (prim_id);
CREATE TABLE IF NOT EXISTS geo_text (id INTEGER PRIMARY KEY, prim_id INTEGER NOT NULL, text TEXT NOT NULL, x REAL, y REAL, height REAL, rot REAL);
CREATE INDEX IF NOT EXISTS geo_text_prim ON geo_text (prim_id);
CREATE TABLE IF NOT EXISTS geo_hatch (id INTEGER PRIMARY KEY, prim_id INTEGER NOT NULL, pattern TEXT NOT NULL, solid INTEGER NOT NULL, area REAL, loops_json TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS geo_hatch_prim ON geo_hatch (prim_id);
CREATE TABLE IF NOT EXISTS geo_pair (id INTEGER PRIMARY KEY, seg_a INTEGER NOT NULL, seg_b INTEGER NOT NULL, gap REAL, overlap REAL, overlap_ratio REAL,
    axis_x1 REAL, axis_y1 REAL, axis_x2 REAL, axis_y2 REAL, band INTEGER, ladder INTEGER);
CREATE TABLE IF NOT EXISTS geo_feature (id INTEGER PRIMARY KEY, feature_key TEXT NOT NULL, kind TEXT NOT NULL, subkind TEXT NOT NULL, confidence REAL NOT NULL, rule_id TEXT NOT NULL,
    rules_version INTEGER NOT NULL, x1 REAL, y1 REAL, x2 REAL, y2 REAL, width REAL, depth REAL, length REAL, props_json TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS geo_feature_key ON geo_feature (feature_key);
CREATE TABLE IF NOT EXISTS geo_feature_member (feature_id INTEGER NOT NULL, prim_id INTEGER NOT NULL, handle TEXT NOT NULL, role TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS geo_member_feature ON geo_feature_member (feature_id);
";

const RTREE: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS geo_rtree USING rtree(id, minx, maxx, miny, maxy)";

/// Open the project file for the tables (WAL, a second writer waits instead of failing).
pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    ensure_schema(&conn)?;
    Ok(conn)
}

/// Create the tables; drop and recreate the derived ones when the stored version differs.
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS geo_schema (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;")?;
    let have: Option<u32> =
        conn.query_row("SELECT value FROM geo_schema WHERE key = 'version'", [], |r| r.get::<_, String>(0)).optional()?.and_then(|v| v.parse().ok());
    if have.is_some_and(|v| v != SCHEMA_VERSION) {
        for t in DERIVED {
            conn.execute_batch(&format!("DROP TABLE IF EXISTS {t};"))?;
        }
    }
    conn.execute_batch(SCHEMA)?;
    // The R-tree needs the SQLite R*Tree module; the bundled build has it. A build without it still works, only without the index table.
    let _ = conn.execute_batch(RTREE);
    conn.execute("INSERT OR REPLACE INTO geo_schema (key, value) VALUES ('version', ?1)", params![SCHEMA_VERSION.to_string()])?;
    Ok(())
}

pub fn has_rtree(conn: &Connection) -> bool {
    conn.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'geo_rtree'", [], |r| r.get::<_, i64>(0)).map(|n| n > 0).unwrap_or(false)
}

// ---- rules ----

pub fn load_rules(conn: &Connection) -> Result<Option<Rules>> {
    let mut whole = serde_json::to_value(Rules::default()).map_err(|e| Error::Invalid(e.to_string()))?;
    let mut st = conn.prepare("SELECT id, params_json, version FROM geo_rule")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?;
    let (mut any, mut version) = (false, 1i64);
    for row in rows {
        let (id, json, v) = row?;
        if let (Some(obj), Ok(val)) = (whole.as_object_mut(), serde_json::from_str::<Value>(&json))
            && SECTIONS.contains(&id.as_str())
        {
            obj.insert(id, val);
            any = true;
            version = version.max(v);
        }
    }
    if !any {
        return Ok(None);
    }
    let mut rules: Rules = serde_json::from_value(whole).map_err(|e| Error::Invalid(format!("stored rules: {e}")))?;
    rules.version = version.max(1) as u32;
    rules.validate()?;
    Ok(Some(rules))
}

pub fn save_rules(conn: &Connection, rules: &Rules) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for kind in SECTIONS {
        if let Some(v) = rules.section(kind) {
            tx.execute(
                "INSERT OR REPLACE INTO geo_rule (id, kind, enabled, params_json, version, weight) VALUES (?1, ?1, 1, ?2, ?3, 1.0)",
                params![kind, v.to_string(), rules.version],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

// ---- reviews ----

pub fn review_set(conn: &Connection, key: &str, state: &str, note: &str, by: &str, at: &str) -> Result<()> {
    if !matches!(state, "accepted" | "rejected" | "edited") {
        return Err(Error::Invalid("state must be accepted, rejected or edited".into()));
    }
    conn.execute(
        "INSERT OR REPLACE INTO geo_review (feature_key, state, note, by, at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![key, state, note, by, at],
    )?;
    Ok(())
}

pub fn review_clear(conn: &Connection, key: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM geo_review WHERE feature_key = ?1", params![key])?)
}

pub fn reviews(conn: &Connection) -> Result<Vec<(String, String, String)>> {
    let mut st = conn.prepare("SELECT feature_key, state, note FROM geo_review ORDER BY feature_key")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

// ---- meta (drafting units, origin stamp) ----

/// Write a key to the project file's `meta` table when it has one. Not part of the `change` log.
pub fn write_meta(conn: &Connection, key: &str, value: &str) -> Result<bool> {
    let has: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'meta'", [], |r| r.get(0))?;
    if has == 0 {
        return Ok(false);
    }
    conn.execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)", params![key, value])?;
    Ok(true)
}

pub fn read_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    let has: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'meta'", [], |r| r.get(0))?;
    if has == 0 {
        return Ok(None);
    }
    Ok(conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0)).optional()?)
}

// ---- writing ----

#[derive(Clone, Debug, Default)]
pub struct WriteStats {
    pub rows: usize,
    pub ms: u128,
    pub rtree: bool,
}

fn hex(h: u64) -> String {
    format!("{h:X}")
}

fn insert_owner(tx: &Transaction, handle: u64, ex: &Extracted, rtree: bool) -> Result<usize> {
    let mut n = 0;
    let h = hex(handle);
    {
        let mut st = tx.prepare_cached("INSERT INTO geo_prim (id, handle, kind, layer, color, linetype, lineweight, closed, space, block_path, minx, miny, maxx, maxy) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'model',?9,?10,?11,?12,?13)")?;
        let mut rt = if rtree { tx.prepare_cached("INSERT INTO geo_rtree (id, minx, maxx, miny, maxy) VALUES (?1,?2,?3,?4,?5)").ok() } else { None };
        for p in &ex.prims {
            let ok = p.bbox[0] <= p.bbox[2] && p.bbox.iter().all(|v| v.is_finite());
            let b = if ok { p.bbox } else { [0.0; 4] };
            st.execute(params![
                p.id as i64,
                h,
                p.kind.as_str(),
                &*p.layer,
                p.color,
                &*p.linetype,
                p.lineweight,
                p.closed,
                &*p.block_path,
                b[0],
                b[1],
                b[2],
                b[3]
            ])?;
            if ok && let Some(rt) = rt.as_mut() {
                rt.execute(params![p.id as i64, b[0], b[2], b[1], b[3]])?;
            }
            n += 1;
        }
    }
    {
        let mut st = tx.prepare_cached("INSERT INTO geo_seg (id, prim_id, seq, x1, y1, x2, y2, len, angle, nx, ny, qx1, qy1, qx2, qy2) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)")?;
        for s in &ex.segs {
            st.execute(params![
                s.id as i64,
                s.prim as i64,
                s.seq,
                s.x1,
                s.y1,
                s.x2,
                s.y2,
                s.len,
                s.angle,
                s.nx,
                s.ny,
                q(s.x1),
                q(s.y1),
                q(s.x2),
                q(s.y2)
            ])?;
            n += 1;
        }
    }
    {
        let mut st =
            tx.prepare_cached("INSERT INTO geo_arc (id, prim_id, cx, cy, r, a0, a1, qcx, qcy, qr) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)")?;
        for a in &ex.arcs {
            st.execute(params![a.id as i64, a.prim as i64, a.cx, a.cy, a.r, a.a0, a.a1, q(a.cx), q(a.cy), q(a.r)])?;
            n += 1;
        }
    }
    {
        let mut st = tx.prepare_cached("INSERT INTO geo_insert (id, prim_id, block, x, y, rot, sx, sy, width, depth, shape_hash, attrs_json, nested) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)")?;
        for i in &ex.inserts {
            let attrs = serde_json::to_string(&i.attrs).unwrap_or_else(|_| "{}".into());
            st.execute(params![
                i.id as i64,
                i.prim as i64,
                &*i.block,
                i.x,
                i.y,
                i.rot,
                i.sx,
                i.sy,
                i.width,
                i.depth,
                format!("{:016x}", i.shape_hash),
                attrs,
                i.nested
            ])?;
            n += 1;
        }
    }
    {
        let mut st = tx.prepare_cached("INSERT INTO geo_text (id, prim_id, text, x, y, height, rot) VALUES (?1,?2,?3,?4,?5,?6,?7)")?;
        for t in &ex.texts {
            st.execute(params![t.id as i64, t.prim as i64, t.text, t.x, t.y, t.height, t.rot])?;
            n += 1;
        }
    }
    {
        let mut st = tx.prepare_cached("INSERT INTO geo_hatch (id, prim_id, pattern, solid, area, loops_json) VALUES (?1,?2,?3,?4,?5,?6)")?;
        for hh in &ex.hatches {
            let loops = serde_json::to_string(&hh.loops).unwrap_or_else(|_| "[]".into());
            st.execute(params![hh.id as i64, hh.prim as i64, hh.pattern, hh.solid, hh.area, loops])?;
            n += 1;
        }
    }
    Ok(n)
}

fn insert_feature(tx: &Transaction, f: &Feature) -> Result<()> {
    tx.prepare_cached("INSERT INTO geo_feature (id, feature_key, kind, subkind, confidence, rule_id, rules_version, x1, y1, x2, y2, width, depth, length, props_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)")?
        .execute(params![f.id as i64, f.key, f.kind, f.subkind, f.confidence, f.rule, f.rules_version, f.x1, f.y1, f.x2, f.y2, f.width, f.depth, f.length, f.props.to_string()])?;
    let mut st = tx.prepare_cached("INSERT INTO geo_feature_member (feature_id, prim_id, handle, role) VALUES (?1,?2,?3,?4)")?;
    for m in &f.members {
        st.execute(params![f.id as i64, m.prim as i64, hex(m.handle), m.role])?;
    }
    Ok(())
}

fn insert_pair(tx: &Transaction, p: &PairRow) -> Result<()> {
    tx.prepare_cached("INSERT INTO geo_pair (id, seg_a, seg_b, gap, overlap, overlap_ratio, axis_x1, axis_y1, axis_x2, axis_y2, band, ladder) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)")?
        .execute(params![p.id as i64, p.seg_a as i64, p.seg_b as i64, p.gap, p.overlap, p.overlap_ratio, p.axis[0], p.axis[1], p.axis[2], p.axis[3], p.band as i64, p.ladder])?;
    Ok(())
}

fn write_run(tx: &Transaction, m: &GeoModel, drawing: &str, revision: i64) -> Result<()> {
    tx.execute("DELETE FROM geo_run", [])?;
    let info = json!({ "summary": m.summary() });
    tx.execute(
        "INSERT INTO geo_run (built_utc, drawing, drawing_sha256, revision, unit_scale, unit, system, units_confirmed, rules_version, info_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![m.built_utc, drawing, m.sha256, revision, m.units.to_mm, m.units.unit, m.units.system.as_str(), m.units.confirmed, m.rules.version, info.to_string()],
    )?;
    Ok(())
}

/// Replace every derived table with this model in one bulk transaction.
pub fn write_full(conn: &mut Connection, m: &GeoModel, drawing: &str, revision: i64) -> Result<WriteStats> {
    let t = std::time::Instant::now();
    let rtree = has_rtree(conn);
    let tx = conn.transaction()?;
    for tname in DERIVED {
        if tname != "geo_rtree" || rtree {
            tx.execute(&format!("DELETE FROM {tname}"), [])?;
        }
    }
    let mut rows = 0;
    for (h, ex) in &m.owners {
        rows += insert_owner(&tx, *h, ex, rtree)?;
    }
    for p in &m.pair_rows {
        insert_pair(&tx, p)?;
        rows += 1;
    }
    for f in &m.features {
        insert_feature(&tx, f)?;
        rows += 1;
    }
    write_run(&tx, m, drawing, revision)?;
    tx.commit()?;
    Ok(WriteStats { rows, ms: t.elapsed().as_millis(), rtree })
}

/// Apply an incremental update to the stored copy: drop the rows of the dirty entities, add their new rows.
pub fn write_update(conn: &mut Connection, m: &GeoModel, delta: &Delta, drawing: &str, revision: i64) -> Result<WriteStats> {
    let t = std::time::Instant::now();
    let rtree = has_rtree(conn);
    let tx = conn.transaction()?;
    for h in &delta.dropped_handles {
        let hs = hex(*h);
        for tname in ["geo_seg", "geo_arc", "geo_insert", "geo_text", "geo_hatch"] {
            tx.execute(&format!("DELETE FROM {tname} WHERE prim_id IN (SELECT id FROM geo_prim WHERE handle = ?1)"), params![hs])?;
        }
        if rtree {
            tx.execute("DELETE FROM geo_rtree WHERE id IN (SELECT id FROM geo_prim WHERE handle = ?1)", params![hs])?;
        }
        tx.execute("DELETE FROM geo_prim WHERE handle = ?1", params![hs])?;
    }
    let mut rows = 0;
    for h in &delta.added_handles {
        if let Some(ex) = m.owners.get(h) {
            rows += insert_owner(&tx, *h, ex, rtree)?;
        }
    }
    for id in &delta.dropped_features {
        tx.execute("DELETE FROM geo_feature_member WHERE feature_id = ?1", params![*id as i64])?;
        tx.execute("DELETE FROM geo_feature WHERE id = ?1", params![*id as i64])?;
    }
    let added: std::collections::HashSet<u64> = delta.added_features.iter().copied().collect();
    for f in m.features.iter().filter(|f| added.contains(&f.id)) {
        insert_feature(&tx, f)?;
        rows += 1;
    }
    if let Some(r) = delta.region {
        tx.execute(
            "DELETE FROM geo_pair WHERE max(axis_x1, axis_x2) >= ?1 AND min(axis_x1, axis_x2) <= ?3 AND max(axis_y1, axis_y2) >= ?2 AND min(axis_y1, axis_y2) <= ?4",
            params![r[0], r[1], r[2], r[3]],
        )?;
        for p in m.pair_rows.iter().filter(|p| {
            p.axis[0].min(p.axis[2]) <= r[2]
                && p.axis[0].max(p.axis[2]) >= r[0]
                && p.axis[1].min(p.axis[3]) <= r[3]
                && p.axis[1].max(p.axis[3]) >= r[1]
        }) {
            insert_pair(&tx, p)?;
            rows += 1;
        }
    }
    write_run(&tx, m, drawing, revision)?;
    tx.commit()?;
    Ok(WriteStats { rows, ms: t.elapsed().as_millis(), rtree })
}

/// Counts straight from the stored tables (a report without the in-memory model).
pub fn read_summary(conn: &Connection) -> Result<Value> {
    let run = conn
        .query_row("SELECT built_utc, drawing, drawing_sha256, revision, unit, system, units_confirmed, rules_version, info_json FROM geo_run LIMIT 1", [], |r| {
            Ok(json!({
                "builtUtc": r.get::<_, String>(0)?, "drawing": r.get::<_, String>(1)?, "sha256": r.get::<_, String>(2)?, "revision": r.get::<_, i64>(3)?,
                "unit": r.get::<_, String>(4)?, "system": r.get::<_, String>(5)?, "unitsConfirmed": r.get::<_, bool>(6)?, "rulesVersion": r.get::<_, i64>(7)?,
                "info": serde_json::from_str::<Value>(&r.get::<_, String>(8)?).unwrap_or(Value::Null),
            }))
        })
        .optional()?;
    let count = |t: &str| -> Result<i64> { Ok(conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))?) };
    let mut feats = Vec::new();
    let mut st = conn.prepare("SELECT kind, subkind, count(*), avg(confidence) FROM geo_feature GROUP BY kind, subkind ORDER BY kind, subkind")?;
    for row in st.query_map([], |r| Ok(json!({ "kind": format!("{}.{}", r.get::<_, String>(0)?, r.get::<_, String>(1)?), "found": r.get::<_, i64>(2)?, "meanConfidence": r.get::<_, f64>(3)? })))? {
        feats.push(row?);
    }
    Ok(json!({
        "run": run,
        "rows": { "prims": count("geo_prim")?, "segs": count("geo_seg")?, "arcs": count("geo_arc")?, "inserts": count("geo_insert")?, "texts": count("geo_text")?, "hatches": count("geo_hatch")?, "pairs": count("geo_pair")?, "features": count("geo_feature")? },
        "features": feats,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_created_rebuilt_on_a_new_version_and_keeps_rules_and_reviews() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.osp");
        let conn = open(&path).unwrap();
        assert!(has_rtree(&conn), "the bundled SQLite has the R*Tree module");
        let mut r = Rules::default();
        r.set_section("pair", &json!({"tolerance_mm": 6.0})).unwrap();
        save_rules(&conn, &r).unwrap();
        review_set(&conn, "abc", "rejected", "not a wall", "test", "now").unwrap();
        assert!(review_set(&conn, "abc", "maybe", "", "", "").is_err());
        conn.execute("INSERT INTO geo_run (built_utc, drawing, drawing_sha256, revision, unit_scale, unit, system, units_confirmed, rules_version, info_json) VALUES ('0','d','h',1,1,'mm','metric',1,1,'{}')", []).unwrap();
        conn.execute("UPDATE geo_schema SET value = '0' WHERE key = 'version'", []).unwrap();
        drop(conn);
        let conn = open(&path).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM geo_run", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "an old schema version drops the derived tables");
        let back = load_rules(&conn).unwrap().unwrap();
        assert_eq!(back.pair.tolerance_mm, 6.0);
        assert_eq!(back.version, r.version);
        assert_eq!(reviews(&conn).unwrap().len(), 1, "a person's decision survives");
    }

    #[test]
    fn meta_is_written_only_when_the_project_has_a_meta_table() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("p.osp")).unwrap();
        assert!(!write_meta(&conn, "drafting_units", "metric").unwrap(), "a bare file has no meta table");
        conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;").unwrap();
        assert!(write_meta(&conn, "drafting_units", "metric").unwrap());
        assert_eq!(read_meta(&conn, "drafting_units").unwrap().as_deref(), Some("metric"));
    }
}
