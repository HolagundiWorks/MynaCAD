//! The session broker client. Kivo serves the named pipe `osas-session-broker`; the contract is Kivo `docs/contracts/session-broker.md`.
//!
//! Tren never holds a company password. It asks Kivo for a session token for one purpose, keeps the token in memory and
//! presents it to Myna's database service.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The pipe Kivo creates (current user only).
pub const PIPE_PATH: &str = r"\\.\pipe\osas-session-broker";

/// One request, one reply, both UTF-8 JSON. A trait so the protocol can be tested without a pipe.
pub trait Transport: Send + Sync {
    fn request(&self, message: &str) -> std::io::Result<String>;
}

/// The real pipe: a message-mode pipe opened like a file; one write is one message.
pub struct PipeTransport;

impl Transport for PipeTransport {
    fn request(&self, message: &str) -> std::io::Result<String> {
        let mut pipe = std::fs::OpenOptions::new().read(true).write(true).open(PIPE_PATH)?;
        pipe.write_all(message.as_bytes())?;
        pipe.flush()?;
        let mut buf = vec![0u8; 64 * 1024];
        let n = pipe.read(&mut buf)?;
        buf.truncate(n);
        String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

/// What a person sees when Kivo cannot give a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrokerError {
    /// The pipe is not there or Kivo did not answer.
    Unavailable(String),
    /// Kivo is not signed in (and, if the sign-in was asked for, it was not completed).
    SignedOut,
    /// The person closed the sign-in.
    Cancelled,
    /// Kivo refused: the user's role does not allow this purpose.
    Denied(String),
    /// An answer this client does not understand.
    Protocol(String),
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrokerError::Unavailable(m) => write!(f, "Kivo is not available: {m}"),
            BrokerError::SignedOut => f.write_str("Kivo is not signed in"),
            BrokerError::Cancelled => f.write_str("the sign-in was cancelled"),
            BrokerError::Denied(m) => write!(f, "Kivo refused: {m}"),
            BrokerError::Protocol(m) => write!(f, "unexpected answer from Kivo: {m}"),
        }
    }
}

impl std::error::Error for BrokerError {}

/// A session handed out by Kivo.
#[derive(Clone, PartialEq, Eq)]
pub struct Session {
    pub token: String,
    pub expires_utc: String,
    pub user: String,
}

// The token is a credential: never print it.
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("token", &"<hidden>").field("user", &self.user).field("expires_utc", &self.expires_utc).finish()
    }
}

fn call(t: &dyn Transport, request: &Value) -> Result<Value, BrokerError> {
    let text = t.request(&request.to_string()).map_err(|e| BrokerError::Unavailable(e.to_string()))?;
    serde_json::from_str::<Value>(&text).map_err(|e| BrokerError::Protocol(e.to_string()))
}

fn error_of(reply: &Value) -> BrokerError {
    let code = reply.get("error").and_then(Value::as_str).unwrap_or("");
    let message = reply.get("message").and_then(Value::as_str).unwrap_or("").to_string();
    match code {
        "signed_out" => BrokerError::SignedOut,
        "cancelled" => BrokerError::Cancelled,
        "denied" => BrokerError::Denied(message),
        "" => BrokerError::Protocol("no error code".into()),
        other => BrokerError::Protocol(format!("{other}: {message}")),
    }
}

/// Never opens a window: is Kivo there, and is anyone signed in?
pub fn status(t: &dyn Transport) -> Result<bool, BrokerError> {
    let reply = call(t, &json!({ "v": 1, "op": "status" }))?;
    if reply.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(reply.get("signed_in").and_then(Value::as_bool).unwrap_or(false))
    } else {
        Err(error_of(&reply))
    }
}

/// Ask for a session for `purpose` (`db_connect`, `sync`, ...). With `interactive` Kivo shows its sign-in when nobody is signed in.
pub fn ask_session(t: &dyn Transport, app: &str, purpose: &str, project_id: Option<&str>, interactive: bool) -> Result<Session, BrokerError> {
    let mut request = json!({ "v": 1, "op": "ask_session", "app": app, "purpose": purpose, "interactive": interactive });
    if let (Some(id), Some(object)) = (project_id, request.as_object_mut()) {
        object.insert("project_id".into(), Value::String(id.to_string()));
    }
    let reply = call(t, &request)?;
    if reply.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(error_of(&reply));
    }
    let text = |key: &str| reply.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let token = text("token");
    if token.is_empty() {
        return Err(BrokerError::Protocol("no token in the answer".into()));
    }
    Ok(Session { token, expires_utc: text("expires_utc"), user: text("user") })
}

/// Wait until Kivo answers, starting it (hidden, `--broker`) when the pipe is not there. Gives up after `wait`.
pub fn ensure_broker(t: &dyn Transport, launch: &dyn Fn() -> std::io::Result<()>, wait: Duration) -> Result<(), BrokerError> {
    if status(t).is_ok() {
        return Ok(());
    }
    launch().map_err(|e| BrokerError::Unavailable(format!("could not start Kivo: {e}")))?;
    let started = Instant::now();
    loop {
        match status(t) {
            Ok(_) => return Ok(()),
            Err(e) if started.elapsed() >= wait => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    }
}

/// Start Kivo without a window: `O-Studio.exe --broker` from the usual install folder (or the folder in `OSTUDIO_KIVO_EXE`).
pub fn launch_kivo() -> std::io::Result<()> {
    let exe = std::env::var_os("OSTUDIO_KIVO_EXE").map(std::path::PathBuf::from).or_else(|| {
        let local = std::env::var_os("LOCALAPPDATA")?;
        Some(std::path::PathBuf::from(local).join("Programs").join("O-Studio").join("O-Studio.exe"))
    });
    let exe = exe.filter(|p| p.is_file()).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Kivo is not installed"))?;
    std::process::Command::new(exe).arg("--broker").spawn().map(|_| ())
}
