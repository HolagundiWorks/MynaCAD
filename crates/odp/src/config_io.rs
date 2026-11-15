//! Reading the configuration from a project file, the read-only probe, and the writer used by tests and tools (MynaOps writes it with its own .NET
//! code; MynaCAD's shell never calls [`write`]). Part of [`super`], not built for the web.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use super::{
    Applied, ConfigError, KEY_APPLIED, KEY_PROPOSAL, MAX_VALUE_BYTES, PARTS, PREFIX, Part, ProjectConfig, Proposal, Read, Result, Schema, parse_applied,
    parse_proposal,
};
use crate::Odp;

/// Everything the configuration keys of one file say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    /// The configuration and the keys that were left out.
    pub read: Read,
    /// What MynaCAD reported having applied (`config.applied.cad`); `None` when absent or unreadable.
    pub applied: Option<Applied>,
    /// The one-time proposal of a legacy project (`config.proposal`); `None` when absent or unreadable.
    pub proposal: Option<Proposal>,
    /// `meta.revision` when it was read.
    pub revision: i64,
}

impl Loaded {
    /// Whether the project is configured by MynaOps.
    pub fn configured(&self) -> bool {
        self.read.config.configured()
    }
}

fn file_err(e: impl std::fmt::Display) -> ConfigError {
    ConfigError::File(e.to_string())
}

/// The shared reader: `len` says how many bytes a key's value has (without reading it), `get` reads it. A value over the cap is an error before it is read.
fn load_with(len: &dyn Fn(&str) -> Result<Option<u64>>, get: &dyn Fn(&str) -> Result<Option<String>>, revision: i64) -> Result<Loaded> {
    let fetch = |key: &str| -> Result<Option<String>> {
        match len(key)? {
            None => Ok(None),
            Some(n) if n > MAX_VALUE_BYTES as u64 => Err(ConfigError::TooLarge { key: key.to_string(), bytes: n, max: MAX_VALUE_BYTES }),
            Some(_) => get(key),
        }
    };
    let mut parts = BTreeMap::new();
    for name in PARTS {
        if let Some(text) = fetch(&format!("{PREFIX}{name}"))? {
            parts.insert(name.to_string(), text);
        }
    }
    let mut read = Read::default();
    let mut applied = None;
    let mut proposal = None;
    if parts.contains_key("schema") {
        read = ProjectConfig::from_texts(&parts)?;
    } else if !parts.is_empty() {
        // Keys without `config.schema`: not configured. They are still checked so that a damaged one is reported, not silently ignored.
        read = ProjectConfig::from_texts(&parts)?;
        read.notes.push("config keys are present without config.schema: the project counts as not configured".into());
        read.config.schema = None;
    }
    // The two keys MynaCAD writes are reports: a damaged one reads as absent and is noted, it never stops the configuration from being read.
    match fetch(KEY_APPLIED)?.map(|t| parse_applied(&t)) {
        Some(Ok(Part::Value(a))) => applied = Some(a),
        Some(Ok(Part::Newer(v))) => read.notes.push(format!("{KEY_APPLIED} is version {v}: left out")),
        Some(Err(e)) => read.notes.push(format!("{KEY_APPLIED} is damaged ({e}): left out")),
        None => {}
    }
    match fetch(KEY_PROPOSAL)?.map(|t| parse_proposal(&t)) {
        Some(Ok(Part::Value(p))) => proposal = Some(p),
        Some(Ok(Part::Newer(v))) => read.notes.push(format!("{KEY_PROPOSAL} is version {v}: left out")),
        Some(Err(e)) => read.notes.push(format!("{KEY_PROPOSAL} is damaged ({e}): left out")),
        None => {}
    }
    Ok(Loaded { read, applied, proposal, revision })
}

/// Read the configuration of an open project file.
pub fn read(odp: &Odp) -> Result<Loaded> {
    let revision = odp.revision().map_err(file_err)?;
    load_with(&|k| odp.meta_len(k).map_err(file_err), &|k| odp.meta_get(k).map_err(file_err), revision)
}

/// Read the configuration of a project file without taking any lock a writer would wait for: the file is opened read-only, a few `meta` keys are read, and
/// it is closed. Never creates or changes the file (the shell calls this when it checks for a newer `config.rev`).
pub fn probe(path: &Path) -> Result<Loaded> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(file_err)?;
    conn.busy_timeout(std::time::Duration::from_secs(1)).map_err(file_err)?;
    let len = |k: &str| -> Result<Option<u64>> {
        let n: Option<i64> = conn
            .query_row("SELECT length(CAST(value AS BLOB)) FROM meta WHERE key = ?1", params![k], |r| r.get(0))
            .optional()
            .map_err(file_err)?;
        Ok(n.map(|n| u64::try_from(n).unwrap_or(0)))
    };
    let get = |k: &str| -> Result<Option<String>> {
        conn.query_row("SELECT value FROM meta WHERE key = ?1", params![k], |r| r.get::<_, String>(0)).optional().map_err(file_err)
    };
    let revision = get("revision")?.and_then(|v| v.parse().ok()).unwrap_or(0);
    load_with(&len, &get, revision)
}

