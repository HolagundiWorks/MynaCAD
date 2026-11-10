//! Native project files: the drawing inside a `.osp`, and the working copy a foreign drawing (DWG, DXF) is converted into the first time it is
//! opened. Design and rules: `docs/NATIVE-OPEN.md`.
//!
//! A `.osp` keeps the drawing twice: as DXF or DWG bytes in the `drawing` table (what MynaOps and older versions read) and as the native model
//! (`aad_io::native`, tables `native_part` / `native_blob`), which opens without parsing. The native model is used only while it carries the hash
//! of the `drawing` row beside it, so a row written by another program always wins.

use std::path::{Path, PathBuf};
use std::time::Instant;

use aad_doc::Drawing;
use serde::Serialize;

use crate::cmd::file::io;

/// Version of the import (what reading a DWG or DXF produces). Raise it when the readers change what they make of a file, so working copies made
/// by an older reader are converted again from their source.
pub const IMPORT_VERSION: u32 = 1;

/// The environment variable that moves the per-user cache of working copies (tests; a managed machine).
pub const CACHE_ENV: &str = "AAD_NATIVE_CACHE";

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|n| n.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Drawing".into())
}

/// What a file on disk is right now: enough to tell that it changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStamp {
    pub path: String,
    pub name: String,
    pub size: u64,
    /// Last-write time in nanoseconds since 1970 (0 when the file system does not say).
    pub mtime_ns: u128,
    pub sha256: String,
}

fn mtime_ns(meta: &std::fs::Metadata) -> u128 {
    meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos())
}

/// Read a source drawing and stamp it. The hash is over the whole file (SHA-256; about 0.1 s for 120 MB).
fn read_source(path: &Path) -> Result<(SourceStamp, Vec<u8>), String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stamp = SourceStamp {
        path: path.to_string_lossy().into_owned(),
        name: file_name(path),
        size: bytes.len() as u64,
        mtime_ns: mtime_ns(&meta),
        sha256: aad_odp::sha256_hex(&bytes),
    };
    Ok((stamp, bytes))
}

/// The drawing in a project file. `native` says whether it came from the native model (fast) or was parsed from the stored DXF or DWG bytes.
pub struct ProjectDrawing {
    pub drawing: Drawing,
    pub native: bool,
    /// Why the native model was not used, when the file has one.
    pub fallback: Option<String>,
}

fn native_model(odp: &aad_odp::Odp, first: &str) -> Result<Option<Drawing>, String> {
    let Some(stamp) = odp.native_stamp().map_err(|e| e.to_string())? else { return Ok(None) };
    if stamp.format != aad_io::native::FORMAT_VERSION {
        return Err(format!("its native model is format {} (this program reads {})", stamp.format, aad_io::native::FORMAT_VERSION));
    }
    if stamp.drawing != first {
        return Err("its native model belongs to another drawing".into());
    }
    if odp.drawing_sha256(first).map_err(|e| e.to_string())?.as_deref() != Some(stamp.drawing_sha256.as_str()) {
        return Err("the drawing was changed by another program since the native model was written".into());
    }
    let parts = odp.native_parts(first).map_err(|e| e.to_string())?;
    aad_io::native::decode(&parts).map(Some).map_err(|e| e.to_string())
}

