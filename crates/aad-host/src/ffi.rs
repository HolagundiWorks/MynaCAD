//! C ABI for the WinUI shell. Pointers are only dereferenced for the duration of the call.
#![allow(unsafe_code)]

use std::ffi::c_char;
use std::panic::AssertUnwindSafe;

use serde_json::Value;

use crate::{Host, Stats};

const TEXT_CAP: usize = 1_048_576;

/// `sample` non-zero opens the sample drawing. Never null.
#[unsafe(no_mangle)]
pub extern "C" fn aad_host_create(sample: i32) -> *mut Host {
    Box::into_raw(Box::new(Host::create(sample != 0)))
}

/// # Safety
/// `host` is null or a pointer returned by [`aad_host_create`] that has not been destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_destroy(host: *mut Host) {
    if !host.is_null() {
        drop(unsafe { Box::from_raw(host) });
    }
}

/// # Safety
/// `host` is live. `command` and `params` are null-terminated UTF-8, each at most 1 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_execute(host: *mut Host, command: *const c_char, params: *const c_char) -> i32 {
    call(host, |host| {
        let command = read_text(command)?;
        let params = read_text(params)?;
        let value: Value = if params.is_empty() { Value::Null } else { serde_json::from_str(&params).map_err(|e| e.to_string())? };
        host.execute(&command, &value)?;
        Ok(())
    })
}

/// # Safety
/// `host` is live. `text` is null-terminated UTF-8, at most 1 MiB. One command-line line.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_cmdline(host: *mut Host, text: *const c_char) -> i32 {
    call(host, |host| {
        let text = read_text(text)?;
        host.submit(&text)
    })
}

/// Remember command-line text that has not been submitted, for Dynamic Input.
///
/// # Safety
/// `host` is live. `text` is null-terminated UTF-8, at most 1 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_draft(host: *mut Host, text: *const c_char) -> i32 {
    call(host, |host| {
        let text = read_text(text)?;
        host.set_draft(&text)
    })
}

/// `1` when Space should type a space (assistant `/` request, open quote, text prompt); `0` when Space
/// submits. Negative or other values are unused.
///
/// # Safety
/// `host` is live. `text` is null-terminated UTF-8, at most 1 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_space_literal(host: *mut Host, text: *const c_char) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    let text = match read_text(text) {
        Ok(text) => text,
        Err(_) => return 0,
    };
    if crate::bar::space_is_literal(&host.session, &text) { 1 } else { 0 }
}

/// `1` when the CAD draft should move to the fixed AI prompt (idle `/…`); `0` otherwise.
///
/// # Safety
/// `host` is live. `text` is null-terminated UTF-8, at most 1 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_slash_transfer(host: *mut Host, text: *const c_char) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    let text = match read_text(text) {
        Ok(text) => text,
        Err(_) => return 0,
    };
    if crate::bar::slash_transfer_eligible(&host.session, &text) { 1 } else { 0 }
}

/// Copy the slash-transfer payload (text after one leading `/` and following spaces).
/// Returns the byte length written (or needed). Does not require a live session.
///
/// # Safety
/// `text` is null-terminated UTF-8. `buf` is null or valid for `length` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_slash_payload(text: *const c_char, buf: *mut u8, length: u32) -> i32 {
    let text = match read_text(text) {
        Ok(text) => text,
        Err(_) => return 0,
    };
    let payload = crate::bar::slash_transfer_payload(&text);
    copy_bytes(payload.as_bytes(), buf, length)
}

/// Ask the assistant with natural-language text from the fixed AI prompt (not CAD `cmdline`).
///
/// # Safety
/// `host` is live. `text` is null-terminated UTF-8, at most 1 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_ask(host: *mut Host, text: *const c_char) -> i32 {
    call(host, |host| {
        let text = read_text(text)?;
        host.ask(&text)
    })
}

/// Autocomplete suggestions for the command-line draft as a JSON array of
/// `{insert, label, reference}` objects. Returns the byte length written (or needed).
///
/// # Safety
/// `host` is live. `text` is null-terminated UTF-8. `buf` is null or valid for `length` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_suggest(host: *mut Host, text: *const c_char, buf: *mut u8, length: u32) -> i32 {
    let Some(_host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    let text = match read_text(text) {
        Ok(text) => text,
        Err(_) => return 0,
    };
    let json = crate::bar::suggestions_json(&text);
    copy_bytes(json.as_bytes(), buf, length)
}

