//! The company file's details in the drawing. The company's own details (name, address, registrations) are kept in Studio
//! (OSAS company file) and a project's details (name, site, client) in Studio / Kivo; a drawing's title block and schedules
//! print them instead of keeping their own copies. A host that has opened the company file reads the engine call
//! `document_context` and hands the answer to this command: the project details of the drawing (title, owner, architect,
//! address, date, project id) are set from it, and the company's details are kept in the drawing so a title block can print
//! them without the company file being open.
//!
//! When the drawing holds company-file identity, those fields are refresh-only: `project` / `project.wizard` refuse to
//! overwrite them. Change company (and linked project) identity in Studio, then `companyfile {refresh: true}`.
//!
//! `companyfile` does no file input of its own. It only reads the JSON it is given, so the engine stays free of the company
//! file's library. `company.pack.import` / `company.pack.export` read and write an `osas-company-pack` JSON file (firm fields
//! only). That pack is how Tren receives company details for title blocks. Tren does not open the company file and does
//! not call Kivo or the launcher.

use serde_json::{Map, Value, json};

use super::project::{get, set};
use super::*;
use crate::{Result, Session};

/// The version of `document_context` this command understands.
const CONTEXT_VERSION: u64 = 1;
/// Longest text kept for any one detail.
const MAX_TEXT: usize = 500;

/// Detail keys filled from firm identity alone (no project block).
const FIRM_LOCKED_DETAILS: &[&str] = &["architect", "date"];
/// Detail keys filled when a project is linked via `project_ref`.
const PROJECT_LOCKED_DETAILS: &[&str] = &["title", "owner", "architect", "address", "pid", "summary", "date"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("companyfile", "Company File Details", run_companyfile)
            .menu(&["Architecture", "Project", "Company File Details"])
            .params("{context?: a `document_context` answer, open?: true (open the company file Kivo gave this start), projects?: true, project_id?: n (read that project), refresh?: true (read the drawing's project again)} → {connected, linked, company, details, projects?, suite?}"),
        CommandSpec::new("company.pack.import", "Import Company Details", run_pack_import)
            .menu(&["Architecture", "Project", "Import Company Details"])
            .params("{path: an osas-company-pack JSON exported from Kivo} → {company, details}. Fills title-block fields from the file. Does not open the company file or talk to Kivo."),
        CommandSpec::new("company.pack.export", "Export Company Details", run_pack_export)
            .params("{path} → {path, bytes}. Writes the company details already stored in this drawing."),
    ]
}

/// The company details kept in the drawing (name, address, contacts, registrations), or `Null`.
pub(crate) fn company(d: &aad_doc::Drawing) -> Value {
    get(d, "company")
}

/// True when the drawing holds company identity applied from the company file.
pub(crate) fn holds_company(d: &aad_doc::Drawing) -> bool {
    company(d).is_object()
}

/// True when the drawing is linked to a project in the company file (`project_ref`).
pub(crate) fn project_linked(d: &aad_doc::Drawing) -> bool {
    company(d).get("project_ref").is_some_and(|r| r.is_object())
}

/// Project-detail keys that must not be free-typed while the drawing holds company-file identity.
/// Firm-only apply locks architect/date; a project link also locks title, client, site, code and summary.
pub(crate) fn locked_detail_keys(d: &aad_doc::Drawing) -> &'static [&'static str] {
    if !holds_company(d) {
        return &[];
    }
    if project_linked(d) { PROJECT_LOCKED_DETAILS } else { FIRM_LOCKED_DETAILS }
}

/// Suite linkage for hosts and `project` / `companyfile` answers.
pub(crate) fn suite_status(d: &aad_doc::Drawing) -> Value {
    let from_file = holds_company(d);
    let linked = project_linked(d);
    json!({
        "company_from_file": from_file,
        "project_linked": linked,
        "locked_details": locked_detail_keys(d),
        "refresh_only": from_file,
    })
}

