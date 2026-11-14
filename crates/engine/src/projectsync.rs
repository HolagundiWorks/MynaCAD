//! Live sync, MynaCAD side (`docs/live-sync.md`): tells the user that the open project file (`.osp`) was changed by another program, without ever
//! reloading it. The detector is read only ([`aad_odp::probe`]); this module keeps the revision this program has accepted (the one it read when it
//! opened the file, or wrote itself) and compares.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aad_odp::{Peek, Probe};
use serde_json::{Value, json};

use crate::cmd::{CommandSpec, always};
use crate::{Result, Session};

/// What this program last read from, or wrote to, a project file.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Accepted {
    revision: i64,
    project_id: String,
}

static ACCEPTED: Mutex<Option<HashMap<PathBuf, Accepted>>> = Mutex::new(None);

fn key(path: &Path) -> PathBuf {
    let p = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    PathBuf::from(p.to_string_lossy().to_lowercase())
}

fn with_map<T>(f: impl FnOnce(&mut HashMap<PathBuf, Accepted>) -> T) -> Option<T> {
    let mut g = ACCEPTED.lock().ok()?;
    Some(f(g.get_or_insert_with(HashMap::new)))
}

fn accepted_of(path: &Path) -> Option<Accepted> {
    with_map(|m| m.get(&key(path)).cloned()).flatten()
}

/// The file was opened: what it holds now is what the user sees.
pub fn note_opened(path: &Path, revision: i64, project_id: &str) {
    with_map(|m| m.insert(key(path), Accepted { revision, project_id: project_id.to_string() }));
}

/// This program wrote the file, taking it from revision `before` to `after`. The write is accepted unless another program had written in between
/// (the accepted revision is not `before`): then the other program's change stays reported.
pub fn note_own_write(path: &Path, before: i64, after: i64, project_id: &str) {
    with_map(|m| {
        let k = key(path);
        match m.get_mut(&k) {
            Some(a) if a.revision != before || a.project_id != project_id => {}
            Some(a) => a.revision = after,
            None => {
                m.insert(k, Accepted { revision: after, project_id: project_id.to_string() });
            }
        }
    });
}

/// The state of the file as the user is told about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Synced,
    Updated,
    OutOfDate,
    Locked,
    Replaced,
    Unreadable(String),
    Missing,
}

impl State {
    fn code(&self) -> &'static str {
        match self {
            State::Synced => "synced",
            State::Updated => "updated",
            State::OutOfDate => "outofdate",
            State::Locked => "locked",
            State::Replaced => "replaced",
            State::Unreadable(_) => "unreadable",
            State::Missing => "missing",
        }
    }
}

fn app_name(app: Option<&str>) -> &'static str {
    match app {
        Some("mynaops") => "MynaOps",
        Some("mynacad") => "MynaCAD",
        _ => "",
    }
}

/// `hh:mm` out of an RFC 3339 UTC text (`2026-11-15T09:30:12Z`).
fn hhmm(t: &str) -> Option<&str> {
    let (_, time) = t.split_once('T')?;
    time.get(..5).filter(|s| s.as_bytes().get(2) == Some(&b':'))
}

/// Decide the state from what was accepted and what a probe found. Pure, so it is tested without a file.
fn evaluate(accepted: Option<&Accepted>, peek: &Peek) -> (State, Option<Probe>) {
    match peek {
        Peek::Missing => (State::Missing, None),
        Peek::Locked => (State::Locked, None),
        Peek::Unreadable(why) => (State::Unreadable(why.clone()), None),
        Peek::Ok(p) => {
            let state = match accepted {
                Some(a) if a.project_id != p.project_id || p.revision < a.revision => State::Replaced,
                Some(a) if p.revision != a.revision => State::Updated,
                _ if !p.stale_slots().is_empty() => State::OutOfDate,
                _ => State::Synced,
            };
            (state, Some(p.clone()))
        }
    }
}

fn describe(state: &State, probe: Option<&Probe>) -> String {
    match state {
        State::Synced => "Synced".into(),
        State::Updated => {
            let who = app_name(probe.and_then(|p| p.last_app.as_deref()));
            let at = probe.and_then(|p| p.last_time.as_deref()).and_then(hhmm);
            match (who, at) {
                ("", _) => "Updated elsewhere".into(),
                (w, Some(t)) => format!("Updated by {w} at {t} UTC"),
                (w, None) => format!("Updated by {w}"),
            }
        }
        State::OutOfDate => "Take-off out of date".into(),
        State::Locked => "Locked".into(),
        State::Replaced => "Project file was replaced".into(),
        State::Unreadable(_) => "Project file unreadable".into(),
        State::Missing => "Project file missing".into(),
    }
}

