//! BUG-0014: a DXF that stops before any named section holds no drawing and is refused;
//! one cut off inside a section still opens with what was read.

fn read(text: &str) -> Result<aad_doc::Drawing, aad_io::IoError> {
    aad_io::read(text.as_bytes(), "x.dxf")
}

#[test]
fn a_file_with_only_a_section_start_is_refused() {
    assert!(read("0\nSECTION\n").is_err());
    assert!(read("0\nSECTION\n0\nSECTION\n").is_err());
}

#[test]
fn a_file_cut_off_inside_entities_keeps_what_was_read() {
    let cut = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5\n21\n0\n";
    let d = read(cut).unwrap();
    assert_eq!(d.model.len(), 1);
}
