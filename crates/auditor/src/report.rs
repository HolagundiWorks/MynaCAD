//! Combined Auditor v0 JSON report.

use std::path::Path;

use aad_engine::Session;
use serde_json::{Map, Value, json};

use crate::drawing::{audit_drawing, validate_drawing};
use crate::parity::check_command_parity;
use crate::ui_tokens::check_ui_tokens;

/// Which detect checks to include in a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetectChecks {
    pub audit: bool,
    pub validate: bool,
    pub parity: bool,
    pub ui_tokens: bool,
}

impl Default for DetectChecks {
    fn default() -> Self {
        Self { audit: true, validate: true, parity: true, ui_tokens: true }
    }
}

impl DetectChecks {
    /// Parse a comma-separated list (`audit,validate,parity,ui`). Empty → all.
    pub fn parse(list: &str) -> Result<Self, String> {
        let t = list.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("all") {
            return Ok(Self::default());
        }
        let mut c = Self { audit: false, validate: false, parity: false, ui_tokens: false };
        for part in t.split(',') {
            match part.trim().to_ascii_lowercase().as_str() {
                "audit" => c.audit = true,
                "validate" => c.validate = true,
                "parity" | "menu" | "command_parity" => c.parity = true,
                "ui" | "ui_tokens" | "tokens" => c.ui_tokens = true,
                "" => {}
                other => return Err(format!("unknown auditor check `{other}` (use audit,validate,parity,ui)")),
            }
        }
        if !(c.audit || c.validate || c.parity || c.ui_tokens) {
            return Err("auditor needs at least one check".into());
        }
        Ok(c)
    }
}

/// Structured detect report.
#[derive(Clone, Debug)]
pub struct Report {
    pub value: Value,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.value.get("ok").and_then(Value::as_bool).unwrap_or(false)
    }
}

/// Screenshot capture stays on the live Windows control / MCP path; metadata only here.
fn screenshot_hook_docs() -> Value {
    json!({
        "availableInThisReport": false,
        "platform": "Windows (live shell)",
        "control": {
            "method": "ui.screenshot",
            "params": { "path": "shot.png" },
            "note": "WinUI writes a drawing PNG (not window chrome). egui captures the window."
        },
        "mcp": {
            "tool": "screenshot",
            "requires": "aad-cli mcp --connect 127.0.0.1:PORT with the app listening on --control PORT"
        },
        "inspect": {
            "control": "ui.inspect",
            "mcp": "ui_inspect",
            "note": "Layout and perf metrics without writing a PNG."
        },
        "outOfScope": "Screenshot golden CI on Linux; Phase B tiling; TLS."
    })
}

fn check_ok(v: &Value) -> bool {
    v.get("ok").and_then(Value::as_bool).unwrap_or(false)
}

/// Run the selected detect checks and return one JSON object.
///
/// `repo_root` is required when parity or ui_tokens is selected. Drawing checks use `session`.
pub fn run_detect(session: &mut Session, repo_root: Option<&Path>, checks: DetectChecks) -> Result<Report, String> {
    let mut checks_map = Map::new();
    let mut failures: Vec<String> = Vec::new();

    if checks.audit {
        let v = audit_drawing(session)?;
        if v.get("errors").and_then(Value::as_u64).unwrap_or(0) > 0 {
            failures.push("audit_drawing".into());
        }
        checks_map.insert("audit_drawing".into(), v);
    }
    if checks.validate {
        let v = validate_drawing(session)?;
        if !check_ok(&v) {
            failures.push("validate_drawing".into());
        }
        checks_map.insert("validate_drawing".into(), v);
    }
    if checks.parity {
        let root = repo_root.ok_or("check_command_parity needs the MynaCAD repository root")?;
        let v = check_command_parity(root)?;
        // Parity gaps are informational for Auditor v0: menu breadth is incomplete by design.
        // Mark ok from the check but do not fail the whole report solely on missing menu items.
        checks_map.insert("check_command_parity".into(), v);
    }
    if checks.ui_tokens {
        let root = repo_root.ok_or("check_ui_tokens needs the MynaCAD repository root")?;
        let v = check_ui_tokens(root)?;
        if !check_ok(&v) {
            failures.push("check_ui_tokens".into());
        }
        checks_map.insert("check_ui_tokens".into(), v);
    }

    let ok = failures.is_empty();
    Ok(Report {
        value: json!({
            "tool": "auditor_report",
            "version": "v0",
            "detectOnly": true,
            "ok": ok,
            "failedChecks": failures,
            "checks": checks_map,
            "screenshotCapture": screenshot_hook_docs(),
            "message": if ok {
                "Auditor v0: no blocking detect findings.".to_string()
            } else {
                format!("Auditor v0: findings in {}.", failures.join(", "))
            },
            "note": "Detect-only. Mutating repairs (audit/validate with fix=true, purge, …) still need confirmation through the agent or an explicit engine.execute."
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo_root_or_err;

    #[test]
    fn parse_checks() {
        assert_eq!(DetectChecks::parse("").unwrap(), DetectChecks::default());
        let c = DetectChecks::parse("audit,ui").unwrap();
        assert!(c.audit && c.ui_tokens && !c.validate && !c.parity);
        assert!(DetectChecks::parse("nope").is_err());
    }

    #[test]
    fn full_report_on_sample() {
        let mut s = Session::new();
        s.open_drawing(aad_engine::sample::default_sample(), "Bracket", None);
        let root = repo_root_or_err().expect("root");
        let r = run_detect(&mut s, Some(&root), DetectChecks::default()).expect("report");
        assert_eq!(r.value["tool"], "auditor_report");
        assert_eq!(r.value["detectOnly"], true);
        assert_eq!(r.value["version"], "v0");
        assert!(r.value["checks"]["audit_drawing"].is_object());
        assert!(r.value["checks"]["validate_drawing"].is_object());
        assert!(r.value["checks"]["check_command_parity"].is_object());
        assert!(r.value["checks"]["check_ui_tokens"].is_object());
        assert_eq!(r.value["screenshotCapture"]["availableInThisReport"], false);
        assert!(r.value["screenshotCapture"]["control"]["method"].as_str() == Some("ui.screenshot"));
    }

    #[test]
    fn drawing_only_without_repo() {
        let mut s = Session::new();
        let r = run_detect(&mut s, None, DetectChecks { audit: true, validate: true, parity: false, ui_tokens: false }).expect("report");
        assert!(r.ok());
        assert!(r.value["checks"].get("check_command_parity").is_none());
    }
}
