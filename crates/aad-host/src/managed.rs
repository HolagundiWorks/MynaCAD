//! Managed mode (`docs/ops-first-workflow.md`, package B): MynaOps owns the project, MynaCAD works inside it. The policy lives here, in the host,
//! and not in the engine commands: a project cannot be opened, created or saved under another name, and the project configuration (levels, layer sets,
//! sheet sets, project details) cannot be edited by commands; it arrives through `project.config.apply`, which MynaCAD runs when the configuration the
//! `.osp` carries is newer than what it has applied. `--standalone` (no managed state) keeps every command as it was.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Host;

/// The message of every refused way of opening a project.
pub const OPEN_REFUSED: &str = "projects open through MynaOps";
/// The message of every refused configuration edit.
pub const CONFIG_REFUSED: &str = "the project configuration (levels, layer sets, sheet sets, project details) is edited in MynaOps";

/// What a managed host knows about its project.
#[derive(Debug, Default)]
pub struct Managed {
    /// The project file given with `--project`.
    pub project: PathBuf,
    /// The MynaOps process that started this MynaCAD (`--ops-pid`), when given.
    pub ops_pid: Option<u32>,
    /// The configuration revision applied (or tried: a failed revision is not retried) in this run.
    pub applied: Option<u64>,
    /// `config.proposal` was offered for an unadopted project in this run.
    pub proposed: bool,
    /// MynaOps has closed (the shell says so): only then may MynaCAD start the headless AI host (docs/ops-first-workflow.md 16.1, Q10).
    pub ops_closed: bool,
}

fn has_key(p: &Value, keys: &[&str]) -> bool {
    p.as_object().is_some_and(|o| keys.iter().any(|k| o.get(*k).is_some_and(|v| !v.is_null())))
}

fn writes(p: &Value) -> bool {
    p.as_object().is_some_and(|o| o.values().any(|v| !v.is_null()))
}

/// The engine policy of a managed session: why `command` with `params` is refused, or `None`.
pub fn refuse(command: &str, params: &Value) -> Option<String> {
    let say = |m: &str| Some(m.to_string());
    match command {
        "open" | "new" | "project.setup" | "project.open" | "backup.restore" => say(OPEN_REFUSED),
        "company.pack.import"
        | "project.wizard"
        | "level.new"
        | "level.delete"
        | "level.setup"
        | "level.fromfloors"
        | "hcwlayers"
        | "sheetset.standard"
        | "sheetset.new"
        | "sheetset.delete"
        | "project.merge" => say(CONFIG_REFUSED),
        // Importing a drawing as a new level adds a level: only MynaOps does that. As annotation of a sheet set it stays.
        "level.import" if !has_key(params, &["sheetSet"]) => say(CONFIG_REFUSED),
        // Moving objects to a level that does not exist makes the level.
        "level.moveto" if params.get("create").and_then(Value::as_bool) == Some(true) => say(CONFIG_REFUSED),
        "level.set" if has_key(params, &["name", "elevation", "height"]) => say(CONFIG_REFUSED),
        "project" | "floors" | "beams" if writes(params) => say(CONFIG_REFUSED),
        "saveas" | "qsave" if params.get("path").and_then(Value::as_str).is_some_and(aad_engine::cmd::file::is_container) => say(OPEN_REFUSED),
        _ => None,
    }
}

pub(crate) fn same_file(a: &str, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(Path::new(a)) == canon(b)
}

impl Host {
    /// Make this host managed for `project` (a project file): the policy is set, and opening is limited to [`Host::open_managed_start`]. Refuses a
    /// path that is not a project file of format `osp`.
    pub fn set_managed(&mut self, project: &str, ops_pid: Option<u32>) -> Result<(), String> {
        if project.is_empty() || project.len() > 4096 || project.contains('\0') {
            return Err("missing project path".into());
        }
        let path = PathBuf::from(project);
        if !path.is_absolute() {
            return Err("the project path must be absolute".into());
        }
        if !aad_engine::cmd::file::is_container(project) {
            return Err(format!("{project} is not a project file (.osp)"));
        }
        if !path.is_file() {
            return Err(format!("{project} does not exist"));
        }
        aad_odp::config::probe(&path).map_err(|e| format!("{project} is not a project file MynaCAD can read: {e}"))?;
        self.managed = Some(Managed { project: path, ops_pid, ..Managed::default() });
        self.session.policy = Some(refuse);
        crate::osas_ai::suppress_autostart(true);
        Ok(())
    }

    /// Open the managed project: the only open a managed host allows.
    pub fn open_managed_start(&mut self) -> Result<(), String> {
        let project = self.managed.as_ref().map(|m| m.project.to_string_lossy().into_owned()).ok_or("the host is not managed")?;
        self.open_start_inner(&project, aad_engine::nativefile::OnChanged::Keep)
    }

