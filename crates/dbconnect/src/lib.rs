//! Tren's connection to the suite database.
//!
//! The workflow (Kivo `docs/WORKFLOW-KIVO-HUB.md`): Tren and Falcon share one `.osp` file that is connected to the suite database. Myna hosts
//! the database service; Kivo signs the company in and hands out sessions. Tren
//!
//! 1. asks **Kivo** for a session when a project file is opened (Kivo opens its sign-in if nobody is signed in),
//! 2. tells **Myna's database service** about the file ([`Service::connect`]), and
//! 3. after each saved revision asks the service to push it ([`Connector::project_saved`]).
//!
//! Without Kivo, without Myna or without a network the drawing still opens and saves as before: the connection only reports its state
//! ([`Snapshot::label`]) and never blocks the UI. A newer shared revision is never adopted silently.
#![forbid(unsafe_code)]

pub mod broker;
pub mod service;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use broker::{BrokerError, PipeTransport, Transport};
use service::{Service, ServiceError};

/// Where the connection stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    /// Not enabled (headless runs, tests) or no project connected yet.
    #[default]
    Idle,
    Connecting,
    Connected,
    /// Nobody is signed in to Kivo, or the sign-in was closed.
    SignedOut,
    /// Kivo or Myna is not running, or the service cannot be reached.
    Offline,
    /// Something this client cannot fix: the file has no project id, Kivo refused the role.
    Error,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub state: State,
    /// The connected project's name.
    pub project: Option<String>,
    /// One sentence for a tooltip: why the state is what it is.
    pub detail: String,
    /// Revisions waiting to go to the shared copy (from the service, when it answered).
    pub pending: i64,
}

impl Snapshot {
    /// The short text for the status bar.
    pub fn label(&self) -> String {
        match self.state {
            State::Idle => String::new(),
            State::Connecting => "Database: connecting".into(),
            State::Connected if self.pending > 0 => format!("Database: connected, {} to send", self.pending),
            State::Connected => "Database: connected".into(),
            State::SignedOut => "Database: sign in to Kivo".into(),
            State::Offline => "Database: offline".into(),
            State::Error => "Database: problem".into(),
        }
    }
}

struct Project {
    id: String,
    name: String,
    path: String,
}

#[derive(Default)]
struct Inner {
    snap: Snapshot,
    token: Option<String>,
    project: Option<Project>,
    dirty: bool,
    pushing: bool,
}

/// How the connector reaches Kivo and Myna. The defaults are the real pipe and the real loopback service.
pub struct Config {
    pub transport: Arc<dyn Transport>,
    pub service: Service,
    /// Start Kivo hidden when its pipe is not there.
    pub launch_kivo: Arc<dyn Fn() -> std::io::Result<()> + Send + Sync>,
    /// `app` in the broker request: `tren` or `falcon`.
    pub app: &'static str,
    /// Let Kivo open its sign-in window when nobody is signed in.
    pub interactive: bool,
    /// How long to wait for Kivo to start.
    pub kivo_wait: Duration,
    /// Saves closer together than this go as one push.
    pub push_delay: Duration,
}

impl Config {
    pub fn real() -> Self {
        Config {
            transport: Arc::new(PipeTransport),
            service: Service::from_env(),
            launch_kivo: Arc::new(broker::launch_kivo),
            app: "tren",
            interactive: true,
            kivo_wait: Duration::from_secs(60),
            push_delay: Duration::from_millis(1500),
        }
    }
}

pub struct Connector {
    cfg: Config,
    enabled: AtomicBool,
    inner: Arc<Mutex<Inner>>,
}

