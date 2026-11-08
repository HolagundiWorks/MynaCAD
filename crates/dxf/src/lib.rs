//! A standalone DXF tag reader and writer.
//!
//! DXF is a sequence of (group code, value) pairs. This crate parses ASCII and binary DXF into a
//! flat list of [`Tag`]s, splits them into sections and entities, and writes tags back. Mapping
//! tags to a drawing model is left to the caller. Written from Autodesk's published DXF
//! Reference; depends on nothing else in the workspace.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::fmt::Write as _;

#[derive(Debug, thiserror::Error)]
pub enum DxfError {
    #[error("not a DXF file")]
    NotDxf,
    #[error("line {line}: bad group code `{text}`")]
    BadCode { line: usize, text: String },
    #[error("unexpected end of file")]
    Eof,
    /// More group codes than [`MAX_TAGS`]: `tags` is the count found (or the count read when the
    /// limit was reached), `limit` the maximum.
    #[error("drawing too large: {tags} DXF group codes, more than the {limit} AAD reads")]
    TooLarge { tags: usize, limit: usize },
}

pub type Result<T> = std::result::Result<T, DxfError>;

/// A typed group value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Real(f64),
    Int(i64),
    Bool(bool),
    /// Binary chunk (310–319, 1004) as hex text.
    Hex(String),
}

impl Value {
    pub fn as_str(&self) -> String {
        match self {
            Value::Str(s) | Value::Hex(s) => s.clone(),
            Value::Real(r) => format_real(*r),
            Value::Int(i) => i.to_string(),
            Value::Bool(b) => i64::from(*b).to_string(),
        }
    }
    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Real(r) if r.is_finite() => *r,
            Value::Int(i) => *i as f64,
            Value::Str(s) => s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(0.0),
            Value::Bool(b) => f64::from(u8::from(*b)),
            _ => 0.0,
        }
    }
    pub fn as_i64(&self) -> i64 {
        match self {
            Value::Int(i) => *i,
            Value::Real(r) if r.is_finite() => *r as i64,
            Value::Str(s) => s.trim().parse::<i64>().unwrap_or_else(|_| s.trim().parse::<f64>().map(|f| f as i64).unwrap_or(0)),
            Value::Bool(b) => i64::from(*b),
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    pub code: i32,
    pub value: Value,
}

impl Tag {
    pub fn new(code: i32, value: Value) -> Self {
        Tag { code, value }
    }
    pub fn s(code: i32, v: impl Into<String>) -> Self {
        Tag { code, value: Value::Str(v.into()) }
    }
    pub fn f(code: i32, v: f64) -> Self {
        Tag { code, value: Value::Real(if v.is_finite() { v } else { 0.0 }) }
    }
    pub fn i(code: i32, v: i64) -> Self {
        Tag { code, value: Value::Int(v) }
    }
    pub fn str(&self) -> String {
        self.value.as_str()
    }
    pub fn f64(&self) -> f64 {
        self.value.as_f64()
    }
    pub fn i64(&self) -> i64 {
        self.value.as_i64()
    }
}

/// The value type for a group code (DXF Reference, "Group Code Value Types").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Str,
    Real,
    Int16,
    Int32,
    Int64,
    Bool,
    Hex,
}

pub fn kind_of(code: i32) -> Kind {
    match code {
        0..=9 => Kind::Str,
        10..=59 => Kind::Real,
        60..=79 => Kind::Int16,
        90..=99 => Kind::Int32,
        100..=109 => Kind::Str,
        110..=149 => Kind::Real,
        160..=169 => Kind::Int64,
        170..=179 => Kind::Int16,
        210..=239 => Kind::Real,
        270..=289 => Kind::Int16,
        290..=299 => Kind::Bool,
        300..=309 => Kind::Str,
        310..=319 => Kind::Hex,
        320..=369 => Kind::Str,
        370..=389 => Kind::Int16,
        390..=399 => Kind::Str,
        400..=409 => Kind::Int16,
        410..=419 => Kind::Str,
        420..=429 => Kind::Int32,
        430..=439 => Kind::Str,
        440..=449 => Kind::Int32,
        450..=459 => Kind::Int32,
        460..=469 => Kind::Real,
        470..=479 => Kind::Str,
        480..=481 => Kind::Str,
        999 => Kind::Str,
        1000..=1003 => Kind::Str,
        1004 => Kind::Hex,
        1005..=1009 => Kind::Str,
        1010..=1059 => Kind::Real,
        1060..=1070 => Kind::Int16,
        1071 => Kind::Int32,
        _ => Kind::Str,
    }
}

