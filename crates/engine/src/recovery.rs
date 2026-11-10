//! Recovery snapshots: the unsaved state of a drawing, written every so often while it has unsaved changes, so a crash or a power cut loses
//! minutes and not the session. What is kept, where and how to restore: `docs/AUTOSAVE-BACKUP.md`.
//!
//! A snapshot never touches the user's file. Each open drawing with unsaved changes has a folder under the recovery root
//! (`%LOCALAPPDATA%\OSAS\recovery`): `manifest.json` (which drawing, when, and the list of parts), `pack-N.bin` files holding the parts of the
//! native model (`aad_io::native`), and `lock`, held open by the program that owns the folder. A snapshot writes only the parts that are not in a
//! pack yet (after a small edit: one chunk of the drawing), then swaps the manifest.
//!
//! Every write is whole or absent: a pack and a manifest are written under a temporary name, flushed to the disk and renamed. A crash between
//! the two leaves the previous manifest, which still describes a complete earlier snapshot. On the next start [`scan`] finds the folders whose
//! owner is gone; every part is checked against its hash when a snapshot is loaded.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aad_doc::Drawing;
use aad_io::native::Part;
use serde::{Deserialize, Serialize};

/// The environment variable that moves the recovery root (tests; a managed machine).
pub const ROOT_ENV: &str = "AAD_RECOVERY_DIR";
const MANIFEST: &str = "manifest.json";
const FORMAT: u32 = 1;
/// More packs than this, or more dead than live bytes in them, and the next snapshot writes everything into one new pack.
const MAX_PACKS: usize = 8;
/// Largest part read back (a damaged length cannot make the reader allocate more).
const MAX_PART_BYTES: u64 = 1 << 30;

static ROOT_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Keep recovery snapshots under `dir` for the rest of the process (`None`: back to the default). Tests use this.
pub fn set_root(dir: Option<PathBuf>) {
    if let Ok(mut slot) = ROOT_OVERRIDE.lock() {
        *slot = dir;
    }
}

/// Where recovery snapshots are kept.
pub fn root() -> PathBuf {
    if let Some(dir) = ROOT_OVERRIDE.lock().ok().and_then(|slot| slot.clone()) {
        return dir;
    }
    if let Some(v) = std::env::var_os(ROOT_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(v);
    }
    match std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        Some(base) => PathBuf::from(base).join("OSAS").join("recovery"),
        None => std::env::temp_dir().join("OSAS").join("recovery"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PartRef {
    hash: String,
    pack: String,
    offset: u64,
    len: u64,
    entities: usize,
}

/// What a snapshot is of.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    /// The encoding of the parts (`aad_io::native::FORMAT_VERSION`).
    pub native_format: u32,
    pub pid: u32,
    /// When the snapshot was taken: seconds since 1970, and the same as UTC text.
    pub saved_unix: u64,
    pub saved_utc: String,
    /// The drawing's file, when it has one (the project file a restored drawing is saved to).
    pub path: Option<String>,
    pub title: String,
    /// The DWG or DXF the drawing was imported from, when its file is a working copy.
    pub source: Option<String>,
    pub entities: usize,
    parts: Vec<PartRef>,
}

/// The facts of a drawing that go into its snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub path: Option<String>,
    pub title: String,
    pub source: Option<String>,
    pub entities: usize,
}

/// What one snapshot wrote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub parts: usize,
    /// Parts that were not in a pack yet.
    pub new_parts: usize,
    pub new_bytes: u64,
    /// Everything was rewritten into one pack (the first snapshot, or a compaction).
    pub full: bool,
}

fn io_err(what: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", what.display())
}

/// Open the lock file of a recovery folder so that no other process can open it while this handle lives. `Err` when another process holds it.
fn take_lock(dir: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(dir.join("lock"))
}

