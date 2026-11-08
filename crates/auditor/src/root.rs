//! Locate the Tren repository root (menu catalog + WinUI sources).

use std::path::{Path, PathBuf};

/// Walk from `start` upward looking for `xtask/data/menu-catalog.txt` and `apps/aad-winui`.
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    for _ in 0..64 {
        if cur.join("xtask/data/menu-catalog.txt").is_file() && cur.join("apps/aad-winui").is_dir() {
            return Some(cur);
        }
        if !cur.pop() {
            break;
        }
    }
    None
}

/// Resolve the repo root: `AAD_REPO_ROOT`, then cwd, then this crate's manifest parent.
pub fn repo_root_or_err() -> Result<PathBuf, String> {
    if let Ok(env) = std::env::var("AAD_REPO_ROOT") {
        let p = PathBuf::from(env.trim());
        if p.join("xtask/data/menu-catalog.txt").is_file() {
            return Ok(p);
        }
        return Err(format!("AAD_REPO_ROOT={} is not an Tren repo root (missing xtask/data/menu-catalog.txt)", p.display()));
    }
    if let Ok(cwd) = std::env::current_dir()
        && let Some(r) = find_repo_root(&cwd)
    {
        return Ok(r);
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(parent) = manifest.parent().and_then(|p| p.parent())
        && let Some(r) = find_repo_root(parent)
    {
        return Ok(r);
    }
    Err("Tren repository root not found (set AAD_REPO_ROOT, or run from a checkout that contains xtask/data/menu-catalog.txt and apps/aad-winui)"
        .into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_this_workspace() {
        let root = repo_root_or_err().expect("repo root");
        assert!(root.join("xtask/data/menu-catalog.txt").is_file());
        assert!(root.join("apps/aad-winui").is_dir());
    }
}
