//! `cargo xtask logo`: draw the Tren bird and write every icon the project ships.
//!
//! The mark is Human Centric Works' own artwork: Tren's bird, white on a Midnight plate
//! (`#102C80`). The curves are the same ones `o-ui` `tools/Brand` draws. A pixel is white when it
//! falls inside the crown or the beak and outside the eye. Everything under `assets/app-icon/`
//! is generated:
//!
//! - `aad-logo.svg`: the vector source (the file names keep the project's code name)
//! - `aad-1024.png`, `-256.png`, `-64.png`: the square icons
//! - `aad-macos-512.png`: the rounded icon with a margin, for macOS
//! - `hicolor/<n>x<n>/apps/io.github.holagundiworks.aad.png`: the Linux sizes
//! - `aad.ico` (Windows) and `aad.icns` (macOS)
//!
//! Coordinates are those of a 194 x 194 box. The PNG writer is a small deflate (fixed Huffman
//! codes with greedy matching): the artwork is flat colour, so files stay small.

use std::path::Path;

/// The side of the artwork's box.
const ART: f64 = 194.0;
/// Midnight, `#102C80`.
const PLATE: [f64; 3] = [16.0, 44.0, 128.0];
const WHITE: [f64; 3] = [255.0, 255.0, 255.0];

pub const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 194 194" width="194" height="194">
  <title>Tren</title>
  <rect width="194" height="194" rx="42" fill="#102C80"/>
  <g fill="#FFFFFF">
    <path d="M16 118C12 52 70 22 124 42C158 54 172 78 154 100C126 76 80 80 30 128C16 138 12 128 16 118Z"/>
    <path d="M148 82L186 104L146 120Z"/>
  </g>
  <ellipse cx="138" cy="70" rx="6" ry="6" fill="#102C80"/>
</svg>
"##;

/// Cubic from `p0` through controls `p1`, `p2` to `p3`, appended as line samples.
fn cubic(out: &mut Vec<(f64, f64)>, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) {
    const N: i32 = 16;
    for i in 1..=N {
        let t = f64::from(i) / f64::from(N);
        let u = 1.0 - t;
        let x = u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0;
        let y = u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1;
        out.push((x, y));
    }
}

fn crown() -> Vec<(f64, f64)> {
    let mut p = vec![(16.0, 118.0)];
    cubic(&mut p, (16.0, 118.0), (12.0, 52.0), (70.0, 22.0), (124.0, 42.0));
    cubic(&mut p, (124.0, 42.0), (158.0, 54.0), (172.0, 78.0), (154.0, 100.0));
    cubic(&mut p, (154.0, 100.0), (126.0, 76.0), (80.0, 80.0), (30.0, 128.0));
    cubic(&mut p, (30.0, 128.0), (16.0, 138.0), (12.0, 128.0), (16.0, 118.0));
    p
}

fn beak() -> Vec<(f64, f64)> {
    vec![(148.0, 82.0), (186.0, 104.0), (146.0, 120.0)]
}

/// Even-odd test against a closed polygon.
fn inside(poly: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut hit = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            hit = !hit;
        }
        j = i;
    }
    hit
}

fn in_eye(x: f64, y: f64) -> bool {
    let dx = (x - 138.0) / 6.0;
    let dy = (y - 70.0) / 6.0;
    dx * dx + dy * dy <= 1.0
}

/// Crown and beak outlines of the bird, as polygon points.
type Bird = (Vec<(f64, f64)>, Vec<(f64, f64)>);

fn bird() -> &'static Bird {
    use std::sync::OnceLock;
    static BIRD: OnceLock<Bird> = OnceLock::new();
    BIRD.get_or_init(|| (crown(), beak()))
}

/// The colour of artwork point (x, y): the Midnight plate or the white bird.
fn art(x: f64, y: f64) -> [f64; 3] {
    let (crown, beak) = bird();
    if in_eye(x, y) {
        return PLATE;
    }
    if inside(crown, x, y) || inside(beak, x, y) { WHITE } else { PLATE }
}

