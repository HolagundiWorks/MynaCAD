//! SVG reading for `import`: shapes and paths become polylines. A small reader of its own (no XML or SVG crate), enough for drawings exported from CAD,
//! Inkscape or Illustrator: `path` (all commands, curves and arcs flattened), `line`, `polyline`, `polygon`, `rect`, `circle`, `ellipse`, groups and their
//! `transform`, the `viewBox` and the size units. Text, images, `use`, gradients, clip paths and styles are not read; the result counts what was left out.

use std::f64::consts::PI;

/// One polyline in millimetres on the paper, y up, and whether it is closed.
pub type Poly = (Vec<(f64, f64)>, bool);

#[derive(Debug, Default)]
pub struct Svg {
    pub polys: Vec<Poly>,
    pub width_mm: f64,
    pub height_mm: f64,
    /// Elements that are not drawn: text, images, `use`, and the like.
    pub skipped: usize,
    pub truncated: bool,
}

type Mat = [f64; 6];
const ID: Mat = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
const MM_PER_PX: f64 = 25.4 / 96.0;
/// Most polylines one file gives, and the deepest group nesting read.
const MAX_POLYS: usize = 200_000;
const MAX_DEPTH: usize = 64;

/// `a` applied after `b`.
fn mul(a: &Mat, b: &Mat) -> Mat {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

fn apply(m: &Mat, x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

struct Tag<'a> {
    name: String,
    attrs: Vec<(String, &'a str)>,
    closing: bool,
    selfclosed: bool,
}

impl Tag<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }
    fn num(&self, key: &str) -> f64 {
        self.get(key).and_then(|v| length(v, 0.0)).unwrap_or(0.0)
    }
}

/// The tags of the file in order; comments, declarations, CDATA and the text between tags are skipped.
fn tags(src: &str) -> Vec<Tag<'_>> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = src[i..].find('<') {
        i += off;
        let rest = &src[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map_or(rest.len(), |e| e + 3);
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            i += rest.find("]]>").map_or(rest.len(), |e| e + 3);
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            i += rest.find('>').map_or(rest.len(), |e| e + 1);
            continue;
        }
        // find the end of the tag, honouring quotes
        let mut j = i + 1;
        let mut quote = 0u8;
        while j < b.len() {
            let c = b[j];
            if quote != 0 {
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' {
                quote = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        if j >= b.len() {
            break;
        }
        let inner = &src[i + 1..j];
        i = j + 1;
        let closing = inner.starts_with('/');
        let selfclosed = inner.ends_with('/');
        let inner = inner.trim_start_matches('/').trim_end_matches('/');
        let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
        let name = inner[..name_end].rsplit(':').next().unwrap_or("").to_ascii_lowercase();
        let mut attrs = Vec::new();
        let mut rest = &inner[name_end..];
        while let Some(eq) = rest.find('=') {
            let key = rest[..eq].trim().rsplit(':').next().unwrap_or("").to_string();
            let after = rest[eq + 1..].trim_start();
            let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
            let Some(end) = after[1..].find(q) else { break };
            attrs.push((key, &after[1..1 + end]));
            rest = &after[end + 2..];
        }
        out.push(Tag { name, attrs, closing, selfclosed });
    }
    out
}