fn typed(code: i32, text: &str) -> Value {
    let t = text.trim();
    match kind_of(code) {
        Kind::Real => t.parse::<f64>().ok().filter(|v| v.is_finite()).map(Value::Real).unwrap_or(Value::Real(0.0)),
        Kind::Int16 | Kind::Int32 | Kind::Int64 => {
            t.parse::<i64>().map(Value::Int).unwrap_or_else(|_| Value::Int(t.parse::<f64>().map(|f| f as i64).unwrap_or(0)))
        }
        Kind::Bool => Value::Bool(t.parse::<i64>().unwrap_or(0) != 0),
        Kind::Hex => Value::Hex(t.to_string()),
        Kind::Str => Value::Str(text.trim_end_matches(['\r', '\n']).to_string()),
    }
}

const BINARY_SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\0";
/// Most group codes read from one file. A `Tag` takes 40 bytes plus its text, so this bounds the
/// tag list to a few GB; real drawings stay far below (a 10 MB DWG is typically a few million).
pub const MAX_TAGS: usize = 50_000_000;

/// Decode bytes as text: UTF-8 when valid (borrowed, no copy), otherwise Latin-1 (old ANSI_1252
/// files).
fn decode_text(b: &[u8]) -> std::borrow::Cow<'_, str> {
    match std::str::from_utf8(b) {
        Ok(s) => std::borrow::Cow::Borrowed(s),
        Err(_) => std::borrow::Cow::Owned(b.iter().map(|&c| char::from(c)).collect()),
    }
}

/// Parse a DXF file (ASCII or binary).
pub fn parse(bytes: &[u8]) -> Result<Vec<Tag>> {
    parse_with_limit(bytes, MAX_TAGS)
}

/// [`parse`] with an explicit group-code limit. For ASCII DXF the limit is checked up front from
/// the line count, before the text is decoded or any tag allocated, so an oversized file fails in
/// milliseconds instead of after it has filled memory.
pub fn parse_with_limit(bytes: &[u8], max_tags: usize) -> Result<Vec<Tag>> {
    if let Some(body) = bytes.strip_prefix(BINARY_SENTINEL) {
        return parse_binary_with_limit(body, max_tags);
    }
    // Every group code takes two lines (code, value).
    let tags = bytes.iter().filter(|&&b| b == b'\n').count().div_ceil(2);
    if tags > max_tags {
        return Err(DxfError::TooLarge { tags, limit: max_tags });
    }
    parse_ascii_with_limit(&decode_text(bytes), max_tags)
}

/// The four hex digits of a `\U+XXXX` escape starting at `chars[i]`, if there is one.
fn escape_at(chars: &[char], i: usize) -> Option<u32> {
    if chars.get(i) != Some(&'\\') || chars.get(i + 1) != Some(&'U') || chars.get(i + 2) != Some(&'+') {
        return None;
    }
    let hex = chars.get(i + 3..i + 7)?;
    if !hex.iter().all(char::is_ascii_hexdigit) {
        return None;
    }
    hex.iter().try_fold(0u32, |acc, c| Some(acc * 16 + c.to_digit(16)?))
}

