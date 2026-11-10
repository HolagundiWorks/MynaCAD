//! Stamps the binary with the git commit it was built from (and the Myna release number when `MYNA_VERSION` is set), so a number measured on an installed
//! `aad-cli.exe` can be tied to a build: `aad-cli --version`.
use std::process::Command;

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short=10", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let dirty = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .is_some_and(|o| o.status.success() && !o.stdout.is_empty());
    let stamp = if commit.is_empty() {
        String::new()
    } else if dirty {
        format!("{commit}+dirty")
    } else {
        commit
    };
    println!("cargo:rustc-env=AAD_GIT_COMMIT={stamp}");
    println!("cargo:rustc-env=MYNA_VERSION={}", std::env::var("MYNA_VERSION").unwrap_or_default());
    println!("cargo:rerun-if-env-changed=MYNA_VERSION");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
}