fn text(v: Option<&Value>, what: &str) -> Result<String> {
    match v {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.chars().count() > MAX_TEXT { Err(bad("companyfile", format!("`{what}` is too long"))) } else { Ok(t.to_string()) }
        }
        Some(_) => Err(bad("companyfile", format!("`{what}` must be text"))),
    }
}

fn office_error(e: aad_office::Error) -> crate::EngineError {
    bad("companyfile", e.to_string())
}

/// Where the link keeps its small working files (the recent-files list and temporary copies); never the company's data.
fn link_folder() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, std::path::PathBuf::from).join("O-Draft").join("office")
}

fn connect(s: &mut Session) -> Result<()> {
    if s.office.is_none() {
        let link = aad_office::Link::open(&aad_office::Settings::from_environment(), &link_folder()).map_err(office_error)?;
        s.office = Some(link);
    }
    Ok(())
}

fn run_companyfile(s: &mut Session, p: &Value) -> Result<Value> {
    let wants = |key: &str| p.get(key).and_then(Value::as_bool).unwrap_or(false);
    if let Some(ctx) = p.get("context") {
        apply(s, ctx)?;
    }
    let mut projects = Value::Null;
    if wants("open") || wants("projects") || wants("refresh") || p.get("project_id").is_some() {
        connect(s)?;
    }
    if let Some(link) = s.office.as_ref() {
        if wants("projects") {
            projects = link.projects().map_err(office_error)?;
        }
        let chosen = match p.get("project_id") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_i64().ok_or_else(|| bad("companyfile", "`project_id` must be a whole number"))?),
        };
        let context = if let Some(id) = chosen {
            Some(link.document_context(Some(id)).map_err(office_error)?)
        } else if wants("refresh") {
            // The drawing remembers which project it belongs to; the company file says what that project is now.
            let reference = company(s.doc()?).get("project_ref").filter(|r| r.is_object()).cloned();
            let id = match reference {
                Some(r) => Some(link.resolve(&r).map_err(office_error)?),
                None => None,
            };
            Some(link.document_context(id).map_err(office_error)?)
        } else {
            None
        };
        if let Some(context) = context {
            apply(s, &context)?;
        }
    }
    let d = s.doc()?;
    let held = company(d);
    let suite = suite_status(d);
    Ok(json!({
        "connected": s.office.is_some(),
        "linked": suite["project_linked"].as_bool().unwrap_or(false),
        "company": held,
        "details": get(d, "details"),
        "projects": projects,
        "suite": suite,
    }))
}

const PACK_FORMAT: &str = "osas-company-pack";
const MAX_PACK: u64 = 1_000_000;

fn run_pack_import(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("company.pack.import", "`path` is required"))?;
    let bytes = std::fs::read(path).map_err(|e| bad("company.pack.import", format!("{path}: {e}")))?;
    if bytes.len() as u64 > MAX_PACK {
        return Err(bad("company.pack.import", "the company pack is too large"));
    }
    let pack: Value = serde_json::from_slice(&bytes).map_err(|e| bad("company.pack.import", e.to_string()))?;
    if pack.get("format").and_then(Value::as_str) != Some(PACK_FORMAT) {
        return Err(bad("company.pack.import", "this file is not a Kivo company pack"));
    }
    let firm = pack.get("company").filter(|c| c.is_object()).ok_or_else(|| bad("company.pack.import", "`company` is missing"))?;
    // Firm identity only: no project link, so Tren does not call Kivo.
    let ctx = json!({ "v": 1, "prepared_on": pack.get("exported_utc").and_then(Value::as_str).unwrap_or(""), "company": firm });
    apply(s, &ctx)?;
    let d = s.doc()?;
    Ok(json!({ "company": company(d), "details": get(d, "details") }))
}