/// Read the drawing of a project file: the native model when it is present and belongs to the stored drawing, else the stored DXF or DWG bytes.
pub fn read_project(path: &str) -> Result<ProjectDrawing, String> {
    let odp = aad_odp::Odp::open(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    let name = odp.drawing_names().map_err(|e| format!("{path}: {e}"))?.into_iter().next().ok_or_else(|| format!("{path} holds no drawing yet"))?;
    let fallback = match native_model(&odp, &name) {
        Ok(Some(drawing)) => return Ok(ProjectDrawing { drawing, native: true, fallback: None }),
        Ok(None) => None,
        Err(why) => Some(why),
    };
    let hooks = io().ok_or_else(|| "file formats are not available in this build".to_string())?;
    let row = odp.drawing(&name).map_err(|e| format!("{path}: {e}"))?.ok_or_else(|| format!("{path}: the drawing {name} is missing"))?;
    let drawing = (hooks.read)(&row.bytes, &row.name)?;
    Ok(ProjectDrawing { drawing, native: false, fallback })
}

/// Write the drawing into a project file: the DXF or DWG bytes in the format of the drawing already in it (a new file takes `format_hint`, DXF if
/// there is none), the structural details and the native model, in one transaction. A drawing whose bytes are unchanged makes no revision, so a
/// save that changed nothing does not make other programs reload. Returns the byte count of the drawing written.
pub fn write_project(path: &str, drawing: &Drawing, format_hint: Option<&str>) -> Result<usize, String> {
    let hooks = io().ok_or_else(|| "file formats are not available in this build".to_string())?;
    let p = Path::new(path);
    let mut odp = if p.exists() { aad_odp::Odp::open(p) } else { aad_odp::Odp::create(p, &stem(p)) }.map_err(|e| format!("{path}: {e}"))?;
    let name = odp
        .drawing_names()
        .map_err(|e| format!("{path}: {e}"))?
        .into_iter()
        .next()
        .unwrap_or_else(|| format!("{}.{}", stem(p), format_hint.unwrap_or("dxf")));
    // The interchange bytes and the native parts are independent: build them side by side.
    let (bytes, parts) = std::thread::scope(|s| {
        let native = s.spawn(|| aad_io::native::encode(drawing).map_err(|e| e.to_string()));
        let bytes = (hooks.write)(drawing, &name);
        (bytes, native.join().unwrap_or_else(|_| Err("internal error while encoding the drawing".into())))
    });
    let (bytes, parts) = (bytes?, parts?);
    let sha = aad_odp::sha256_hex(&bytes);
    // A save that replaces the drawing keeps the file as it was first (rolling backups, `backup.rs`). If that copy cannot be made the save
    // fails and the file is not touched: a save never destroys the only earlier version.
    let before = odp.drawing_sha256(&name).map_err(|e| format!("{path}: {e}"))?;
    let policy = crate::backup::Policy::current();
    if policy.count > 0 && before.as_deref().is_some_and(|old| old != sha) {
        crate::backup::keep_copy(p, "save", &policy)
            .map_err(|e| format!("{path}: the backup before saving could not be made, nothing was saved ({e})"))?;
    }
    // The structural details (RCC blocks with their parameters) go in the same revision as the drawing, for MynaOps to draw read only.
    let details = crate::structural_details(drawing, &name);
    let mut edit = odp.edit().map_err(|e| format!("{path}: {e}"))?;
    if edit.drawing_sha256(&name).map_err(|e| format!("{path}: {e}"))?.as_deref() != Some(sha.as_str()) {
        let ext = Path::new(&name).extension().and_then(|e| e.to_str()).unwrap_or("dxf").to_string();
        edit.put_drawing(&name, &ext, &bytes).map_err(|e| format!("{path}: {e}"))?;
    }
    edit.replace_details(&name, &details).map_err(|e| format!("{path}: {e}"))?;
    edit.put_native(&name, &sha, aad_io::native::FORMAT_VERSION, &part_refs(&parts)).map_err(|e| format!("{path}: {e}"))?;
    edit.set_local_meta("native.saved", &aad_odp::now_utc()).map_err(|e| format!("{path}: {e}"))?;
    edit.commit().map_err(|e| format!("{path}: {e}"))?;
    Ok(bytes.len())
}

fn part_refs(parts: &[aad_io::native::Part]) -> Vec<aad_odp::NativePart<'_>> {
    parts.iter().map(|p| aad_odp::NativePart { hash: &p.hash, bytes: &p.bytes, entities: p.entities }).collect()
}

/// The hooks a host installs so `open`, `save` and `saveas` handle project files.
pub fn container_hooks() -> crate::cmd::file::ContainerHooks {
    crate::cmd::file::ContainerHooks { read: |path| read_project(path).map(|p| p.drawing), write: write_project }
}

/// What to do when the source of a working copy has changed since it was imported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OnChanged {
    /// Import again when the working copy was never saved after its import (nothing of the user's is in it); otherwise open the working copy and
    /// report the change, so a person decides.
    #[default]
    Auto,
    /// Import again. The working copy as it was is kept in its backups folder first.
    Reimport,
    /// Open the working copy as it is.
    Keep,
}

impl OnChanged {
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "auto" | "" => Some(OnChanged::Auto),
            "reimport" => Some(OnChanged::Reimport),
            "keep" => Some(OnChanged::Keep),
            _ => None,
        }
    }
}

