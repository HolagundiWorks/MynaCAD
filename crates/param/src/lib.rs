//! The parametric graph of MynaCAD: typed nodes and ports, dimensions and units, a restricted
//! formula language, validation with cycle detection, and incremental evaluation with a cache.
//!
//! This crate only computes. It reads no drawing and writes nothing: geometry it produces (such
//! as a wall's centre line and thickness) is handed back to the host, which commits it through
//! its own commands and undo, and take-off quantities stay with MynaOps.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod diag;
pub mod eval;
pub mod formula;
pub mod graph;
pub mod nodes;
pub mod schema;
pub mod template;
pub mod types;
pub mod units;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Serialize;

pub use diag::{Code, Diagnostic, Severity};
pub use eval::{Cache, Evaluation, NodeResult, NodeStatus, Output, Status, dependencies, evaluate, validate};
pub use graph::{Edge, Editor, Graph, Group, Node, NodeState, SCHEMA_VERSION, UnitsContext};
pub use nodes::{Effect, LibraryEntry, NodeSpec, PortSpec, Source, library};
pub use schema::{ComponentType, component_type, component_types};
pub use types::{DataType, Value, WallValue};
pub use units::Dim;

pub const MAX_NODES: usize = 5_000;
pub const MAX_EDGES: usize = 20_000;
pub const MAX_GROUPS: usize = 1_000;
pub const MAX_ID_LEN: usize = 64;
pub const MAX_EXPR_LEN: usize = 2_000;
pub const MAX_EXPR_DEPTH: usize = 64;
pub const MAX_PATH_POINTS: usize = 10_000;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("{0}")]
    Format(String),
    #[error("{0}")]
    Limit(String),
}

/// What the host knows that nodes may read: the project's levels.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    /// Wall height of each level, in metres.
    pub level_heights: BTreeMap<String, f64>,
    pub default_level: Option<String>,
}

/// 64-bit FNV-1a.
pub fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// JSON text with object keys sorted, so equal values always give equal text (and cache keys).
pub fn canonical(v: &serde_json::Value) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out, 0);
    out
}

fn write_canonical(v: &serde_json::Value, out: &mut String, depth: usize) {
    use serde_json::Value as J;
    if depth > 128 {
        out.push_str("null");
        return;
    }
    match v {
        J::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&J::String((*k).clone()).to_string());
                out.push(':');
                if let Some(x) = m.get(*k) {
                    write_canonical(x, out, depth + 1);
                }
            }
            out.push('}');
        }
        J::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(x, out, depth + 1);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}
