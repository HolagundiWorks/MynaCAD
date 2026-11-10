//! Rolling backups of a project file: the file as it was before a save (or a re-import) replaced its drawing. Where, how many and how to
//! restore: `docs/AUTOSAVE-BACKUP.md`.
//!
//! Backups live in `<name>.backups` beside the project file, one whole `.osp` per backup, named `<name> <UTC time> <reason>.osp`. A backup is
//! written under a temporary name and renamed, so the folder never holds a partial one. After each new backup the folder is pruned by count and
//! age, so it cannot grow without bound.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// How many backups are kept and for how long.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Policy {
    /// Most backups kept for one file (0 turns backups off).
    pub count: usize,
    /// Backups older than this many days are removed, except the newest one, which is always kept.
    pub max_age_days: u32,
}

impl Default for Policy {
    fn default() -> Self {
        Policy { count: 5, max_age_days: 90 }
    }
}

impl Policy {
    /// The policy of this session: `BACKUPCOUNT` backups, 90 days.
    pub fn current() -> Self {
        Policy { count: crate::sysvars::BACKUP_COUNT.load(std::sync::atomic::Ordering::Relaxed) as usize, ..Policy::default() }
    }
}

/// One backup in the folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub path: String,
    pub name: String,
    /// The UTC time in the file name (`20261110T174455Z`).
    pub stamp: String,
    /// Why it was made (`save`, `before-reimport`, `before-restore`).
    pub reason: String,
    pub bytes: u64,
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Drawing".into())
}

/// The backups folder of a project file: `<name>.backups` beside it.
pub fn folder(file: &Path) -> PathBuf {
    file.with_file_name(format!("{}.backups", stem(file)))
}

/// `2026-11-10T17:44:55Z` as `20261110T174455Z`.
fn compact(utc: &str) -> String {
    utc.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

fn parse_name(file_stem: &str, name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix(file_stem)?.strip_prefix(' ')?.strip_suffix(".osp")?;
    let (stamp, reason) = rest.split_once(' ').unwrap_or((rest, ""));
    let ok = stamp.len() == 16 && stamp.ends_with('Z') && stamp.chars().filter(char::is_ascii_digit).count() == 14;
    ok.then(|| (stamp.to_string(), reason.to_string()))
}

/// The backups of `file`, newest first. Files in the folder that are not backups of this file are ignored (and never deleted).
pub fn list(file: &Path) -> Vec<Backup> {
    let s = stem(file);
    let mut out: Vec<(Backup, std::time::SystemTime)> = std::fs::read_dir(folder(file))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let (stamp, reason) = parse_name(&s, &name)?;
            let meta = entry.metadata().ok()?;
            let made = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            meta.is_file().then(|| (Backup { path: entry.path().to_string_lossy().into_owned(), name, stamp, reason, bytes: meta.len() }, made))
        })
        .collect();
    // Newest first: by the time in the name, then (several in one second) by when the file was written.
    out.sort_by(|(a, ta), (b, tb)| b.stamp.cmp(&a.stamp).then_with(|| tb.cmp(ta)).then_with(|| b.name.cmp(&a.name)));
    out.into_iter().map(|(b, _)| b).collect()
}

