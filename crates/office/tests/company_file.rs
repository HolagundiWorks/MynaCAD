//! Runs against the real `office.dll`. Set `OFFICE_DLL_DIR` to the folder that holds it (built from `office-ffi` in the O-STUDIO
//! repository with `cargo build --release`). Without it the tests do nothing, so they never fail on a machine that has no library.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use aad_office::{Link, Settings};
use serde_json::json;
use std::path::PathBuf;

const PASSWORD: &str = "correct horse battery";

fn library() -> Option<PathBuf> {
    let dir = std::env::var_os("OFFICE_DLL_DIR")?;
    let path = PathBuf::from(dir).join("office.dll");
    path.is_file().then_some(path)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aad-office-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A company with one project, made through the library the way OSAS and Kivo make them. Returns the file.
fn company(dir: &std::path::Path, library: &std::path::Path) -> PathBuf {
    let file = dir.join("Studio.osas");
    let maker = Link::bare(library, &dir.join("maker-app")).unwrap();
    maker
        .call(
            "company.create",
            &json!({ "path": dir.join("Studio").to_string_lossy(), "name": "Studio One", "username": "owner", "password": PASSWORD, "confirm": PASSWORD }),
        )
        .unwrap();
    maker.call("company.close", &json!({})).unwrap();
    drop(maker);
    let link = Link::open_with_password(library, &dir.join("setup-app"), &file, "owner", PASSWORD).unwrap();
    let client = link
        .call("client_save", &json!({ "client": { "id": null, "name": "Mehta family", "contact_person": "", "email": "", "phone": "", "address": "", "pan": "", "gstin": "" } }))
        .unwrap();
    link.call(
        "project_save",
        &json!({ "project": { "id": null, "client_id": client, "code": "", "name": "Mehta residence", "site_address": "Plot 4, Hospet", "status": "active" } }),
    )
    .unwrap();
    file
}

#[test]
fn the_library_is_found_when_its_folder_is_given() {
    if std::env::var_os("OFFICE_DLL_DIR").is_some() {
        assert!(library().is_some(), "office.dll was not found in OFFICE_DLL_DIR");
    }
}

#[test]
fn the_company_and_project_details_are_read_without_writing_anything() {
    let Some(library) = library() else { return };
    let dir = scratch("read");
    let file = company(&dir, &library);
    let link = Link::open_with_password(&library, &dir.join("app"), &file, "owner", PASSWORD).unwrap();
    let projects = link.projects().unwrap();
    let id = projects[0]["id"].as_i64().unwrap();

    let context = link.document_context(Some(id)).unwrap();
    assert_eq!(context["company"]["name"], "Studio One");
    assert_eq!(context["project"]["project"]["name"], "Mehta residence");
    assert_eq!(context["project"]["project"]["site_address"], "Plot 4, Hospet");
    assert_eq!(context["project"]["client"]["name"], "Mehta family");
    assert_eq!(link.resolve(&context["project"]["project_ref"]).unwrap(), id);

    let company_only = link.document_context(None).unwrap();
    assert!(company_only["project"].is_null());
    assert!(link.document_context(Some(id + 1000)).is_err());
    drop(link);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_ticket_or_a_missing_file_is_refused_with_a_message() {
    let Some(library) = library() else { return };
    let dir = scratch("refuse");
    let file = company(&dir, &library);
    let wrong = Settings { company: Some(file), ticket: Some("not-a-ticket".into()), library: Some(library.clone()) };
    let error = Link::open(&wrong, &dir.join("app")).err().expect("a wrong ticket must be refused");
    assert!(!error.to_string().is_empty());
    let missing = Settings { company: Some(dir.join("nope.osas")), ticket: Some("x".into()), library: Some(library) };
    assert!(Link::open(&missing, &dir.join("app2")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
