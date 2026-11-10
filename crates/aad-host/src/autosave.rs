//! Autosave: while a drawing has unsaved changes, write a recovery snapshot of it now and then, on a worker thread, into the recovery folder
//! (`aad_engine::recovery`). The user's file and its revision are never touched. What is kept and how to restore: `docs/AUTOSAVE-BACKUP.md`.
//!
//! [`Autosave::tick`] runs on every pump of the host and only looks at pointers and clocks; the work is done by a thread it starts. A snapshot is
//! due when the drawing changed since the last one and either the interval has passed or no edit has arrived for the settle time. Both stretch
//! when snapshots are slow (a very large drawing on a slow disk), so the worker never runs back to back.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aad_engine::Session;
use aad_engine::doc::Drawing;
use aad_engine::recovery::{self, Facts, Store, Written};
use aad_io::native::Encoder;

/// A pause in editing this long makes a snapshot due.
pub(crate) const SETTLE: Duration = Duration::from_secs(10);
/// The interval and the settle time are at least this many times the duration of the last snapshot.
const INTERVAL_FACTOR: u32 = 20;
const SETTLE_FACTOR: u32 = 4;

/// What the worker keeps between snapshots of one drawing.
#[derive(Default)]
struct Work {
    store: Option<Store>,
    encoder: Encoder,
}

struct Done {
    written: Written,
    ms: f64,
}

struct DocSaver {
    work: Arc<Mutex<Work>>,
    /// The drawing last seen, to notice an edit (a new drawing pointer).
    seen: Arc<Drawing>,
    last_edit: Instant,
    /// The drawing of the last snapshot that succeeded.
    snapped: Option<Arc<Drawing>>,
    /// When the first edit not yet in a snapshot was seen (the interval runs from here). `None`: nothing new since the last snapshot. A
    /// drawing that is unsaved from the moment it is opened (flattened on import) has nothing to recover until it is edited.
    pending: Option<Instant>,
    last_ms: f64,
    job: Option<(Arc<Drawing>, Receiver<Result<Done, String>>)>,
    /// A snapshot left by a crashed run that this drawing was restored from: removed once the drawing is saved or has a snapshot of its own.
    adopted: Option<String>,
    /// After a failure, do not try again before this.
    retry_at: Option<Instant>,
}

/// The last thing that happened to the active drawing's file or snapshot, for the status line.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveStatus {
    /// `saved`, `autosaved`, `failed` (a save), `autosaveFailed`, or empty when nothing has happened yet.
    pub kind: String,
    /// Seconds since 1970 (the shell shows it as local hh:mm).
    pub unix: u64,
    /// The reason of a failure; for a save, the file.
    pub message: String,
    /// The drawing it is about.
    pub title: String,
}

pub(crate) struct Autosave {
    docs: HashMap<u64, DocSaver>,
    /// The saved state of each open drawing as last seen: a new one means it was saved (by any path: the menu, the command line, a script).
    saved: HashMap<u64, Arc<Drawing>>,
    pub(crate) status: SaveStatus,
    /// Snapshot every drawing with unsaved changes on the next tick, whatever the clocks say (`autosave.now`, tests).
    pub(crate) force: bool,
    /// Counters for tests and `autosave.status`.
    pub(crate) snapshots: u64,
    pub(crate) failures: u64,
    pub(crate) last: Option<(Written, f64)>,
    root: std::path::PathBuf,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The worker pool for encoding snapshots: two threads, so a first snapshot of a very large drawing does not take every core from the UI.
fn pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: std::sync::OnceLock<Option<rayon::ThreadPool>> = std::sync::OnceLock::new();
    POOL.get_or_init(|| rayon::ThreadPoolBuilder::new().num_threads(2).thread_name(|i| format!("aad-autosave-{i}")).build().ok()).as_ref()
}

fn snapshot(work: &Mutex<Work>, root: &std::path::Path, key: &str, facts: &Facts, doc: &Drawing) -> Result<Done, String> {
    let t = Instant::now();
    let mut work = work.lock().map_err(|_| "autosave: the worker state is poisoned".to_string())?;
    let work = &mut *work;
    let parts = match pool() {
        Some(pool) => pool.install(|| work.encoder.encode(doc)),
        None => work.encoder.encode(doc),
    }
    .map_err(|e| e.to_string())?;
    if work.store.is_none() {
        std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
        work.store = Some(Store::create(root, key)?);
    }
    let Some(store) = work.store.as_mut() else { return Err("autosave: no recovery folder".into()) };
    let written = store.write(facts, &parts)?;
    Ok(Done { written, ms: t.elapsed().as_secs_f64() * 1e3 })
}

