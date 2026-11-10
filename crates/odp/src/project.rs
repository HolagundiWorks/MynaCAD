//! The `.osp` project file (format `osp`, version 1): one SQLite file that holds everything about a project, so the same file opens
//! as a drawing in MynaCAD, as a take-off and a project in MynaOps, and in a viewer on another device. Spec: `docs/ODP-PROJECT-FILE-AND-SYNC.md`
//! (OSAS repository).
//!
//! The file is written in WAL mode; a writer takes an immediate transaction, so two programs can have it open and one writes while the other reads.
//! Every committed change raises `meta.revision` by one and leaves `change` rows saying what changed, which is what sync reads.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

#[path = "import.rs"]
mod import;
#[path = "time.rs"]
mod time;

pub use import::PDF_CHUNK;

/// `meta.format`.
pub const FORMAT: &str = "osp";
/// The schema version this crate writes and reads.
pub const VERSION: i64 = 1;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not a project file: {0}")]
    NotAProject(String),
    #[error("the file is version {0}; this program reads version 1")]
    TooNew(i64),
    #[error("{0}")]
    Import(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Facts from `meta`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    pub project_id: String,
    pub name: String,
    pub created: String,
    pub revision: i64,
}

/// One drawing stored in the file: the user's own DWG or DXF bytes, untouched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawing {
    pub name: String,
    /// `dwg` or `dxf`, lower case, no dot.
    pub ext: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

/// A take-off row: the same JSON the bridge file's row has (`walls[]`, `openings[]`, ...). `kind` follows the take-off record kinds (3 wall, 4 opening,
/// 5 column, 6 lintel, 7 room, 8 slab, 10 board, 11 circuit); `n` is the item number inside the drawing slot.
#[derive(Clone, Debug, PartialEq)]
pub struct TakeoffRow {
    pub slot: u32,
    pub kind: u8,
    pub n: u32,
    pub json: serde_json::Value,
}

/// The header of one drawing's take-off (`state` is `recording` or `complete`).
#[derive(Clone, Debug, PartialEq)]
pub struct TakeoffHeader {
    pub slot: u32,
    pub state: String,
    pub json: serde_json::Value,
}

/// A viewer markup or comment.
#[derive(Clone, Debug, PartialEq)]
pub struct Markup {
    pub id: String,
    pub drawing: String,
    pub author: String,
    pub created: String,
    pub kind: String,
    pub json: serde_json::Value,
    pub text: String,
    pub resolved: bool,
}

/// A structural detail drawn in MynaCAD: one parametric RCC block placed in a drawing, with the parameters it was drawn with. MynaCAD records
/// these when it saves the drawing into the file; MynaOps reads them and draws each one read only as the detail of its beam, column, slab or footing.
/// The drawing is never changed from MynaOps. `kind` is `beams`, `columns`, `slabs`, `footings` or `lintels` (the MynaOps element pages).
#[derive(Clone, Debug, PartialEq)]
pub struct Detail {
    /// `<drawing>/<handle>`: unique in the file.
    pub id: String,
    pub drawing: String,
    pub handle: String,
    pub kind: String,
    /// The block definition (`rcc.beam.section`, ...).
    pub block: String,
    /// Parameter values by name (millimetres, grades as numbers), as the block was placed.
    pub params: serde_json::Value,
    pub layer: String,
    /// The block's title with its main size, for a list.
    pub label: String,
}

/// A stored attachment (an annotated PDF, a photo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub id: String,
    pub slot: Option<u32>,
    pub name: String,
    pub mime: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