/// Tab at a point prompt. Copies the text with the Dynamic Input separator appended.
/// Returns 0 when Tab does not apply. A negative length means `buf` was too small.
///
/// # Safety
/// `host` is live. `text` is null-terminated UTF-8. `buf` is null or valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_dyn_tab(host: *mut Host, text: *const c_char, buf: *mut u8, len: u32) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    let text = match read_text(text) {
        Ok(text) => text,
        Err(_) => return 0,
    };
    match host.dyn_tab(&text) {
        Some(next) => copy_bytes(next.as_bytes(), buf, len),
        None => 0,
    }
}

/// The settings dialog for `id`. Returns 0 when that command has no dialog.
///
/// # Safety
/// `host` is live. `id` is null-terminated UTF-8. `buf` is null or valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_form(host: *mut Host, id: *const c_char, buf: *mut u8, len: u32) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    let id = match read_text(id) {
        Ok(id) => id,
        Err(_) => return 0,
    };
    match host.form_json(&id) {
        Some(json) => copy_bytes(json.as_bytes(), buf, len),
        None => 0,
    }
}

/// Start a command with the dialog's answers. Points still missing are picked on the drawing.
///
/// # Safety
/// `host` is live. `id` and `preset` are null-terminated UTF-8. `preset` is a JSON object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_start_form(host: *mut Host, id: *const c_char, preset: *const c_char) -> i32 {
    call(host, |host| {
        let id = read_text(id)?;
        let preset = read_text(preset)?;
        let value: Value = if preset.is_empty() { Value::Null } else { serde_json::from_str(&preset).map_err(|e| e.to_string())? };
        host.start_form(&id, &value)
    })
}

/// Apply a typed point to the hot grip. Returns 1 when the grip consumed the text
/// (applied, or rejected as not a point). Returns 0 when no grip is hot or the text is empty.
///
/// # Safety
/// `host` is live. `text` is null-terminated UTF-8, at most 1 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_grip_point(host: *mut Host, text: *const c_char) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return 0;
    };
    let text = match read_text(text) {
        Ok(text) => text,
        Err(_) => return 0,
    };
    i32::from(host.grip_point(&text))
}

/// Cycle the hot grip. Returns 1 when a grip mode changed, 0 when no grip is hot.
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_grip_cycle(host: *mut Host) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return 0;
    };
    i32::from(host.grip_cycle())
}

/// Tab to the next object snap under the cursor.
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_snap_tab(host: *mut Host) -> i32 {
    call(host, |host| host.snap_tab())
}

/// Move the drafting cursor (`dx`/`dy` are steps; positive `dy` is down). Non-zero `ctrl` pans;
/// non-zero `shift` uses a larger step.
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_key_nudge(host: *mut Host, dx: i32, dy: i32, ctrl: i32, shift: i32) -> i32 {
    call(host, |host| host.key_nudge(dx, dy, ctrl != 0, shift != 0))
}

/// Activate at the keyboard cursor (pick, place, grip, or close a selection window).
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_key_activate(host: *mut Host, shift: i32) -> i32 {
    call(host, |host| host.key_activate(shift != 0))
}

/// # Safety
/// `host` is live. Cancels the running command.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_cancel(host: *mut Host) -> i32 {
    call(host, |host| host.cancel_command())
}

/// Clear the command-line log. Does not cancel the running command or change the drawing.
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_clear_log(host: *mut Host) -> i32 {
    call(host, |host| host.clear_log())
}

/// # Safety
/// `host` is live. Non-zero `dark` is a black drawing area; zero is white.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_theme(host: *mut Host, dark: i32) -> i32 {
    call(host, |host| host.set_dark(dark != 0))
}

