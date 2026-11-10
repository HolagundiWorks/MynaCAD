//! The `.osp` project file: one SQLite file holding a drawing, its take-off, the MynaOps project, assets, markups and the drawing history.
//!
//! The file itself needs SQLite and is not built for the web; [`history`] (the version-history model) is, and only touches a file where SQLite exists.

pub mod history;
#[cfg(not(target_arch = "wasm32"))]
mod project;
#[cfg(not(target_arch = "wasm32"))]
pub use project::*;