/// Unescape the `\U+XXXX` sequences pre-R2007 DXF uses for characters outside the file's code
/// page (four hex digits, one UTF-16 code unit; characters above U+FFFF are a surrogate pair of
/// escapes). Malformed escapes and unpaired surrogates are kept as literal text.
fn unescape_unicode(s: String) -> String {
    if !s.contains("\\U+") {
        return s;
    }
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if let Some(unit) = escape_at(&chars, i) {
            if let Some(ch) = char::from_u32(unit) {
                out.push(ch);
                i += 7;
                continue;
            }
            if (0xD800..0xDC00).contains(&unit)
                && let Some(low) = escape_at(&chars, i + 7).filter(|u| (0xDC00..0xE000).contains(u))
                && let Some(ch) = char::from_u32(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00))
            {
                out.push(ch);
                i += 14;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

pub fn parse_ascii(text: &str) -> Result<Vec<Tag>> {
    parse_ascii_with_limit(text, MAX_TAGS)
}

#[allow(clippy::while_let_loop)]
/// Text smaller than this is parsed on one thread: cutting it up costs more than it saves.
#[cfg(not(target_arch = "wasm32"))]
const PARALLEL_MIN_BYTES: usize = 8 << 20;

/// Where a record starts: a line that is exactly the group code 0 and is followed by a line that begins with a letter (the record
/// type). A value line equal to `0` (a layer named `0`) is followed by a group code, a number, so it never matches.
#[cfg(not(target_arch = "wasm32"))]
fn record_start_at(text: &str, line_start: usize) -> bool {
    let rest = text.get(line_start..).unwrap_or("");
    let mut lines = rest.split('\n');
    let Some(first) = lines.next() else { return false };
    if first.trim() != "0" {
        return false;
    }
    lines.next().is_some_and(|next| next.trim_start().chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
}

/// Byte offsets that cut `text` into about `parts` pieces, each starting at a record. None when no safe cut is found.
#[cfg(not(target_arch = "wasm32"))]
fn record_cuts(text: &str, parts: usize) -> Option<Vec<usize>> {
    let mut cuts = vec![0usize];
    for k in 1..parts {
        let mut pos = text.len() / parts * k;
        let floor = cuts.last().copied().unwrap_or(0);
        if pos <= floor {
            continue;
        }
        // Move to the start of the next line (a byte scan: the offset may fall inside a multi-byte character, and a line
        // start never does), then look for a record start within a bounded distance.
        let bytes = text.as_bytes();
        let next_line = |from: usize| bytes.get(from..).and_then(|b| b.iter().position(|&c| c == b'\n')).map(|n| from + n + 1);
        pos = next_line(pos)?;
        let limit = (pos + (4 << 20)).min(text.len());
        let mut found = None;
        let mut at = pos;
        while at < limit {
            if record_start_at(text, at) {
                found = Some(at);
                break;
            }
            at = next_line(at)?;
        }
        // A record bigger than the search window (one huge object): leave this cut out, the neighbours still divide the work.
        let Some(cut) = found else { continue };
        if cut > floor {
            cuts.push(cut);
        }
    }
    cuts.push(text.len());
    Some(cuts)
}

/// Parse a large ASCII DXF on every core. The cuts are only trusted when each piece parses cleanly; otherwise None, and the caller
/// parses sequentially (which also gives the exact error line).
#[cfg(not(target_arch = "wasm32"))]
fn parse_ascii_parallel(text: &str, max_tags: usize) -> Option<Vec<Tag>> {
    use rayon::prelude::*;
    let parts = rayon::current_num_threads().clamp(1, 32);
    if parts < 2 || text.len() < PARALLEL_MIN_BYTES {
        return None;
    }
    let cuts = record_cuts(text, parts * 2)?;
    let pieces: Vec<&str> = cuts.windows(2).filter_map(|w| text.get(*w.first()?..*w.get(1)?)).collect();
    let parsed: Vec<Vec<Tag>> = pieces.par_iter().map(|piece| parse_ascii_piece(piece).ok()).collect::<Option<Vec<_>>>()?;
    let total: usize = parsed.iter().map(Vec::len).sum();
    if total > max_tags {
        return None;
    }
    let mut tags = Vec::with_capacity(total);
    for mut part in parsed {
        tags.append(&mut part);
    }
    // The sequential reader stops after the first EOF record.
    if let Some(eof) = tags.iter().position(|t| t.code == 0 && t.str() == "EOF") {
        tags.truncate(eof + 1);
    }
    if tags.is_empty() { None } else { Some(tags) }
}

/// One piece of ASCII DXF: pairs of lines, with the sequential reader's rules (blank code lines are skipped).
#[cfg(not(target_arch = "wasm32"))]
fn parse_ascii_piece(text: &str) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();
    let mut lines = text.lines().enumerate();
    while let Some((ln, code_line)) = lines.next() {
        let ct = code_line.trim();
        if ct.is_empty() {
            continue;
        }
        let code: i32 = ct.parse().map_err(|_| DxfError::BadCode { line: ln + 1, text: ct.chars().take(40).collect() })?;
        let Some((_, val)) = lines.next() else { return Err(DxfError::Eof) };
        let v = match typed(code, val) {
            Value::Str(s) => Value::Str(unescape_unicode(s)),
            o => o,
        };
        tags.push(Tag { code, value: v });
    }
    Ok(tags)
}

fn parse_ascii_with_limit(text: &str, max_tags: usize) -> Result<Vec<Tag>> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(tags) = parse_ascii_parallel(text, max_tags) {
        return Ok(tags);
    }
    parse_ascii_sequential(text, max_tags)
}

/// The reference reader: one thread, line by line. It defines what the parallel reader must produce, and gives the exact error line.
fn parse_ascii_sequential(text: &str, max_tags: usize) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();
    let mut lines = text.lines().enumerate();
    while let Some((ln, code_line)) = lines.next() {
        let ct = code_line.trim();
        if ct.is_empty() {
            continue;
        }
        let code: i32 = ct.parse().map_err(|_| DxfError::BadCode { line: ln + 1, text: ct.chars().take(40).collect() })?;
        let Some((_, val)) = lines.next() else { return Err(DxfError::Eof) };
        let v = typed(code, val);
        let v = match v {
            Value::Str(s) => Value::Str(unescape_unicode(s)),
            o => o,
        };
        tags.push(Tag { code, value: v });
        if tags.len() > max_tags {
            return Err(DxfError::TooLarge { tags: tags.len(), limit: max_tags });
        }
        if code == 0 && tags.last().is_some_and(|t| t.str() == "EOF") {
            break;
        }
    }
    if tags.is_empty() {
        return Err(DxfError::NotDxf);
    }
    Ok(tags)
}

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl Cur<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.p.checked_add(n).ok_or(DxfError::Eof)?;
        let s = self.b.get(self.p..end).ok_or(DxfError::Eof)?;
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(*self.take(1)?.first().ok_or(DxfError::Eof)?)
    }
    fn i16(&mut self) -> Result<i16> {
        let s = self.take(2)?;
        Ok(i16::from_le_bytes([s[0], s[1]]))
    }
    fn i32(&mut self) -> Result<i32> {
        let s = self.take(4)?;
        Ok(i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i64(&mut self) -> Result<i64> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(i64::from_le_bytes(a))
    }
    fn f64(&mut self) -> Result<f64> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(f64::from_le_bytes(a))
    }
    fn cstr(&mut self) -> Result<String> {
        let start = self.p;
        while self.p < self.b.len() && self.b.get(self.p) != Some(&0) {
            self.p += 1;
        }
        let s = decode_text(self.b.get(start..self.p).unwrap_or(&[])).into_owned();
        if self.p >= self.b.len() {
            return Err(DxfError::Eof);
        }
        self.p += 1;
        Ok(s)
    }
}

