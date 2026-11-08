//! A project file written by the .NET side (`Office.Shared.OdpFile`, Falcon) is read by this crate: the two implementations share one format.

use aad_odp::Odp;

#[test]
fn the_dotnet_file_reads_here() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("from_dotnet.osp");
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/from_dotnet.osp"), &path).unwrap();
    let mut odp = Odp::open(&path).expect("the .NET file opens");
    let meta = odp.meta().unwrap();
    assert_eq!((meta.name.as_str(), meta.revision), ("FromDotnet", 2));
    let d = odp.drawing("plan.dxf").unwrap().unwrap();
    assert_eq!(d.ext, "dxf");
    assert!(d.bytes.starts_with(b"0\nSECTION"));
    assert_eq!(d.sha256, aad_odp::sha256_hex(&d.bytes));
    assert!(odp.project_doc().unwrap().unwrap().contains("Gr\u{fc}\u{df}e \u{2014} house"));
    assert_eq!(odp.changes_since(0).unwrap().len(), 2);
    // and this side can write to it
    let mut e = odp.edit().unwrap();
    e.put_project_doc("{\"project\":{}}").unwrap();
    assert_eq!(e.commit().unwrap(), 3);
}