/// One icon as RGBA. `mac`: the artwork sits in a rounded square with a margin on a transparent
/// ground, as macOS draws icons; otherwise it fills the whole square.
fn render(size: u32, mac: bool) -> Vec<u8> {
    const SS: u32 = 4;
    let s = f64::from(size);
    let (inner, off) = if mac { (s * 0.8125, s * 0.09375) } else { (s, 0.0) };
    let radius = if mac { inner * 0.2237 } else { inner * 42.0 / ART };
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (mut acc, mut alpha) = ([0.0f64; 3], 0.0f64);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = f64::from(px) + (f64::from(sx) + 0.5) / f64::from(SS);
                    let y = f64::from(py) + (f64::from(sy) + 0.5) / f64::from(SS);
                    let (u, v) = (x - off, y - off);
                    if u < 0.0 || v < 0.0 || u > inner || v > inner {
                        continue;
                    }
                    if radius > 0.0 {
                        let cx = u.clamp(radius, inner - radius);
                        let cy = v.clamp(radius, inner - radius);
                        if (u - cx).hypot(v - cy) > radius {
                            continue;
                        }
                    }
                    let c = art(u / inner * ART, v / inner * ART);
                    for k in 0..3 {
                        acc[k] += c[k];
                    }
                    alpha += 1.0;
                }
            }
            let n = f64::from(SS * SS);
            if alpha == 0.0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend_from_slice(&[
                    (acc[0] / alpha).round() as u8,
                    (acc[1] / alpha).round() as u8,
                    (acc[2] / alpha).round() as u8,
                    (alpha / n * 255.0).round() as u8,
                ]);
            }
        }
    }
    out
}