    /// `host.opsclosed {closed}`: the shell reports that MynaOps ended (or is back). While MynaOps runs a managed MynaCAD never starts the AI host;
    /// after it ended, the host may be started by a question (Q10). Returns `{managed, closed, aiAutostart}`; ignored when not managed.
    pub(crate) fn ops_closed(&mut self, params: &Value) -> Value {
        let closed = params.get("closed").and_then(Value::as_bool).unwrap_or(false);
        if let Some(m) = self.managed.as_mut() {
            m.ops_closed = closed;
        }
        let allowed = self.ai_autostart_allowed();
        crate::osas_ai::suppress_autostart(self.managed.is_some() && !allowed);
        json!({ "managed": self.managed.is_some(), "closed": closed && self.managed.is_some(), "aiAutostart": allowed })
    }

    /// Whether this host may start the AI host: a standalone host always, a managed one only after MynaOps closed.
    pub(crate) fn ai_autostart_allowed(&self) -> bool {
        self.managed.as_ref().is_none_or(|m| m.ops_closed)
    }

    /// `host.managed`: whether the host is managed and for which project.
    pub(crate) fn managed_info(&self) -> Value {
        match &self.managed {
            Some(m) => json!({ "managed": true, "project": m.project.to_string_lossy(), "opsPid": m.ops_pid, "appliedRev": m.applied }),
            None => json!({ "managed": false }),
        }
    }

    /// `project.config.poll {idle}`: compare the configuration revision in the project file with what was applied and, when the shell says it is
    /// idle and the engine has nothing interactive running, apply it as one engine step ("Project configuration"). Cheap when nothing changed
    /// (one read-only open of the file). The shell calls it about every 3 seconds and once after opening, before the drawing is shown.
    pub(crate) fn config_poll(&mut self, params: &Value) -> Result<Value, String> {
        let Some(m) = self.managed.as_ref() else {
            return Ok(json!({ "state": "none", "text": "" }));
        };
        // Realtime sync off: the configuration is not read and nothing is applied until Sync now (`force`) or switching on.
        if !self.sync.on && !params.get("force").and_then(Value::as_bool).unwrap_or(false) {
            return Ok(json!({ "state": "paused", "text": crate::syncctl::SYNC_OFF }));
        }
        let project = m.project.clone();
        let project_text = project.to_string_lossy().into_owned();
        let on_project = self.session.state().ok().and_then(|st| st.path.clone()).is_some_and(|p| same_file(&p, &project));
        if !on_project {
            return Ok(json!({ "state": "none", "text": "" }));
        }
        self.sync.reads += 1;
        let loaded = match aad_odp::config::probe(&project) {
            Ok(l) => l,
            Err(e) => return Ok(json!({ "state": "unreadable", "text": "Project configuration unreadable", "reason": e.to_string() })),
        };
        let config = &loaded.read.config;
        if !config.configured() {
            // A project MynaOps has not adopted: offer what the drawing has once, and say so. The drawing itself stays as it is.
            let proposed = self.managed.as_ref().is_some_and(|m| m.proposed);
            let mut recorded = false;
            if !proposed {
                recorded = self.session.execute("project.config.propose", &json!({ "path": project_text, "record": true })).is_ok();
                if let Some(m) = self.managed.as_mut() {
                    m.proposed = true;
                }
            }
            return Ok(json!({ "state": "unadopted", "text": "Adopt this project in MynaOps first", "recorded": recorded }));
        }
        if config.schema.as_ref().is_some_and(|s| s.min_reader > aad_odp::config::READER_VERSION) {
            return Ok(json!({ "state": "update", "text": "Update MynaCAD" }));
        }
        let rev = config.revision();
        // The first poll of a run always applies, whatever `config.applied.cad` says: the file's report can name a revision whose result was never
        // saved with the drawing (closed or crashed before a save), and applying what the drawing already has changes nothing.
        let applied = self.managed.as_ref().and_then(|m| m.applied);
        if applied == Some(rev) {
            if let Some(m) = self.managed.as_mut() {
                m.applied = Some(rev);
            }
            return Ok(json!({ "state": "current", "text": "", "rev": rev }));
        }
        let idle = params.get("idle").and_then(Value::as_bool).unwrap_or(false);
        let busy = !idle || self.session.running.is_some() || self.session.block_edit.is_some() || self.loading.is_some() || self.saving.is_some();
        if busy {
            return Ok(json!({ "state": "waiting", "text": "Configuration update waiting", "rev": rev }));
        }
        let outcome = self.session.execute("project.config.apply", &json!({ "path": project_text, "record": true }));
        if let Some(m) = self.managed.as_mut() {
            // A failed revision is recorded by the command and is not tried again until MynaOps writes a newer one.
            m.applied = Some(rev);
        }
        #[cfg(windows)]
        {
            self.paint = None;
            self.paint_dirty = true;
        }
        match outcome {
            Ok(v) => Ok(json!({
                "state": "applied",
                "text": "",
                "rev": rev,
                "result": v.get("result"),
                "changed": v.get("changed"),
                "items": v.get("items"),
            })),
            Err(e) => Ok(json!({ "state": "failed", "text": "Configuration update failed", "reason": e.to_string(), "rev": rev })),
        }
    }
}