/// Write `bytes` to `path` so that the file is whole or absent: a temporary name in the same folder, flushed to the disk, then renamed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{}.tmp-{}", path.extension().and_then(|e| e.to_str()).unwrap_or("x"), std::process::id()));
    let done = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// The recovery folder of one open drawing, owned by this process.
pub struct Store {
    dir: PathBuf,
    _lock: std::fs::File,
    /// Where each part already written lives: hash to (pack, offset, length).
    have: HashMap<String, (String, u64, u64)>,
    /// Pack files in the folder with their sizes.
    packs: Vec<(String, u64)>,
    next_pack: u64,
}

impl Store {
    /// Make a new recovery folder under `root`. `key` names it (letters, digits, `-`); a folder of that name left by an earlier run is replaced.
    pub fn create(root: &Path, key: &str) -> Result<Store, String> {
        let key: String = key.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(64).collect();
        if key.is_empty() {
            return Err("recovery: no key".into());
        }
        let dir = root.join(format!("rec-{key}"));
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
        let lock = take_lock(&dir).map_err(|e| io_err(&dir, e))?;
        Ok(Store { dir, _lock: lock, have: HashMap::new(), packs: Vec::new(), next_pack: 1 })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Write a snapshot: the parts that are not in a pack yet, then the manifest. The folder holds the previous snapshot until the manifest is
    /// swapped, and this one after. Packs the new manifest does not use are removed.
    pub fn write(&mut self, facts: &Facts, parts: &[Arc<Part>]) -> Result<Written, String> {
        let live: u64 = parts.iter().map(|p| p.bytes.len() as u64).sum();
        let stored: u64 = self.packs.iter().map(|(_, n)| *n).sum();
        let full = self.packs.is_empty() || self.packs.len() >= MAX_PACKS || stored > live.saturating_mul(2).max(1 << 20);
        if full {
            self.have.clear();
        }
        let mut done = Written { parts: parts.len(), full, ..Written::default() };
        let pack_name = format!("pack-{}.bin", self.next_pack);
        let mut body: Vec<u8> = Vec::new();
        let mut fresh: HashMap<&str, (u64, u64)> = HashMap::new();
        for p in parts {
            if self.have.contains_key(&p.hash) || fresh.contains_key(p.hash.as_str()) {
                continue;
            }
            fresh.insert(p.hash.as_str(), (body.len() as u64, p.bytes.len() as u64));
            body.extend_from_slice(&p.bytes);
            done.new_parts += 1;
        }
        done.new_bytes = body.len() as u64;
        if !fresh.is_empty() {
            let path = self.dir.join(&pack_name);
            write_atomic(&path, &body).map_err(|e| io_err(&path, e))?;
            self.next_pack += 1;
            self.packs.push((pack_name.clone(), body.len() as u64));
            for (hash, (offset, len)) in fresh {
                self.have.insert(hash.to_string(), (pack_name.clone(), offset, len));
            }
        }
        let mut refs = Vec::with_capacity(parts.len());
        for p in parts {
            let (pack, offset, len) = self.have.get(&p.hash).cloned().ok_or_else(|| "recovery: a part was not stored".to_string())?;
            refs.push(PartRef { hash: p.hash.clone(), pack, offset, len, entities: p.entities });
        }
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let manifest = Manifest {
            format: FORMAT,
            native_format: aad_io::native::FORMAT_VERSION,
            pid: std::process::id(),
            saved_unix: secs,
            saved_utc: aad_odp::now_utc(),
            path: facts.path.clone(),
            title: facts.title.clone(),
            source: facts.source.clone(),
            entities: facts.entities,
            parts: refs,
        };
        let text = serde_json::to_vec(&manifest).map_err(|e| format!("recovery: {e}"))?;
        let path = self.dir.join(MANIFEST);
        write_atomic(&path, &text).map_err(|e| io_err(&path, e))?;
        // Only now is the previous snapshot no longer needed: drop the packs nothing refers to.
        let used: std::collections::HashSet<&str> = manifest.parts.iter().map(|r| r.pack.as_str()).collect();
        let (keep, drop): (Vec<_>, Vec<_>) = std::mem::take(&mut self.packs).into_iter().partition(|(name, _)| used.contains(name.as_str()));
        for (name, _) in drop {
            let _ = std::fs::remove_file(self.dir.join(&name));
            self.have.retain(|_, (pack, ..)| *pack != name);
        }
        self.packs = keep;
        Ok(done)
    }

    /// Remove the folder: the drawing was saved or closed, there is nothing to recover.
    pub fn remove(self) {
        let dir = self.dir.clone();
        drop(self);
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// A snapshot left by a program that is no longer running.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// The folder name: what `load` and `discard` take.
    pub id: String,
    pub manifest: Manifest,
    /// The snapshot is newer than the drawing's file (or the drawing has no file, or the file is gone): there is something to restore.
    pub newer_than_file: bool,
    pub bytes: u64,
}

fn read_manifest(dir: &Path) -> Option<Manifest> {
    let text = std::fs::read(dir.join(MANIFEST)).ok()?;
    let m: Manifest = serde_json::from_slice(&text).ok()?;
    (m.format == FORMAT && !m.parts.is_empty()).then_some(m)
}

fn folder(root: &Path, id: &str) -> Result<PathBuf, String> {
    let ok = id.starts_with("rec-") && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !ok {
        return Err("recovery: no such snapshot".into());
    }
    Ok(root.join(id))
}

/// The snapshots under `root` whose owner is gone (it crashed, or was killed, or the machine went down), newest first. Folders in use by a
/// running program are skipped. A leftover folder without a readable snapshot is removed.
pub fn scan(root: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let dir = entry.path();
        let id = entry.file_name().to_string_lossy().into_owned();
        if !dir.is_dir() || !id.starts_with("rec-") {
            continue;
        }
        // The owner holds the lock for as long as it runs.
        let Ok(lock) = take_lock(&dir) else { continue };
        drop(lock);
        let Some(manifest) = read_manifest(&dir) else {
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        };
        let newer_than_file = match manifest.path.as_deref().map(std::fs::metadata) {
            Some(Ok(meta)) => {
                let file = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                manifest.saved_unix > file
            }
            _ => true,
        };
        let bytes = manifest.parts.iter().map(|p| p.len).sum();
        out.push(Found { id, manifest, newer_than_file, bytes });
    }
    out.sort_by(|a, b| b.manifest.saved_unix.cmp(&a.manifest.saved_unix).then_with(|| a.id.cmp(&b.id)));
    out
}

/// Read the drawing of a snapshot. Every part is checked against its hash: a damaged snapshot is an error, never a partial drawing.
pub fn load(root: &Path, id: &str) -> Result<(Manifest, Drawing), String> {
    let dir = folder(root, id)?;
    let manifest = read_manifest(&dir).ok_or_else(|| "recovery: the snapshot cannot be read".to_string())?;
    if manifest.native_format != aad_io::native::FORMAT_VERSION {
        return Err(format!("recovery: the snapshot is format {} (this program reads {})", manifest.native_format, aad_io::native::FORMAT_VERSION));
    }
    let mut files: HashMap<&str, std::fs::File> = HashMap::new();
    let mut parts: Vec<Vec<u8>> = Vec::with_capacity(manifest.parts.len());
    for r in &manifest.parts {
        if r.len > MAX_PART_BYTES || !r.pack.starts_with("pack-") || r.pack.contains(['/', '\\']) {
            return Err("recovery: the snapshot is damaged".into());
        }
        if !files.contains_key(r.pack.as_str()) {
            let f = std::fs::File::open(dir.join(&r.pack)).map_err(|e| io_err(&dir.join(&r.pack), e))?;
            files.insert(r.pack.as_str(), f);
        }
        let Some(f) = files.get_mut(r.pack.as_str()) else { return Err("recovery: the snapshot is damaged".into()) };
        let mut bytes = vec![0u8; r.len as usize];
        f.seek(SeekFrom::Start(r.offset)).and_then(|_| f.read_exact(&mut bytes)).map_err(|_| "recovery: the snapshot is incomplete".to_string())?;
        if aad_odp::sha256_hex(&bytes) != r.hash {
            return Err("recovery: the snapshot is damaged (a part does not match its hash)".into());
        }
        parts.push(bytes);
    }
    let drawing = aad_io::native::decode(&parts).map_err(|e| format!("recovery: {e}"))?;
    Ok((manifest, drawing))
}

/// Remove a snapshot that was restored or that the user does not want.
pub fn discard(root: &Path, id: &str) -> Result<(), String> {
    let dir = folder(root, id)?;
    // Not while its owner is still running.
    let lock = take_lock(&dir).map_err(|_| "recovery: the snapshot belongs to a running program".to_string())?;
    drop(lock);
    std::fs::remove_dir_all(&dir).map_err(|e| io_err(&dir, e))
}

#[cfg(test)]
mod tests {
    use aad_doc::{Entity, EntityKind, Handle, Line};
    use aad_geom::Vec3;
    use aad_io::native::Encoder;