/// Where snapshots go: [`recovery::root`] (`AAD_RECOVERY_DIR`, else `%LOCALAPPDATA%\OSAS\recovery`). The unit tests of this crate never touch
/// the user's folder: without `AAD_RECOVERY_DIR` they use a folder of their own under the temporary directory.
fn default_root() -> std::path::PathBuf {
    if cfg!(test) && std::env::var_os(recovery::ROOT_ENV).is_none() {
        return std::env::temp_dir().join(format!("aad-host-test-recovery-{}", std::process::id()));
    }
    recovery::root()
}

impl Autosave {
    pub(crate) fn new() -> Self {
        Autosave {
            docs: HashMap::new(),
            saved: HashMap::new(),
            status: SaveStatus::default(),
            force: false,
            snapshots: 0,
            failures: 0,
            last: None,
            root: default_root(),
        }
    }

    /// The recovery folder this autosave writes to and `recovery.*` reads.
    pub(crate) fn root(&self) -> &std::path::Path {
        &self.root
    }

    #[cfg(test)]
    pub(crate) fn set_root(&mut self, root: std::path::PathBuf) {
        self.root = root;
    }

    /// `autosave.status`: the status line and the counters.
    pub(crate) fn status_json(&self, interval: Duration) -> serde_json::Value {
        serde_json::json!({
            "status": self.status,
            "intervalSecs": interval.as_secs(),
            "settleSecs": SETTLE.as_secs(),
            "snapshots": self.snapshots,
            "failures": self.failures,
            "busy": self.busy(),
            "root": self.root.to_string_lossy(),
            "last": self.last.map(|(w, ms)| serde_json::json!({
                "parts": w.parts, "newParts": w.new_parts, "newBytes": w.new_bytes, "full": w.full, "ms": ms
            })),
        })
    }

    /// Whether a snapshot is being written right now.
    pub(crate) fn busy(&self) -> bool {
        self.docs.values().any(|d| d.job.is_some())
    }

    /// Tie a restored drawing to the snapshot it came from, and make its first own snapshot due at once.
    pub(crate) fn adopt(&mut self, uid: u64, doc: &Arc<Drawing>, id: String) {
        let now = Instant::now();
        let saver = self.docs.entry(uid).or_insert_with(|| new_saver(doc, now));
        saver.adopted = Some(id);
        self.force = true;
    }

    pub(crate) fn note_save_failed(&mut self, title: &str, error: &str) {
        self.status = SaveStatus { kind: "failed".into(), unix: unix_now(), message: error.to_string(), title: title.to_string() };
    }