/// Binary DXF (R13+: 2-byte group codes).
pub fn parse_binary(b: &[u8]) -> Result<Vec<Tag>> {
    parse_binary_with_limit(b, MAX_TAGS)
}

fn parse_binary_with_limit(b: &[u8], max_tags: usize) -> Result<Vec<Tag>> {
    let mut c = Cur { b, p: 0 };
    let mut tags = Vec::new();
    while c.p < b.len() {
        let mut code = i32::from(c.i16()?);
        if code == 255 {
            code = i32::from(c.i16()?);
        }
        let v = match kind_of(code) {
            Kind::Str => Value::Str(unescape_unicode(c.cstr()?)),
            Kind::Real => Value::Real(c.f64()?),
            Kind::Int16 => Value::Int(i64::from(c.i16()?)),
            Kind::Int32 => Value::Int(i64::from(c.i32()?)),
            Kind::Int64 => Value::Int(c.i64()?),
            Kind::Bool => Value::Bool(c.u8()? != 0),
            Kind::Hex => {
                let n = usize::from(c.u8()?);
                let bytes = c.take(n)?;
                let mut s = String::with_capacity(n * 2);
                for x in bytes {
                    let _ = write!(s, "{x:02X}");
                }
                Value::Hex(s)
            }
        };
        let eof = code == 0 && matches!(&v, Value::Str(s) if s == "EOF");
        tags.push(Tag { code, value: v });
        if tags.len() > max_tags {
            return Err(DxfError::TooLarge { tags: tags.len(), limit: max_tags });
        }
        if eof {
            break;
        }
    }
    Ok(tags)
}

