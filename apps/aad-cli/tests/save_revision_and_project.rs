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

#[test]
fn saving_over_a_file_keeps_the_first_version_as_bak() {
    let out = temp("keep.dxf");
    let bak = PathBuf::from(format!("{}.bak", out.display()));
    let _ = std::fs::remove_file(&bak);
    let _ = std::fs::remove_file(&out);
    for _ in 0..2 {
        let o = cli(&["run", "--sample", "--save", out.to_str().unwrap()]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    assert!(bak.is_file(), "no .bak beside {}", out.display());
}

/// `--save` to a project file puts the drawing in it as a revision, with the structural details O-CoreOp draws; the file opens again.
#[test]
fn saving_into_a_project_file_records_the_drawing_and_its_structural_details() {
    let osp = temp("details.osp");
    let _ = std::fs::remove_file(&osp);
    let o = cli(&[
        "run",
        "--metric",
        "--cmd",
        r#"smartblock.insert {"id":"rcc.column.rect","at":[0,0]}"#,
        "--cmd",
        r#"smartblock.insert {"id":"rcc.beam.section","at":[2000,0]}"#,
        "--save",
        osp.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let odp = aad_odp::Odp::open(&osp).unwrap();
    assert_eq!(odp.drawing_names().unwrap().len(), 1);
    let kinds: Vec<String> = odp.details().unwrap().into_iter().map(|d| d.kind).collect();
    assert_eq!(kinds.len(), 2, "{kinds:?}");
    assert!(kinds.contains(&"columns".to_string()) && kinds.contains(&"beams".to_string()));
    let o = cli(&["info", osp.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("entityCount"));
}