    use super::*;

    fn drawing(n: u64) -> Drawing {
        let mut d = Drawing::new_metric();
        for h in 1..=n {
            let x = h as f64;
            d.model.push(Entity::new(Handle(h), EntityKind::Line(Line { a: Vec3 { x, y: 0.0, z: 0.0 }, b: Vec3 { x, y: 5.0, z: 0.0 } })));
        }
        d.handseed = n + 1;
        d
    }

    fn facts(d: &Drawing) -> Facts {
        Facts { path: None, title: "Drawing1".into(), source: None, entities: d.entity_count() }
    }

    #[test]
    fn a_snapshot_is_found_after_its_owner_is_gone_and_loads_exactly() {
        let tmp = tempfile::tempdir().unwrap();
        let d = drawing(5000);
        let mut enc = Encoder::default();
        let mut store = Store::create(tmp.path(), "1-1").unwrap();
        let w = store.write(&facts(&d), &enc.encode(&d).unwrap()).unwrap();
        assert!(w.full && w.new_parts == w.parts);
        // While the owner runs, nobody else sees the folder as abandoned (the lock is exclusive on Windows).
        if cfg!(windows) {
            assert!(scan(tmp.path()).is_empty());
            assert!(discard(tmp.path(), "rec-1-1").is_err());
        }
        let dir = store.dir().to_path_buf();
        drop(store); // the owner dies without cleaning up
        let found = scan(tmp.path());
        assert_eq!(found.len(), 1);
        let f = found.first().unwrap();
        assert_eq!((f.id.as_str(), f.manifest.title.as_str(), f.manifest.entities, f.newer_than_file), ("rec-1-1", "Drawing1", 5000, true));
        let (m, back) = load(tmp.path(), &f.id).unwrap();
        assert_eq!(back, d);
        assert_eq!(m.pid, std::process::id());
        discard(tmp.path(), &f.id).unwrap();
        assert!(!dir.exists() && scan(tmp.path()).is_empty());
        assert!(load(tmp.path(), "..").is_err() && discard(tmp.path(), "rec-../x").is_err());
    }