/// The time now as UTC text (`2026-10-30T07:16:41Z`), the form every timestamp in the file has.
pub fn now_utc() -> String {
    time::now_rfc3339()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS drawing (
    name TEXT PRIMARY KEY, ext TEXT NOT NULL, sha256 TEXT NOT NULL, bytes BLOB NOT NULL, revision INTEGER NOT NULL, created TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS drawing_state (drawing TEXT PRIMARY KEY, json TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS takeoff_header (slot INTEGER PRIMARY KEY, state TEXT NOT NULL, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS takeoff_row (
    slot INTEGER NOT NULL, kind INTEGER NOT NULL, n INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY (slot, kind, n)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS project_doc (id INTEGER PRIMARY KEY CHECK (id = 1), json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS asset (
    id TEXT PRIMARY KEY, slot INTEGER, name TEXT NOT NULL, mime TEXT NOT NULL, sha256 TEXT NOT NULL, bytes BLOB NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS markup (
    id TEXT PRIMARY KEY, drawing TEXT NOT NULL, author TEXT NOT NULL, created TEXT NOT NULL, kind TEXT NOT NULL,
    json TEXT NOT NULL, text TEXT NOT NULL DEFAULT '', resolved INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS detail (
    id TEXT PRIMARY KEY, drawing TEXT NOT NULL, handle TEXT NOT NULL, kind TEXT NOT NULL, block TEXT NOT NULL, params TEXT NOT NULL,
    layer TEXT NOT NULL, label TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS history (id INTEGER PRIMARY KEY, created TEXT NOT NULL, label TEXT NOT NULL, bytes BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS change (revision INTEGER NOT NULL, tbl TEXT NOT NULL, key TEXT NOT NULL, op TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS change_revision ON change (revision);
CREATE TABLE IF NOT EXISTS history_record (id INTEGER PRIMARY KEY, payload BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS native_blob (id INTEGER PRIMARY KEY, hash TEXT NOT NULL UNIQUE, bytes BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS native_part (
    drawing TEXT NOT NULL, seq INTEGER NOT NULL, hash TEXT NOT NULL, entities INTEGER NOT NULL, PRIMARY KEY (drawing, seq)
) WITHOUT ROWID;
";

/// The `meta` keys that describe the file itself. [`Odp::set_local_meta`] never writes them.
const CORE_META: &[&str] = &["format", "version", "project_id", "name", "created", "revision"];

/// What a file says about its native model (see [`Edit::put_native`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeStamp {
    /// The encoding of the parts (the writer's `aad_io::native::FORMAT_VERSION`).
    pub format: u32,
    /// The drawing the model belongs to.
    pub drawing: String,
    /// The SHA-256 of the `drawing` row the model was made from. A reader uses the model only while the row still has this hash, so a drawing
    /// replaced by another program (or by an older MynaCAD) is read from its own bytes.
    pub drawing_sha256: String,
    pub parts: usize,
    pub entities: usize,
}

/// One part of a native model handed to [`Edit::put_native`].
#[derive(Clone, Copy, Debug)]
pub struct NativePart<'a> {
    /// The name the bytes are kept under: the SHA-256 of `bytes` as lower-case hex (the encoder computes it once).
    pub hash: &'a str,
    pub bytes: &'a [u8],
    pub entities: usize,
}

/// What [`Edit::put_native`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeWrite {
    pub parts: usize,
    /// Parts whose bytes were not in the file yet.
    pub written: usize,
    pub bytes_written: usize,
}

fn read_details(conn: &Connection) -> Result<Vec<Detail>> {
    let mut st = conn.prepare("SELECT id, drawing, handle, kind, block, params, layer, label FROM detail ORDER BY id")?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, drawing, handle, kind, block, params, layer, label) = row?;
        out.push(Detail { id, drawing, handle, kind, block, params: parse_json(&params)?, layer, label });
    }
    Ok(out)
}

/// An open project file.
pub struct Odp {
    conn: Connection,
}

/// A group of changes that become one revision when [`Edit::commit`] is called. Dropping it without committing undoes everything.
pub struct Edit<'a> {
    tx: Transaction<'a>,
    revision: i64,
    changed: bool,
    /// Data local to this computer was written (the native model, local `meta` keys): committed, but not a revision.
    local: bool,
}

impl Odp {
    /// Create a new project file (it must not exist).
    pub fn create(path: &Path, name: &str) -> Result<Odp> {
        Self::create_with(path, name, false)
    }

    /// Create a new project file that nobody else can see yet (a temporary name), for writing a lot at once: no journal and no flushes while it is
    /// filled, so a large drawing is written once instead of twice. The file is not safe against a crash until [`Odp::finish_bulk`] has returned;
    /// the caller renames it into place only after that.
    pub fn create_bulk(path: &Path, name: &str) -> Result<Odp> {
        Self::create_with(path, name, true)
    }

    /// End a [`Odp::create_bulk`]: switch the file to the normal write-ahead mode, close it and flush it to the disk.
    pub fn finish_bulk(self, path: &Path) -> Result<()> {
        Self::tune(&self.conn)?;
        self.conn.close().map_err(|(_, e)| Error::Sqlite(e))?;
        std::fs::OpenOptions::new().read(true).write(true).open(path)?.sync_all()?;
        Ok(())
    }

    fn create_with(path: &Path, name: &str, bulk: bool) -> Result<Odp> {
        if path.exists() {
            return Err(Error::NotAProject(format!("{} already exists", path.display())));
        }
        let conn = Connection::open(path)?;
        if bulk {
            conn.pragma_update(None, "journal_mode", "OFF")?;
            conn.pragma_update(None, "synchronous", "OFF")?;
        } else {
            Self::tune(&conn)?;
        }
        conn.execute_batch(SCHEMA)?;
        let now = time::now_rfc3339();
        let project_id = {
            let seed = format!("{name}|{now}|{}|{}", std::process::id(), path.display());
            sha256_hex(seed.as_bytes())[..32].to_string()
        };
        for (k, v) in [
            ("format", FORMAT.to_string()),
            ("version", VERSION.to_string()),
            ("project_id", project_id),
            ("name", name.to_string()),
            ("created", now),
            ("revision", "0".to_string()),
        ] {
            conn.execute("INSERT INTO meta (key, value) VALUES (?1, ?2)", params![k, v])?;
        }
        Ok(Odp { conn })
    }

    /// Open an existing project file for reading and writing.
    pub fn open(path: &Path) -> Result<Odp> {
        if !path.exists() {
            return Err(Error::NotAProject(format!("{} does not exist", path.display())));
        }
        let conn = Connection::open(path)?;
        Self::tune(&conn).map_err(|_| Error::NotAProject("it is not a SQLite file".into()))?;
        let odp = Odp { conn };
        let format = odp.meta_value("format").map_err(|_| Error::NotAProject("it has no meta table".into()))?;
        if !matches!(format.as_deref(), Some(FORMAT) | Some("osas-project")) {
            return Err(Error::NotAProject(format!("its format is {format:?}")));
        }
        let version: i64 = odp.meta_value("version")?.and_then(|v| v.parse().ok()).unwrap_or(0);
        if version > VERSION {
            return Err(Error::TooNew(version));
        }
        odp.conn.execute_batch(SCHEMA)?; // a file from an older version gets the tables it lacks
        Ok(odp)
    }

    fn tune(conn: &Connection) -> Result<()> {
        // WAL: one writer with readers beside it. A second writer waits (up to 5 s) instead of failing.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(())
    }

    fn meta_value(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0)).optional()?)
    }

    pub fn meta(&self) -> Result<Meta> {
        let get = |k: &str| self.meta_value(k).map(Option::unwrap_or_default);
        Ok(Meta { project_id: get("project_id")?, name: get("name")?, created: get("created")?, revision: get("revision")?.parse().unwrap_or(0) })
    }

    /// A `meta` value by key (`None` when the key is absent).
    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        self.meta_value(key)
    }

    /// Write a `meta` value that is local to this computer (where a drawing was imported from, when it was saved). It is not a revision and is not
    /// in the change log, so sync never sends it. The keys that describe the file itself cannot be written this way.
    pub fn set_local_meta(&mut self, pairs: &[(&str, &str)]) -> Result<()> {
        let mut edit = self.edit()?;
        for (k, v) in pairs {
            edit.set_local_meta(k, v)?;
        }
        edit.commit().map(|_| ())
    }

    /// The stored hash of a drawing, without reading its bytes.
    pub fn drawing_sha256(&self, name: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT sha256 FROM drawing WHERE name = ?1", params![name], |r| r.get(0)).optional()?)
    }

    /// The stamp of the native model in the file, if it has one.
    pub fn native_stamp(&self) -> Result<Option<NativeStamp>> {
        let (Some(format), Some(drawing), Some(sha)) =
            (self.meta_value("native.format")?, self.meta_value("native.drawing")?, self.meta_value("native.drawing_sha256")?)
        else {
            return Ok(None);
        };
        let Ok(format) = format.parse::<u32>() else { return Ok(None) };
        let (parts, entities): (i64, i64) =
            self.conn.query_row("SELECT COUNT(*), COALESCE(SUM(entities), 0) FROM native_part WHERE drawing = ?1", params![drawing], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        if parts <= 0 {
            return Ok(None);
        }
        Ok(Some(NativeStamp { format, drawing, drawing_sha256: sha, parts: parts.max(0) as usize, entities: entities.max(0) as usize }))
    }

    /// The parts of the native model of `drawing`, in order. A part whose bytes are missing is an error: the caller reads the drawing's own bytes.
    pub fn native_parts(&self, drawing: &str) -> Result<Vec<Vec<u8>>> {
        let mut st = self
            .conn
            .prepare("SELECT p.seq, b.bytes FROM native_part p LEFT JOIN native_blob b ON b.hash = p.hash WHERE p.drawing = ?1 ORDER BY p.seq")?;
        let rows = st.query_map(params![drawing], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<Vec<u8>>>(1)?)))?;
        let mut out = Vec::new();
        for (expect, row) in rows.enumerate() {
            let (seq, bytes) = row?;
            if seq != expect as i64 {
                return Err(Error::Import(format!("the native model is incomplete (part {expect} is missing)")));
            }
            out.push(bytes.ok_or_else(|| Error::Import(format!("the native model is incomplete (part {seq} has no bytes)")))?);
        }
        Ok(out)
    }

    /// Copy the whole file, as it is now, to `to` (which must not exist), safely while it is open here or elsewhere: SQLite's `VACUUM INTO` reads
    /// one consistent state and writes a compact file. The copy is complete or absent; the caller renames it into place.
    pub fn copy_to(&self, to: &Path) -> Result<()> {
        if to.exists() {
            return Err(Error::NotAProject(format!("{} already exists", to.display())));
        }
        self.conn.execute("VACUUM INTO ?1", params![to.to_string_lossy().as_ref()])?;
        Ok(())
    }

    /// Move the write-ahead log into the main file and make it durable, so the `.osp` alone holds everything (before it is copied or renamed).
    pub fn checkpoint(&self) -> Result<()> {
        self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }

    /// The revision now (cheap: a reader polls this to learn that something changed).
    pub fn revision(&self) -> Result<i64> {
        Ok(self.meta_value("revision")?.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    /// Start a group of changes. Takes the write lock now (waiting for another writer).
    pub fn edit(&mut self) -> Result<Edit<'_>> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let revision: i64 = tx.query_row("SELECT value FROM meta WHERE key = 'revision'", [], |r| r.get::<_, String>(0))?.parse().unwrap_or(0);
        Ok(Edit { tx, revision: revision + 1, changed: false, local: false })
    }

    // ---- reads ----

    pub fn drawing(&self, name: &str) -> Result<Option<Drawing>> {
        Ok(self.conn.query_row("SELECT name, ext, sha256, bytes FROM drawing WHERE name = ?1", params![name], row_drawing).optional()?)
    }

    pub fn drawing_names(&self) -> Result<Vec<String>> {
        let mut st = self.conn.prepare("SELECT name FROM drawing ORDER BY created, name")?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn takeoff_headers(&self) -> Result<Vec<TakeoffHeader>> {
        let mut st = self.conn.prepare("SELECT slot, state, json FROM takeoff_header ORDER BY slot")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (slot, state, json) = row?;
            out.push(TakeoffHeader { slot, state, json: parse_json(&json)? });
        }
        Ok(out)
    }

    pub fn takeoff_rows(&self, slot: u32) -> Result<Vec<TakeoffRow>> {
        let mut st = self.conn.prepare("SELECT kind, n, json FROM takeoff_row WHERE slot = ?1 ORDER BY kind, n")?;
        let rows = st.query_map(params![slot], |r| Ok((r.get::<_, u8>(0)?, r.get::<_, u32>(1)?, r.get::<_, String>(2)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, n, json) = row?;
            out.push(TakeoffRow { slot, kind, n, json: parse_json(&json)? });
        }
        Ok(out)
    }

    pub fn project_doc(&self) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT json FROM project_doc WHERE id = 1", [], |r| r.get(0)).optional()?)
    }

    pub fn asset(&self, id: &str) -> Result<Option<Asset>> {
        Ok(self
            .conn
            .query_row("SELECT id, slot, name, mime, sha256, bytes FROM asset WHERE id = ?1", params![id], |r| {
                Ok(Asset { id: r.get(0)?, slot: r.get(1)?, name: r.get(2)?, mime: r.get(3)?, sha256: r.get(4)?, bytes: r.get(5)? })
            })
            .optional()?)
    }

    pub fn asset_ids(&self) -> Result<Vec<String>> {
        let mut st = self.conn.prepare("SELECT id FROM asset ORDER BY id")?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn markups(&self, drawing: &str) -> Result<Vec<Markup>> {
        let mut st = self
            .conn
            .prepare("SELECT id, drawing, author, created, kind, json, text, resolved FROM markup WHERE drawing = ?1 ORDER BY created, id")?;
        let rows = st.query_map(params![drawing], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, drawing, author, created, kind, json, text, resolved) = row?;
            out.push(Markup { id, drawing, author, created, kind, json: parse_json(&json)?, text, resolved: resolved != 0 });
        }
        Ok(out)
    }

    /// The structural details recorded for the file, in drawing and handle order.
    pub fn details(&self) -> Result<Vec<Detail>> {
        read_details(&self.conn)
    }

    /// What changed in revisions after `since`: `(revision, table, key, op)`, oldest first. Sync sends these.
    pub fn changes_since(&self, since: i64) -> Result<Vec<(i64, String, String, String)>> {
        let mut st = self.conn.prepare("SELECT revision, tbl, key, op FROM change WHERE revision > ?1 ORDER BY revision, rowid")?;
        let rows = st.query_map(params![since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

fn row_drawing(r: &rusqlite::Row<'_>) -> rusqlite::Result<Drawing> {
    Ok(Drawing { name: r.get(0)?, ext: r.get(1)?, sha256: r.get(2)?, bytes: r.get(3)? })
}

fn parse_json(text: &str) -> Result<serde_json::Value> {
    serde_json::from_str(text).map_err(|e| Error::Import(format!("stored JSON is not valid: {e}")))
}

impl Edit<'_> {
    fn log(&mut self, table: &str, key: &str, op: &str) -> Result<()> {
        self.changed = true;
        self.tx.execute("INSERT INTO change (revision, tbl, key, op) VALUES (?1, ?2, ?3, ?4)", params![self.revision, table, key, op])?;
        Ok(())
    }

    /// Store (or replace) a drawing's bytes.
    pub fn put_drawing(&mut self, name: &str, ext: &str, bytes: &[u8]) -> Result<()> {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        self.tx.execute(
            "INSERT INTO drawing (name, ext, sha256, bytes, revision, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(name) DO UPDATE SET ext = ?2, sha256 = ?3, bytes = ?4, revision = ?5",
            params![name, ext, sha256_hex(bytes), bytes, self.revision, time::now_rfc3339()],
        )?;
        self.log("drawing", name, "put")
    }

    pub fn put_takeoff_header(&mut self, h: &TakeoffHeader) -> Result<()> {
        self.tx.execute(
            "INSERT INTO takeoff_header (slot, state, json) VALUES (?1, ?2, ?3) ON CONFLICT(slot) DO UPDATE SET state = ?2, json = ?3",
            params![h.slot, h.state, h.json.to_string()],
        )?;
        self.log("takeoff_header", &h.slot.to_string(), "put")
    }

    pub fn put_takeoff_row(&mut self, r: &TakeoffRow) -> Result<()> {
        self.tx.execute(
            "INSERT INTO takeoff_row (slot, kind, n, json) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(slot, kind, n) DO UPDATE SET json = ?4",
            params![r.slot, r.kind, r.n, r.json.to_string()],
        )?;
        self.log("takeoff_row", &format!("{}/{}/{}", r.slot, r.kind, r.n), "put")
    }

    /// Remove the rows of `slot` that are not in `keep` (kind, n): the file then equals the current take-off.
    pub fn retain_takeoff_rows(&mut self, slot: u32, keep: &[(u8, u32)]) -> Result<usize> {
        let existing: Vec<(u8, u32)> = {
            let mut st = self.tx.prepare("SELECT kind, n FROM takeoff_row WHERE slot = ?1")?;
            st.query_map(params![slot], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?
        };
        let keep: std::collections::HashSet<(u8, u32)> = keep.iter().copied().collect();
        let mut removed = 0;
        for (kind, n) in existing {
            if !keep.contains(&(kind, n)) {
                self.tx.execute("DELETE FROM takeoff_row WHERE slot = ?1 AND kind = ?2 AND n = ?3", params![slot, kind, n])?;
                self.log("takeoff_row", &format!("{slot}/{kind}/{n}"), "delete")?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn delete_takeoff_row(&mut self, slot: u32, kind: u8, n: u32) -> Result<()> {
        self.tx.execute("DELETE FROM takeoff_row WHERE slot = ?1 AND kind = ?2 AND n = ?3", params![slot, kind, n])?;
        self.log("takeoff_row", &format!("{slot}/{kind}/{n}"), "delete")
    }

    /// Remove a drawing's take-off header and every row under it.
    pub fn delete_takeoff_header(&mut self, slot: u32) -> Result<()> {
        self.tx.execute("DELETE FROM takeoff_row WHERE slot = ?1", params![slot])?;
        self.tx.execute("DELETE FROM takeoff_header WHERE slot = ?1", params![slot])?;
        self.log("takeoff_header", &slot.to_string(), "delete")
    }

    pub fn delete_asset(&mut self, id: &str) -> Result<()> {
        self.tx.execute("DELETE FROM asset WHERE id = ?1", params![id])?;
        self.log("asset", id, "delete")
    }

    /// The SHA-256 stored for an asset, if there is one (to skip writing the same bytes again).
    pub fn asset_hash(&self, id: &str) -> Result<Option<String>> {
        Ok(self.tx.query_row("SELECT sha256 FROM asset WHERE id = ?1", params![id], |r| r.get(0)).optional()?)
    }

    /// The ids of the annotated-PDF assets.
    pub fn annotated_asset_ids(&self) -> Result<Vec<String>> {
        let mut st = self.tx.prepare("SELECT id FROM asset WHERE id LIKE 'annotated-%'")?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Replace the MynaOps project document.
    pub fn put_project_doc(&mut self, json: &str) -> Result<()> {
        serde_json::from_str::<serde_json::Value>(json).map_err(|e| Error::Import(format!("the project is not valid JSON: {e}")))?;
        self.tx.execute("INSERT INTO project_doc (id, json) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET json = ?1", params![json])?;
        self.log("project_doc", "1", "put")
    }

    pub fn put_asset(&mut self, a: &Asset) -> Result<()> {
        self.tx.execute(
            "INSERT INTO asset (id, slot, name, mime, sha256, bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET slot = ?2, name = ?3, mime = ?4, sha256 = ?5, bytes = ?6",
            params![a.id, a.slot, a.name, a.mime, sha256_hex(&a.bytes), a.bytes],
        )?;
        self.log("asset", &a.id, "put")
    }

    pub fn put_markup(&mut self, m: &Markup) -> Result<()> {
        self.tx.execute(
            "INSERT INTO markup (id, drawing, author, created, kind, json, text, resolved) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET json = ?6, text = ?7, resolved = ?8",
            params![m.id, m.drawing, m.author, m.created, m.kind, m.json.to_string(), m.text, i64::from(m.resolved)],
        )?;
        self.log("markup", &m.id, "put")
    }

    /// Make the details of `drawing` exactly `list` (a drawing's details are replaced as a set). Nothing is written, and no revision is made for it,
    /// when they are already the same.
    pub fn replace_details(&mut self, drawing: &str, list: &[Detail]) -> Result<()> {
        let have: Vec<Detail> = read_details(&self.tx)?.into_iter().filter(|d| d.drawing == drawing).collect();
        let mut want: Vec<Detail> = list.to_vec();
        want.sort_by(|a, b| a.id.cmp(&b.id));
        if have == want {
            return Ok(());
        }
        for d in &have {
            if !want.iter().any(|w| w.id == d.id) {
                self.tx.execute("DELETE FROM detail WHERE id = ?1", params![d.id])?;
                self.log("detail", &d.id, "delete")?;
            }
        }
        for d in &want {
            if have.iter().any(|h| h == d) {
                continue;
            }
            self.tx.execute(
                "INSERT INTO detail (id, drawing, handle, kind, block, params, layer, label) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET kind = ?4, block = ?5, params = ?6, layer = ?7, label = ?8",
                params![d.id, d.drawing, d.handle, d.kind, d.block, d.params.to_string(), d.layer, d.label],
            )?;
            self.log("detail", &d.id, "put")?;
        }
        Ok(())
    }

    /// See [`Odp::set_local_meta`].
    pub fn set_local_meta(&mut self, key: &str, value: &str) -> Result<()> {
        if CORE_META.contains(&key) {
            return Err(Error::Import(format!("`{key}` is not a local meta key")));
        }
        self.tx.execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2", params![key, value])?;
        self.local = true;
        Ok(())
    }

    /// The stored hash of a drawing as this group of changes sees it.
    pub fn drawing_sha256(&self, name: &str) -> Result<Option<String>> {
        Ok(self.tx.query_row("SELECT sha256 FROM drawing WHERE name = ?1", params![name], |r| r.get(0)).optional()?)
    }

    /// Store the native model of `drawing`: MynaCAD's own encoding of the drawing, read back without parsing DXF or DWG. `parts` are the encoded
    /// pieces with their entity counts, in order; `drawing_sha256` is the hash of the `drawing` row they were made from. Parts are kept by content
    /// hash, so a save writes only the parts an edit changed. The model is local, derived data: it is not a revision and is not in the change log.
    pub fn put_native(&mut self, drawing: &str, drawing_sha256: &str, format: u32, parts: &[NativePart<'_>]) -> Result<NativeWrite> {
        let mut done = NativeWrite { parts: parts.len(), ..NativeWrite::default() };
        self.tx.execute("DELETE FROM native_part", [])?;
        {
            let mut has = self.tx.prepare("SELECT 1 FROM native_blob WHERE hash = ?1")?;
            let mut blob = self.tx.prepare("INSERT INTO native_blob (hash, bytes) VALUES (?1, ?2)")?;
            let mut part = self.tx.prepare("INSERT INTO native_part (drawing, seq, hash, entities) VALUES (?1, ?2, ?3, ?4)")?;
            for (seq, p) in parts.iter().enumerate() {
                if has.query_row(params![p.hash], |_| Ok(())).optional()?.is_none() {
                    blob.execute(params![p.hash, p.bytes])?;
                    done.written += 1;
                    done.bytes_written = done.bytes_written.saturating_add(p.bytes.len());
                }
                part.execute(params![drawing, seq as i64, p.hash, p.entities as i64])?;
            }
        }
        self.tx.execute("DELETE FROM native_blob WHERE hash NOT IN (SELECT hash FROM native_part)", [])?;
        self.set_local_meta("native.format", &format.to_string())?;
        self.set_local_meta("native.drawing", drawing)?;
        self.set_local_meta("native.drawing_sha256", drawing_sha256)?;
        Ok(done)
    }

    /// Make the changes one revision. A group with no change leaves the revision as it was; local data alone is committed without a revision.
    pub fn commit(self) -> Result<i64> {
        if !self.changed {
            if self.local {
                self.tx.commit()?;
            } else {
                self.tx.rollback()?;
            }
            return Ok(self.revision - 1);
        }
        self.tx.execute("UPDATE meta SET value = ?1 WHERE key = 'revision'", params![self.revision.to_string()])?;
        self.tx.commit()?;
        Ok(self.revision)
    }
}

impl Odp {
    /// Every record of the drawing history kept in this file (see [`history`]).
    pub fn history_records(&self) -> Result<std::collections::BTreeMap<u64, Vec<u8>>> {
        let mut stmt = self.conn.prepare("SELECT id, payload FROM history_record")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, Vec<u8>>(1)?)))?;
        let mut out = std::collections::BTreeMap::new();
        for row in rows {
            let (id, payload) = row?;
            out.insert(id, payload);
        }
        Ok(out)
    }

    /// Make the history records exactly `records`. History is local to this computer: it is not a revision and is not in the change log.
    pub fn history_store(&mut self, records: &std::collections::BTreeMap<u64, Vec<u8>>) -> Result<()> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM history_record", [])?;
        {
            let mut ins = tx.prepare("INSERT INTO history_record (id, payload) VALUES (?1, ?2)")?;
            for (id, payload) in records {
                ins.execute(rusqlite::params![*id as i64, payload])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
