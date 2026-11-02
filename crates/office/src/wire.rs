#![allow(dead_code)]

//! O-Studio suite wire constants (env vars, local paths, JSON format ids).

pub const LOCAL_APP_FOLDER: &str = "O-Studio";

pub const ENV_COMPANY: &str = "OSTUDIO_COMPANY";
pub const ENV_TICKET: &str = "OSTUDIO_TICKET";
pub const ENV_USER: &str = "OSTUDIO_USER";
pub const ENV_OFFICE_DLL: &str = "OSTUDIO_OFFICE_DLL";
pub const ENV_PROJECT_ID: &str = "OSTUDIO_PROJECT_ID";
pub const ENV_PROJECT_OSP: &str = "OSTUDIO_PROJECT_OSP";

pub const PROJECT_FORMAT: &str = "osp";
pub const ACTIVE_PROJECT_FORMAT: &str = "studio-active-project";
pub const SELECTED_PROJECT_FORMAT: &str = "studio-selected-project";
pub const SELECTED_PROJECT_FILE: &str = "selected-project.json";

pub fn local_path(name: &str) -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|p| {
                let mut b = std::path::PathBuf::from(p);
                b.push("AppData");
                b.push("Local");
                b.into()
            })
        })
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join(LOCAL_APP_FOLDER).join(name)
}

pub fn is_project_format(format: Option<&str>) -> bool {
    matches!(format, Some(PROJECT_FORMAT) | Some("osas-project"))
}

pub fn is_active_project_format(format: Option<&str>) -> bool {
    matches!(format, Some(ACTIVE_PROJECT_FORMAT) | Some("osas-active-project"))
}