fn run_pack_export(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("company.pack.export", "`path` is required"))?;
    let held = company(s.doc()?);
    if !held.is_object() {
        return Err(bad("company.pack.export", "this drawing has no company details to export"));
    }
    let pack = json!({
        "format": PACK_FORMAT,
        "version": 1,
        "exported_utc": "",
        "company": held,
    });
    let text = serde_json::to_string_pretty(&pack).map_err(|e| bad("company.pack.export", e.to_string()))?;
    if let Some(parent) = std::path::Path::new(path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| bad("company.pack.export", e.to_string()))?;
    }
    std::fs::write(path, &text).map_err(|e| bad("company.pack.export", e.to_string()))?;
    Ok(json!({ "path": path, "bytes": text.len() }))
}

fn apply(s: &mut Session, ctx: &Value) -> Result<()> {
    let version = ctx.get("v").and_then(Value::as_u64).unwrap_or(CONTEXT_VERSION);
    if version != CONTEXT_VERSION {
        return Err(bad("companyfile", format!("this build reads version {CONTEXT_VERSION} of the company file's answer, not {version}")));
    }
    let co = ctx.get("company").filter(|c| c.is_object()).ok_or_else(|| bad("companyfile", "`context.company` is missing"))?;
    let mut kept = Map::new();
    for key in ["name", "address", "phone", "email", "pan", "gstin", "coa_registration", "architect_name"] {
        kept.insert(key.to_string(), json!(text(co.get(key), key)?));
    }
    kept.insert("prepared_on".to_string(), json!(text(ctx.get("prepared_on"), "prepared_on")?));

    let mut details = match get(s.doc()?, "details") {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    // Who prepared the drawing: the named architect, else the firm.
    let architect = {
        let named = text(co.get("architect_name"), "architect_name")?;
        if named.is_empty() { text(co.get("name"), "name")? } else { named }
    };
    if !architect.is_empty() {
        details.insert("architect".to_string(), json!(architect));
    }
    let date = text(ctx.get("prepared_on"), "prepared_on")?;
    if !date.is_empty() {
        details.insert("date".to_string(), json!(date));
    }

    match ctx.get("project").filter(|p| p.is_object()) {
        Some(block) => {
            let project = block.get("project").filter(|p| p.is_object()).ok_or_else(|| bad("companyfile", "`context.project.project` is missing"))?;
            for (detail, value) in [
                ("title", text(project.get("name"), "name")?),
                ("pid", text(project.get("code"), "code")?),
                ("address", text(project.get("site_address"), "site_address")?),
                ("summary", text(project.get("summary"), "summary")?),
                ("owner", text(block.pointer("/client/name").or_else(|| project.get("client_name")), "client")?),
            ] {
                if !value.is_empty() {
                    details.insert(detail.to_string(), json!(value));
                }
            }
            kept.insert("project_ref".to_string(), block.get("project_ref").cloned().unwrap_or(Value::Null));
            kept.insert("consultancy".to_string(), json!(text(block.get("consultancy_label"), "consultancy_label")?));
        }
        None => {
            kept.insert("project_ref".to_string(), Value::Null);
        }
    }
    let d = s.doc_mut()?;
    set(d, "company", &Value::Object(kept));
    set(d, "details", &Value::Object(details));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Value {
        json!({
            "v": 1,
            "prepared_on": "2026-10-19",
            "company": { "name": "Studio One", "address": "12 MG Road, Hospet", "phone": "+91 99999", "email": "a@b.in", "pan": "ABCDE1234F",
                         "gstin": "29ABCDE1234F1Z5", "coa_registration": "CA/2001/1234", "architect_name": "Aditi Rao" },
            "project": {
                "project": { "id": 7, "code": "P-007", "name": "Mehta residence", "site_address": "Plot 4, Hospet", "summary": "Two-storey residence with courtyard.", "client_name": "Mehta family" },
                "client": { "name": "Mehta family" },
                "consultancy_label": "Interior architecture",
                "project_ref": { "v": 1, "company_uuid": "u", "project_id": 7 }
            }
        })
    }

    #[test]
    fn the_projects_details_come_from_the_company_file() {
        let mut s = Session::new();
        let r = s.execute("companyfile", &json!({ "context": context() })).unwrap();
        assert_eq!(r["linked"], true);
        assert_eq!(r["suite"]["company_from_file"], true);
        assert_eq!(r["suite"]["project_linked"], true);
        assert_eq!(r["suite"]["refresh_only"], true);
        assert!(r["suite"]["locked_details"].as_array().unwrap().iter().any(|k| k == "title"));
        assert_eq!(r["company"]["name"], "Studio One");
        assert_eq!(r["details"]["title"], "Mehta residence");
        assert_eq!(r["details"]["owner"], "Mehta family");
        assert_eq!(r["details"]["architect"], "Aditi Rao");
        assert_eq!(r["details"]["address"], "Plot 4, Hospet");
        assert_eq!(r["details"]["summary"], "Two-storey residence with courtyard.");
        assert_eq!(r["details"]["pid"], "P-007");
        assert_eq!(r["details"]["date"], "2026-10-19");
    }

    #[test]
    fn linked_company_identity_cannot_be_overwritten_through_project() {
        let mut s = Session::new();
        s.execute("companyfile", &json!({ "context": context() })).unwrap();
        let err = s.execute("project", &json!({ "details": { "architect": "Someone Else" } })).unwrap_err().to_string();
        assert!(err.contains("Studio") || err.contains("company file"), "{err}");
        assert!(s.execute("project", &json!({ "details": { "title": "Renamed" } })).is_err());
        // Same value is allowed (forms may resubmit locked fields).
        s.execute("project", &json!({ "details": { "architect": "Aditi Rao" } })).unwrap();
        // Local-only fields stay editable.
        let r = s.execute("project", &json!({ "details": { "engineer": "K. Iyer", "plotUse": "Residential" } })).unwrap();
        assert_eq!(r["details"]["engineer"], "K. Iyer");
        assert_eq!(r["details"]["plotUse"], "Residential");
        assert_eq!(r["details"]["architect"], "Aditi Rao");
        assert_eq!(r["suite"]["locked_details"].as_array().unwrap().len(), 7);
    }

    #[test]
    fn firm_only_link_locks_architect_but_not_project_title() {
        let mut s = Session::new();
        let mut c = context();
        c.as_object_mut().unwrap().remove("project");
        s.execute("companyfile", &json!({ "context": c })).unwrap();
        assert!(s.execute("project", &json!({ "details": { "architect": "Other" } })).is_err());
        let r = s.execute("project", &json!({ "details": { "title": "Local title", "engineer": "E" } })).unwrap();
        assert_eq!(r["details"]["title"], "Local title");
        assert_eq!(r["suite"]["project_linked"], false);
        assert_eq!(r["suite"]["company_from_file"], true);
    }

    #[test]
    fn a_change_in_the_company_file_replaces_what_the_drawing_held() {
        let mut s = Session::new();
        s.execute("companyfile", &json!({ "context": context() })).unwrap();
        let mut next = context();
        next["project"]["project"]["site_address"] = json!("Plot 9, Hubli");
        next["company"]["name"] = json!("New Name Architects");
        let r = s.execute("companyfile", &json!({ "context": next })).unwrap();
        assert_eq!(r["details"]["address"], "Plot 9, Hubli");
        assert_eq!(r["company"]["name"], "New Name Architects");
    }

    #[test]
    fn details_the_company_file_does_not_hold_are_left_alone() {
        let mut s = Session::new();
        s.execute("project", &json!({ "details": { "plotUse": "Residential", "authority": "HUDA" } })).unwrap();
        let r = s.execute("companyfile", &json!({ "context": context() })).unwrap();
        assert_eq!(r["details"]["plotUse"], "Residential");
        assert_eq!(r["details"]["authority"], "HUDA");
    }

    #[test]
    fn a_company_without_a_project_keeps_only_the_company() {
        let mut s = Session::new();
        let mut c = context();
        c.as_object_mut().unwrap().remove("project");
        let r = s.execute("companyfile", &json!({ "context": c })).unwrap();
        assert_eq!(r["linked"], false);
        assert_eq!(r["suite"]["company_from_file"], true);
        assert_eq!(r["suite"]["project_linked"], false);
        assert_eq!(r["company"]["gstin"], "29ABCDE1234F1Z5");
        assert!(r["details"].get("title").is_none());
    }

    #[test]
    fn bad_input_is_refused_and_changes_nothing() {
        let mut s = Session::new();
        assert!(s.execute("companyfile", &json!({ "context": { "v": 2, "company": {} } })).is_err());
        assert!(s.execute("companyfile", &json!({ "context": { "v": 1 } })).is_err());
        assert!(s.execute("companyfile", &json!({ "context": { "company": { "name": 5 } } })).is_err());
        let mut long = context();
        long["company"]["address"] = json!("x".repeat(501));
        assert!(s.execute("companyfile", &json!({ "context": long })).is_err());
        let r = s.execute("companyfile", &json!({})).unwrap();
        assert_eq!(r["linked"], false);
        assert!(r["company"].is_null());
    }

    #[test]
    fn hostile_shapes_never_panic() {
        let mut s = Session::new();
        for bad_context in [
            json!(null),
            json!([]),
            json!("x"),
            json!({ "company": [] }),
            json!({ "company": {}, "project": { "project": 5 } }),
            json!({ "company": {}, "project": 7 }),
        ] {
            let _ = s.execute("companyfile", &json!({ "context": bad_context }));
        }
    }

    #[test]
    fn opening_without_an_osas_start_says_how_to_get_the_details() {
        let mut s = Session::new();
        // This test machine is not started from Kivo, so no company file or ticket is in the environment.
        if std::env::var_os("OSTUDIO_COMPANY").is_none() {
            let message = s.execute("companyfile", &json!({ "open": true })).err().map(|e| e.to_string()).unwrap_or_default();
            assert!(message.contains("Kivo"), "{message}");
            assert!(s.execute("companyfile", &json!({ "projects": true })).is_err());
            assert!(s.execute("companyfile", &json!({ "refresh": true })).is_err());
        }
        let shown = s.execute("companyfile", &json!({})).unwrap();
        assert_eq!(shown["connected"], false);
    }

    #[test]
    fn a_company_pack_fills_title_block_fields_without_a_project_link() {
        let dir = std::env::temp_dir().join(format!("aad-company-pack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("firm.json");
        let pack = json!({
            "format": "osas-company-pack",
            "version": 1,
            "exported_utc": "2026-11-06",
            "company": { "name": "Studio One", "address": "12 MG Road", "phone": "1", "email": "a@b.in",
                         "pan": "ABCDE1234F", "gstin": "29ABCDE1234F1Z5", "coa_registration": "CA/1", "architect_name": "Aditi Rao" }
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&pack).unwrap()).unwrap();
        let mut s = Session::new();
        let path_str = path.to_string_lossy();
        let imported = s.execute("company.pack.import", &json!({ "path": path_str })).unwrap();
        assert_eq!(imported["company"]["name"], "Studio One");
        assert_eq!(imported["details"]["architect"], "Aditi Rao");
        assert_eq!(imported["details"]["date"], "2026-11-06");
        assert!(imported["company"].get("project_ref").is_none_or(Value::is_null));
        let out = dir.join("out.json");
        s.execute("company.pack.export", &json!({ "path": out.to_string_lossy() })).unwrap();
        let written: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(written["format"], "osas-company-pack");
        assert_eq!(written["company"]["architect_name"], "Aditi Rao");
        assert!(s.execute("company.pack.import", &json!({ "path": dir.join("missing.json").to_string_lossy() })).is_err());
        std::fs::write(dir.join("bad.json"), b"{\"format\":\"other\",\"company\":{}}").unwrap();
        assert!(s.execute("company.pack.import", &json!({ "path": dir.join("bad.json").to_string_lossy() })).is_err());
        let huge = dir.join("huge.json");
        std::fs::write(&huge, vec![b'x'; 1_000_001]).unwrap();
        assert!(s.execute("company.pack.import", &json!({ "path": huge.to_string_lossy() })).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
