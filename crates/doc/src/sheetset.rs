//! Sheet sets: named disciplines (Working drawing, Electrical, Plumbing, ...) that add annotation
//! on top of levels (design: `docs/levels-and-sheet-sets.md`).
//!
//! A sheet set keeps one annotation store per level. In a *sheet view* (a sheet set and a level)
//! `Drawing::model` holds that annotation store, so every drawing command writes annotation, and
//! the level's own objects wait in the level's slot as a read-only reference. Design mode (no
//! sheet set) keeps the level's objects in `model` as before. `Drawing::switch_target` is the only
//! place that swaps stores.

use std::collections::BTreeMap;

use crate::levels::{LevelId, level_name_ok};
use crate::{DocError, Drawing, EntityStore, Result};

/// Most sheet sets in one drawing.
pub const MAX_SHEET_SETS: usize = 32;
/// Longest layer prefix of a sheet set, in characters.
pub const MAX_LAYER_PREFIX: usize = 8;

/// Stable sheet-set identity: never reused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct SheetSetId(pub u32);

/// Whose objects are in `Drawing::model`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditTarget {
    /// A level's design objects.
    Level(LevelId),
    /// A sheet set's annotation on a level.
    Sheet(SheetSetId, LevelId),
    /// A section or elevation's own objects.
    View(crate::SpaceId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SheetSet {
    pub id: SheetSetId,
    /// 1..=64 characters, unique (case-insensitive).
    pub name: String,
    /// New annotation goes on layers that start with this (`E-` gives `E-DIM`, `E-TEXT`, ...).
    pub layer_prefix: String,
    /// The drawing space (saved layer state) that decides which level layers show, if any.
    pub space: Option<String>,
    pub dim_style: Option<String>,
    pub text_style: Option<String>,
    /// Layers that belong to this sheet set, as name patterns (`*` matches any run of characters,
    /// case ignored); the layer prefix counts as one more pattern. The Layers panel shows only
    /// these while the set is the sheet view.
    pub layers: Vec<String>,
    /// Blocks that belong to this sheet set, as name patterns; the Blocks panel shows only these.
    pub blocks: Vec<String>,
    /// Annotation per level while it is not the one in `Drawing::model`.
    pub annotations: BTreeMap<LevelId, EntityStore>,
}

fn invalid(msg: impl Into<String>) -> DocError {
    DocError::Invalid(msg.into())
}

/// How many layer (or block) patterns one sheet set maps, and how long a pattern is.
pub const MAX_MAP_PATTERNS: usize = 64;
pub const MAX_PATTERN_LEN: usize = 64;

/// Whether `p` is usable as a mapping pattern.
pub fn pattern_ok(p: &str) -> bool {
    !p.is_empty() && p.chars().count() <= MAX_PATTERN_LEN && !p.chars().any(char::is_control)
}

/// Case-insensitive match of `name` against `pattern`, where `*` matches any run of characters.
/// Linear in the product of the lengths at worst; both are capped.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().flat_map(char::to_lowercase).take(256).collect();
    let n: Vec<char> = name.chars().flat_map(char::to_lowercase).take(1024).collect();
    let (mut pi, mut ni) = (0usize, 0usize);
    // After a `*`: where the pattern resumes and the name position to retry from.
    let mut retry: Option<(usize, usize)> = None;
    while ni < n.len() {
        match p.get(pi) {
            Some('*') => {
                pi += 1;
                retry = Some((pi, ni));
            }
            Some(c) if Some(c) == n.get(ni) => {
                pi += 1;
                ni += 1;
            }
            _ => match retry {
                Some((rp, rn)) => {
                    pi = rp;
                    ni = rn + 1;
                    retry = Some((rp, rn + 1));
                }
                None => return false,
            },
        }
    }
    p.get(pi..).is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

impl SheetSet {
    /// Whether the layer belongs to this sheet set: it starts with the layer prefix or matches a
    /// mapped pattern.
    pub fn maps_layer(&self, name: &str) -> bool {
        let prefixed = !self.layer_prefix.is_empty() && name.to_ascii_uppercase().starts_with(&self.layer_prefix.to_ascii_uppercase());
        prefixed || self.layers.iter().any(|p| glob_match(p, name))
    }

    /// Whether the block belongs to this sheet set (a mapped pattern matches its name).
    pub fn maps_block(&self, name: &str) -> bool {
        self.blocks.iter().any(|p| glob_match(p, name))
    }
}

/// Whether `prefix` is usable as a layer prefix (empty is allowed).
pub fn layer_prefix_ok(prefix: &str) -> bool {
    prefix.chars().count() <= MAX_LAYER_PREFIX && !prefix.chars().any(|c| c.is_control() || c.is_whitespace())
}

impl Drawing {
    pub fn sheet_set(&self, id: SheetSetId) -> Option<&SheetSet> {
        self.sheet_sets.iter().find(|s| s.id == id)
    }

    pub fn sheet_set_by_name(&self, name: &str) -> Option<&SheetSet> {
        self.sheet_sets.iter().find(|s| s.name.eq_ignore_ascii_case(name.trim()))
    }

    /// The sheet set of the sheet view being edited, if any.
    pub fn active_sheet_ref(&self) -> Option<&SheetSet> {
        self.active_sheet.and_then(|id| self.sheet_set(id))
    }

    /// Whose objects are in `model` now (`None` while the drawing has no explicit levels).
    pub fn target(&self) -> Option<EditTarget> {
        let level = self.active_level?;
        if let Some(v) = self.active_view {
            return Some(EditTarget::View(v));
        }
        Some(match self.active_sheet {
            Some(s) => EditTarget::Sheet(s, level),
            None => EditTarget::Level(level),
        })
    }

    /// The level's own objects while a sheet view shows them for reference (read-only).
    pub fn reference_store(&self) -> Option<&EntityStore> {
        self.active_sheet?;
        self.level(self.active_level?).map(|l| &l.entities)
    }

    /// A sheet set's annotation of a level, wherever it is stored now.
    pub fn annotation_store(&self, set: SheetSetId, level: LevelId) -> Option<&EntityStore> {
        let s = self.sheet_set(set)?;
        if self.target() == Some(EditTarget::Sheet(set, level)) {
            return Some(&self.model);
        }
        s.annotations.get(&level)
    }

    fn slot_mut(&mut self, t: EditTarget) -> Option<&mut EntityStore> {
        match t {
            EditTarget::Level(id) => self.levels.iter_mut().find(|l| l.id == id).map(|l| &mut l.entities),
            EditTarget::Sheet(set, level) => self.sheet_sets.iter_mut().find(|s| s.id == set).map(|s| s.annotations.entry(level).or_default()),
            EditTarget::View(id) => self.view_spaces.iter_mut().find(|v| v.rec.id == id).map(|v| &mut v.entities),
        }
    }

    /// Make `to` the store being edited: the current store goes back to its slot and the new one
    /// becomes `model`.
    pub fn switch_target(&mut self, to: EditTarget) -> Result<()> {
        // A view keeps the active level (the one to return to); it never has a sheet view.
        let (set, level, view) = match to {
            EditTarget::Level(l) => (None, l, None),
            EditTarget::Sheet(s, l) => (Some(s), l, None),
            EditTarget::View(v) => {
                if self.view_space(v).is_none() {
                    return Err(invalid("no such view"));
                }
                (None, self.active_level.ok_or_else(|| invalid("the drawing has no explicit levels yet"))?, Some(v))
            }
        };
        if self.level(level).is_none() {
            return Err(invalid("no such level"));
        }
        if set.is_some_and(|s| self.sheet_set(s).is_none()) {
            return Err(invalid("no such sheet set"));
        }
        if self.target() == Some(to) {
            return Ok(());
        }
        if let Some(cur) = self.target() {
            let parked = std::mem::take(&mut self.model);
            match self.slot_mut(cur) {
                Some(slot) => *slot = parked,
                None => {
                    self.model = parked;
                    return Err(invalid("the store being edited has no slot"));
                }
            }
        }
        self.model = self.slot_mut(to).map(std::mem::take).unwrap_or_default();
        self.active_level = Some(level);
        self.active_sheet = set;
        self.active_view = view;
        Ok(())
    }

    /// Add a sheet set; returns its id.
    pub fn add_sheet_set(&mut self, name: &str, layer_prefix: &str) -> Result<SheetSetId> {
        if self.sheet_sets.len() >= MAX_SHEET_SETS {
            return Err(invalid(format!("at most {MAX_SHEET_SETS} sheet sets")));
        }
        self.check_sheet_name(name, None)?;
        if !layer_prefix_ok(layer_prefix) {
            return Err(invalid(format!("a layer prefix is at most {MAX_LAYER_PREFIX} characters without spaces")));
        }
        self.sheet_seed = self.sheet_seed.saturating_add(1);
        let id = SheetSetId(self.sheet_seed);
        self.sheet_sets.push(SheetSet {
            id,
            name: name.trim().to_string(),
            layer_prefix: layer_prefix.to_string(),
            space: None,
            dim_style: None,
            text_style: None,
            layers: Vec::new(),
            blocks: Vec::new(),
            annotations: BTreeMap::new(),
        });
        Ok(id)
    }

    fn check_sheet_name(&self, name: &str, except: Option<SheetSetId>) -> Result<()> {
        if !level_name_ok(name) {
            return Err(invalid("a sheet set name is 1 to 64 characters without control characters"));
        }
        if self.sheet_sets.iter().any(|s| Some(s.id) != except && s.name.eq_ignore_ascii_case(name.trim())) {
            return Err(invalid(format!("there is already a sheet set named `{}`", name.trim())));
        }
        Ok(())
    }

    /// Rename a sheet set.
    pub fn rename_sheet_set(&mut self, id: SheetSetId, name: &str) -> Result<()> {
        self.check_sheet_name(name, Some(id))?;
        let s = self.sheet_sets.iter_mut().find(|s| s.id == id).ok_or_else(|| invalid("no such sheet set"))?;
        s.name = name.trim().to_string();
        Ok(())
    }

    /// Remove a sheet set and all its annotation. Leaves the sheet view first when it is the active one.
    pub fn remove_sheet_set(&mut self, id: SheetSetId) -> Result<()> {
        if self.sheet_set(id).is_none() {
            return Err(invalid("no such sheet set"));
        }
        if self.active_sheet == Some(id) {
            self.leave_sheet_view()?;
        }
        self.sheet_sets.retain(|s| s.id != id);
        Ok(())
    }

    /// Show `id` for the active level: its annotation becomes the store being edited and the level
    /// becomes a read-only reference.
    pub fn enter_sheet_view(&mut self, id: SheetSetId) -> Result<()> {
        let level = self.active_level.ok_or_else(|| invalid("the drawing has no explicit levels yet"))?;
        self.switch_target(EditTarget::Sheet(id, level))
    }

    /// Back to design mode on the active level.
    pub fn leave_sheet_view(&mut self) -> Result<()> {
        match self.active_level {
            Some(level) if self.active_sheet.is_some() => self.switch_target(EditTarget::Level(level)),
            _ => Ok(()),
        }
    }
}
