//! Loading `office.dll` and calling it. Windows only; elsewhere the link reports that it is not available.
//!
//! Every pointer crossing the boundary is a NUL-terminated string this module made, or the result struct the library returned, which
//! is freed exactly once. An error from the library comes back as text; nothing here panics.
#![allow(unsafe_code)]

use crate::{Error, Result, Settings};
use serde_json::{Value, json};

/// An open, signed-in session on the company file. Dropping it closes the session and releases the file.
pub struct Link {
    inner: imp::Session,
}

impl Link {
    /// Opens the company file named in `settings` and signs in with its ticket.
    pub fn open(settings: &Settings, app_dir: &std::path::Path) -> Result<Link> {
        let company = settings
            .company
            .as_ref()
            .ok_or_else(|| Error::Unavailable("No company file was given. Open Tren from Kivo so it can read the company's details.".into()))?;
        let ticket = settings
            .ticket
            .as_deref()
            .ok_or_else(|| Error::Unavailable("Kivo gave no sign-in with this start. Open Tren from Kivo, signed in.".into()))?;
        let library = settings.find_library().ok_or_else(|| {
            Error::Unavailable("office.dll was not found beside Tren or in the Kivo folder, so the company file cannot be read.".into())
        })?;
        let link = Link { inner: imp::Session::start(&library, app_dir)? };
        link.call("company.open", &json!({ "path": company.to_string_lossy(), "secret": "", "unlock": "" }))?;
        link.call("auth.login_ticket", &json!({ "ticket": ticket }))?;
        Ok(link)
    }

    /// Opens the company file with a user name and password. For tests and tools; Kivo starts the app with a ticket instead.
    pub fn open_with_password(
        library: &std::path::Path,
        app_dir: &std::path::Path,
        company: &std::path::Path,
        user: &str,
        password: &str,
    ) -> Result<Link> {
        let link = Link { inner: imp::Session::start(library, app_dir)? };
        link.call("company.open", &json!({ "path": company.to_string_lossy(), "secret": "", "unlock": "" }))?;
        link.call("auth.login", &json!({ "username": user, "password": password }))?;
        Ok(link)
    }

    /// A session with no company open, for tools and tests that make a company file first.
    pub fn bare(library: &std::path::Path, app_dir: &std::path::Path) -> Result<Link> {
        Ok(Link { inner: imp::Session::start(library, app_dir)? })
    }

    /// One operation of the office interface. Returns its answer, or the message the library gave.
    pub fn call(&self, op: &str, args: &Value) -> Result<Value> {
        self.inner.call(op, &args.to_string())
    }

    /// The company's details and today's date, and, when `project_id` is given, that project's details.
    pub fn document_context(&self, project_id: Option<i64>) -> Result<Value> {
        let args = match project_id {
            Some(id) => json!({ "project_id": id }),
            None => json!({}),
        };
        self.call("document_context", &args)
    }

    /// The projects in the company file, for choosing the one a drawing belongs to.
    pub fn projects(&self) -> Result<Value> {
        self.call("project_list", &json!({ "client_id": null, "include_archived": false }))
    }

    /// The project a stored `ProjectRef` names in this company file.
    pub fn resolve(&self, reference: &Value) -> Result<i64> {
        self.call("project_resolve", &json!({ "reference": reference }))?
            .as_i64()
            .ok_or_else(|| Error::Refused("The company file did not name a project for that reference.".into()))
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
    }

    #[repr(C)]
    struct CResult {
        status: i32,
        data: *mut u8,
        data_len: usize,
        error: *mut c_char,
    }

    type Create = unsafe extern "C" fn(*const c_char) -> CResult;
    type Destroy = unsafe extern "C" fn(u64) -> CResult;
    type Call = unsafe extern "C" fn(u64, *const c_char, *const c_char) -> CResult;
    type Free = unsafe extern "C" fn(CResult);
    type Version = unsafe extern "C" fn() -> u32;

    /// The interface version this crate speaks.
    const ABI: u32 = 1;

    pub struct Session {
        module: *mut c_void,
        handle: u64,
        destroy: Destroy,
        call: Call,
        free: Free,
    }

    // The library's session registry is thread safe and the handle is a plain number.
    unsafe impl Send for Session {}

