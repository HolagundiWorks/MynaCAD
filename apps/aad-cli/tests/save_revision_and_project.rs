//! BUG-0011 and BUG-0013: `aad-cli` records the drawing revision when it saves, and opens a drawing held in an `.osp` project file.

use std::path::PathBuf;
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aad-cli")).args(args).output().expect("aad-cli runs")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aad-cli-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn saving_from_the_command_line_writes_the_revision_sidecar() {
    let out = temp("rev.dxf");
    let o = cli(&["run", "--sample", "--save", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let sidecar = PathBuf::from(format!("{}.aadrev.json", out.display()));
    assert!(sidecar.is_file(), "no sidecar beside {}", out.display());
}

#[test]
fn a_drawing_inside_a_project_file_opens() {
    let dxf = temp("inside.dxf");
    let o = cli(&["run", "--sample", "--save", dxf.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let osp = temp("round.osp");
    let _ = std::fs::remove_file(&osp);
    let mut odp = aad_odp::Odp::create(&osp, "round").unwrap();
    let mut edit = odp.edit().unwrap();
    edit.put_drawing("round.dxf", "dxf", &std::fs::read(&dxf).unwrap()).unwrap();
    edit.commit().unwrap();
    let o = cli(&["info", osp.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("entityCount"));
}