/// What opening a file for editing did.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenInfo {
    /// The project file the drawing is edited in and saved to.
    pub native: String,
    /// The DWG or DXF it was imported from, when it is a working copy.
    pub source: Option<String>,
    /// `project` (a project file was opened), `imported` (first open: converted), `reused` (working copy opened, source unchanged),
    /// `reimported` (source changed: converted again), `kept` (source changed: the working copy was opened as it is).
    pub action: String,
    /// `beside` the source or in the per-user `cache`.
    pub location: String,
    /// The source differs from what the working copy was imported from (only with `kept`).
    pub source_changed: bool,
    /// The working copy was saved after its import (it holds edits a re-import would not have).
    pub edited: bool,
    /// The copy kept before a re-import replaced the drawing.
    pub backup: Option<String>,
    /// The drawing came from the native model, not from parsing DXF or DWG.
    pub from_native: bool,
    /// Milliseconds: `hash` (read and hash the source), `convert` (parse DWG/DXF), `encode`, `write` (the project file), `read` (the native model).
    pub ms: std::collections::BTreeMap<String, f64>,
    /// One line for the status bar.
    pub message: String,
}

/// A drawing opened for editing with where it lives.
pub struct Opened {
    pub drawing: Drawing,
    /// The project file to tie the document to.
    pub path: String,
    pub info: OpenInfo,
}

static CACHE_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Keep working copies that cannot live beside their source under `dir` for the rest of the process (`None`: back to the default). Tests use
/// this; a program is configured with [`CACHE_ENV`].
pub fn set_cache_root(dir: Option<PathBuf>) {
    if let Ok(mut slot) = CACHE_OVERRIDE.lock() {
        *slot = dir;
    }
}

fn cache_root() -> PathBuf {
    if let Some(dir) = CACHE_OVERRIDE.lock().ok().and_then(|slot| slot.clone()) {
        return dir;
    }
    if let Some(v) = std::env::var_os(CACHE_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(v);
    }
    match std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        Some(base) => PathBuf::from(base).join("OSAS").join("native-cache"),
        None => std::env::temp_dir().join("OSAS").join("native-cache"),
    }
}

/// The working copy of `source` in the per-user cache: `<cache>/<16 hex digits of the hash of the source's full path>/<name>.osp`.
pub fn cache_path(source: &Path) -> PathBuf {
    let full = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    let key = aad_odp::sha256_hex(full.to_string_lossy().to_lowercase().as_bytes());
    cache_root().join(key.get(..16).unwrap_or("0")).join(format!("{}.osp", stem(source)))
}

/// Where an existing project file says it was imported from: `(source file name, stored stamp)`; `None` when it is not a working copy.
fn import_record(odp: &aad_odp::Odp) -> Option<(String, SourceStamp, u32, String)> {
    let get = |k: &str| odp.meta_get(k).ok().flatten();
    let name = get("source.name")?;
    let stamp = SourceStamp {
        path: get("source.path").unwrap_or_default(),
        name: name.clone(),
        size: get("source.size")?.parse().ok()?,
        mtime_ns: get("source.mtime_ns").and_then(|v| v.parse().ok()).unwrap_or(0),
        sha256: get("source.sha256")?,
    };
    let version = get("source.import_version").and_then(|v| v.parse().ok()).unwrap_or(0);
    Some((name, stamp, version, get("source.drawing_sha256").unwrap_or_default()))
}

/// Whether the project file at `path` is a working copy of a source with this file name. A file that is not (the user's own project) is never
/// written by an import.
fn is_working_copy_of(path: &Path, source_name: &str) -> Option<bool> {
    if !path.exists() {
        return None;
    }
    let ours = aad_odp::Odp::open(path).ok().and_then(|odp| import_record(&odp)).is_some_and(|(name, ..)| name.eq_ignore_ascii_case(source_name));
    Some(ours)
}