    fn symbol(module: *mut c_void, name: &str) -> Result<*mut c_void> {
        let c = CString::new(name).map_err(|_| Error::Unavailable("bad symbol name".into()))?;
        let p = unsafe { GetProcAddress(module, c.as_ptr()) };
        if p.is_null() { Err(Error::Unavailable(format!("office.dll has no `{name}`; it is not a version this Tren can use."))) } else { Ok(p) }
    }

    /// Frees the result and returns its status, data and error text.
    fn take(free: Free, r: CResult) -> (i32, String, String) {
        let data = if r.data.is_null() {
            String::new()
        } else {
            String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(r.data, r.data_len) }).into_owned()
        };
        let error = if r.error.is_null() { String::new() } else { unsafe { CStr::from_ptr(r.error) }.to_string_lossy().into_owned() };
        let status = r.status;
        unsafe { free(r) };
        (status, data, error)
    }

    impl Session {
        pub fn start(library: &Path, app_dir: &Path) -> Result<Session> {
            let wide: Vec<u16> = library.as_os_str().encode_wide().chain(Some(0)).collect();
            let module = unsafe { LoadLibraryW(wide.as_ptr()) };
            if module.is_null() {
                return Err(Error::Unavailable(format!("{} could not be loaded.", library.display())));
            }
            match Self::bind(module, app_dir) {
                Ok(session) => Ok(session),
                Err(e) => {
                    unsafe { FreeLibrary(module) };
                    Err(e)
                }
            }
        }

        fn bind(module: *mut c_void, app_dir: &Path) -> Result<Session> {
            // SAFETY: each pointer is the address of the exported function of that name, whose signature is fixed by interface version 1.
            let version: Version = unsafe { std::mem::transmute::<*mut c_void, Version>(symbol(module, "office_api_version")?) };
            let found = unsafe { version() };
            if found != ABI {
                return Err(Error::Unavailable(format!(
                    "office.dll speaks version {found} of the company file interface; this Tren speaks {ABI}."
                )));
            }
            let create: Create = unsafe { std::mem::transmute::<*mut c_void, Create>(symbol(module, "office_session_create")?) };
            let destroy: Destroy = unsafe { std::mem::transmute::<*mut c_void, Destroy>(symbol(module, "office_session_destroy")?) };
            let call: Call = unsafe { std::mem::transmute::<*mut c_void, Call>(symbol(module, "office_call")?) };
            let free: Free = unsafe { std::mem::transmute::<*mut c_void, Free>(symbol(module, "office_result_free")?) };
            std::fs::create_dir_all(app_dir)
                .map_err(|e| Error::Unavailable(format!("The folder for the company file link could not be made: {e}")))?;
            let dir =
                CString::new(app_dir.to_string_lossy().as_bytes()).map_err(|_| Error::Unavailable("The folder name has a NUL character.".into()))?;
            let (status, data, error) = take(free, unsafe { create(dir.as_ptr()) });
            if status != 0 {
                return Err(Error::Refused(error));
            }
            let handle = serde_json::from_str::<Value>(&data)
                .ok()
                .and_then(|v| v.get("handle").and_then(Value::as_u64))
                .ok_or_else(|| Error::Refused("office.dll gave no session.".into()))?;
            Ok(Session { module, handle, destroy, call, free })
        }

        pub fn call(&self, op: &str, args: &str) -> Result<Value> {
            let op = CString::new(op).map_err(|_| Error::Refused("bad operation name".into()))?;
            let args = CString::new(args).map_err(|_| Error::Refused("the arguments had a NUL character".into()))?;
            let (status, data, error) = take(self.free, unsafe { (self.call)(self.handle, op.as_ptr(), args.as_ptr()) });
            if status != 0 {
                return Err(Error::Refused(if error.is_empty() {
                    format!("The company file refused the request (status {status}).")
                } else {
                    error
                }));
            }
            if data.is_empty() {
                Ok(Value::Null)
            } else {
                serde_json::from_str(&data).map_err(|e| Error::Refused(format!("The company file's answer could not be read: {e}")))
            }
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            let _ = take(self.free, unsafe { (self.destroy)(self.handle) });
            unsafe { FreeLibrary(self.module) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub struct Session;

    impl Session {
        pub fn start(_library: &std::path::Path, _app_dir: &std::path::Path) -> Result<Session> {
            Err(Error::Unavailable("The company file link is available on Windows only.".into()))
        }

        pub fn call(&self, _op: &str, _args: &str) -> Result<Value> {
            Err(Error::Unavailable("The company file link is available on Windows only.".into()))
        }
    }
}