/// The sync status of the project file at `path` as JSON (the reply of `project.sync`). `ack` accepts the revision the file has now.
pub fn status(path: &Path, ack: bool) -> Value {
    let peek = aad_odp::probe(path);
    if ack {
        if let Peek::Ok(p) = &peek {
            note_opened(path, p.revision, &p.project_id);
        }
    }
    let mut accepted = accepted_of(path);
    // A file this program never opened or wrote (a path given by hand) has nothing to compare with: its current revision is the baseline.
    if let (None, Peek::Ok(p)) = (&accepted, &peek) {
        note_opened(path, p.revision, &p.project_id);
        accepted = Some(Accepted { revision: p.revision, project_id: p.project_id.clone() });
    }
    let (state, probe) = evaluate(accepted.as_ref(), &peek);
    let mut out = json!({ "path": path.to_string_lossy(), "state": state.code(), "text": describe(&state, probe.as_ref()) });
    if let State::Unreadable(why) = &state {
        out["reason"] = json!(why);
    }
    if let Some(a) = &accepted {
        out["accepted"] = json!(a.revision);
    }
    if let Some(p) = probe {
        out["revision"] = json!(p.revision);
        out["lastApp"] = json!(p.last_app);
        out["lastTime"] = json!(p.last_time);
        out["staleSlots"] = json!(p.stale_slots());
        let takeoff: Vec<Value> = p
            .takeoff
            .iter()
            .map(|t| {
                let levels: Vec<Value> = t.levels.iter().map(|(id, r)| json!({ "id": id, "revision": r })).collect();
                json!({ "slot": t.slot, "revision": t.revision, "recorded": t.recorded, "levels": levels })
            })
            .collect();
        out["takeoff"] = json!(takeoff);
    }
    out
}

fn run_sync(s: &mut Session, p: &Value) -> Result<Value> {
    let path = match p.get("path").and_then(Value::as_str) {
        Some(x) => x.to_string(),
        None => match s.state().ok().and_then(|d| d.path.clone()).filter(|d| crate::cmd::file::is_container(d)) {
            Some(x) => x,
            None => return Ok(json!({ "state": "none", "text": "" })),
        },
    };
    Ok(status(Path::new(&path), p.get("ack").and_then(Value::as_bool).unwrap_or(false)))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("project.sync", "Project Sync Status", run_sync)
            .menu(&["File", "Project", "Sync Status"])
            .alias(&["syncstatus"])
            .enabled(always)
            .noundo()
            .params("{path?: project file (default: the open one), ack?: accept the file as it is now} → {state: none|synced|updated|outofdate|locked|replaced|unreadable|missing, text, revision?, accepted?, lastApp?, lastTime?, staleSlots?, takeoff?, reason?}. Read only: it never reloads or writes the file"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(rev: i64, id: &str, app: &str, stale: bool) -> Peek {
        Peek::Ok(Probe {
            project_id: id.into(),
            revision: rev,
            last_app: Some(app.into()),
            last_revision: Some(rev),
            last_time: Some("2026-11-15T09:30:12Z".into()),
            drawing_revision: Some(if stale { 9 } else { 1 }),
            takeoff: vec![aad_odp::TakeoffStamp { slot: 1, revision: 5, recorded: "t".into(), levels: vec![] }],
        })
    }
    fn acc(rev: i64, id: &str) -> Accepted {
        Accepted { revision: rev, project_id: id.into() }
    }

    #[test]
    fn the_state_follows_the_revision_the_project_and_the_stamps() {
        let s = |a: Option<Accepted>, p: Peek| evaluate(a.as_ref(), &p).0;
        assert_eq!(s(Some(acc(5, "a")), ok(5, "a", "mynacad", false)), State::Synced);
        assert_eq!(s(Some(acc(5, "a")), ok(6, "a", "mynaops", false)), State::Updated);
        assert_eq!(s(Some(acc(5, "a")), ok(4, "a", "mynaops", false)), State::Replaced, "revision went backwards");
        assert_eq!(s(Some(acc(5, "a")), ok(5, "b", "mynaops", false)), State::Replaced, "another project");
        assert_eq!(s(Some(acc(5, "a")), ok(5, "a", "mynacad", true)), State::OutOfDate);
        assert_eq!(s(Some(acc(5, "a")), ok(7, "a", "mynacad", true)), State::Updated, "an outside change comes first");
        assert_eq!(s(None, Peek::Locked), State::Locked);
        assert_eq!(s(None, Peek::Missing), State::Missing);
        assert!(matches!(s(None, Peek::Unreadable("x".into())), State::Unreadable(_)));
    }

    #[test]
    fn the_text_names_the_writer_and_the_time() {
        let (st, p) = evaluate(Some(&acc(1, "a")), &ok(2, "a", "mynaops", false));
        assert_eq!(describe(&st, p.as_ref()), "Updated by MynaOps at 09:30 UTC");
        assert_eq!(describe(&State::Synced, None), "Synced");
        let (st, p) = evaluate(Some(&acc(1, "a")), &ok(2, "a", "something else", false));
        assert_eq!(describe(&st, p.as_ref()), "Updated elsewhere");
    }

    #[test]
    fn own_writes_are_accepted_but_an_outside_write_in_between_is_not_swallowed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.osp");
        note_opened(&path, 3, "a");
        note_own_write(&path, 3, 4, "a");
        assert_eq!(accepted_of(&path).map(|a| a.revision), Some(4));
        // Another program wrote 5 and 6 before our write took the file from 6 to 7: the accepted revision stays 4.
        note_own_write(&path, 6, 7, "a");
        assert_eq!(accepted_of(&path).map(|a| a.revision), Some(4));
    }
}