/// A length such as `12`, `3.5mm`, `1in`; `None` when it is a percentage or not a number. Unitless is pixels (96 per inch); `rel` is unused for now.
fn length(s: &str, _rel: f64) -> Option<f64> {
    let s = s.trim();
    let end = s.find(|c: char| !(c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'))).unwrap_or(s.len());
    // an exponent letter that starts a unit (`em`, `ex`) is not part of the number
    let mut end = end;
    while end > 0 && s[..end].parse::<f64>().is_err() {
        end -= 1;
    }
    let v: f64 = s[..end].parse().ok()?;
    let k = match s[end..].trim() {
        "" | "px" => 1.0,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        "mm" => 96.0 / 25.4,
        "cm" => 96.0 / 2.54,
        "in" => 96.0,
        _ => return None,
    };
    Some(v * k)
}

fn numbers(s: &str) -> Vec<f64> {
    let mut sc = Scan { s: s.as_bytes(), i: 0 };
    let mut v = Vec::new();
    while let Some(n) = sc.number() {
        v.push(n);
    }
    v
}

struct Scan<'a> {
    s: &'a [u8],
    i: usize,
}

impl Scan<'_> {
    fn skip(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }
    fn more(&mut self) -> bool {
        self.skip();
        self.i < self.s.len()
    }
    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.s.get(self.i).copied()
    }
    /// A number such as `1`, `-.5`, `1e-3`; a second `.` or a sign starts the next number, so `1.5.5` is 1.5 and .5.
    fn number(&mut self) -> Option<f64> {
        self.skip();
        let start = self.i;
        let mut j = self.i;
        if matches!(self.s.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let mut digits = 0;
        while self.s.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
            digits += 1;
        }
        if self.s.get(j) == Some(&b'.') {
            j += 1;
            while self.s.get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return None;
        }
        if matches!(self.s.get(j), Some(b'e' | b'E')) {
            let mut k = j + 1;
            if matches!(self.s.get(k), Some(b'+' | b'-')) {
                k += 1;
            }
            if self.s.get(k).is_some_and(u8::is_ascii_digit) {
                while self.s.get(k).is_some_and(u8::is_ascii_digit) {
                    k += 1;
                }
                j = k;
            }
        }
        self.i = j;
        std::str::from_utf8(&self.s[start..j]).ok()?.parse().ok()
    }
    /// An arc flag: one `0` or `1`, which may be written without a separator after it.
    fn flag(&mut self) -> Option<bool> {
        self.skip();
        let c = *self.s.get(self.i)?;
        if c == b'0' || c == b'1' {
            self.i += 1;
            Some(c == b'1')
        } else {
            None
        }
    }
}

fn transform(s: &str) -> Mat {
    let mut m = ID;
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let Some(close) = rest[open..].find(')') else { break };
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let a = numbers(&rest[open + 1..open + close]);
        rest = &rest[open + close + 1..];
        let t: Mat = match (name, a.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => [*a, *b, *c, *d, *e, *f],
            ("translate", [x]) => [1.0, 0.0, 0.0, 1.0, *x, 0.0],
            ("translate", [x, y]) => [1.0, 0.0, 0.0, 1.0, *x, *y],
            ("scale", [x]) => [*x, 0.0, 0.0, *x, 0.0, 0.0],
            ("scale", [x, y]) => [*x, 0.0, 0.0, *y, 0.0, 0.0],
            ("rotate", [d]) => rot(*d, 0.0, 0.0),
            ("rotate", [d, x, y]) => rot(*d, *x, *y),
            ("skewX", [d]) => [1.0, 0.0, d.to_radians().tan(), 1.0, 0.0, 0.0],
            ("skewY", [d]) => [1.0, d.to_radians().tan(), 0.0, 1.0, 0.0, 0.0],
            _ => continue,
        };
        m = mul(&m, &t);
    }
    m
}

fn rot(deg: f64, cx: f64, cy: f64) -> Mat {
    let (s, c) = deg.to_radians().sin_cos();
    mul(&mul(&[1.0, 0.0, 0.0, 1.0, cx, cy], &[c, s, -s, c, 0.0, 0.0]), &[1.0, 0.0, 0.0, 1.0, -cx, -cy])
}

/// Points of the cubic Bezier from `p0` (control points `p1`, `p2`, end `p3`), without `p0`.
fn cubic(out: &mut Vec<(f64, f64)>, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) {
    const N: usize = 16;
    for i in 1..=N {
        let t = i as f64 / N as f64;
        let u = 1.0 - t;
        out.push((
            u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0,
            u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1,
        ));
    }
}

fn quad(out: &mut Vec<(f64, f64)>, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64)) {
    const N: usize = 12;
    for i in 1..=N {
        let t = i as f64 / N as f64;
        let u = 1.0 - t;
        out.push((u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0, u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1));
    }
}

