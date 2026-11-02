//! O-Draft's read-only link to the O-Studio company file.
//!
//! The company file holds practice-wide details; O-Studio chooses it and signs sibling apps in. This crate loads
//! `office.dll` at run time, redeems the one-time ticket O-Studio passes, and asks for the `document_context` answer:
//! the company profile, the date, and a project's details. It never writes to the company file.
//! Product path signs in with a one-time ticket (`OSTUDIO_TICKET`); password login is for tests and tools only.
//!
//! Where the library and the file come from (see [`Settings::from_environment`]):
//! `OSTUDIO_COMPANY`, `OSTUDIO_TICKET`, and optional `OSTUDIO_OFFICE_DLL`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod link;
mod wire;

pub use link::Link;
pub use wire::{ACTIVE_PROJECT_FORMAT, ENV_COMPANY, ENV_OFFICE_DLL, ENV_TICKET, LOCAL_APP_FOLDER, PROJECT_FORMAT, local_path};

use std::path::PathBuf;

/// Why the company file could not be read. The text is meant to be shown to the person.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Refused(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// What is needed to open the company file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub company: Option<PathBuf>,
    pub ticket: Option<String>,
    pub library: Option<PathBuf>,
}

impl Settings {
    /// From the environment O-Studio starts an app with.
    pub fn from_environment() -> Self {
        let get = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        Settings { company: get(ENV_COMPANY).map(PathBuf::from), ticket: get(ENV_TICKET), library: get(ENV_OFFICE_DLL).map(PathBuf::from) }
    }

    /// The library to load: the one named, else `office.dll` beside the program, else the one O-Studio is installed with.
    pub fn find_library(&self) -> Option<PathBuf> {
        if let Some(path) = &self.library {
            return path.is_file().then(|| path.clone());
        }
        let mut places = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            places.extend(exe.parent().map(|p| p.join("office.dll")));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            places.push(PathBuf::from(local).join("Programs").join("O-Studio").join("office.dll"));
        }
        places.into_iter().find(|p| p.is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_library_that_is_missing_is_not_replaced_by_a_guess() {
        let settings = Settings { library: Some(PathBuf::from("Z:/nowhere/office.dll")), ..Settings::default() };
        assert_eq!(settings.find_library(), None);
    }

    #[test]
    fn nothing_set_means_no_company_file() {
        let settings = Settings::default();
        assert!(settings.company.is_none() && settings.ticket.is_none());
    }
}
