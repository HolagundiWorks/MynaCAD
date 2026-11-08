//! The database service client. Myna serves loopback HTTP on `127.0.0.1:8743`; the contract is Kivo
//! `docs/contracts/session-broker.md` section 2. Every call except `health` carries the session token from Kivo.

use std::time::Duration;

use serde_json::{Value, json};

pub const DEFAULT_URL: &str = "http://127.0.0.1:8743";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceError {
    /// Nothing is listening: Myna is not running.
    Unreachable(String),
    /// The token was refused (401): ask Kivo again.
    Unauthorized,
    /// 409: the remote moved since this revision.
    Conflict,
    /// Any other answer from the service.
    Http(u16, String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceError::Unreachable(m) => write!(f, "the database service is not running ({m})"),
            ServiceError::Unauthorized => f.write_str("the session was refused"),
            ServiceError::Conflict => f.write_str("the shared copy has a newer revision"),
            ServiceError::Http(code, m) => write!(f, "the database service answered {code}: {m}"),
        }
    }
}

impl std::error::Error for ServiceError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Health {
    pub state: String,
    pub sync: String,
    pub pending: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connected {
    pub remote_revision: Option<i64>,
    /// `none`, `push`, `pull` or `conflict`.
    pub action: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectStatus {
    pub local_revision: Option<i64>,
    pub remote_revision: Option<i64>,
    pub state: String,
    pub pending: i64,
}

pub struct Service {
    base: String,
    agent: ureq::Agent,
}

impl Service {
    pub fn new(base: &str) -> Self {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).build();
        Service { base: base.trim_end_matches('/').to_string(), agent }
    }

    /// The service address: `OSAS_DB_URL` (set by Myna for the apps it starts) or the default.
    pub fn from_env() -> Self {
        Service::new(&std::env::var("OSAS_DB_URL").unwrap_or_else(|_| DEFAULT_URL.to_string()))
    }

    fn finish(result: Result<ureq::Response, ureq::Error>) -> Result<Value, ServiceError> {
        match result {
            Ok(response) => response.into_json::<Value>().map_err(|e| ServiceError::Http(200, e.to_string())),
            Err(ureq::Error::Status(401, _)) => Err(ServiceError::Unauthorized),
            Err(ureq::Error::Status(409, _)) => Err(ServiceError::Conflict),
            Err(ureq::Error::Status(code, response)) => {
                let message = response.into_json::<Value>().ok().and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string)).unwrap_or_default();
                Err(ServiceError::Http(code, message))
            }
            Err(ureq::Error::Transport(t)) => Err(ServiceError::Unreachable(t.to_string())),
        }
    }

    fn get(&self, path: &str, token: Option<&str>) -> Result<Value, ServiceError> {
        let mut request = self.agent.get(&format!("{}{path}", self.base));
        if let Some(t) = token {
            request = request.set("Authorization", &format!("Bearer {t}"));
        }
        Self::finish(request.call())
    }

    fn post(&self, path: &str, token: &str, body: &Value) -> Result<Value, ServiceError> {
        Self::finish(self.agent.post(&format!("{}{path}", self.base)).set("Authorization", &format!("Bearer {token}")).send_json(body.clone()))
    }

    pub fn health(&self) -> Result<Health, ServiceError> {
        let v = self.get("/v1/health", None)?;
        Ok(Health {
            state: v.get("state").and_then(Value::as_str).unwrap_or("").to_string(),
            sync: v.get("sync").and_then(Value::as_str).unwrap_or("").to_string(),
            pending: v.get("pending").and_then(Value::as_i64).unwrap_or(0),
        })
    }

    pub fn connect(&self, token: &str, project_id: &str, name: &str, osp_path: &str, revision: i64) -> Result<Connected, ServiceError> {
        let v = self.post("/v1/projects/connect", token, &json!({ "project_id": project_id, "name": name, "osp_path": osp_path, "revision": revision }))?;
        Ok(Connected { remote_revision: v.get("remote_revision").and_then(Value::as_i64), action: v.get("action").and_then(Value::as_str).unwrap_or("none").to_string() })
    }

    pub fn project(&self, token: &str, project_id: &str) -> Result<ProjectStatus, ServiceError> {
        let v = self.get(&format!("/v1/projects/{project_id}"), Some(token))?;
        Ok(ProjectStatus {
            local_revision: v.get("local_revision").and_then(Value::as_i64),
            remote_revision: v.get("remote_revision").and_then(Value::as_i64),
            state: v.get("state").and_then(Value::as_str).unwrap_or("").to_string(),
            pending: v.get("pending").and_then(Value::as_i64).unwrap_or(0),
        })
    }

    /// Send the saved revision. The service reads the file from the path registered at connect. `None` means the service keeps project
    /// files on this PC (`local_only`): there is nothing to send and the local file is the shared database.
    pub fn push(&self, token: &str, project_id: &str, revision: i64) -> Result<Option<i64>, ServiceError> {
        let v = self.post(&format!("/v1/projects/{project_id}/push"), token, &json!({ "revision": revision }))?;
        if v.get("local_only").and_then(Value::as_bool) == Some(true) {
            return Ok(None);
        }
        Ok(Some(v.get("remote_revision").and_then(Value::as_i64).unwrap_or(revision)))
    }
}
