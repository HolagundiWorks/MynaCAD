//! AAD desktop app.
//!
//! Usage: `aad [--control <port>] [--sample] [--egui] [files…]`
//!
//! On Windows the default binary only starts the WinUI shell (`ODraft.WinUi.exe` beside the program
//! or under `target/aad-winui`), forwarding `--sample`, `--control`, and drawing paths. It does
//! not link eframe. Rebuild with `--features egui` and pass `--egui` for the optional egui window
//! (widget automation, wasm/web still use `ui-egui`).
//!
//! On macOS and Linux the egui/eframe window is the desktop shell.
//!
//! `--control <port>` (or `AAD_CONTROL_PORT`) starts a localhost JSON-lines control server.
//! On WinUI the host answers engine/cmdline/drawing methods plus `ui.inspect` / `ui.pointer` /
//! `ui.render` / `ui.screenshot` / `ui.menu.*`. egui widget automation needs `--features egui`
//! and `--egui --control`.
//!
//! `AAD_VSYNC=1` presents with vsync on Linux/BSD when the egui shell is used; see
//! `aad_ui_egui::gpu::surface_config`.
//!
//! `log` records go to standard error and `<settings dir>/logs/aad.log` (`RUST_LOG` sets the
//! levels); see [`logging`] and [`config_dir`].
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(any(not(windows), feature = "egui"))]
mod assistant;
#[cfg(any(not(windows), feature = "egui"))]
mod control_server;
#[cfg(any(not(windows), feature = "egui"))]
mod egui_shell;
#[cfg(all(windows, feature = "egui"))]
mod graphics;
mod logging;
#[cfg(all(target_os = "macos", feature = "egui"))]
mod native_menu;
#[cfg(any(not(windows), feature = "egui"))]
mod workspace;

/// The per-user settings directory (today it holds `logs/`): `AAD_CONFIG_DIR` if set, else
/// `$XDG_CONFIG_HOME/aad` or `~/.config/aad` on Linux and the BSDs,
/// `~/Library/Application Support/AAD` on macOS, `%APPDATA%\AAD` on Windows.
fn config_dir() -> Option<std::path::PathBuf> {
    config_dir_from(|k| std::env::var_os(k))
}

/// [`config_dir`] with the environment passed in (`var` looks a variable up). An empty variable
/// counts as unset, so a blank value never means the working directory.
fn config_dir_from(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<std::path::PathBuf> {
    let set = |k: &str| var(k).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    if let Some(dir) = set("AAD_CONFIG_DIR") {
        return Some(dir);
    }
    if cfg!(target_os = "macos") {
        set("HOME").map(|h| h.join("Library/Application Support/AAD"))
    } else if cfg!(windows) {
        set("APPDATA").map(|a| a.join("AAD"))
    } else {
        // The XDG spec ignores a relative XDG_CONFIG_HOME.
        set("XDG_CONFIG_HOME").filter(|p| p.is_absolute()).or_else(|| set("HOME").map(|h| h.join(".config"))).map(|c| c.join("aad"))
    }
}

fn version_string() -> String {
    let sha = option_env!("AAD_BUILD_SHA").unwrap_or("dev");
    format!("aad {} ({sha})", env!("CARGO_PKG_VERSION"))
}

/// The control port from `--control <port>` or `AAD_CONTROL_PORT` (`source`); a value that is
/// not a port is logged and ignored instead of silently starting no control channel.
fn control_port_from(source: &str, value: Option<String>) -> Option<u16> {
    let value = value?;
    let port = value.trim().parse().ok();
    if port.is_none() {
        log::warn!("{source}: {value:?} is not a port number; the control channel is off");
    }
    port
}

/// `ODraft.WinUi.exe` (or the earlier name, `Aad.WinUi.exe`) beside this program, or the unpackaged build under `target/aad-winui`.
#[cfg(windows)]
fn winui_executable() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("AAD_WINUI") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    ["ODraft.WinUi.exe", "Aad.WinUi.exe"]
        .into_iter()
        .flat_map(|name| [dir.join(name), dir.join("../aad-winui").join(name)])
        .find(|path| path.is_file())
}

#[cfg(windows)]
fn launch_winui(control_port: Option<u16>, sample: bool, files: &[String]) -> Result<(), i32> {
    let Some(path) = winui_executable() else {
        eprintln!(
            "MynaCAD: WinUI shell not found (build apps/aad-winui into target/aad-winui, or set AAD_WINUI).\n\
             Optional egui shell: cargo build -p aad --features egui && aad --egui"
        );
        return Err(1);
    };
    let mut cmd = std::process::Command::new(&path);
    if sample {
        cmd.arg("--sample");
    }
    if let Some(port) = control_port {
        cmd.arg("--control").arg(port.to_string());
        cmd.env("AAD_CONTROL_PORT", port.to_string());
    }
    for file in files {
        cmd.arg(file);
    }
    match cmd.spawn() {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("MynaCAD: WinUI shell ({}): {error}", path.display());
            Err(1)
        }
    }
}

