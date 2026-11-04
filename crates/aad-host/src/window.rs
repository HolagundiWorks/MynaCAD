//! Child window the host paints into. The procedure only records input; drawing happens on
//! `Host::pump`, so a message cannot re-enter the session.
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicPtr, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient, ValidateRect};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, GW_HWNDPREV, GWLP_USERDATA, GWLP_WNDPROC, GetClientRect,
    GetWindow, GetWindowLongPtrW, HWND_TOP, IsWindowVisible, MA_NOACTIVATE, RegisterClassW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetCursor, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONDOWN, WM_SETCURSOR, WNDCLASSW,
    WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};

use crate::{
    Bridge, MOD_SHIFT, POINTER_LEFT_DOUBLE, POINTER_LEFT_DOWN, POINTER_LEFT_UP, POINTER_MIDDLE_DOUBLE, POINTER_MIDDLE_DOWN, POINTER_MIDDLE_UP,
    POINTER_MOVE, POINTER_RIGHT, Pointer,
};

const CLASS: &[u16] = &[
    b'A' as u16,
    b'A' as u16,
    b'D' as u16,
    b'_' as u16,
    b'H' as u16,
    b'O' as u16,
    b'S' as u16,
    b'T' as u16,
    b'_' as u16,
    b'C' as u16,
    b'A' as u16,
    b'N' as u16,
    b'V' as u16,
    b'A' as u16,
    b'S' as u16,
    0,
];

const ALREADY_EXISTS: u32 = 1410;

fn ensure_class() -> Result<(), String> {
    static READY: AtomicBool = AtomicBool::new(false);
    if READY.load(Ordering::Acquire) {
        return Ok(());
    }
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    if instance.is_null() {
        return Err("no module handle".into());
    }
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: std::ptr::null_mut(),
        // NULL: the procedure hides the OS cursor under the drawn crosshair (WM_SETCURSOR).
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: CLASS.as_ptr(),
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if err != ALREADY_EXISTS {
            return Err(format!("drawing window class failed ({err})"));
        }
    }
    READY.store(true, Ordering::Release);
    Ok(())
}

/// Create a child of `parent`. `bridge` receives wheel events and must outlive the window.
pub fn create(parent: isize, x: i32, y: i32, width: u32, height: u32, bridge: *const Bridge) -> Result<isize, String> {
    ensure_class()?;
    let parent = parent as HWND;
    if parent.is_null() {
        return Err("missing parent window".into());
    }
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            CLASS.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            x,
            y,
            i32::try_from(width).unwrap_or(i32::MAX),
            i32::try_from(height).unwrap_or(i32::MAX),
            parent,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        return Err(format!("drawing window failed ({err})"));
    }
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, bridge as isize) };
    remember_child(parent, hwnd, bridge);
    raise(hwnd as isize);
    Ok(hwnd as isize)
}

/// The WinUI island is a sibling that covers this child after layout. Lift the drawing window
/// back above any visible sibling so the swap chain is what the user sees.
pub fn raise(hwnd: isize) {
    let hwnd = hwnd as HWND;
    if hwnd.is_null() || !covered(hwnd) {
        return;
    }
    unsafe {
        SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

fn covered(hwnd: HWND) -> bool {
    let mut above = unsafe { GetWindow(hwnd, GW_HWNDPREV) };
    while !above.is_null() {
        if unsafe { IsWindowVisible(above) } != 0 {
            return true;
        }
        above = unsafe { GetWindow(above, GW_HWNDPREV) };
    }
    false
}

/// Move and size the drawing window. It is shown only when `show` is set, so a page drawn over it (the welcome screen) is not covered by a move.
pub fn move_to(hwnd: isize, x: i32, y: i32, width: u32, height: u32, show: bool) -> Result<(), String> {
    let hwnd = hwnd as HWND;
    let ok = unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOP,
            x,
            y,
            i32::try_from(width).unwrap_or(i32::MAX),
            i32::try_from(height).unwrap_or(i32::MAX),
            SWP_NOACTIVATE | if show { SWP_SHOWWINDOW } else { 0 },
        )
    };
    if ok == 0 {
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        return Err(format!("moving the drawing window failed ({err})"));
    }
    Ok(())
}

pub fn hide(hwnd: isize) {
    let hwnd = hwnd as HWND;
    if hwnd.is_null() {
        return;
    }
    unsafe {
        ShowWindow(hwnd, 0);
    }
}

pub fn destroy(hwnd: isize) {
    let hwnd = hwnd as HWND;
    if hwnd.is_null() {
        return;
    }
    forget_child();
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        DestroyWindow(hwnd);
    }
}

static PARENT_PROC: AtomicIsize = AtomicIsize::new(0);
static PARENT: AtomicIsize = AtomicIsize::new(0);
static CHILD: AtomicIsize = AtomicIsize::new(0);
static BRIDGE: AtomicPtr<Bridge> = AtomicPtr::new(std::ptr::null_mut());

fn remember_child(parent: HWND, child: HWND, bridge: *const Bridge) {
    CHILD.store(child as isize, Ordering::Release);
    BRIDGE.store(bridge.cast_mut(), Ordering::Release);
    if PARENT_PROC.load(Ordering::Acquire) != 0 {
        return;
    }
    let previous = unsafe { SetWindowLongPtrW(parent, GWLP_WNDPROC, parent_proc as *const () as isize) };
    if previous != 0 {
        PARENT_PROC.store(previous, Ordering::Release);
        PARENT.store(parent as isize, Ordering::Release);
    }
}