/// Format a real the way DXF writers usually do (shortest round-trip, with a decimal point).
pub fn format_real(v: f64) -> String {
    if !v.is_finite() {
        return "0.0".into();
    }
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") { s } else { format!("{s}.0") }
}

/// Write tags as ASCII DXF (CRLF line ends, codes right-aligned to 3 as is conventional).
///
/// The output is pure ASCII: non-ASCII characters in strings are written as `\U+XXXX` escapes,
/// the pre-R2007 convention, so it is correct whatever `$DWGCODEPAGE` the header declares.
pub fn write_ascii(tags: &[Tag]) -> String {
    let mut out = String::with_capacity(tags.len() * 16);
    for t in tags {
        let _ = write!(out, "{:>3}\r\n", t.code);
        match &t.value {
            Value::Str(s) => escape_unicode(s, &mut out),
            Value::Real(r) => out.push_str(&format_real(*r)),
            Value::Int(i) => out.push_str(&i.to_string()),
            Value::Bool(b) => out.push_str(&i64::from(*b).to_string()),
            Value::Hex(h) => out.push_str(h),
        }
        out.push_str("\r\n");
    }
    out
}

/// Escape a string for a pre-R2007 DXF, whose text is in the `$DWGCODEPAGE` code page: ASCII
/// stays as it is and every other character becomes `\U+XXXX` (a surrogate pair of escapes
/// above U+FFFF), so the bytes are valid in any code page. Line breaks become spaces.
fn escape_unicode(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '\r' | '\n' => out.push(' '),
            c if c.is_ascii() => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\U+{u:04X}");
                }
            }
        }
    }
}

/// A section: `0 SECTION / 2 NAME ... 0 ENDSEC`.
#[derive(Clone, Debug, Default)]
pub struct Section {
    pub name: String,
    pub tags: Vec<Tag>,
}

/// A section that borrows its tags from the stream it was cut from (nothing is copied).
#[derive(Debug, Clone)]
pub struct SectionRef<'a> {
    pub name: String,
    pub tags: &'a [Tag],
}