fn main() {
    // A DWG is read by a copy of this program started with a flag (see aad_dwg::isolated): do
    // that and exit before anything else starts; otherwise isolation is on for this run.
    aad_dwg::isolated::run_worker_if_requested();
    aad_dwg::isolated::enable();
    // First, so every start-up record is captured; see `logging`.
    let logger = logging::install();
    // lossy, so a value that is not Unicode is reported like any other bad port instead of ignored
    let mut control_port = control_port_from("AAD_CONTROL_PORT", std::env::var_os("AAD_CONTROL_PORT").map(|p| p.to_string_lossy().into_owned()));
    let mut files = Vec::new();
    let mut sample = false;
    let mut want_egui = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = control_port_from("--control", args.next()),
            "--sample" => sample = true,
            "--egui" => want_egui = true,
            "--version" | "-V" => {
                println!("{}", version_string());
                return;
            }
            _ => files.push(a),
        }
    }
    // The log file lives under the settings directory; opened after the arguments, so `--version`
    // leaves no file behind. Records logged until now are written to it first.
    if let Some(logger) = logger {
        match config_dir().map(|d| logger.attach_dir(&d.join(logging::LOG_DIR))) {
            Some(Ok(path)) => log::info!("{} ({} {}), log file {}", version_string(), std::env::consts::OS, std::env::consts::ARCH, path.display()),
            Some(Err(e)) => log::warn!("no log file: {e}"),
            None => {
                logger.stderr_only();
                log::warn!("no log file: no settings directory (set AAD_CONFIG_DIR)");
            }
        }
    }

    #[cfg(windows)]
    if !want_egui {
        if let Err(code) = launch_winui(control_port, sample, &files) {
            std::process::exit(code);
        }
        return;
    }

    // Windows without `--features egui`: `--egui` is unavailable (WinUI is the default shell).
    #[cfg(all(windows, not(feature = "egui")))]
    {
        let _ = (control_port, sample, files, want_egui);
        eprintln!("MynaCAD: this Windows build has no egui shell. Rebuild with:\n  cargo build -p aad --features egui\nthen run: aad --egui …");
        std::process::exit(2);
    }

    #[cfg(any(not(windows), feature = "egui"))]
    {
        let _ = want_egui;
        if let Err(error) = egui_shell::run(control_port, sample, files) {
            eprintln!("MynaCAD: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{config_dir_from, control_port_from};
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn control_port_values_that_are_not_ports_are_ignored() {
        assert_eq!(control_port_from("--control", Some("7979".into())), Some(7979));
        assert_eq!(control_port_from("--control", Some(" 7979 ".into())), Some(7979));
        assert_eq!(control_port_from("--control", None), None);
        for bad in ["", "abc", "-1", "65536", "99999999999999999999", "\u{0}"] {
            assert_eq!(control_port_from("--control", Some(bad.into())), None, "{bad:?}");
        }
    }

    #[test]
    fn the_override_variable_wins_and_empty_values_are_unset() {
        let vars = [("AAD_CONFIG_DIR", "/tmp/cc"), ("HOME", "/home/u"), ("XDG_CONFIG_HOME", "/x"), ("APPDATA", "C:\\a")];
        assert_eq!(config_dir_from(env(&vars)), Some(PathBuf::from("/tmp/cc")));
        let empty = [("AAD_CONFIG_DIR", ""), ("HOME", ""), ("XDG_CONFIG_HOME", ""), ("APPDATA", "")];
        assert_eq!(config_dir_from(env(&empty)), None, "no usable variable means no directory, not the working directory");
        assert_eq!(config_dir_from(env(&[])), None);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_and_bsd_follow_xdg_config_home() {
        assert_eq!(config_dir_from(env(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/home/u")])), Some(PathBuf::from("/x/aad")));
        assert_eq!(config_dir_from(env(&[("HOME", "/home/u")])), Some(PathBuf::from("/home/u/.config/aad")));
        assert_eq!(config_dir_from(env(&[("XDG_CONFIG_HOME", "rel"), ("HOME", "/home/u")])), Some(PathBuf::from("/home/u/.config/aad")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_application_support() {
        assert_eq!(config_dir_from(env(&[("HOME", "/Users/u")])), Some(PathBuf::from("/Users/u/Library/Application Support/AAD")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_appdata() {
        assert_eq!(
            config_dir_from(env(&[("APPDATA", "C:\\Users\\u\\AppData\\Roaming")])),
            Some(PathBuf::from("C:\\Users\\u\\AppData\\Roaming").join("AAD"))
        );
    }
}