enum Slot {
    /// A working copy of this source exists here.
    Existing(PathBuf, &'static str),
    /// Nothing is here and the folder can be written.
    Free(PathBuf, &'static str),
}

fn can_create_in(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".aad-write-test-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// The rule for where a working copy lives: beside the source as `<name>.osp` when that file is already this source's working copy, or does not
/// exist and the folder can be written; otherwise (a project of the user's own has the name, or the folder is read-only) in the per-user cache.
fn slot_for(source: &Path) -> Result<Slot, String> {
    let name = file_name(source);
    let beside = source.with_extension("osp");
    match is_working_copy_of(&beside, &name) {
        Some(true) => return Ok(Slot::Existing(beside, "beside")),
        None if beside.parent().is_some_and(can_create_in) => return Ok(Slot::Free(beside, "beside")),
        _ => {}
    }
    let cached = cache_path(source);
    match is_working_copy_of(&cached, &name) {
        Some(true) => Ok(Slot::Existing(cached, "cache")),
        Some(false) => Err(format!("{} is in the way of the working copy and is not one", cached.display())),
        None if cached.parent().is_some_and(can_create_in) => Ok(Slot::Free(cached, "cache")),
        None => Err(format!("no folder to keep the working copy in ({} cannot be written)", cached.display())),
    }
}

fn source_meta<'a>(s: &'a SourceStamp, size: &'a str, mtime: &'a str, version: &'a str, now: &'a str) -> [(&'static str, &'a str); 8] {
    [
        ("source.path", s.path.as_str()),
        ("source.name", s.name.as_str()),
        ("source.size", size),
        ("source.mtime_ns", mtime),
        ("source.sha256", s.sha256.as_str()),
        ("source.drawing_sha256", s.sha256.as_str()),
        ("source.import_version", version),
        ("source.imported", now),
    ]
}

/// Write the import into the project file open in `odp`: the source bytes as the drawing, the native model, and where it came from. One
/// transaction: the file holds the whole import or none of it.
fn write_import(odp: &mut aad_odp::Odp, stamp: &SourceStamp, bytes: &[u8], parts: &[aad_io::native::Part], drawing: &Drawing) -> Result<(), String> {
    let ext = Path::new(&stamp.name).extension().and_then(|e| e.to_str()).unwrap_or("dxf").to_ascii_lowercase();
    // An import into an existing working copy replaces its drawing under the name the file already uses.
    let name = odp.drawing_names().map_err(|e| e.to_string())?.into_iter().next().unwrap_or_else(|| stamp.name.clone());
    let refs = part_refs(parts);
    let (size, mtime, version, now) = (stamp.size.to_string(), stamp.mtime_ns.to_string(), IMPORT_VERSION.to_string(), aad_odp::now_utc());
    let mut edit = odp.edit().map_err(|e| e.to_string())?;
    edit.put_drawing(&name, &ext, bytes).map_err(|e| e.to_string())?;
    edit.replace_details(&name, &crate::structural_details(drawing, &name)).map_err(|e| e.to_string())?;
    edit.put_native(&name, &stamp.sha256, aad_io::native::FORMAT_VERSION, &refs).map_err(|e| e.to_string())?;
    for (k, v) in source_meta(stamp, &size, &mtime, &version, &now) {
        edit.set_local_meta(k, v).map_err(|e| e.to_string())?;
    }
    // The origin and unit stamps, under the keys a save writes for the geometry database (`write_project_meta`). A drawing from another program
    // has no MynaCAD stamp and gets none here: it stays foreign until it is saved. What its header says about units is recorded as read.
    if let Some(v) = drawing.project.get(aad_geodb::origin::ORIGIN_KEY) {
        edit.set_local_meta("origin", v).map_err(|e| e.to_string())?;
    }
    if let Some(v) = drawing.project.get(aad_geodb::units::UNITS_KEY).and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok()) {
        if let Some(system) = v.get("system").and_then(serde_json::Value::as_str) {
            edit.set_local_meta("drafting_units", system).map_err(|e| e.to_string())?;
        }
        if let Some(unit) = v.get("unit").and_then(serde_json::Value::as_str) {
            edit.set_local_meta("drafting_unit", unit).map_err(|e| e.to_string())?;
        }
    }
    edit.set_local_meta("source.insunits", &drawing.header.i64("INSUNITS", 0).to_string()).map_err(|e| e.to_string())?;
    edit.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// Create the working copy at `target` (which does not exist): built under a temporary name in the same folder and renamed into place, so a
/// failure or a crash leaves no half-written project file.
fn create_import(target: &Path, stamp: &SourceStamp, bytes: &[u8], parts: &[aad_io::native::Part], drawing: &Drawing) -> Result<(), String> {
    let dir = target.parent().ok_or_else(|| format!("{}: no folder", target.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = dir.join(format!("{}.importing-{}", file_name(target), std::process::id()));
    let cleanup = |tmp: &Path| {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", tmp.display()));
        }
    };
    cleanup(&tmp);
    let built = (|| {
        let mut odp = aad_odp::Odp::create_bulk(&tmp, &stem(target)).map_err(|e| e.to_string())?;
        write_import(&mut odp, stamp, bytes, parts, drawing)?;
        // Closed and flushed: the file is whole before it is renamed.
        odp.finish_bulk(&tmp).map_err(|e| e.to_string())
    })();
    if let Err(e) = built {
        cleanup(&tmp);
        return Err(format!("{}: {e}", target.display()));
    }
    if target.exists() {
        cleanup(&tmp);
        return Err(format!("{} appeared while the drawing was being converted; it was not replaced", target.display()));
    }
    std::fs::rename(&tmp, target).map_err(|e| {
        cleanup(&tmp);
        format!("{}: {e}", target.display())
    })
}

fn convert(bytes: &[u8], name: &str) -> Result<Drawing, String> {
    let hooks = io().ok_or_else(|| "file formats are not available in this build".to_string())?;
    (hooks.read)(bytes, name)
}

/// Convert `source` (DWG or DXF) into a new project file at `out`, which must not exist. For scripts and tests (`aad-cli convert IN OUT.osp`).
pub fn import_to(source: &str, out: &str) -> Result<OpenInfo, String> {
    let (src, target) = (Path::new(source), Path::new(out));
    if target.exists() {
        return Err(format!("{out} already exists; it was not replaced"));
    }
    let mut info = OpenInfo { native: out.to_string(), source: Some(source.to_string()), action: "imported".into(), ..OpenInfo::default() };
    let t = Instant::now();
    let (stamp, bytes) = read_source(src)?;
    info.ms.insert("hash".into(), ms(t));
    let t = Instant::now();
    let drawing = convert(&bytes, source)?;
    info.ms.insert("convert".into(), ms(t));
    let t = Instant::now();
    let parts = aad_io::native::encode(&drawing).map_err(|e| e.to_string())?;
    info.ms.insert("encode".into(), ms(t));
    let t = Instant::now();
    create_import(target, &stamp, &bytes, &parts, &drawing)?;
    info.ms.insert("write".into(), ms(t));
    info.message = format!("Converted {} into {out}.", stamp.name);
    Ok(info)
}

/// Open `path` for editing. A project file opens as it is. A DWG or DXF is opened through its working copy: converted once into a project file
/// (beside it, or in the per-user cache: see [`slot_for`]) and read from there afterwards for as long as the source has not changed. The source
/// is only ever read. `progress` receives one line per stage for the status bar.
pub fn open_for_edit(path: &str, on_changed: OnChanged, progress: &dyn Fn(&str)) -> Result<Opened, String> {
    let src = Path::new(path);
    if crate::cmd::file::is_container(path) {
        progress(&format!("Opening {}", file_name(src)));
        let t = Instant::now();
        let read = read_project(path)?;
        let mut info = OpenInfo { native: path.to_string(), action: "project".into(), from_native: read.native, ..OpenInfo::default() };
        info.ms.insert("read".into(), ms(t));
        if let Ok(odp) = aad_odp::Odp::open(src) {
            info.source = import_record(&odp).map(|(_, stamp, ..)| stamp.path);
        }
        return Ok(Opened { drawing: read.drawing, path: path.to_string(), info });
    }
    let mut info = OpenInfo { source: Some(path.to_string()), ..OpenInfo::default() };
    progress(&format!("Checking {}", file_name(src)));
    let t = Instant::now();
    let (stamp, bytes) = read_source(src)?;
    info.ms.insert("hash".into(), ms(t));

    let (target, existing) = match slot_for(src)? {
        Slot::Existing(p, loc) => {
            info.location = loc.into();
            (p, true)
        }
        Slot::Free(p, loc) => {
            info.location = loc.into();
            (p, false)
        }
    };
    info.native = target.to_string_lossy().into_owned();
    let where_note = if info.location == "cache" { " (in the MynaCAD cache folder)" } else { "" };

    if existing {
        let record = aad_odp::Odp::open(&target).ok().and_then(|odp| {
            let rec = import_record(&odp)?;
            let first = odp.drawing_names().ok()?.into_iter().next()?;
            let row = odp.drawing_sha256(&first).ok().flatten().unwrap_or_default();
            Some((rec, row))
        });
        if let Some(((_, was, version, imported_row), row_now)) = record {
            info.edited = !imported_row.is_empty() && imported_row != row_now;
            let unchanged = was.size == stamp.size && was.sha256 == stamp.sha256;
            let reimport = match (unchanged, on_changed) {
                // An older reader made this copy: convert again, unless that would drop the user's saved edits.
                (true, _) => version != IMPORT_VERSION && !info.edited,
                (false, OnChanged::Reimport) => true,
                (false, OnChanged::Keep) => false,
                (false, OnChanged::Auto) => !info.edited,
            };
            if !reimport {
                progress(&format!("Opening the working copy {}", file_name(&target)));
                let t = Instant::now();
                let read = read_project(&info.native)?;
                info.ms.insert("read".into(), ms(t));
                info.from_native = read.native;
                info.source_changed = !unchanged;
                info.action = if unchanged { "reused" } else { "kept" }.into();
                info.message = if unchanged {
                    format!("Working copy {}{where_note}, imported from {}. The original is not changed by saving.", info.native, stamp.name)
                } else {
                    format!(
                        "{} has changed since it was imported. The working copy {}{where_note} has saved edits and was opened as it is; import again to take the new {}.",
                        stamp.name, info.native, stamp.name
                    )
                };
                return Ok(Opened { drawing: read.drawing, path: info.native.clone(), info });
            }
        }
    }

    progress(&format!("Converting {} to the native format (first open of this version of the file)", stamp.name));
    let t = Instant::now();
    let drawing = convert(&bytes, path)?;
    info.ms.insert("convert".into(), ms(t));
    progress(&format!("Writing the working copy {}", file_name(&target)));
    let t = Instant::now();
    let parts = aad_io::native::encode(&drawing).map_err(|e| e.to_string())?;
    info.ms.insert("encode".into(), ms(t));
    let t = Instant::now();
    if existing {
        // The working copy keeps its take-off, project record and history: only the drawing is replaced, after a copy of the file is put aside.
        // Kept even with BACKUPCOUNT 0: a re-import replaces the user's saved edits, the copy is the only way back.
        let policy = crate::backup::Policy::current();
        let policy = crate::backup::Policy { count: policy.count.max(1), ..policy };
        let backup = crate::backup::keep_copy(&target, "before-reimport", &policy)?;
        info.backup = Some(backup.to_string_lossy().into_owned());
        let mut odp = aad_odp::Odp::open(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        write_import(&mut odp, &stamp, &bytes, &parts, &drawing).map_err(|e| format!("{}: {e}", target.display()))?;
        info.action = "reimported".into();
        info.message = format!(
            "{} changed and was converted again into {}{where_note}. The earlier working copy is kept as {}.",
            stamp.name,
            info.native,
            info.backup.as_deref().unwrap_or_default()
        );
    } else {
        create_import(&target, &stamp, &bytes, &parts, &drawing)?;
        info.action = "imported".into();
        info.message = format!(
            "{} was converted to the working copy {}{where_note}. Save writes the working copy; the original is not changed.",
            stamp.name, info.native
        );
    }
    info.ms.insert("write".into(), ms(t));
    Ok(Opened { drawing, path: info.native.clone(), info })
}

/// What opening `path` would find, without converting anything: for a shell that must ask before it opens.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peek {
    /// `project` (a project file), `new` (no working copy yet: the open converts), `current` (the working copy matches the source), `changed`
    /// (the source differs from what the working copy was imported from).
    pub state: String,
    /// The working copy, when there is one or one would be made.
    pub native: Option<String>,
    /// The working copy was saved after its import: a re-import would not have those edits (they stay in the backup it makes).
    pub edited: bool,
}

/// Look at `path` and its working copy. The source is hashed only when its size is unchanged and its time is not, so the usual cases cost two
/// file-system calls.
pub fn peek(path: &str) -> Result<Peek, String> {
    let src = Path::new(path);
    if crate::cmd::file::is_container(path) {
        return Ok(Peek { state: "project".into(), native: Some(path.to_string()), edited: false });
    }
    let meta = std::fs::metadata(src).map_err(|e| format!("{path}: {e}"))?;
    let (target, existing) = match slot_for(src)? {
        Slot::Existing(p, _) => (p, true),
        Slot::Free(p, _) => (p, false),
    };
    let native = Some(target.to_string_lossy().into_owned());
    if !existing {
        return Ok(Peek { state: "new".into(), native, edited: false });
    }
    let odp = aad_odp::Odp::open(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    let Some((_, was, _, imported_row)) = import_record(&odp) else {
        return Ok(Peek { state: "new".into(), native, edited: false });
    };
    let row_now = odp.drawing_names().ok().and_then(|n| n.into_iter().next()).and_then(|n| odp.drawing_sha256(&n).ok().flatten()).unwrap_or_default();
    let edited = !imported_row.is_empty() && imported_row != row_now;
    let same = was.size == meta.len() && (was.mtime_ns == mtime_ns(&meta) || read_source(src).is_ok_and(|(now, _)| now.sha256 == was.sha256));
    Ok(Peek { state: if same { "current" } else { "changed" }.into(), native, edited })
}