    #[test]
    fn a_later_snapshot_writes_only_what_changed_and_packs_stay_bounded() {
        let tmp = tempfile::tempdir().unwrap();
        let mut d = drawing(20_000);
        let mut enc = Encoder::default();
        let mut store = Store::create(tmp.path(), "k").unwrap();
        let first = store.write(&facts(&d), &enc.encode(&d).unwrap()).unwrap();
        let mut sizes = Vec::new();
        for round in 0..30u64 {
            d.model.modify(Handle(1 + round * 500), |e| e.common.layer = format!("L{round}"));
            let w = store.write(&facts(&d), &enc.encode(&d).unwrap()).unwrap();
            if !w.full {
                assert!(w.new_parts <= 2, "the head and the chunk touched, not {} parts", w.new_parts);
                assert!(w.new_bytes < first.new_bytes / 4);
            }
            let packs = std::fs::read_dir(store.dir()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("pack-")).count();
            assert!(packs <= MAX_PACKS, "{packs} packs");
            sizes.push(std::fs::read_dir(store.dir()).unwrap().flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum::<u64>());
        }
        assert!(sizes.iter().all(|s| *s < first.new_bytes * 4), "the folder does not grow without bound: {sizes:?}");
        let dir_name = store.dir().file_name().unwrap().to_string_lossy().into_owned();
        drop(store);
        assert_eq!(load(tmp.path(), &dir_name).unwrap().1, d);
    }