// ---------------- PNG ----------------

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, t) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut c = 0xffff_ffffu32;
    for b in data {
        c = table[((c ^ u32::from(*b)) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for x in chunk {
            a += u32::from(*x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, bits: u32) {
        self.acc |= v << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push((self.acc & 0xff) as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// A Huffman code: packed most significant bit first.
    fn code(&mut self, code: u32, bits: u32) {
        let mut r = 0;
        for i in 0..bits {
            r |= ((code >> i) & 1) << (bits - 1 - i);
        }
        self.put(r, bits);
    }
    fn literal(&mut self, v: u32) {
        match v {
            0..=143 => self.code(0x30 + v, 8),
            144..=255 => self.code(0x190 + v - 144, 9),
            256..=279 => self.code(v - 256, 7),
            _ => self.code(0xc0 + v - 280, 8),
        }
    }
}

const LEN_BASE: [u32; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u32; 30] =
    [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u32; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// zlib stream: one fixed-Huffman deflate block with greedy LZ77 matching.
fn zlib(data: &[u8]) -> Vec<u8> {
    let mut b = Bits { out: vec![0x78, 0x01], acc: 0, n: 0 };
    b.put(1, 1);
    b.put(1, 2);
    const HASH: usize = 1 << 15;
    let mut head = vec![u32::MAX; HASH];
    let mut prev = vec![u32::MAX; data.len()];
    let hash = |i: usize| -> usize { ((u32::from(data[i]) << 10) ^ (u32::from(data[i + 1]) << 5) ^ u32::from(data[i + 2])) as usize & (HASH - 1) };
    let mut i = 0;
    while i < data.len() {
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        if i + 3 <= data.len() {
            let h = hash(i);
            let mut cand = head[h];
            let mut steps = 0;
            while cand != u32::MAX && steps < 24 {
                let c = cand as usize;
                let dist = i - c;
                if dist > 32768 {
                    break;
                }
                let max = (data.len() - i).min(258);
                let mut l = 0;
                while l < max && data[c + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = dist;
                    if l == max {
                        break;
                    }
                }
                cand = prev[c];
                steps += 1;
            }
        }
        let advance = if best_len >= 3 {
            let li = LEN_BASE.iter().rposition(|&x| x <= best_len as u32).unwrap_or(0);
            b.literal(257 + li as u32);
            b.put(best_len as u32 - LEN_BASE[li], LEN_EXTRA[li]);
            let di = DIST_BASE.iter().rposition(|&x| x <= best_dist as u32).unwrap_or(0);
            b.code(di as u32, 5);
            b.put(best_dist as u32 - DIST_BASE[di], DIST_EXTRA[di]);
            best_len
        } else {
            b.literal(u32::from(data[i]));
            1
        };
        for (k, slot) in prev.iter_mut().enumerate().skip(i).take(advance) {
            if k + 3 <= data.len() {
                let h = hash(k);
                *slot = head[h];
                head[h] = k as u32;
            }
        }
        i += advance;
    }
    b.literal(256);
    if b.n > 0 {
        b.out.push(b.acc as u8);
    }
    b.out.extend_from_slice(&adler32(data).to_be_bytes());
    b.out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

pub fn png(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgba.len() + size as usize);
    for row in rgba.chunks((size * 4) as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// An `.icns` file of PNG images: (OSType, PNG bytes).
fn icns(parts: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let total: usize = 8 + parts.iter().map(|(_, d)| 8 + d.len()).sum::<usize>();
    let mut out = b"icns".to_vec();
    out.extend_from_slice(&(total as u32).to_be_bytes());
    for (kind, data) in parts {
        out.extend_from_slice(*kind);
        out.extend_from_slice(&((8 + data.len()) as u32).to_be_bytes());
        out.extend_from_slice(data);
    }
    out
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn run(root: &Path) -> Result<(), String> {
    let dir = root.join("assets/app-icon");
    let id = "io.github.holagundiworks.aad";
    write(&dir.join("aad-logo.svg"), SVG.as_bytes())?;
    let square = |n: u32| png(n, &render(n, false));
    let mac = |n: u32| png(n, &render(n, true));
    for n in [64u32, 256, 1024] {
        write(&dir.join(format!("aad-{n}.png")), &square(n))?;
    }
    write(&dir.join("aad-macos-512.png"), &mac(512))?;
    for n in [16u32, 24, 32, 48, 64, 128, 256, 512] {
        write(&dir.join(format!("hicolor/{n}x{n}/apps/{id}.png")), &square(n))?;
    }
    let pngs: Vec<Vec<u8>> = [16u32, 20, 24, 32, 40, 48, 64, 128, 256].iter().map(|n| square(*n)).collect();
    write(&dir.join("aad.ico"), &crate::ico::pack(&pngs)?)?;
    let icns_bytes = icns(&[
        (b"icp4", mac(16)),
        (b"icp5", mac(32)),
        (b"icp6", mac(64)),
        (b"ic07", mac(128)),
        (b"ic08", mac(256)),
        (b"ic09", mac(512)),
        (b"ic10", mac(1024)),
    ]);
    write(&dir.join("aad.icns"), &icns_bytes)?;
    println!("logo written to {}", dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_is_the_tren_bird_on_a_midnight_plate() {
        assert_eq!(art(40.0, 20.0), PLATE, "above the crown");
        assert_eq!(art(80.0, 70.0), WHITE, "inside the crown");
        assert_eq!(art(170.0, 104.0), WHITE, "inside the beak");
        assert_eq!(art(138.0, 70.0), PLATE, "the eye is a hole");
    }

    #[test]
    fn the_plate_is_midnight_and_the_svg_names_tren() {
        assert_eq!(PLATE, [16.0, 44.0, 128.0]);
        assert!(SVG.contains("#102C80") && SVG.contains("<title>Tren</title>"));
        assert!(!SVG.contains("O-Draft") && !SVG.contains("0066ff"));
    }

    #[test]
    fn an_icon_is_midnight_with_a_white_bird_and_the_mac_icon_has_a_transparent_margin() {
        let px = |rgba: &[u8], size: u32, x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        let sq = render(64, false);
        assert_eq!(px(&sq, 64, 1, 1)[3], 0, "rounded corner");
        assert_eq!(px(&sq, 64, 32, 8), [16, 44, 128, 255], "plate");
        let mac = render(64, true);
        assert_eq!(px(&mac, 64, 1, 1)[3], 0, "margin");
        assert_eq!(px(&mac, 64, 32, 32)[3], 255, "inside");
    }

    #[test]
    fn png_output_is_a_valid_deflate_stream_that_ico_accepts() {
        let rgba = render(64, false);
        let p = png(64, &rgba);
        assert_eq!(crate::ico::png_size(&p), Ok((64, 64)));
        assert!(p.len() < 2000, "flat artwork compresses: {} bytes", p.len());
        // The stored adler32 matches the data the stream decodes to: decode it with a tiny inflater.
        assert_eq!(inflate_fixed(&p), expected_raw(&rgba, 64));
        let ico = crate::ico::pack(&[p]).unwrap();
        assert_eq!(&ico[..4], &[0, 0, 1, 0]);
    }

    fn expected_raw(rgba: &[u8], size: u32) -> Vec<u8> {
        let mut raw = Vec::new();
        for row in rgba.chunks((size * 4) as usize) {
            raw.push(0);
            raw.extend_from_slice(row);
        }
        raw
    }

    /// Decode the first IDAT of a PNG written by `png` (fixed-Huffman deflate only).
    fn inflate_fixed(png: &[u8]) -> Vec<u8> {
        let mut i = 8;
        let mut idat = Vec::new();
        while i + 8 <= png.len() {
            let len = u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
            if &png[i + 4..i + 8] == b"IDAT" {
                idat = png[i + 8..i + 8 + len].to_vec();
            }
            i += 12 + len;
        }
        let data = &idat[2..idat.len() - 4];
        let (mut pos, mut out) = (0usize, Vec::new());
        let bit = |pos: &mut usize| -> u32 {
            let v = (data[*pos / 8] >> (*pos % 8)) & 1;
            *pos += 1;
            u32::from(v)
        };
        let bits = |pos: &mut usize, n: u32| -> u32 {
            let mut v = 0;
            for k in 0..n {
                v |= bit(pos) << k;
            }
            v
        };
        assert_eq!(bits(&mut pos, 3), 0b011, "final block, fixed codes");
        loop {
            // Read a literal/length code, most significant bit first.
            let mut code = 0u32;
            let mut len = 0;
            let sym = loop {
                code = (code << 1) | bit(&mut pos);
                len += 1;
                match len {
                    7 if code <= 0x17 => break 256 + code,
                    8 if (0x30..=0xbf).contains(&code) => break code - 0x30,
                    8 if (0xc0..=0xc7).contains(&code) => break 280 + code - 0xc0,
                    9 if (0x190..=0x1ff).contains(&code) => break 144 + code - 0x190,
                    _ => {}
                }
                assert!(len < 10, "bad code");
            };
            match sym {
                0..=255 => out.push(sym as u8),
                256 => break,
                _ => {
                    let li = (sym - 257) as usize;
                    let l = (LEN_BASE[li] + bits(&mut pos, LEN_EXTRA[li])) as usize;
                    let mut d = 0u32;
                    for _ in 0..5 {
                        d = (d << 1) | bit(&mut pos);
                    }
                    let dist = (DIST_BASE[d as usize] + bits(&mut pos, DIST_EXTRA[d as usize])) as usize;
                    for _ in 0..l {
                        out.push(out[out.len() - dist]);
                    }
                }
            }
        }
        out
    }

    #[test]
    fn the_icns_header_covers_every_part() {
        let parts = [(b"ic07", vec![1u8, 2, 3]), (b"ic08", vec![4u8; 10])];
        let f = icns(&parts);
        assert_eq!(&f[..4], b"icns");
        assert_eq!(u32::from_be_bytes([f[4], f[5], f[6], f[7]]) as usize, f.len());
        assert_eq!(f.len(), 8 + 8 + 3 + 8 + 10);
    }
}