/// Copy the current layout as a one-page PDF. Returns the byte length, or a negative length when
/// `buf` is too small and nothing was written. Returns 0 and sets the last error on failure.
///
/// # Safety
/// `host` is live. `buf` is null or valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_pdf(host: *mut Host, buf: *mut u8, len: u32) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return 0;
    };
    match std::panic::catch_unwind(AssertUnwindSafe(|| host.pdf_bytes())) {
        Ok(Ok(bytes)) => {
            host.error.clear();
            copy_bytes(&bytes, buf, len)
        }
        Ok(Err(e)) => {
            host.error = e;
            0
        }
        Err(_) => {
            #[cfg(windows)]
            {
                host.in_call = false;
            }
            host.error = "internal error".into();
            0
        }
    }
}

/// Copy a UTF-8 snapshot. `slot` 0 is the menu tree, 1 is the command-line prompt and history,
/// 2 is the properties, layers and blocks snapshot.
/// Returns the byte length, or a negative length when `buf` is too small and nothing was written.
///
/// # Safety
/// `host` is live. `buf` is either null or valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_text(host: *mut Host, slot: i32, buf: *mut u8, len: u32) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    let text = std::panic::catch_unwind(AssertUnwindSafe(|| match slot {
        0 => host.menu_json(),
        1 => host.chrome_json(),
        2 => host.panels_json(),
        _ => String::new(),
    }));
    let text = text.unwrap_or_default();
    copy_bytes(text.as_bytes(), buf, len)
}

/// # Safety
/// `host` is live and `out` points at a [`Stats`] for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_stats(host: *mut Host, out: *mut Stats) -> i32 {
    call(host, |host| {
        if out.is_null() {
            return Err("missing stats".into());
        }
        unsafe { *out = host.stats() };
        Ok(())
    })
}

/// # Safety
/// `host` is live. `parent` is a Win32 window handle. The child is positioned in physical pixels.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_embed(host: *mut Host, parent: isize, x: i32, y: i32, width: u32, height: u32) -> i32 {
    call(host, |host| host.embed(parent, x, y, width, height))
}

/// # Safety
/// `host` is live and already embedded.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_move(host: *mut Host, x: i32, y: i32, width: u32, height: u32) -> i32 {
    call(host, |host| host.move_child(x, y, width, height))
}

/// # Safety
/// `host` is live. Applies queued pointer events and presents one frame.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_pump(host: *mut Host) -> i32 {
    call(host, |host| host.pump())
}

/// 1 when the child drawing window exists.
///
/// # Safety
/// `host` is null or live.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_embedded(host: *mut Host) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    i32::from(host.hwnd != 0)
}

/// Copy the pending right-click menu and clear it. A null `buf` only reports the length.
///
/// # Safety
/// `host` is live. `buf` receives UTF-8 without a terminator. Returns the byte length, or a
/// negative length when `buf` is too small and nothing was written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_popup(host: *mut Host, buf: *mut u8, len: u32) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return 0;
    };
    let n = copy_bytes(host.menu_request.as_bytes(), buf, len);
    if n > 0 && !buf.is_null() {
        host.menu_request.clear();
    }
    n
}

/// Copy a finished `plot.window` request as JSON and clear it. A null `buf` only reports the length.
///
/// # Safety
/// `host` is live. `buf` receives UTF-8 without a terminator. Returns the byte length, or a
/// negative length when `buf` is too small and nothing was written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_plot_request(host: *mut Host, buf: *mut u8, len: u32) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return 0;
    };
    let json = host.plot_request_json();
    let n = copy_bytes(json.as_bytes(), buf, len);
    if n > 0 && !buf.is_null() {
        host.clear_plot_request();
    }
    n
}

/// Copy the drawing clipboard as DXF text. A null `buf` only reports the length.
///
/// # Safety
/// `host` is live. `buf` receives UTF-8 without a terminator. Returns the byte length, or a
/// negative length when `buf` is too small and nothing was written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_clipboard(host: *mut Host, buf: *mut u8, len: u32) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    copy_bytes(host.clipboard_text().as_bytes(), buf, len)
}

/// Replace the drawing clipboard with DXF text. Returns 1 when the text was a drawing.
///
/// # Safety
/// `host` is live. `text` is null-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_clipboard_load(host: *mut Host, text: *const c_char) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return 0;
    };
    match read_text(text) {
        Ok(text) => i32::from(host.load_clipboard(&text)),
        Err(e) => {
            host.error = e;
            0
        }
    }
}

