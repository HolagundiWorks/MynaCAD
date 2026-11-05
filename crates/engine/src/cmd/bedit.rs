//! 2D block editor (BEDIT / BSAVE / BCLOSE) and REFEDIT for xref inserts.
//!
//! Model space is temporarily replaced with the block's entities. BSAVE writes them back into
//! the definition; BCLOSE restores the saved model (optionally after saving).

use std::sync::Arc;

use aad_doc::{Block, Entity, EntityKind, EntityStore, Space};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("bedit", "Block Editor", run_bedit)
            .menu(&["Tools", "Block Editor", "Open"])
            .params("{name | handle? (an INSERT)} → {name, entities} (model space becomes the block definition)"),
        CommandSpec::new("bsave", "Save Block Definition", run_bsave).menu(&["Tools", "Block Editor", "Save"]).params("{} → {name, entities}"),
        CommandSpec::new("bclose", "Close Block Editor", run_bclose)
            .menu(&["Tools", "Block Editor", "Close"])
            .params("{save?: bool (default false)} → {name, saved}"),
        CommandSpec::new("refedit", "Edit Reference", run_refedit)
            .menu(&["Tools", "Edit Reference"])
            .params("{handle? (INSERT)} → opens the block or xref definition in the block editor"),
    ]
}

/// Session state while a block definition is open in model space.
#[derive(Clone, Debug)]
pub struct BlockEdit {
    pub name: String,
    pub saved_model: EntityStore,
    pub saved_selection: Vec<aad_doc::Handle>,
    pub from_xref: bool,
}

fn open_name(s: &mut Session, name: &str, from_xref: bool) -> Result<Value> {
    const CMD: &str = "bedit";
    if s.block_edit.is_some() {
        return Err(bad(CMD, "already editing a block: BSAVE or BCLOSE first"));
    }
    let blk = s.doc()?.block(name).ok_or_else(|| bad(CMD, format!("no block `{name}`")))?.clone();
    if blk.xref_path.is_some() && !from_xref {
        return Err(bad(CMD, format!("`{name}` is an xref: use REFEDIT on its insert")));
    }
    let saved_model = s.doc()?.model.clone();
    let saved_selection = s.selection();
    let copies: Vec<(aad_doc::Common, EntityKind)> = blk.entities.iter().map(|e| (e.common.clone(), e.kind.clone())).collect();
    {
        let d = s.doc_mut()?;
        d.model = EntityStore::new();
        for (common, kind) in copies {
            d.add(&Space::Model, common, kind)?;
        }
    }
    let n = s.doc()?.model.len();
    s.block_edit = Some(BlockEdit { name: blk.name.clone(), saved_model, saved_selection, from_xref });
    s.set_selection(Vec::new());
    Ok(json!({ "name": blk.name, "entities": n, "xref": from_xref }))
}

fn run_bedit(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "bedit";
    let name = if let Some(n) = str_param(p, "name") {
        n.to_string()
    } else {
        let h = targets(s, p)?.into_iter().next().ok_or_else(|| bad(CMD, "give `name` or select an INSERT"))?;
        match &s.doc()?.entity(h).ok_or_else(|| bad(CMD, "no such object"))?.kind {
            EntityKind::Insert(i) => i.block.clone(),
            _ => return Err(bad(CMD, "select a block reference")),
        }
    };
    if name.trim().is_empty() {
        return Err(bad(CMD, "block name is empty"));
    }
    open_name(s, &name, false)
}

fn run_refedit(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "refedit";
    let h = targets(s, p)?.into_iter().next().ok_or_else(|| bad(CMD, "select an INSERT"))?;
    let name = match &s.doc()?.entity(h).ok_or_else(|| bad(CMD, "no such object"))?.kind {
        EntityKind::Insert(i) => i.block.clone(),
        _ => return Err(bad(CMD, "select a block or xref reference")),
    };
    open_name(s, &name, true)
}

fn write_back(s: &mut Session) -> Result<(String, usize)> {
    const CMD: &str = "bsave";
    let edit = s.block_edit.as_ref().ok_or_else(|| bad(CMD, "no block is open in the editor"))?.clone();
    let entities: Vec<Entity> = s.doc()?.model.iter().map(|e| (**e).clone()).collect();
    let n = entities.len();
    let d = s.doc_mut()?;
    let Some(existing) = d.block(&edit.name) else {
        return Err(bad(CMD, format!("block `{}` was removed", edit.name)));
    };
    let mut blk = Block {
        name: existing.name.clone(),
        base: existing.base,
        entities: EntityStore::new(),
        description: existing.description.clone(),
        anonymous: existing.anonymous,
        xref_path: existing.xref_path.clone(),
        explodable: existing.explodable,
        units: existing.units,
        building: existing.building.clone(),
    };
    for e in entities {
        let h = d.new_handle();
        blk.entities.push(Entity { handle: h, common: e.common, kind: e.kind });
    }
    d.blocks.insert(edit.name.clone(), Arc::new(blk));
    Ok((edit.name, n))
}

fn run_bsave(s: &mut Session, _: &Value) -> Result<Value> {
    let (name, n) = write_back(s)?;
    Ok(json!({ "name": name, "entities": n }))
}

fn run_bclose(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "bclose";
    let save = bool_or(p, "save", false);
    let edit = s.block_edit.take().ok_or_else(|| bad(CMD, "no block is open in the editor"))?;
    let mut saved = false;
    if save {
        s.block_edit = Some(edit.clone());
        write_back(s)?;
        let _ = s.block_edit.take();
        saved = true;
    }
    s.doc_mut()?.model = edit.saved_model;
    s.set_selection(edit.saved_selection);
    Ok(json!({ "name": edit.name, "saved": saved }))
}