    /// Look at every open drawing: collect finished snapshots, drop the snapshots of drawings that were saved or closed, start the ones due.
    /// `interval` is the configured time between snapshots (zero turns the timer off; a forced snapshot still runs).
    pub(crate) fn tick(&mut self, session: &Session, interval: Duration) {
        let now = Instant::now();
        let force = std::mem::take(&mut self.force);
        // Drawings that were closed: nothing to recover.
        let open: std::collections::HashSet<u64> = session.docs.iter().map(|d| d.uid).collect();
        let closed: Vec<u64> = self.docs.keys().copied().filter(|uid| !open.contains(uid)).collect();
        for uid in closed {
            if let Some(saver) = self.docs.remove(&uid) {
                self.retire(saver);
            }
        }
        self.saved.retain(|uid, _| open.contains(uid));
        for state in &session.docs {
            // A save moves the drawing's saved state: that is the "Saved hh:mm" of the status line, whatever path the save took.
            match self.saved.get(&state.uid) {
                Some(prev) if !Arc::ptr_eq(prev, &state.saved) => {
                    if !state.is_dirty() && state.path.is_some() {
                        self.status = SaveStatus {
                            kind: "saved".into(),
                            unix: unix_now(),
                            message: state.path.clone().unwrap_or_default(),
                            title: state.title.clone(),
                        };
                    }
                    self.saved.insert(state.uid, state.saved.clone());
                }
                Some(_) => {}
                None => {
                    self.saved.insert(state.uid, state.saved.clone());
                }
            }
            let saver = self.docs.entry(state.uid).or_insert_with(|| new_saver(&state.doc, now));
            // A finished snapshot.
            if let Some((doc, rx)) = saver.job.take() {
                match rx.try_recv() {
                    Err(TryRecvError::Empty) => saver.job = Some((doc, rx)),
                    Ok(Ok(done)) => {
                        saver.snapped = Some(doc);
                        saver.last_ms = done.ms;
                        saver.retry_at = None;
                        self.snapshots += 1;
                        self.last = Some((done.written, done.ms));
                        self.status = SaveStatus { kind: "autosaved".into(), unix: unix_now(), message: String::new(), title: state.title.clone() };
                        if let Some(id) = saver.adopted.take() {
                            let _ = recovery::discard(&self.root, &id);
                        }
                    }
                    Ok(Err(e)) => fail(&mut self.status, &mut self.failures, saver, &state.title, e, now),
                    Err(TryRecvError::Disconnected) => {
                        fail(&mut self.status, &mut self.failures, saver, &state.title, "the autosave worker stopped".into(), now)
                    }
                }
            }
            if !Arc::ptr_eq(&saver.seen, &state.doc) {
                saver.seen = state.doc.clone();
                saver.last_edit = now;
                saver.pending.get_or_insert(now);
            }
            if !state.is_dirty() {
                // Saved (or never changed, or undone back to the saved state): a snapshot is of no use any more. The saver stays (so the
                // next edit is seen as one); only one that wrote something is replaced, which removes its folder.
                saver.pending = None;
                let wrote = saver.snapped.is_some() || saver.adopted.is_some() || saver.retry_at.is_some();
                if saver.job.is_none() && wrote {
                    let fresh = new_saver(&state.doc, now);
                    if let Some(old) = self.docs.insert(state.uid, fresh) {
                        self.retire(old);
                    }
                }
                continue;
            }
            if saver.job.is_some() || saver.snapped.as_ref().is_some_and(|s| Arc::ptr_eq(s, &state.doc)) {
                continue;
            }
            let slow = Duration::from_secs_f64(saver.last_ms / 1e3);
            let timed = !interval.is_zero() && saver.pending.is_some();
            let due_interval = timed && saver.pending.is_some_and(|since| now.duration_since(since) >= interval.max(slow * INTERVAL_FACTOR));
            let due_settle = timed && now.duration_since(saver.last_edit) >= SETTLE.max(slow * SETTLE_FACTOR);
            let waiting = saver.retry_at.is_some_and(|at| now < at);
            if !(force || ((due_interval || due_settle) && !waiting)) {
                continue;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let (work, doc, root) = (saver.work.clone(), state.doc.clone(), self.root.clone());
            let key = format!("{}-{}", std::process::id(), state.uid);
            let facts =
                Facts { path: state.path.clone(), title: state.title.clone(), source: state.source.clone(), entities: state.doc.entity_count() };
            let spawned = std::thread::Builder::new().name("aad-autosave".into()).spawn(move || {
                let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| snapshot(&work, &root, &key, &facts, &doc)))
                    .unwrap_or_else(|_| Err("internal error while writing the recovery snapshot".into()));
                let _ = tx.send(done);
            });
            match spawned {
                Ok(_) => {
                    saver.job = Some((state.doc.clone(), rx));
                    saver.pending = None;
                }
                Err(e) => fail(&mut self.status, &mut self.failures, saver, &state.title, e.to_string(), now),
            }
        }
    }

    /// Wait for the snapshots being written (tests, and a clean shutdown that wants them finished).
    pub(crate) fn wait(&mut self, session: &Session, limit: Duration) {
        let end = Instant::now() + limit;
        while self.busy() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(2));
            self.tick(session, Duration::ZERO);
        }
    }

    fn retire(&mut self, saver: DocSaver) {
        if let Some(id) = &saver.adopted {
            let _ = recovery::discard(&self.root, id);
        }
        // A worker still writing holds the lock; it finishes first, then the folder goes.
        let work = saver.work;
        let remove = move || {
            if let Ok(mut work) = work.lock()
                && let Some(store) = work.store.take()
            {
                store.remove();
            }
        };
        if saver.job.is_some() {
            let _ = std::thread::Builder::new().name("aad-autosave-clean".into()).spawn(remove);
        } else {
            remove();
        }
    }
}

fn new_saver(doc: &Arc<Drawing>, now: Instant) -> DocSaver {
    DocSaver {
        work: Arc::new(Mutex::new(Work::default())),
        seen: doc.clone(),
        last_edit: now,
        snapped: None,
        pending: None,
        last_ms: 0.0,
        job: None,
        adopted: None,
        retry_at: None,
    }
}

fn fail(status: &mut SaveStatus, failures: &mut u64, saver: &mut DocSaver, title: &str, error: String, now: Instant) {
    *failures += 1;
    saver.retry_at = Some(now + Duration::from_secs(30));
    *status = SaveStatus { kind: "autosaveFailed".into(), unix: unix_now(), message: error, title: title.to_string() };
}

impl Drop for Autosave {
    /// A clean end of the host: the drawings were saved or given up, so their snapshots go. A crash or a kill never gets here, and the
    /// snapshots stay for the next start.
    fn drop(&mut self) {
        for (_, saver) in self.docs.drain() {
            if let Some(id) = &saver.adopted {
                let _ = recovery::discard(&self.root, id);
            }
            if let Ok(mut work) = saver.work.lock()
                && let Some(store) = work.store.take()
            {
                store.remove();
            }
        }
    }
}
