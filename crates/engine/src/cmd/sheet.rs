//! Title-block field helpers kept for the revision register (`revision.issue` writes the revision
//! code into the title blocks of layouts that a foreign drawing brought along).
//!
//! The sheet commands that lived here (`sheet.titleblock`, `sheet.designer`, `sheet.fields`,
//! `sheet.template.*`, `viewport.list|add|remove`, `sheet.index`, `sheet.plotwindow`) were removed
//! with paper space (`docs/layout-book.md` 11.1). Sheets are made in MynaOps.

use std::collections::BTreeMap;

use aad_doc::{Drawing, EntityKind};

/// The block name of the standard title block.
pub const TITLE_BLOCK: &str = "AAD-TITLEBLOCK";
/// Layout names of the old landscape / portrait nameplates and the legacy single title plate.
/// Revision commands skip them when they pick the layouts of a drawing.
pub const TITLE_PLATE_L: &str = "Nameplate L";
pub const TITLE_PLATE_P: &str = "Nameplate P";
pub const TITLE_PLATE: &str = "Title plate";

/// The fields of the standard title block, in the order the panel shows them.
pub const FIELDS: &[(&str, &str)] = &[
    ("PROJECT", "Project"),
    ("CLIENT", "Client"),
    ("TITLE", "Drawing title"),
    ("FLOOR", "Floor"),
    ("DRAWN", "Drawn by"),
    ("DATE", "Date"),
    ("SHEET", "Sheet no."),
    ("SCALE", "Scale"),
    ("REV", "Revision"),
    ("CHECKED", "Checked by"),
    ("APPROVED", "Approved by"),
    ("DISCIPLINE", "Discipline"),
    ("STATUS", "Status"),
    ("ISSUE", "Issue"),
];

/// The handles of the title block references (or any block with title fields) in `layout`.
pub(crate) fn title_inserts(d: &Drawing, layout: &str) -> Vec<aad_doc::Handle> {
    let Some(l) = d.layouts.iter().find(|l| l.name == layout) else { return Vec::new() };
    l.entities
        .iter()
        .filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(TITLE_BLOCK) || i.attribs.iter().any(|a| FIELDS.iter().any(|(t, _)| a.tag.eq_ignore_ascii_case(t)))))
        .map(|e| e.handle)
        .collect()
}

/// Write title-block attribute values on `layout`. Used by `sheet.fields`.
pub(crate) fn write_fields(d: &mut Drawing, layout: &str, fields: &BTreeMap<String, String>) -> usize {
    let mut changed = 0;
    for h in title_inserts(d, layout) {
        let Some(l) = d.layouts.iter_mut().find(|l| l.name == layout) else { continue };
        l.entities.modify(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                for a in &mut i.attribs {
                    if let Some(v) = fields.get(&a.tag.to_ascii_uppercase())
                        && a.text.value != *v
                    {
                        a.text.value = v.clone();
                        changed += 1;
                    }
                }
            }
        });
    }
    changed
}
