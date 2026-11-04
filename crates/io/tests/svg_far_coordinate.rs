//! BUG-0016: a corrupted coordinate far outside any drawing must not stretch the SVG view and print hundreds of digits for every point.

#[test]
fn a_far_coordinate_does_not_flood_the_svg() {
    let mut text = String::from(
        "0
SECTION
2
ENTITIES
0
LWPOLYLINE
90
3
70
1
10
-1e308
20
200.0
10
10
20
0
10
10
20
10
",
    );
    for i in 0..2000 {
        text.push_str(&format!(
            "0
LINE
10
{i}
20
0
11
{i}
21
5
"
        ));
    }
    text.push_str(
        "0
ENDSEC
0
EOF
",
    );
    let d = aad_io::read(text.as_bytes(), "x.dxf").unwrap();
    let svg = aad_io::write(&d, "x.svg").unwrap();
    assert!(svg.len() < 1_000_000, "{} bytes", svg.len());
}