fn lock(m: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Connector {
    pub fn new(cfg: Config, enabled: bool) -> Arc<Self> {
        Arc::new(Connector { cfg, enabled: AtomicBool::new(enabled), inner: Arc::new(Mutex::new(Inner::default())) })
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::SeqCst);
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.inner).snap.clone()
    }

    fn set(&self, state: State, detail: impl Into<String>) {
        let mut g = lock(&self.inner);
        g.snap.state = state;
        g.snap.detail = detail.into();
    }

    /// A session from Kivo: the one already held, else a new one (starting Kivo and its sign-in when needed).
    fn token(&self, project_id: &str, fresh: bool) -> Result<String, BrokerError> {
        if !fresh && let Some(t) = lock(&self.inner).token.clone() {
            return Ok(t);
        }
        broker::ensure_broker(self.cfg.transport.as_ref(), self.cfg.launch_kivo.as_ref(), self.cfg.kivo_wait)?;
        let session = broker::ask_session(self.cfg.transport.as_ref(), self.cfg.app, "db_connect", Some(project_id), self.cfg.interactive)?;
        lock(&self.inner).token = Some(session.token.clone());
        Ok(session.token)
    }

    fn broker_failed(&self, e: &BrokerError) {
        match e {
            BrokerError::SignedOut | BrokerError::Cancelled => self.set(State::SignedOut, e.to_string()),
            BrokerError::Unavailable(_) => self.set(State::Offline, e.to_string()),
            BrokerError::Denied(_) | BrokerError::Protocol(_) => self.set(State::Error, e.to_string()),
        }
    }

    /// A project file was opened: connect it. Runs on a background thread; returns at once.
    pub fn project_opened(self: &Arc<Self>, osp_path: &str) {
        if !self.enabled.load(Ordering::SeqCst) {
            return;
        }
        let me = Arc::clone(self);
        let path = osp_path.to_string();
        std::thread::spawn(move || me.connect_blocking(&path));
    }

    /// The work of [`Connector::project_opened`], on the calling thread.
    pub fn connect_blocking(&self, osp_path: &str) {
        if !osp_path.to_ascii_lowercase().ends_with(".osp") {
            return;
        }
        let meta = match aad_odp::Odp::open(Path::new(osp_path)).and_then(|o| o.meta()) {
            Ok(m) => m,
            Err(e) => return self.set(State::Error, format!("The project file could not be read: {e}")),
        };
        if meta.project_id.is_empty() {
            return self.set(State::Error, "The project file has no project id.");
        }
        {
            let mut g = lock(&self.inner);
            g.project = Some(Project { id: meta.project_id.clone(), name: meta.name.clone(), path: osp_path.to_string() });
            g.snap.project = Some(meta.name.clone());
            g.snap.state = State::Connecting;
            g.snap.detail = "Asking Kivo for a session".into();
        }
        let mut fresh = false;
        for _ in 0..2 {
            let token = match self.token(&meta.project_id, fresh) {
                Ok(t) => t,
                Err(e) => return self.broker_failed(&e),
            };
            match self.cfg.service.connect(&token, &meta.project_id, &meta.name, osp_path, meta.revision) {
                Ok(done) => {
                    let detail = match done.action.as_str() {
                        "pull" => "The shared copy has a newer revision; it is not applied until you choose to.".to_string(),
                        "conflict" => "The shared copy and this file both changed; nothing was overwritten.".to_string(),
                        _ => "Connected to the suite database.".to_string(),
                    };
                    self.set(State::Connected, detail);
                    if done.action == "push" {
                        self.push_now(meta.revision);
                    }
                    return;
                }
                Err(ServiceError::Unauthorized) if !fresh => fresh = true,
                Err(ServiceError::Unauthorized) => return self.set(State::SignedOut, "Kivo's session was refused."),
                Err(e @ ServiceError::Unreachable(_)) => return self.set(State::Offline, format!("{e}. Start Myna to sync.")),
                Err(e) => return self.set(State::Error, e.to_string()),
            }
        }
    }

    /// A revision was saved into the project file: send it. Several saves in a row go as one push.
    pub fn project_saved(self: &Arc<Self>, osp_path: &str) {
        if !self.enabled.load(Ordering::SeqCst) {
            return;
        }
        {
            let mut g = lock(&self.inner);
            let same = g.project.as_ref().is_some_and(|p| p.path.eq_ignore_ascii_case(osp_path));
            if !same || g.snap.state == State::Idle {
                // Not connected yet (first save of a new project file, or Kivo was not there at open): try to connect instead.
                drop(g);
                return self.project_opened(osp_path);
            }
            g.dirty = true;
            if g.pushing {
                return;
            }
            g.pushing = true;
        }
        let me = Arc::clone(self);
        std::thread::spawn(move || me.push_loop());
    }

    fn push_loop(&self) {
        loop {
            std::thread::sleep(self.cfg.push_delay);
            let path = {
                let mut g = lock(&self.inner);
                if !g.dirty {
                    g.pushing = false;
                    return;
                }
                g.dirty = false;
                g.project.as_ref().map(|p| p.path.clone())
            };
            let Some(path) = path else {
                lock(&self.inner).pushing = false;
                return;
            };
            match aad_odp::Odp::open(Path::new(&path)).and_then(|o| o.revision()) {
                Ok(revision) => self.push_now(revision),
                Err(e) => self.set(State::Error, format!("The project file could not be read: {e}")),
            }
        }
    }

    /// Send `revision` of the connected project, on the calling thread.
    pub fn push_now(&self, revision: i64) {
        let (id, name) = match lock(&self.inner).project.as_ref() {
            Some(p) => (p.id.clone(), p.name.clone()),
            None => return,
        };
        let mut fresh = false;
        for _ in 0..2 {
            let token = match self.token(&id, fresh) {
                Ok(t) => t,
                Err(e) => return self.broker_failed(&e),
            };
            match self.cfg.service.push(&token, &id, revision) {
                Ok(remote) => {
                    let pending = self.cfg.service.project(&token, &id).map(|s| s.pending).unwrap_or(0);
                    let mut g = lock(&self.inner);
                    g.snap.state = State::Connected;
                    g.snap.pending = pending;
                    g.snap.detail = match remote {
                        Some(r) => format!("{name}: revision {r} sent to the suite database."),
                        None => format!("{name}: saved. Project files stay on this PC; the local file is the shared database."),
                    };
                    return;
                }
                Err(ServiceError::Unauthorized) if !fresh => fresh = true,
                Err(ServiceError::Unauthorized) => return self.set(State::SignedOut, "Kivo's session was refused."),
                Err(ServiceError::Conflict) => return self.set(State::Connected, "The shared copy has a newer revision; this save was not sent. Nothing was overwritten."),
                Err(e @ ServiceError::Unreachable(_)) => return self.set(State::Offline, format!("{e}. The save stays local until Myna runs.")),
                Err(e) => return self.set(State::Error, e.to_string()),
            }
        }
    }
}

static GLOBAL: OnceLock<Arc<Connector>> = OnceLock::new();

/// The connector of this process. Off until [`enable`] is called, so headless runs (command line, MCP, tests) never start Kivo.
pub fn global() -> &'static Arc<Connector> {
    GLOBAL.get_or_init(|| Connector::new(Config::real(), false))
}

/// Turn the connection on: the desktop shell calls this once. `AAD_DB_CONNECT=0` keeps it off.
pub fn enable() {
    if std::env::var("AAD_DB_CONNECT").is_ok_and(|v| v == "0") {
        return;
    }
    global().set_enabled(true);
}

#[cfg(test)]
mod tests;