/// Copy the JSON result of the last successful [`aad_host_execute`]. A failed command clears it.
///
/// # Safety
/// `host` is live. `buf` receives UTF-8 without a terminator. Returns the byte length, or a
/// negative length when `buf` is too small and nothing was written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_result(host: *mut Host, buf: *mut u8, len: u32) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    copy_bytes(host.result.as_bytes(), buf, len)
}

/// # Safety
/// `host` is live. `buf` receives UTF-8 without a terminator. Returns the byte length, or a
/// negative length when `buf` is too small and nothing was written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_last_error(host: *mut Host, buf: *mut u8, len: u32) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    copy_bytes(host.error.as_bytes(), buf, len)
}

#[cfg(test)]
pub(crate) fn create(sample: i32) -> *mut Host {
    aad_host_create(sample)
}

/// # Safety
/// Same as [`aad_host_destroy`].
#[cfg(test)]
pub(crate) unsafe fn destroy(host: *mut Host) {
    unsafe { aad_host_destroy(host) };
}

/// # Safety
/// Same as [`aad_host_execute`].
#[cfg(test)]
pub(crate) unsafe fn execute(host: *mut Host, command: *const c_char, params: *const c_char) -> i32 {
    unsafe { aad_host_execute(host, command, params) }
}

/// # Safety
/// Same as [`aad_host_stats`].
#[cfg(test)]
pub(crate) unsafe fn stats(host: *mut Host, out: *mut Stats) -> i32 {
    unsafe { aad_host_stats(host, out) }
}

#[cfg(test)]
#[cfg(test)]
pub(crate) fn result_text(host: *mut Host) -> String {
    let mut buf = vec![0u8; 64];
    let n = unsafe { aad_host_result(host, buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 {
        return String::new();
    }
    let size = usize::try_from(n.unsigned_abs()).unwrap_or(0);
    if n < 0 {
        buf.resize(size, 0);
        let n = unsafe { aad_host_result(host, buf.as_mut_ptr(), buf.len() as u32) };
        if n <= 0 {
            return String::new();
        }
        let n = usize::try_from(n).unwrap_or(0).min(buf.len());
        return String::from_utf8_lossy(&buf[..n]).into_owned();
    }
    let n = usize::try_from(n).unwrap_or(0).min(buf.len());
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

#[cfg(test)]
pub(crate) fn error_text(host: *mut Host) -> String {
    let mut buf = vec![0u8; 512];
    let n = unsafe { aad_host_last_error(host, buf.as_mut_ptr(), buf.len() as u32) };
    if n <= 0 {
        return String::new();
    }
    let n = usize::try_from(n).unwrap_or(0).min(buf.len());
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// Show or hide the world-coordinate icon.
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_ucs(host: *mut Host, on: i32) -> i32 {
    call(host, |host| host.set_ucs(on != 0))
}

/// Show or hide the child drawing window.
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_canvas(host: *mut Host, visible: i32) -> i32 {
    call(host, |host| host.set_canvas_visible(visible != 0))
}

/// Answer a pending assistant confirmation. Non-zero allows the tool.
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_assistant(host: *mut Host, accept: i32) -> i32 {
    call(host, |host| host.assistant_reply(accept != 0))
}

/// Bind the loopback control channel. Returns the port, or `-1` on failure.
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_listen(host: *mut Host, port: u16) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return -1;
    };
    match std::panic::catch_unwind(AssertUnwindSafe(|| host.listen(port))) {
        Ok(Ok(bound)) => {
            host.error.clear();
            i32::from(bound)
        }
        Ok(Err(e)) => {
            host.error = e;
            -1
        }
        Err(_) => {
            host.error = "internal error".into();
            -1
        }
    }
}

fn call(host: *mut Host, f: impl FnOnce(&mut Host) -> Result<(), String>) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return 1;
    };
    match std::panic::catch_unwind(AssertUnwindSafe(|| f(host))) {
        Ok(Ok(())) => {
            host.error.clear();
            0
        }
        Ok(Err(e)) => {
            host.error = e;
            1
        }
        Err(_) => {
            #[cfg(windows)]
            {
                host.in_call = false;
            }
            host.error = "internal error".into();
            1
        }
    }
}

