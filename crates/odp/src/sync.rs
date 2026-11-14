//! The read-only side of live sync (`docs/live-sync.md`): look at a project file another program may be writing, without taking a lock, without
//! changing it, and without ever failing the caller. Every problem becomes a [`Peek`] value.

use std::path::Path;

use rusqlite::{Connection, ErrorCode, OpenFlags};

use super::{FORMAT, VERSION};

/// The name this program puts in `meta.sync.last` (MynaOps writes `mynaops`).
pub const SYNC_APP: &str = "mynacad";

/// How long a probe waits for a writer before it reports `Locked`.
const BUSY_MS: u64 = 250;
/// Longest `meta` value read (a hostile file cannot make the probe read more).
const MAX_VALUE: usize = 4096;
/// Most `sync.takeoff.*` stamps read.
const MAX_STAMPS: usize = 64;

/// What a take-off stamp says (`meta.sync.takeoff.<slot>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakeoffStamp {
    pub slot: u32,
    /// The file revision the take-off reflects.
    pub revision: i64,
    pub recorded: String,
    /// `(level id, revision)` for the levels that have rows.
    pub levels: Vec<(String, i64)>,
}

/// The facts a probe reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    pub project_id: String,
    pub revision: i64,
    /// `meta.sync.last`: the program that wrote the latest revision it knows of, that revision, and when.
    pub last_app: Option<String>,
    pub last_revision: Option<i64>,
    pub last_time: Option<String>,
    /// The newest `drawing.revision` (the revision at which a drawing was last saved), `None` when no drawing is stored.
    pub drawing_revision: Option<i64>,
    pub takeoff: Vec<TakeoffStamp>,
}

impl Probe {
    /// The slots whose take-off is older than the drawing (see the stale marker in `docs/live-sync.md`).
    pub fn stale_slots(&self) -> Vec<u32> {
        let Some(d) = self.drawing_revision else { return Vec::new() };
        self.takeoff.iter().filter(|t| d > t.revision).map(|t| t.slot).collect()
    }
}

/// The outcome of looking at a project file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Peek {
    Ok(Probe),
    Missing,
    /// Another program holds the file; try again later.
    Locked,
    /// Not a project file, damaged, cut short or newer than this program reads. The text says why.
    Unreadable(String),
}

fn classify(e: &rusqlite::Error) -> Peek {
    match e {
        rusqlite::Error::SqliteFailure(f, _) if matches!(f.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Peek::Locked,
        other => Peek::Unreadable(other.to_string()),
    }
}

fn meta(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    match conn.query_row("SELECT substr(value, 1, ?2) FROM meta WHERE key = ?1", rusqlite::params![key, MAX_VALUE as i64], |r| r.get::<_, String>(0)) {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Look at the project file at `path` (read only, no write lock, at most a quarter of a second of waiting).
pub fn probe(path: &Path) -> Peek {
    if !path.exists() {
        return Peek::Missing;
    }
    let conn = match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX) {
        Ok(c) => c,
        Err(e) => return classify(&e),
    };
    if let Err(e) = conn.busy_timeout(std::time::Duration::from_millis(BUSY_MS)) {
        return classify(&e);
    }
    match read(&conn) {
        Ok(p) => p,
        Err(e) => classify(&e),
    }
}

fn read(conn: &Connection) -> rusqlite::Result<Peek> {
    match meta(conn, "format")? {
        Some(f) if f == FORMAT => {}
        other => return Ok(Peek::Unreadable(format!("its format is {:?}", other.unwrap_or_default()))),
    }
    let version: i64 = meta(conn, "version")?.and_then(|v| v.parse().ok()).unwrap_or(0);
    if version > VERSION {
        return Ok(Peek::Unreadable(format!("the file is version {version}; this program reads version {VERSION}")));
    }
    let project_id = meta(conn, "project_id")?.unwrap_or_default();
    let revision: i64 = meta(conn, "revision")?.and_then(|v| v.parse().ok()).unwrap_or(0);
    let last: Option<serde_json::Value> = meta(conn, "sync.last")?.and_then(|v| serde_json::from_str(&v).ok());
    let text = |v: &Option<serde_json::Value>, k: &str| v.as_ref().and_then(|v| v.get(k)).and_then(|x| x.as_str()).map(|s| s.chars().take(64).collect::<String>());
    let last_app = text(&last, "app");
    let last_time = text(&last, "time");
    let last_revision = last.as_ref().and_then(|v| v.get("revision")).and_then(|x| x.as_i64());
    // A file from an older program may lack a table; that is "nothing stored", not an error.
    let drawing_revision = match conn.query_row("SELECT max(revision) FROM drawing", [], |r| r.get::<_, Option<i64>>(0)) {
        Ok(v) => v,
        Err(rusqlite::Error::SqliteFailure(f, _)) if f.code == ErrorCode::Unknown => None,
        Err(e) => return Err(e),
    };
    let mut takeoff = Vec::new();
    let mut st = conn.prepare("SELECT key, substr(value, 1, ?1) FROM meta WHERE key LIKE 'sync.takeoff.%' LIMIT ?2")?;
    let rows = st.query_map(rusqlite::params![MAX_VALUE as i64, MAX_STAMPS as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (key, value) = row?;
        let Some(slot) = key.strip_prefix("sync.takeoff.").and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&value) else { continue };
        let Some(rev) = v.get("revision").and_then(|x| x.as_i64()) else { continue };
        let recorded = v.get("recorded").and_then(|x| x.as_str()).unwrap_or_default().chars().take(64).collect();
        let levels = v
            .get("levels")
            .and_then(|l| l.as_object())
            .map(|m| m.iter().take(256).map(|(k, x)| (k.clone(), x.get("revision").and_then(|r| r.as_i64()).unwrap_or(rev))).collect())
            .unwrap_or_default();
        takeoff.push(TakeoffStamp { slot, revision: rev, recorded, levels });
    }
    Ok(Peek::Ok(Probe { project_id, revision, last_app, last_revision, last_time, drawing_revision, takeoff }))
}
