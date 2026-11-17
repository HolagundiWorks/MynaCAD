//! XCLIP (block/xref insert clip). VPCLIP (layout viewport clip) was removed with paper space (docs/layout-book.md 11.1).

use aad_doc::EntityKind;
use aad_geom::Vec2;
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

const MAX_CLIP: usize = 10_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("xclip", "Clip Block Reference", run_xclip).menu(&["Modify", "Clip", "Xref / Block"]).alias(&["xclipframe"]).params(
        "{handles? (INSERT), points?: [[x,y],...] (world; 2 = rectangle corners, 3+ = polygon), \
                 delete?: bool, on?: bool, off?: bool} → {handles, clipping, points}",
    )]
}

fn clip_points(cmd: &str, p: &Value) -> Result<Vec<Vec2>> {
    let pts = points_param(p, "points").ok_or_else(|| bad(cmd, "`points` is required (unless delete/on/off)"))?;
    let pts = distinct_points(&pts);
    if pts.len() < 2 || pts.len() > MAX_CLIP {
        return Err(bad(cmd, format!("`points` needs 2 to {MAX_CLIP} distinct points")));
    }
    if pts.iter().any(|q| !q.is_finite()) {
        return Err(bad(cmd, "`points` must be finite"));
    }
    Ok(pts)
}

fn run_xclip(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "xclip";
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad(CMD, "select one or more block references"));
    }
    let delete = bool_or(p, "delete", false);
    let on = bool_or(p, "on", false);
    let off = bool_or(p, "off", false);
    let pts = if delete || (on || off) && p.get("points").is_none() { None } else { Some(clip_points(CMD, p)?) };
    let mut done = Vec::new();
    for h in hs {
        let is_ins = matches!(s.doc()?.entity(h).map(|e| &e.kind), Some(EntityKind::Insert(_)));
        if !is_ins {
            continue;
        }
        s.doc_mut()?.modify_entity(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                if delete {
                    i.clip.clear();
                    i.clipping = false;
                } else if let Some(ref pts) = pts {
                    i.clip = pts.iter().map(|q| Vec2::new(q.x, q.y)).collect();
                    i.clipping = !off;
                } else if on {
                    i.clipping = !i.clip.is_empty();
                } else if off {
                    i.clipping = false;
                }
                done.push(json!({ "handle": h.hex(), "clipping": i.clipping, "points": i.clip.len() }));
            }
        })?;
    }
    if done.is_empty() {
        return Err(bad(CMD, "no INSERT in the selection"));
    }
    Ok(json!({ "clipped": done }))
}