/// The elliptical arc from `p0` to `p1` (SVG endpoint form), flattened in steps of at most 6 degrees.
#[allow(clippy::too_many_arguments)]
fn arc(out: &mut Vec<(f64, f64)>, p0: (f64, f64), mut rx: f64, mut ry: f64, rot_deg: f64, large: bool, sweep: bool, p1: (f64, f64)) {
    rx = rx.abs();
    ry = ry.abs();
    if p0 == p1 {
        return;
    }
    if rx < 1e-12 || ry < 1e-12 {
        out.push(p1);
        return;
    }
    let (sin, cos) = rot_deg.to_radians().sin_cos();
    let dx = (p0.0 - p1.0) / 2.0;
    let dy = (p0.1 - p1.1) / 2.0;
    let x1 = cos * dx + sin * dy;
    let y1 = -sin * dx + cos * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let k = lambda.sqrt();
        rx *= k;
        ry *= k;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut co = if den == 0.0 { 0.0 } else { (num / den).max(0.0).sqrt() };
    if large == sweep {
        co = -co;
    }
    let cxp = co * rx * y1 / ry;
    let cyp = -co * ry * x1 / rx;
    let cx = cos * cxp - sin * cyp + (p0.0 + p1.0) / 2.0;
    let cy = sin * cxp + cos * cyp + (p0.1 + p1.1) / 2.0;
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = ang(1.0, 0.0, (x1 - cxp) / rx, (y1 - cyp) / ry);
    let mut dt = ang((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry);
    if !sweep && dt > 0.0 {
        dt -= 2.0 * PI;
    } else if sweep && dt < 0.0 {
        dt += 2.0 * PI;
    }
    let n = ((dt.abs() / (6.0f64).to_radians()).ceil() as usize).max(1);
    for i in 1..=n {
        let t = t1 + dt * i as f64 / n as f64;
        let (st, ct) = t.sin_cos();
        out.push((cos * rx * ct - sin * ry * st + cx, sin * rx * ct + cos * ry * st + cy));
    }
    // land exactly on the end point
    if let Some(l) = out.last_mut() {
        *l = p1;
    }
}

/// The subpaths of path data `d`, in the path's own units.
fn path_data(d: &str) -> Vec<Poly> {
    let mut sc = Scan { s: d.as_bytes(), i: 0 };
    let mut out: Vec<Poly> = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    let (mut x, mut y) = (0.0, 0.0);
    let (mut sx, mut sy) = (0.0, 0.0);
    let mut last_ctl: Option<(f64, f64)> = None; // second control point of the last cubic
    let mut last_q: Option<(f64, f64)> = None;
    let mut cmd = b'M';
    let flush = |cur: &mut Vec<(f64, f64)>, out: &mut Vec<Poly>, closed: bool| {
        if cur.len() >= 2 {
            out.push((std::mem::take(cur), closed));
        } else {
            cur.clear();
        }
    };
    while sc.more() {
        if let Some(c) = sc.peek().filter(u8::is_ascii_alphabetic) {
            cmd = c;
            sc.i += 1;
            if matches!(cmd, b'Z' | b'z') {
                if !cur.is_empty() {
                    flush(&mut cur, &mut out, true);
                }
                x = sx;
                y = sy;
                last_ctl = None;
                last_q = None;
                continue;
            }
        } else if matches!(cmd, b'Z' | b'z') {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
        let mut ok = true;
        match cmd.to_ascii_uppercase() {
            b'M' => {
                if let (Some(a), Some(b)) = (sc.number(), sc.number()) {
                    flush(&mut cur, &mut out, false);
                    x = ox + a;
                    y = oy + b;
                    sx = x;
                    sy = y;
                    cur.push((x, y));
                    // further pairs after a move are lines
                    cmd = if rel { b'l' } else { b'L' };
                } else {
                    ok = false;
                }
            }
            b'L' => {
                if let (Some(a), Some(b)) = (sc.number(), sc.number()) {
                    if cur.is_empty() {
                        cur.push((x, y));
                    }
                    x = ox + a;
                    y = oy + b;
                    cur.push((x, y));
                } else {
                    ok = false;
                }
            }
            b'H' => {
                if let Some(a) = sc.number() {
                    if cur.is_empty() {
                        cur.push((x, y));
                    }
                    x = ox + a;
                    cur.push((x, y));
                } else {
                    ok = false;
                }
            }
            b'V' => {
                if let Some(b) = sc.number() {
                    if cur.is_empty() {
                        cur.push((x, y));
                    }
                    y = oy + b;
                    cur.push((x, y));
                } else {
                    ok = false;
                }
            }
            b'C' => {
                let v: Vec<Option<f64>> = (0..6).map(|_| sc.number()).collect();
                if v.iter().all(Option::is_some) {
                    let v: Vec<f64> = v.into_iter().flatten().collect();
                    if cur.is_empty() {
                        cur.push((x, y));
                    }
                    let (p1, p2, p3) = ((ox + v[0], oy + v[1]), (ox + v[2], oy + v[3]), (ox + v[4], oy + v[5]));
                    cubic(&mut cur, (x, y), p1, p2, p3);
                    last_ctl = Some(p2);
                    (x, y) = p3;
                } else {
                    ok = false;
                }
            }
            b'S' => {
                let v: Vec<Option<f64>> = (0..4).map(|_| sc.number()).collect();
                if v.iter().all(Option::is_some) {
                    let v: Vec<f64> = v.into_iter().flatten().collect();
                    if cur.is_empty() {
                        cur.push((x, y));
                    }
                    let p1 = last_ctl.map_or((x, y), |c| (2.0 * x - c.0, 2.0 * y - c.1));
                    let (p2, p3) = ((ox + v[0], oy + v[1]), (ox + v[2], oy + v[3]));
                    cubic(&mut cur, (x, y), p1, p2, p3);
                    last_ctl = Some(p2);
                    (x, y) = p3;
                } else {
                    ok = false;
                }
            }
            b'Q' => {
                let v: Vec<Option<f64>> = (0..4).map(|_| sc.number()).collect();
                if v.iter().all(Option::is_some) {
                    let v: Vec<f64> = v.into_iter().flatten().collect();
                    if cur.is_empty() {
                        cur.push((x, y));
                    }
                    let (p1, p2) = ((ox + v[0], oy + v[1]), (ox + v[2], oy + v[3]));
                    quad(&mut cur, (x, y), p1, p2);
                    last_q = Some(p1);
                    (x, y) = p2;
                } else {
                    ok = false;
                }
            }
            b'T' => {
                if let (Some(a), Some(b)) = (sc.number(), sc.number()) {
                    if cur.is_empty() {
                        cur.push((x, y));
                    }
                    let p1 = last_q.map_or((x, y), |c| (2.0 * x - c.0, 2.0 * y - c.1));
                    let p2 = (ox + a, oy + b);
                    quad(&mut cur, (x, y), p1, p2);
                    last_q = Some(p1);
                    (x, y) = p2;
                } else {
                    ok = false;
                }
            }
            b'A' => {
                let r = (sc.number(), sc.number(), sc.number(), sc.flag(), sc.flag(), sc.number(), sc.number());
                if let (Some(rx), Some(ry), Some(rot), Some(large), Some(sweep), Some(a), Some(b)) = r {
                    if cur.is_empty() {
                        cur.push((x, y));
                    }
                    let p1 = (ox + a, oy + b);
                    arc(&mut cur, (x, y), rx, ry, rot, large, sweep, p1);
                    (x, y) = p1;
                } else {
                    ok = false;
                }
            }
            _ => ok = false,
        }
        if !ok {
            break;
        }
        if !matches!(cmd.to_ascii_uppercase(), b'C' | b'S') {
            last_ctl = None;
        }
        if !matches!(cmd.to_ascii_uppercase(), b'Q' | b'T') {
            last_q = None;
        }
    }
    flush(&mut cur, &mut out, false);
    out
}

fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Poly {
    const N: usize = 72;
    ((0..N).map(|i| (cx + rx * (2.0 * PI * i as f64 / N as f64).cos(), cy + ry * (2.0 * PI * i as f64 / N as f64).sin())).collect(), true)
}

fn points(s: &str) -> Vec<(f64, f64)> {
    numbers(s).chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

/// Read an SVG file. Sizes are in millimetres on the paper (an SVG without a size is read at 96 pixels per inch).
pub fn read(src: &str) -> Result<Svg, String> {
    let all = tags(src);
    let root = all.iter().find(|t| t.name == "svg" && !t.closing).ok_or("this is not an SVG file (no <svg> element)")?;
    let vb = root.get("viewBox").map(numbers).filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0);
    let w_px = root.get("width").and_then(|v| length(v, 0.0));
    let h_px = root.get("height").and_then(|v| length(v, 0.0));
    let (vbx, vby, vbw, vbh) = match &vb {
        Some(v) => (v[0], v[1], v[2], v[3]),
        None => (0.0, 0.0, w_px.unwrap_or(0.0), h_px.unwrap_or(0.0)),
    };
    if vbw <= 0.0 || vbh <= 0.0 {
        return Err("the SVG has neither a viewBox nor a width and height".into());
    }
    // user units to millimetres: the viewBox is stretched to the size when there is one (the aspect is kept, as `meet` does)
    let k = match (&vb, w_px, h_px) {
        (Some(_), Some(w), Some(h)) => (w / vbw).min(h / vbh) * MM_PER_PX,
        (Some(_), Some(w), None) => w / vbw * MM_PER_PX,
        (Some(_), None, Some(h)) => h / vbh * MM_PER_PX,
        _ => MM_PER_PX,
    };
    let base: Mat = [k, 0.0, 0.0, -k, -vbx * k, (vby + vbh) * k];
    let mut svg = Svg { width_mm: vbw * k, height_mm: vbh * k, ..Svg::default() };

    // stack of group transforms; `skip` > 0 while inside defs, clip paths and the like
    let mut stack: Vec<Mat> = vec![base];
    let mut skip = 0usize;
    let mut seen_root = false;
    for t in &all {
        let hidden = matches!(t.name.as_str(), "defs" | "clippath" | "mask" | "symbol" | "marker" | "pattern" | "style" | "metadata" | "title" | "desc");
        if t.closing {
            if skip > 0 {
                if hidden || t.name == "g" || t.name == "svg" || t.name == "a" || t.name == "switch" {
                    skip = skip.saturating_sub(1);
                }
            } else if matches!(t.name.as_str(), "g" | "a" | "switch" | "svg") && stack.len() > 1 {
                stack.pop();
            }
            continue;
        }
        if t.name == "svg" && !seen_root {
            seen_root = true;
            continue;
        }
        if skip > 0 {
            if !t.selfclosed && (hidden || matches!(t.name.as_str(), "g" | "svg" | "a" | "switch")) {
                skip += 1;
            }
            continue;
        }
        if hidden {
            if !t.selfclosed {
                skip = 1;
            }
            continue;
        }
        let parent = *stack.last().unwrap_or(&base);
        let m = match t.get("transform") {
            Some(tr) => mul(&parent, &transform(tr)),
            None => parent,
        };
        if t.get("display") == Some("none") || t.get("style").is_some_and(|s| s.replace(' ', "").contains("display:none")) {
            if !t.selfclosed && matches!(t.name.as_str(), "g" | "svg" | "a" | "switch") {
                skip = 1;
            }
            continue;
        }
        let mut shapes: Vec<Poly> = Vec::new();
        match t.name.as_str() {
            "g" | "a" | "switch" | "svg" => {
                if !t.selfclosed {
                    if stack.len() >= MAX_DEPTH {
                        return Err("the SVG nests groups too deeply".into());
                    }
                    stack.push(m);
                }
                continue;
            }
            "path" => shapes = path_data(t.get("d").unwrap_or("")),
            "line" => shapes.push((vec![(t.num("x1"), t.num("y1")), (t.num("x2"), t.num("y2"))], false)),
            "polyline" => shapes.push((points(t.get("points").unwrap_or("")), false)),
            "polygon" => shapes.push((points(t.get("points").unwrap_or("")), true)),
            "rect" => {
                let (x, y, w, h) = (t.num("x"), t.num("y"), t.num("width"), t.num("height"));
                if w > 0.0 && h > 0.0 {
                    shapes.push((vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)], true));
                }
            }
            "circle" => {
                let r = t.num("r");
                if r > 0.0 {
                    shapes.push(ellipse(t.num("cx"), t.num("cy"), r, r));
                }
            }
            "ellipse" => {
                let (rx, ry) = (t.num("rx"), t.num("ry"));
                if rx > 0.0 && ry > 0.0 {
                    shapes.push(ellipse(t.num("cx"), t.num("cy"), rx, ry));
                }
            }
            "text" | "image" | "use" | "foreignobject" | "textpath" => svg.skipped += 1,
            _ => {}
        }
        if t.get("fill") == Some("none") && t.get("stroke").is_none_or(|s| s == "none") && t.get("style").is_none() && t.name != "path" {
            // an invisible shape
            continue;
        }
        for (pts, closed) in shapes {
            if pts.len() < 2 {
                continue;
            }
            if svg.polys.len() >= MAX_POLYS {
                svg.truncated = true;
                break;
            }
            let pts: Vec<(f64, f64)> = pts.into_iter().map(|(x, y)| apply(&m, x, y)).filter(|(x, y)| x.is_finite() && y.is_finite()).collect();
            if pts.len() >= 2 {
                svg.polys.push((pts, closed));
            }
        }
    }
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    #[test]
    fn a_rect_in_a_millimetre_sized_svg_is_a_closed_polyline_with_y_up() {
        let svg = read(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50"><rect x="10" y="5" width="20" height="10" stroke="black"/></svg>"#)
            .unwrap();
        assert!((svg.width_mm - 100.0).abs() < 1e-6 && (svg.height_mm - 50.0).abs() < 1e-6);
        let (pts, closed) = &svg.polys[0];
        assert!(*closed);
        // SVG y runs down: the rectangle's top edge (y = 5) is 45 mm above the bottom
        assert!(near(pts[0], (10.0, 45.0)) && near(pts[2], (30.0, 35.0)));
    }

    #[test]
    fn path_commands_lines_curves_and_arcs() {
        let p = path_data("M0 0 L10 0 h5 v5 z M20 20 l5 5");
        assert_eq!(p.len(), 2);
        assert!(p[0].1 && p[0].0.len() == 4 && near(p[0].0[3], (15.0, 5.0)));
        assert!(!p[1].1 && near(p[1].0[1], (25.0, 25.0)));
        let c = path_data("M0 0 C 0 10, 10 10, 10 0");
        assert!(near(*c[0].0.last().unwrap(), (10.0, 0.0)) && c[0].0.len() > 8);
        // a half circle of radius 5 from (0,0) to (10,0): its top is 5 away from the chord
        let a = path_data("M0 0 A5 5 0 0 1 10 0");
        let top = a[0].0.iter().map(|p| p.1.abs()).fold(0.0, f64::max);
        assert!((top - 5.0).abs() < 0.1, "{top}");
        assert!(near(*a[0].0.last().unwrap(), (10.0, 0.0)));
    }

    #[test]
    fn compact_numbers_and_arc_flags() {
        assert_eq!(numbers("1.5.5-2e1 3"), vec![1.5, 0.5, -20.0, 3.0]);
        let a = path_data("M0 0a5 5 0 1010 0");
        assert!(near(*a[0].0.last().unwrap(), (10.0, 0.0)));
    }

    #[test]
    fn group_transforms_apply_and_defs_are_skipped() {
        let svg = read(
            r#"<svg width="10" height="10"><defs><rect width="1" height="1"/></defs><g transform="translate(2,3) scale(2)"><line x1="0" y1="0" x2="1" y2="0"/></g><text>x</text></svg>"#,
        )
        .unwrap();
        assert_eq!(svg.polys.len(), 1);
        assert_eq!(svg.skipped, 1);
        let k = MM_PER_PX;
        let p = &svg.polys[0].0;
        assert!(near(p[0], (2.0 * k, (10.0 - 3.0) * k)) && near(p[1], (4.0 * k, (10.0 - 3.0) * k)), "{p:?}");
    }

    #[test]
    fn a_file_that_is_not_svg_is_refused() {
        assert!(read("<html></html>").is_err());
        assert!(read(r#"<svg></svg>"#).is_err());
    }
}
