//! Realtime sync on/off and Sync now (`docs/ops-first-workflow.md` section 15, `docs/live-sync.md`). The switch pauses the **detector**: the
//! `project.sync` probe and the `project.config.poll`. It never pauses writing (a save, `takeoff.record`). The state is per run, on at the start,
//! and not stored in the project file.

use std::path::Path;

use serde_json::{Value, json};

use crate::Host;

/// The text shown while sync is off.
pub const SYNC_OFF: &str = "Sync off";
/// Longest Sync now waits for the flushing save.
const FLUSH_WAIT: std::time::Duration = std::time::Duration::from_secs(300);

/// The per-run switch and the counter of detector reads (what the tests look at).
#[derive(Debug)]
pub struct Control {
    pub on: bool,
    /// File reads made by the detector since the run started: the probe, the configuration poll and the waiting probe.
    pub reads: u64,
}

impl Default for Control {
    fn default() -> Self {
        Control { on: true, reads: 0 }
    }
}

/// `Sync off`, or `Sync off - 2 changes waiting`.
pub fn off_text(waiting: i64) -> String {
    match waiting {
        n if n <= 0 => SYNC_OFF.to_string(),
        1 => format!("{SYNC_OFF} - 1 change waiting"),
        n => format!("{SYNC_OFF} - {n} changes waiting"),
    }
}

impl Host {
    fn active_project(&self) -> Option<String> {
        self.session.state().ok().and_then(|st| st.path.clone()).filter(|p| aad_engine::cmd::file::is_container(p))
    }

    /// The `sync` object: `{on, waiting, text, detectorReads}`. `look` takes the one probe that counts the waiting changes (only while off).
    pub(crate) fn sync_json(&mut self, look: bool) -> Value {
        let mut waiting = 0;
        if look
            && !self.sync.on
            && let Some(path) = self.active_project()
        {
            self.sync.reads += 1;
            waiting = aad_engine::projectsync::pending(Path::new(&path));
        }
        let text = if self.sync.on { String::new() } else { off_text(waiting) };
        json!({ "on": self.sync.on, "waiting": waiting, "text": text, "detectorReads": self.sync.reads })
    }

    /// `sync.set {on}`: switch the detector on or off. Switching off takes the one probe that counts what is waiting.
    pub(crate) fn sync_set(&mut self, params: &Value) -> Result<Value, String> {
        let on = params.get("on").and_then(Value::as_bool).ok_or("sync.set needs on: true or false")?;
        self.sync.on = on;
        Ok(json!({ "sync": self.sync_json(!on) }))
    }

    /// `sync.status`: the `sync` object; while off, one probe for the waiting count.
    pub(crate) fn sync_status(&mut self) -> Result<Value, String> {
        Ok(json!({ "sync": self.sync_json(true) }))
    }

    /// `project.sync` as the detector: nothing is read while sync is off. `force` is Sync now.
    pub(crate) fn project_sync(&mut self, params: &Value, force: bool) -> Value {
        if !self.sync.on && !force {
            return json!({ "state": "paused", "text": SYNC_OFF });
        }
        let Some(path) = self.active_project() else {
            return json!({ "state": "none", "text": "" });
        };
        self.sync.reads += 1;
        aad_engine::projectsync::status(Path::new(&path), params.get("ack").and_then(Value::as_bool).unwrap_or(false))
    }

    /// `sync.now {flush?, idle?}`: save unsaved work (unless `flush` is false: the shell has saved already), then one forced configuration poll and one forced probe that accepts the file, whatever the switch says. Writing is never refused by the switch.
    pub(crate) fn sync_now(&mut self, params: &Value) -> Result<Value, String> {
        let flush = params.get("flush").and_then(Value::as_bool).unwrap_or(true);
        let idle = params.get("idle").and_then(Value::as_bool).unwrap_or(true);
        let mut flushed = false;
        if flush {
            let started = std::time::Instant::now();
            // A save already running is waited for; an unsaved project is saved the normal way.
            while self.saving.is_some() {
                if self.save_poll()? != 1 {
                    break;
                }
                if started.elapsed() > FLUSH_WAIT {
                    return Err("the save is still running".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let dirty = self.session.state().map(|st| st.is_dirty()).unwrap_or(false) && self.active_project().is_some();
            if dirty {
                self.save_start(None)?;
                while self.save_poll()? == 1 {
                    if started.elapsed() > FLUSH_WAIT {
                        return Err("the save is still running".into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                flushed = true;
            }
        }
        let config = self.config_poll(&json!({ "idle": idle, "force": true }))?;
        // What the file holds now is taken: `ack` makes its revision the accepted one, so nothing is left waiting (and the flush save above does not
        // look like another program's change).
        let project = self.project_sync(&json!({ "ack": true }), true);
        Ok(json!({ "flushed": flushed, "project": project, "config": config, "sync": self.sync_json(true) }))
    }
}