/// Days from 1970-01-01 for a `YYYYMMDD…` stamp (civil-from-days inverse; good for comparing ages).
fn stamp_days(stamp: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| stamp.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(4..6)?, n(6..8)?);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Remove the backups of `file` beyond the policy; returns how many were removed. `now` is a compact UTC stamp.
pub fn prune(file: &Path, policy: &Policy, now: &str) -> usize {
    let today = stamp_days(now);
    let mut removed = 0;
    for (i, b) in list(file).iter().enumerate() {
        let too_many = i >= policy.count;
        let too_old = i > 0 && today.zip(stamp_days(&b.stamp)).is_some_and(|(t, d)| t.saturating_sub(d) > i64::from(policy.max_age_days));
        if (too_many || too_old) && std::fs::remove_file(&b.path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Keep a copy of the project file `file` as it is now, then prune. The copy is made by SQLite from one consistent state of the file (it is safe
/// while MynaOps has the file open), under a temporary name, and renamed when complete.
pub fn keep_copy(file: &Path, reason: &str, policy: &Policy) -> Result<PathBuf, String> {
    keep_copy_at(file, reason, policy, &compact(&aad_odp::now_utc()))
}

pub(crate) fn keep_copy_at(file: &Path, reason: &str, policy: &Policy, now: &str) -> Result<PathBuf, String> {
    let dir = folder(file);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let reason: String = reason.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(32).collect();
    let s = stem(file);
    // Two backups in one second get a counter, so neither replaces the other.
    let mut target = dir.join(format!("{s} {now} {reason}.osp"));
    let mut n = 1;
    while target.exists() && n < 100 {
        n += 1;
        target = dir.join(format!("{s} {now} {reason}-{n}.osp"));
    }
    let tmp = dir.join(format!(".{s}.{}.backup-tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let made = aad_odp::Odp::open(file).and_then(|odp| odp.copy_to(&tmp)).map_err(|e| format!("backup of {}: {e}", file.display()));
    if let Err(e) = made {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", target.display())
    })?;
    prune(file, policy, now);
    Ok(target)
}

/// Put a backup back: the project file as it is now is kept as a `before-restore` backup first, then replaced by a copy of `backup`. The
/// replacement is a rename of a complete file, so the project file is the old one or the restored one, never part of each. Fails (and changes
/// nothing) while another program holds the project file open in a way that stops the rename.
pub fn restore(file: &Path, backup: &Path, policy: &Policy) -> Result<PathBuf, String> {
    if !backup.is_file() {
        return Err(format!("{}: no such backup", backup.display()));
    }
    // The backup must be a project file before anything is touched.
    aad_odp::Odp::open(backup).map_err(|e| format!("{}: {e}", backup.display()))?;
    // What the restore replaces is kept even when backups are off (count 0): it may hold work the backup does not.
    let keep = Policy { count: policy.count.max(1), ..*policy };
    let kept = if file.exists() { Some(keep_copy(file, "before-restore", &keep)?) } else { None };
    let tmp = file.with_file_name(format!(".{}.{}.restore-tmp", stem(file), std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let copied = aad_odp::Odp::open(backup).and_then(|odp| odp.copy_to(&tmp)).map_err(|e| format!("restore of {}: {e}", backup.display()));
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // The write-ahead log of the file being replaced belongs to it, not to the restored file.
    if let Ok(odp) = aad_odp::Odp::open(file) {
        let _ = odp.checkpoint();
    }
    std::fs::rename(&tmp, file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e} (is the project open in another program?)", file.display())
    })?;
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", file.display()));
    }
    Ok(kept.unwrap_or_else(|| backup.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
        let path = dir.join(name);
        let mut odp = aad_odp::Odp::create(&path, "P").unwrap();
        let mut e = odp.edit().unwrap();
        e.put_drawing("a.dxf", "dxf", body).unwrap();
        e.commit().unwrap();
        path
    }

    fn body(path: &Path) -> Vec<u8> {
        aad_odp::Odp::open(path).unwrap().drawing("a.dxf").unwrap().unwrap().bytes
    }

    #[test]
    fn a_backup_is_a_whole_project_file_and_the_folder_is_pruned_by_count() {
        let dir = tempfile::tempdir().unwrap();
        let file = project(dir.path(), "House.osp", b"one");
        let policy = Policy { count: 3, max_age_days: 90 };
        for i in 0..6 {
            let made = keep_copy_at(&file, "save", &policy, &format!("20261110T1200{i:02}Z")).unwrap();
            assert_eq!(body(&made), b"one");
        }
        let kept = list(&file);
        assert_eq!(kept.len(), 3);
        assert_eq!(kept.iter().map(|b| b.stamp.as_str()).collect::<Vec<_>>(), ["20261110T120005Z", "20261110T120004Z", "20261110T120003Z"]);
        assert!(kept.iter().all(|b| b.reason == "save"));
        // No temporary file is left behind.
        assert!(std::fs::read_dir(folder(&file)).unwrap().flatten().all(|e| e.file_name().to_string_lossy().ends_with(".osp")));
    }

    #[test]
    fn old_backups_go_but_the_newest_always_stays_and_strangers_are_not_touched() {
        let dir = tempfile::tempdir().unwrap();
        let file = project(dir.path(), "House.osp", b"one");
        let policy = Policy { count: 5, max_age_days: 30 };
        keep_copy_at(&file, "save", &policy, "20260101T000000Z").unwrap();
        keep_copy_at(&file, "save", &policy, "20260105T000000Z").unwrap();
        let stranger = folder(&file).join("notes.txt");
        std::fs::write(&stranger, b"mine").unwrap();
        let other = folder(&file).join("Other 20200101T000000Z save.osp");
        std::fs::write(&other, b"x").unwrap();
        // A year later both are old: the newest stays.
        assert_eq!(prune(&file, &policy, "20270101T000000Z"), 1);
        assert_eq!(list(&file).iter().map(|b| b.stamp.as_str()).collect::<Vec<_>>(), ["20260105T000000Z"]);
        keep_copy_at(&file, "save", &policy, "20270101T000001Z").unwrap();
        assert_eq!(list(&file).len(), 1, "the old one went when a new one arrived");
        assert!(stranger.exists() && other.exists());
        // Count 0: no backups are kept at all.
        keep_copy_at(&file, "save", &Policy { count: 0, max_age_days: 30 }, "20270101T000002Z").unwrap();
        assert!(list(&file).is_empty());
    }

    #[test]
    fn the_settings_are_session_variables_not_drawing_header_variables() {
        let mut s = crate::Session::empty();
        s.new_drawing(true);
        let before = s.state().unwrap().doc.clone();
        s.execute("setvar", &serde_json::json!({ "name": "BACKUPCOUNT", "value": 3 })).unwrap();
        assert_eq!(Policy::current().count, 3);
        assert!(s.execute("setvar", &serde_json::json!({ "name": "BACKUPCOUNT", "value": 51 })).is_err());
        assert!(s.execute("setvar", &serde_json::json!({ "name": "AUTOSAVESECS", "value": 5 })).is_err(), "0 or 10 to 3600");
        s.execute("setvar", &serde_json::json!({ "name": "AUTOSAVESECS", "value": 120 })).unwrap();
        assert_eq!(crate::sysvars::get(&s, "BACKUPCOUNT"), Some(serde_json::json!(3)));
        s.execute("setvar", &serde_json::json!({ "name": "BACKUPCOUNT", "value": 5 })).unwrap();
        let after = &s.state().unwrap().doc;
        assert!(std::sync::Arc::ptr_eq(&before, after), "the drawing is not changed by these settings");
        assert!(after.header.get("BACKUPCOUNT").is_none() && after.header.get("AUTOSAVESECS").is_none());
    }

    #[test]
    fn two_backups_in_one_second_are_both_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = project(dir.path(), "House.osp", b"one");
        let policy = Policy::default();
        let a = keep_copy_at(&file, "save", &policy, "20261110T120000Z").unwrap();
        let b = keep_copy_at(&file, "save", &policy, "20261110T120000Z").unwrap();
        let c = keep_copy_at(&file, "before-restore", &policy, "20261110T120000Z").unwrap();
        assert_ne!(a, b);
        // Newest first, also within one second; `before-restore` is a reason, not a counter.
        let names: Vec<String> = list(&file).into_iter().map(|b| b.path).collect();
        assert_eq!(names, [c, b, a].map(|p| p.to_string_lossy().into_owned()));
    }

    #[test]
    fn restore_puts_a_backup_back_and_keeps_what_it_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let file = project(dir.path(), "House.osp", b"one");
        let policy = Policy::default();
        let first = keep_copy_at(&file, "save", &policy, "20261110T120000Z").unwrap();
        {
            let mut odp = aad_odp::Odp::open(&file).unwrap();
            let mut e = odp.edit().unwrap();
            e.put_drawing("a.dxf", "dxf", b"two").unwrap();
            e.commit().unwrap();
        }
        assert_eq!(body(&file), b"two");
        let kept = restore(&file, &first, &policy).unwrap();
        assert_eq!(body(&file), b"one");
        assert_eq!(body(&kept), b"two", "what the restore replaced is itself a backup");
        assert!(restore(&file, &dir.path().join("missing.osp"), &policy).is_err());
        let junk = dir.path().join("junk.osp");
        std::fs::write(&junk, b"not a project").unwrap();
        assert!(restore(&file, &junk, &policy).is_err());
        assert_eq!(body(&file), b"one", "a bad backup changes nothing");
    }
}