/// Split a tag stream into sections without copying the tags.
pub fn sections_ref(tags: &[Tag]) -> Vec<SectionRef<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tags.len() {
        let is_sec = tags.get(i).is_some_and(|t| t.code == 0 && t.str() == "SECTION");
        if is_sec {
            let name = tags.get(i + 1).filter(|t| t.code == 2).map(Tag::str).unwrap_or_default();
            let start = i + 2;
            let mut j = start;
            while j < tags.len() && !(tags.get(j).is_some_and(|t| t.code == 0 && t.str() == "ENDSEC")) {
                j += 1;
            }
            out.push(SectionRef { name, tags: tags.get(start..j).unwrap_or(&[]) });
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// Split a tag stream into sections.
pub fn sections(tags: &[Tag]) -> Vec<Section> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tags.len() {
        let is_sec = tags.get(i).is_some_and(|t| t.code == 0 && t.str() == "SECTION");
        if is_sec {
            let name = tags.get(i + 1).filter(|t| t.code == 2).map(Tag::str).unwrap_or_default();
            let start = i + 2;
            let mut j = start;
            while j < tags.len() && !(tags.get(j).is_some_and(|t| t.code == 0 && t.str() == "ENDSEC")) {
                j += 1;
            }
            out.push(Section { name, tags: tags.get(start..j).unwrap_or(&[]).to_vec() });
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// Split a run of tags into records starting at each code-0 tag: (type, tags after the 0). The tags of a record are a slice of
/// the run, so nothing is copied.
pub fn records_ref(tags: &[Tag]) -> Vec<(String, &[Tag])> {
    let mut out: Vec<(String, &[Tag])> = Vec::new();
    let mut start = 0;
    let mut kind: Option<String> = None;
    for (i, t) in tags.iter().enumerate() {
        if t.code == 0 {
            if let Some(k) = kind.take() {
                out.push((k, tags.get(start..i).unwrap_or(&[])));
            }
            kind = Some(t.str());
            start = i + 1;
        }
    }
    if let Some(k) = kind {
        out.push((k, tags.get(start..).unwrap_or(&[])));
    }
    out
}

/// Split a run of tags into records starting at each code-0 tag: (type, tags after the 0).
pub fn records(tags: &[Tag]) -> Vec<(String, Vec<Tag>)> {
    let mut out: Vec<(String, Vec<Tag>)> = Vec::new();
    for t in tags {
        if t.code == 0 {
            out.push((t.str(), Vec::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push(t.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DXF big enough for the parallel reader, with the things that could fool a cut: layers named `0` (a value line equal to
    /// the record-start code), non-ASCII text, one record far bigger than the search window, and an early EOF followed by more text.
    #[cfg(not(target_arch = "wasm32"))]
    fn awkward_dxf(entities: usize, early_eof: bool) -> String {
        let mut s = String::from("  0
SECTION
  2
ENTITIES
");
        for i in 0..entities {
            let layer = if i % 3 == 0 { "0" } else { "Wände é ü" };
            s.push_str(&format!("  0
LINE
  8
{layer}
 10
{i}.5
 20
2.25
  1
0
"));
            if i == entities / 2 {
                // One object with a huge payload (millions of vertices in real files).
                s.push_str("  0
LWPOLYLINE
  8
0
");
                for v in 0..400_000 {
                    s.push_str(&format!(" 10
{v}
 20
0
"));
                }
            }
        }
        s.push_str("  0
ENDSEC
  0
EOF
");
        if early_eof {
            s.push_str("  0
SECTION
  2
TRAILING
  0
ENDSEC
");
        }
        s
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_parallel_reader_gives_exactly_what_the_sequential_reader_gives() {
        for early_eof in [false, true] {
            let text = awkward_dxf(120_000, early_eof);
            assert!(text.len() > PARALLEL_MIN_BYTES, "the test file must be big enough to be cut ({} bytes)", text.len());
            let parallel = parse_ascii_parallel(&text, usize::MAX).expect("the parallel reader should take this file");
            let sequential = parse_ascii_sequential(&text, usize::MAX).unwrap();
            assert_eq!(parallel.len(), sequential.len());
            assert!(parallel == sequential, "the tags differ");
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_damaged_piece_falls_back_to_the_sequential_reader() {
        let mut text = awkward_dxf(60_000, false);
        // A group code that is not a number, far from the start.
        let at = text.len() / 2;
        let at = text[at..].find("  0
LINE").map(|n| n + at).unwrap();
        text.insert_str(at, "  X
broken
");
        assert!(text.len() > PARALLEL_MIN_BYTES);
        assert!(parse_ascii_parallel(&text, usize::MAX).is_none());
        let err = parse_ascii_with_limit(&text, usize::MAX).unwrap_err();
        assert!(matches!(err, DxfError::BadCode { .. }), "{err}");
    }

    #[test]
    fn ascii_roundtrip() {
        let tags = vec![
            Tag::s(0, "SECTION"),
            Tag::s(2, "ENTITIES"),
            Tag::s(0, "LINE"),
            Tag::s(8, "0"),
            Tag::f(10, 1.5),
            Tag::f(20, 2.0),
            Tag::i(62, 1),
            Tag::s(0, "ENDSEC"),
            Tag::s(0, "EOF"),
        ];
        let text = write_ascii(&tags);
        let back = parse(text.as_bytes()).unwrap();
        assert_eq!(back, tags);
        let secs = sections(&back);
        assert_eq!(secs.len(), 1);
        assert_eq!(secs[0].name, "ENTITIES");
        let recs = records(&secs[0].tags);
        assert_eq!(recs[0].0, "LINE");
        assert_eq!(recs[0].1.len(), 4);
    }

    #[test]
    fn binary_parse() {
        let mut b = BINARY_SENTINEL.to_vec();
        let push_str = |b: &mut Vec<u8>, code: i16, s: &str| {
            b.extend_from_slice(&code.to_le_bytes());
            b.extend_from_slice(s.as_bytes());
            b.push(0);
        };
        push_str(&mut b, 0, "LINE");
        b.extend_from_slice(&10i16.to_le_bytes());
        b.extend_from_slice(&3.25f64.to_le_bytes());
        b.extend_from_slice(&62i16.to_le_bytes());
        b.extend_from_slice(&5i16.to_le_bytes());
        push_str(&mut b, 0, "EOF");
        let tags = parse(&b).unwrap();
        assert_eq!(tags[1], Tag::f(10, 3.25));
        assert_eq!(tags[2], Tag::i(62, 5));
    }

    #[test]
    fn malformed_inputs() {
        assert!(parse(b"").is_err());
        assert!(parse(b"abc\nxyz\n").is_err());
        assert!(parse(b"0\n").is_err());
        let mut b = BINARY_SENTINEL.to_vec();
        b.extend_from_slice(&10i16.to_le_bytes());
        b.extend_from_slice(&[1, 2, 3]);
        assert!(parse(&b).is_err());
        // Bad numbers become 0 rather than failing.
        let t = parse(b"10\nnotanumber\n0\nEOF\n").unwrap();
        assert_eq!(t[0].f64(), 0.0);
    }

    #[test]
    fn unicode_escapes_and_latin1() {
        let t = parse(b"1\nCaf\\U+00E9\n0\nEOF\n").unwrap();
        assert_eq!(t[0].str(), "Café");
        let t = parse(b"1\nCaf\xe9\n0\nEOF\n").unwrap();
        assert_eq!(t[0].str(), "Café");
    }

    #[test]
    fn surrogate_pair_escapes() {
        let t = parse(b"1\n\\U+D83D\\U+DE00 \\u+56fe\\U+53f7\n0\nEOF\n").unwrap();
        assert_eq!(t[0].str(), "😀 \\u+56fe号");
    }

    #[test]
    fn hostile_escapes_stay_literal() {
        for s in [
            "\\U+",
            "\\U",
            "\\",
            "a\\U+ZZZZ",
            "\\U+12",
            "\\U+D800",
            "\\U+DC00",
            "\\U+D800\\U+0041",
            "\\U+D83D\\U+DE",
            "\\U++123",
            "\\U+-123",
            "x\\U+12\u{e9}4",
        ] {
            let t = parse(format!("1\n{s}\n0\nEOF\n").as_bytes()).unwrap();
            let expect = if s == "\\U+D800\\U+0041" { "\\U+D800A" } else { s };
            assert_eq!(t[0].str(), expect, "{s:?}");
        }
    }

    #[test]
    fn writes_non_ascii_as_escapes() {
        let tags = vec![Tag::s(1, "图号 Ä°ø 😀\r\nend"), Tag::s(1000, "Ø"), Tag::s(0, "EOF")];
        let text = write_ascii(&tags);
        assert!(text.is_ascii());
        assert!(text.contains("\\U+56FE\\U+53F7 \\U+00C4\\U+00B0\\U+00F8 \\U+D83D\\U+DE00  end\r\n"));
        let back = parse(text.as_bytes()).unwrap();
        assert_eq!(back[0].str(), "图号 Ä°ø 😀  end");
        assert_eq!(back[1].str(), "Ø");
    }

    #[test]
    fn binary_strings_unescape() {
        let mut b = BINARY_SENTINEL.to_vec();
        b.extend_from_slice(&1i16.to_le_bytes());
        b.extend_from_slice(b"\\U+56FE\\U+53F7\0");
        b.extend_from_slice(&0i16.to_le_bytes());
        b.extend_from_slice(b"EOF\0");
        assert_eq!(parse(&b).unwrap()[0].str(), "图号");
    }
}