fn forget_child() {
    CHILD.store(0, Ordering::Release);
    BRIDGE.store(std::ptr::null_mut(), Ordering::Release);
    let parent = PARENT.swap(0, Ordering::AcqRel);
    let previous = PARENT_PROC.swap(0, Ordering::AcqRel);
    if parent != 0 && previous != 0 {
        unsafe { SetWindowLongPtrW(parent as HWND, GWLP_WNDPROC, previous) };
    }
}

unsafe extern "system" fn parent_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_MOUSEWHEEL && wheel_over_child(lparam, wparam) {
        return 0;
    }
    let previous = PARENT_PROC.load(Ordering::Acquire);
    if previous == 0 {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let proc = unsafe { std::mem::transmute::<isize, Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>>(previous) };
    unsafe { CallWindowProcW(proc, hwnd, msg, wparam, lparam) }
}

fn wheel_over_child(lparam: LPARAM, wparam: WPARAM) -> bool {
    let child = CHILD.load(Ordering::Acquire) as HWND;
    let bridge = BRIDGE.load(Ordering::Acquire);
    if child.is_null() || bridge.is_null() {
        return false;
    }
    let (x, y) = client_point(child, lparam);
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    if unsafe { GetClientRect(child, &mut rect) } == 0 || x < 0 || y < 0 || x >= rect.right || y >= rect.bottom {
        return false;
    }
    let delta = ((wparam >> 16) & 0xffff) as u16 as i16;
    unsafe { &*bridge }.push_wheel(f64::from(x), f64::from(y), f32::from(delta) / 120.0);
    true
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            unsafe { ValidateRect(hwnd, std::ptr::null()) };
            0
        }
        // Hide the system arrow so only the AutoCAD-style crosshair shows over the drawing.
        WM_SETCURSOR => {
            unsafe { SetCursor(std::ptr::null_mut()) };
            1
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_MOUSEWHEEL => {
            if let Some(bridge) = bridge(hwnd) {
                let (x, y) = client_point(hwnd, lparam);
                let delta = ((wparam >> 16) & 0xffff) as u16 as i16;
                bridge.push_wheel(f64::from(x), f64::from(y), f32::from(delta) / 120.0);
            }
            0
        }
        WM_LBUTTONDBLCLK => {
            post(hwnd, POINTER_LEFT_DOUBLE, lparam, wparam);
            0
        }
        WM_MBUTTONDBLCLK => {
            post(hwnd, POINTER_MIDDLE_DOUBLE, lparam, wparam);
            0
        }
        WM_LBUTTONDOWN | WM_MBUTTONDOWN => {
            unsafe { SetCapture(hwnd) };
            post(hwnd, if msg == WM_LBUTTONDOWN { POINTER_LEFT_DOWN } else { POINTER_MIDDLE_DOWN }, lparam, wparam);
            0
        }
        WM_LBUTTONUP => {
            post(hwnd, POINTER_LEFT_UP, lparam, wparam);
            release_if_idle(wparam);
            0
        }
        WM_MBUTTONUP => {
            post(hwnd, POINTER_MIDDLE_UP, lparam, wparam);
            release_if_idle(wparam);
            0
        }
        WM_RBUTTONDOWN => {
            post(hwnd, POINTER_RIGHT, lparam, wparam);
            0
        }
        WM_MOUSEMOVE => {
            post(hwnd, POINTER_MOVE, lparam, wparam);
            0
        }
        WM_DESTROY => 0,
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn bridge(hwnd: HWND) -> Option<&'static Bridge> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Bridge;
    if ptr.is_null() { None } else { Some(unsafe { &*ptr }) }
}

fn post(hwnd: HWND, action: u32, lparam: LPARAM, wparam: WPARAM) {
    let Some(bridge) = bridge(hwnd) else { return };
    let x = (lparam & 0xffff) as u16 as i16 as i32;
    let y = ((lparam >> 16) & 0xffff) as u16 as i16 as i32;
    let mut mods = 0;
    if wparam & 0x0004 != 0 {
        mods |= MOD_SHIFT;
    }
    if wparam & 0x0008 != 0 {
        mods |= crate::MOD_CTRL;
    }
    bridge.push(Pointer { x: f64::from(x), y: f64::from(y), action, wheel: 0.0, mods });
}

fn release_if_idle(wparam: WPARAM) {
    if wparam & (0x0001 | 0x0002 | 0x0010) == 0 {
        unsafe { ReleaseCapture() };
    }
}

/// Device pixels per logical point for this window. One when the DPI is unknown.
/// Client pixels of the child window, as screen pixels. The point itself when there is no window.
pub fn client_to_screen(hwnd: isize, x: f64, y: f64) -> (i32, i32) {
    let mut pt = POINT { x: i32::try_from(x as i64).unwrap_or(0), y: i32::try_from(y as i64).unwrap_or(0) };
    if hwnd != 0 {
        unsafe { ClientToScreen(hwnd as HWND, &mut pt) };
    }
    (pt.x, pt.y)
}

pub fn dpi_scale(hwnd: isize) -> f32 {
    if hwnd == 0 {
        return 1.0;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd as HWND) };
    if dpi == 0 { 1.0 } else { (dpi as f32 / 96.0).clamp(1.0, 4.0) }
}

fn client_point(hwnd: HWND, lparam: LPARAM) -> (i32, i32) {
    let mut pt = POINT { x: (lparam & 0xffff) as u16 as i16 as i32, y: ((lparam >> 16) & 0xffff) as u16 as i16 as i32 };
    unsafe { ScreenToClient(hwnd, &mut pt) };
    (pt.x, pt.y)
}