fn read_text(ptr: *const c_char) -> Result<String, String> {
    if ptr.is_null() {
        return Err("missing text".into());
    }
    let mut n = 0usize;
    while n < TEXT_CAP {
        if unsafe { *ptr.add(n) } == 0 {
            break;
        }
        n += 1;
    }
    if n == TEXT_CAP {
        return Err("text is too long".into());
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), n) };
    String::from_utf8(bytes.to_vec()).map_err(|_| "text is not utf-8".into())
}

fn copy_bytes(src: &[u8], buf: *mut u8, len: u32) -> i32 {
    let needed = i32::try_from(src.len()).unwrap_or(i32::MAX);
    if buf.is_null() || u32::try_from(src.len()).unwrap_or(u32::MAX) > len {
        return -needed;
    }
    if !src.is_empty() {
        unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), buf, src.len()) };
    }
    needed
}

/// Start reading a drawing file on a worker thread. Returns at once; poll with
/// [`aad_host_open_poll`].
///
/// # Safety
/// `host` is live. `path` is null-terminated UTF-8, at most 1 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_open_start(host: *mut Host, path: *const c_char) -> i32 {
    call(host, |host| {
        let path = read_text(path)?;
        host.open_start(&path)
    })
}

/// `0` nothing is loading, `1` still reading, `2` the drawing is open and fitted, `-1` the open
/// failed (the message is in the host error).
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_open_poll(host: *mut Host) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return -1;
    };
    match std::panic::catch_unwind(AssertUnwindSafe(|| host.open_poll())) {
        Ok(Ok(state)) => state,
        Ok(Err(e)) => {
            host.error = e;
            -1
        }
        Err(_) => {
            host.error = "internal error".into();
            -1
        }
    }
}

/// Start writing the active drawing on a worker thread: to `path`, or to its own file when `path`
/// is null or empty. Returns at once; poll with [`aad_host_save_poll`].
///
/// # Safety
/// `host` is live. `path` is null or null-terminated UTF-8, at most 1 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_save_start(host: *mut Host, path: *const c_char) -> i32 {
    call(host, |host| {
        let path = if path.is_null() { String::new() } else { read_text(path)? };
        host.save_start(if path.is_empty() { None } else { Some(path.as_str()) })
    })
}

/// `0` nothing is saving, `1` still writing, `2` saved, `-1` failed (the message is in the host error).
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_save_poll(host: *mut Host) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return -1;
    };
    match std::panic::catch_unwind(AssertUnwindSafe(|| host.save_poll())) {
        Ok(Ok(state)) => state,
        Ok(Err(e)) => {
            host.error = e;
            -1
        }
        Err(_) => {
            host.error = "internal error".into();
            -1
        }
    }
}

/// What the last completed save could not write, as `164 MULTILEADER, 2 MLINE` (empty when the file keeps everything).
/// `buf` receives UTF-8 without a terminator. Returns the byte length, or a negative length when `buf` is too small.
///
/// # Safety
/// `host` is live. `buf` is writable for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_save_notice(host: *mut Host, buf: *mut u8, len: u32) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    copy_bytes(host.save_notice().as_bytes(), buf, len)
}

/// Start a plot (`plot` or `exportpdf`) on a worker thread. `params` is the JSON of the command,
/// including `path`. Returns at once; poll with [`aad_host_plot_poll`].
///
/// # Safety
/// `host` is live. `command` and `params` are null-terminated UTF-8, each at most 1 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_plot_start(host: *mut Host, command: *const c_char, params: *const c_char) -> i32 {
    call(host, |host| {
        let command = read_text(command)?;
        let params = read_text(params)?;
        let value: Value = if params.is_empty() { Value::Null } else { serde_json::from_str(&params).map_err(|e| e.to_string())? };
        host.plot_start(&command, &value)
    })
}

/// `0` no plot is running, `1` still building, `2` the file is written, `-1` failed (the message is
/// in the host error).
///
/// # Safety
/// `host` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aad_host_plot_poll(host: *mut Host) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return -1;
    };
    match std::panic::catch_unwind(AssertUnwindSafe(|| host.plot_poll())) {
        Ok(Ok(state)) => state,
        Ok(Err(e)) => {
            host.error = e;
            -1
        }
        Err(_) => {
            host.error = "internal error".into();
            -1
        }
    }
}
