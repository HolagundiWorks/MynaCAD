//! Export for exchange: plain DXF files that look right in any other CAD program.
//!
//! A drawing with levels and sheet sets stores them as blocks (see `docs/levels-and-sheet-sets.md`),
//! so another program shows only the first level as model space and a viewport bound to another
//! level still shows model space. `exchange.export` writes one ordinary single-level DXF per level,
//! and one per (sheet set, level) that has annotation: the level's geometry with that annotation
//! on top. The files carry no level or sheet-set data and no layout content (viewports are bound to
//! levels, which the plain files no longer have); the drawing itself is not changed.

use aad_doc::{Drawing, EntityStore};
use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use serde_json::json;

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("exchange.export", "Export for Exchange", run_export)
            .menu(&["File", "Export for Exchange"])
            .alias(&["exportlevels"])
            .params("{folder, sheetSets? (default true: also one file per sheet set and level with annotation), overwrite? (default false)} → {folder, files: [{file, level, sheetSet, objects}]}. Plain single-level DXF files without layout content.")
            .noundo(),
    ]
}

/// A file-name part: letters, digits, `-` and `_`, other characters become `_`, at most 40.
fn safe(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(40).collect();
    if s.is_empty() { "_".into() } else { s }
}

/// The drawing as one plain level: `store` is the model space, no levels, sheet sets or layouts.
pub(super) fn plain(d: &Drawing, store: EntityStore) -> Drawing {
    let mut w = d.clone();
    w.model = store;
    w.levels.clear();
    w.active_level = None;
    w.view_spaces.clear();
    w.active_view = None;
    w.sheet_sets.clear();
    w.active_sheet = None;
    w.layouts.clear();
    w
}

fn run_export(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err(bad("exchange.export", "files are not available on the web"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let folder = str_param(p, "folder")
            .filter(|f| !f.trim().is_empty() && f.len() < 1024 && !f.contains('\0'))
            .ok_or_else(|| bad("exchange.export", "`folder` is required"))?;
        let with_sets = bool_or(p, "sheetSets", true);
        let overwrite = bool_or(p, "overwrite", false);
        let hooks = super::file::io().ok_or_else(|| bad("exchange.export", "file formats are not available in this build"))?;
        let d = s.doc()?;
        if !d.has_levels() {
            return Err(bad("exchange.export", "the drawing has one level, so a normal save is already a plain file"));
        }
        // Work out every file first so nothing is written for a drawing that cannot be exported.
        let mut jobs: Vec<(String, Drawing, Value)> = Vec::new();
        for (i, l) in d.levels.iter().enumerate() {
            let Some(geometry) = d.level_store(l.id) else { continue };
            let base = format!("{:02}_{}", i + 1, safe(&l.name));
            jobs.push((format!("{base}.dxf"), plain(d, geometry.clone()), json!({ "level": l.name, "sheetSet": null, "objects": geometry.len() })));
            if with_sets {
                for set in &d.sheet_sets {
                    let Some(ann) = d.annotation_store(set.id, l.id).filter(|a| !a.is_empty()) else { continue };
                    let mut merged = geometry.clone();
                    for e in ann.iter() {
                        merged.push((**e).clone());
                    }
                    let n = merged.len();
                    jobs.push((
                        format!("{base}__{}.dxf", safe(&set.name)),
                        plain(d, merged),
                        json!({ "level": l.name, "sheetSet": set.name, "objects": n }),
                    ));
                }
            }
        }
        let dir = std::path::Path::new(folder);
        if dir.exists() && !dir.is_dir() {
            return Err(bad("exchange.export", format!("`{folder}` is a file, not a folder")));
        }
        if !overwrite && let Some((name, _, _)) = jobs.iter().find(|(n, _, _)| dir.join(n).exists()) {
            return Err(bad("exchange.export", format!("`{name}` already exists in `{folder}`; pass `overwrite` to replace it")));
        }
        // Build every file's bytes before touching the disk.
        let mut files: Vec<(std::path::PathBuf, Vec<u8>, Value)> = Vec::new();
        for (name, w, info) in jobs {
            let bytes = (hooks.write)(&w, &name).map_err(|e| bad("exchange.export", format!("{name}: {e}")))?;
            let mut info = info;
            info["file"] = json!(name);
            files.push((dir.join(&name), bytes, info));
        }
        std::fs::create_dir_all(dir).map_err(|e| bad("exchange.export", format!("{folder}: {e}")))?;
        let mut out = Vec::new();
        for (path, bytes, info) in files {
            std::fs::write(&path, bytes).map_err(|e| bad("exchange.export", format!("{}: {e}", path.display())))?;
            out.push(info);
        }
        Ok(json!({ "folder": folder, "files": out }))
    }
}