/// A part's text read and written out again, so two texts that mean the same compare equal (a writer in another language may omit a null).
/// `None` when the text is not a valid part.
fn normalized(name: &str, text: &str) -> Option<String> {
    let parts = BTreeMap::from([(name.to_string(), text.to_string())]);
    let read = ProjectConfig::from_texts(&parts).ok()?;
    read.config.to_texts().ok()?.remove(name)
}

/// What [`write`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    /// `meta.revision` after the write (unchanged when nothing differed).
    pub revision: i64,
    /// `config.rev` after the write.
    pub rev: u64,
    /// The parts whose keys were written (short names), `rev` included when anything was.
    pub changed: Vec<String>,
}

/// Write `cfg` into the file the way MynaOps does: every part that differs from the file, and the bumped `config.rev`, in **one** transaction, so
/// `meta.revision` rises once. A part that is `None` is left as it is (the writer never deletes a key); `schema` is added when missing; `cfg.rev` is
/// ignored (the writer sets it). Nothing is written, and no revision made, when no part differs. `app` is the name put in `config.rev.app`.
pub fn write(odp: &mut Odp, cfg: &ProjectConfig, app: &str) -> Result<Written> {
    cfg.validate()?;
    let mut texts = cfg.to_texts()?;
    texts.remove("rev");
    if !texts.contains_key("schema") {
        let schema = serde_json::to_string(&Schema { v: 1, min_reader: 1 }).map_err(file_err)?;
        texts.insert("schema".into(), schema);
    }
    let mut changed: Vec<String> = Vec::new();
    for (name, text) in &texts {
        let key = format!("{PREFIX}{name}");
        let have = match odp.meta_len(&key).map_err(file_err)? {
            Some(n) if n > MAX_VALUE_BYTES as u64 => None,
            _ => odp.meta_get(&key).map_err(file_err)?,
        };
        if !have.is_some_and(|h| normalized(name, &h) == normalized(name, text)) {
            changed.push(name.clone());
        }
    }
    let rev_key = format!("{PREFIX}rev");
    let current_rev = match odp.meta_len(&rev_key).map_err(file_err)? {
        Some(n) if n <= MAX_VALUE_BYTES as u64 => odp
            .meta_get(&rev_key)
            .map_err(file_err)?
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("rev").and_then(serde_json::Value::as_u64))
            .unwrap_or(0),
        _ => 0,
    };
    if changed.is_empty() {
        return Ok(Written { revision: odp.revision().map_err(file_err)?, rev: current_rev, changed });
    }
    let rev = current_rev.saturating_add(1);
    let rev_text = serde_json::json!({ "v": 1, "rev": rev, "app": app, "time": crate::now_utc() }).to_string();
    let mut edit = odp.edit().map_err(file_err)?;
    for name in &changed {
        let text = texts.get(name).map(String::as_str).unwrap_or("{}");
        edit.put_meta(&format!("{PREFIX}{name}"), text).map_err(file_err)?;
    }
    edit.put_meta(&rev_key, &rev_text).map_err(file_err)?;
    let revision = edit.commit().map_err(file_err)?;
    changed.push("rev".into());
    Ok(Written { revision, rev, changed })
}

/// Record what MynaCAD has applied (`config.applied.cad`), a local key: committed, not a revision.
pub fn write_applied(odp: &mut Odp, applied: &Applied) -> Result<()> {
    let text = serde_json::to_string(applied).map_err(file_err)?;
    parse_applied(&text)?;
    odp.set_local_meta(&[(KEY_APPLIED, text.as_str())]).map_err(file_err)
}

/// Record the one-time proposal of a legacy project (`config.proposal`), a local key. An existing proposal is kept unless `replace`; a project that
/// already has `config.schema` gets none. Returns whether it was written.
pub fn write_proposal(odp: &mut Odp, proposal: &Proposal, replace: bool) -> Result<bool> {
    if odp.meta_get(&format!("{PREFIX}schema")).map_err(file_err)?.is_some() {
        return Ok(false);
    }
    if !replace && odp.meta_get(KEY_PROPOSAL).map_err(file_err)?.is_some() {
        return Ok(false);
    }
    let text = serde_json::to_string(proposal).map_err(file_err)?;
    parse_proposal(&text)?;
    odp.set_local_meta(&[(KEY_PROPOSAL, text.as_str())]).map_err(file_err)?;
    Ok(true)
}