    #[test]
    fn a_crash_in_the_middle_of_a_snapshot_leaves_the_previous_one_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let d = drawing(3000);
        let mut enc = Encoder::default();
        let mut store = Store::create(tmp.path(), "k").unwrap();
        store.write(&facts(&d), &enc.encode(&d).unwrap()).unwrap();
        let dir = store.dir().to_path_buf();
        drop(store);
        // What a kill during the next snapshot can leave: a half-written pack and a half-written manifest, both under temporary names.
        std::fs::write(dir.join("pack-2.bin.tmp-999"), b"half a pack").unwrap();
        std::fs::write(dir.join("manifest.json.tmp-999"), b"{\"format\":1,\"parts\":[").unwrap();
        let found = scan(tmp.path());
        assert_eq!(found.len(), 1);
        assert_eq!(load(tmp.path(), "rec-k").unwrap().1, d);

        // A damaged pack is detected by the part hashes; a truncated one by its length; a damaged manifest is not a snapshot at all.
        let pack = dir.join("pack-1.bin");
        let mut bytes = std::fs::read(&pack).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x55;
        std::fs::write(&pack, &bytes).unwrap();
        assert!(load(tmp.path(), "rec-k").unwrap_err().contains("damaged"));
        bytes.truncate(mid);
        std::fs::write(&pack, &bytes).unwrap();
        assert!(load(tmp.path(), "rec-k").is_err());
        std::fs::write(dir.join(MANIFEST), b"{ not json").unwrap();
        assert!(scan(tmp.path()).is_empty(), "and the unreadable folder is cleared away");
        assert!(!dir.exists());
    }

    #[test]
    fn a_snapshot_older_than_its_file_is_marked() {
        let tmp = tempfile::tempdir().unwrap();
        let d = drawing(10);
        let file = tmp.path().join("Plan.osp");
        let mut store = Store::create(tmp.path(), "k").unwrap();
        let mut f = facts(&d);
        f.path = Some(file.to_string_lossy().into_owned());
        store.write(&f, &Encoder::default().encode(&d).unwrap()).unwrap();
        let dir = store.dir().to_path_buf();
        drop(store);
        assert!(scan(tmp.path()).first().unwrap().newer_than_file, "the file does not exist");
        // Make the snapshot an hour old, then save the file now.
        let mut m: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join(MANIFEST)).unwrap()).unwrap();
        m["savedUnix"] = serde_json::json!(m["savedUnix"].as_u64().unwrap() - 3600);
        std::fs::write(dir.join(MANIFEST), serde_json::to_vec(&m).unwrap()).unwrap();
        std::fs::write(&file, b"saved later").unwrap();
        assert!(!scan(tmp.path()).first().unwrap().newer_than_file);
    }

    #[test]
    fn write_atomic_replaces_or_leaves_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        // The destination cannot be replaced (it is a folder): the write fails and nothing temporary is left.
        let blocked = tmp.path().join("dir.json");
        std::fs::create_dir(&blocked).unwrap();
        assert!(write_atomic(&blocked, b"x").is_err());
        let names: Vec<String> = std::fs::read_dir(tmp.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(names.iter().all(|n| !n.contains("tmp")), "{names:?}");
    }
}
