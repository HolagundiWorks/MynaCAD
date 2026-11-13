//! View spaces: sections and elevations drafted by hand in 2D, each tied to a marker on a level
//! (design: `docs/levels-and-sheet-sets.md`, "Sections and elevations").
//!
//! A view space has its own entity store. X is the distance along the marker line from its first
//! point; Y is the height in drawing units above datum 0. While a view is the edit target,
//! `Drawing::model` holds its store and the active level's objects wait in the level's slot, so
//! drawing commands keep writing to `model`. `Drawing::switch_target` is still the only place that
//! swaps stores. View ids come from the same counter as level ids (`Drawing::level_seed`).

use aad_geom::Vec2;

use crate::levels::{LevelId, MAX_LEVELS, MAX_TRACES, level_name_ok};
use crate::{DocError, Drawing, EditTarget, EntityStore, Handle, Result};

/// Most view spaces in one drawing.
pub const MAX_VIEW_SPACES: usize = 128;
/// Largest absolute plan coordinate or depth accepted for a view line (hostile input).
pub const VIEW_COORD_MAX: f64 = 1.0e9;

/// Stable view-space identity; shares the id counter with [`LevelId`] and is never reused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct SpaceId(pub u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ViewKind {
    #[default]
    Section,
    Elevation,
}

impl ViewKind {
    pub fn label(self) -> &'static str {
        match self {
            ViewKind::Section => "section",
            ViewKind::Elevation => "elevation",
        }
    }
}

/// Which side of the marker line (first point to second point) the view looks towards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ViewLook {
    #[default]
    Left,
    Right,
}

/// A view space: its saved record plus its objects.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewSpace {
    pub rec: ViewRecord,
    /// The view's objects while it is not the edit target; empty while it is in `Drawing::model`.
    pub entities: EntityStore,
}

/// Everything about a view space except its objects (what a file stores as data).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ViewRecord {
    pub id: SpaceId,
    pub kind: ViewKind,
    /// 1..=64 characters, unique among view spaces (case-insensitive).
    pub name: String,
    /// The level whose plan holds the marker.
    pub level: LevelId,
    /// The marker's drawing handle (the section line, the elevation circle); `None` when it was deleted.
    #[serde(default)]
    pub marker: Option<Handle>,
    /// The marker line in plan coordinates, first point to second point.
    pub line: (Vec2, Vec2),
    #[serde(default)]
    pub look: ViewLook,
    /// How far past the line the view looks, in drawing units (> 0).
    pub depth: f64,
    /// The levels the guides come from; `None` = every level.
    #[serde(default)]
    pub covers: Option<Vec<LevelId>>,
    /// Other view spaces shown faded under this one.
    #[serde(default)]
    pub traces: Vec<SpaceId>,
    /// Last view (centre, height) in this space.
    #[serde(default)]
    pub view: Option<(Vec2, f64)>,
}

impl ViewRecord {
    /// Whether every number and list is usable (also run on file data).
    pub fn is_valid(&self) -> bool {
        level_name_ok(&self.name)
            && line_ok(self.line.0, self.line.1)
            && self.depth.is_finite()
            && self.depth > 0.0
            && self.depth <= VIEW_COORD_MAX
            && self.covers.as_ref().is_none_or(|c| c.len() <= MAX_LEVELS)
            && self.traces.len() <= MAX_TRACES
            && !self.traces.contains(&self.id)
            && self.view.is_none_or(|(c, h)| c.is_finite() && h.is_finite() && h > 0.0)
    }

    /// Unit vector along the line (first point to second point).
    pub fn along(&self) -> Vec2 {
        (self.line.1 - self.line.0).normalized()
    }

    /// Unit vector in the direction the view looks.
    pub fn facing(&self) -> Vec2 {
        let u = self.along();
        let left = Vec2::new(-u.y, u.x);
        match self.look {
            ViewLook::Left => left,
            ViewLook::Right => left * -1.0,
        }
    }

    /// Length of the marker line.
    pub fn length(&self) -> f64 {
        (self.line.1 - self.line.0).len()
    }
}

fn line_ok(a: Vec2, b: Vec2) -> bool {
    a.is_finite()
        && b.is_finite()
        && a.x.abs() <= VIEW_COORD_MAX
        && a.y.abs() <= VIEW_COORD_MAX
        && b.x.abs() <= VIEW_COORD_MAX
        && b.y.abs() <= VIEW_COORD_MAX
        && !a.near(b, 1e-9)
}

fn invalid(msg: impl Into<String>) -> DocError {
    DocError::Invalid(msg.into())
}

impl Drawing {
    /// The view space with this id.
    pub fn view_space(&self, id: SpaceId) -> Option<&ViewSpace> {
        self.view_spaces.iter().find(|v| v.rec.id == id)
    }

    /// The view space with this name (case-insensitive).
    pub fn view_space_by_name(&self, name: &str) -> Option<&ViewSpace> {
        self.view_spaces.iter().find(|v| v.rec.name.eq_ignore_ascii_case(name.trim()))
    }

    /// The view space being edited, if any.
    pub fn active_view_ref(&self) -> Option<&ViewSpace> {
        self.active_view.and_then(|id| self.view_space(id))
    }

    /// The objects of a view space, whether or not it is the edit target.
    pub fn view_store(&self, id: SpaceId) -> Option<&EntityStore> {
        let v = self.view_space(id)?;
        Some(if self.active_view == Some(id) { &self.model } else { &v.entities })
    }

    fn check_view_name(&self, name: &str, except: Option<SpaceId>) -> Result<()> {
        if !level_name_ok(name) {
            return Err(invalid("a view name is 1 to 64 characters without control characters"));
        }
        if self.view_spaces.iter().any(|v| Some(v.rec.id) != except && v.rec.name.eq_ignore_ascii_case(name.trim())) {
            return Err(invalid(format!("there is already a view named `{}`", name.trim())));
        }
        Ok(())
    }

    /// Add a view space tied to `level`. The drawing must already have explicit levels.
    pub fn add_view_space(
        &mut self,
        kind: ViewKind,
        name: &str,
        level: LevelId,
        line: (Vec2, Vec2),
        look: ViewLook,
        depth: f64,
        marker: Option<Handle>,
    ) -> Result<SpaceId> {
        if self.levels.is_empty() {
            return Err(invalid("the drawing has no explicit levels yet"));
        }
        if self.view_spaces.len() >= MAX_VIEW_SPACES {
            return Err(invalid(format!("at most {MAX_VIEW_SPACES} sections and elevations")));
        }
        if self.level(level).is_none() {
            return Err(invalid("no such level"));
        }
        self.check_view_name(name, None)?;
        if !line_ok(line.0, line.1) {
            return Err(invalid("the marker line must have two different finite points"));
        }
        if !(depth.is_finite() && depth > 0.0 && depth <= VIEW_COORD_MAX) {
            return Err(invalid("the depth must be a positive finite number"));
        }
        self.level_seed = self.level_seed.saturating_add(1);
        let id = SpaceId(self.level_seed);
        self.view_spaces.push(ViewSpace {
            rec: ViewRecord {
                id,
                kind,
                name: name.trim().to_string(),
                level,
                marker,
                line,
                look,
                depth,
                covers: None,
                traces: Vec::new(),
                view: None,
            },
            entities: EntityStore::new(),
        });
        Ok(id)
    }

    /// Change a view's record after checking the whole result (the id never changes).
    pub fn update_view_record(&mut self, rec: ViewRecord) -> Result<()> {
        if !rec.is_valid() {
            return Err(invalid("the view settings are not valid"));
        }
        self.check_view_name(&rec.name, Some(rec.id))?;
        if self.level(rec.level).is_none() {
            return Err(invalid("no such level"));
        }
        if rec.covers.as_ref().is_some_and(|c| c.iter().any(|l| self.level(*l).is_none())) {
            return Err(invalid("a covered level does not exist"));
        }
        if rec.traces.iter().any(|t| self.view_space(*t).is_none()) {
            return Err(invalid("a traced view does not exist"));
        }
        let v = self.view_spaces.iter_mut().find(|v| v.rec.id == rec.id).ok_or_else(|| invalid("no such view"))?;
        v.rec = ViewRecord { name: rec.name.trim().to_string(), ..rec };
        Ok(())
    }

    /// Make a view space the one being edited: the current store goes back to its slot.
    pub fn enter_view_space(&mut self, id: SpaceId) -> Result<()> {
        self.switch_target(EditTarget::View(id))
    }

    /// Back to the level that was active when the view was entered (design mode).
    pub fn leave_view_space(&mut self) -> Result<()> {
        match (self.active_view, self.active_level) {
            (Some(_), Some(level)) => self.switch_target(EditTarget::Level(level)),
            _ => Ok(()),
        }
    }

    /// Remove a view space and its objects, and every reference to it. Leaves it first when it is
    /// the one being edited.
    pub fn remove_view_space(&mut self, id: SpaceId) -> Result<()> {
        if self.view_space(id).is_none() {
            return Err(invalid("no such view"));
        }
        if self.active_view == Some(id) {
            self.leave_view_space()?;
        }
        self.view_spaces.retain(|v| v.rec.id != id);
        for v in &mut self.view_spaces {
            v.rec.traces.retain(|t| *t != id);
        }
        Ok(())
    }

    /// Drop everything that names `level` (called when a level is removed).
    pub(crate) fn drop_level_from_views(&mut self, level: LevelId) -> Result<()> {
        let gone: Vec<SpaceId> = self.view_spaces.iter().filter(|v| v.rec.level == level).map(|v| v.rec.id).collect();
        if self.active_view.is_some_and(|a| gone.contains(&a)) {
            self.leave_view_space()?;
        }
        self.view_spaces.retain(|v| v.rec.level != level);
        for v in &mut self.view_spaces {
            v.rec.traces.retain(|t| !gone.contains(t));
            if let Some(c) = &mut v.rec.covers {
                c.retain(|l| *l != level);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Common, Entity, EntityKind, Line};
    use aad_geom::Vec3;

    fn two_levels() -> (Drawing, LevelId, LevelId) {
        let mut d = Drawing::new_metric();
        let g = d.materialize_levels("Ground", 0.0, 3000.0).unwrap();
        let f = d.add_level("First", 3000.0, 3000.0, None).unwrap();
        (d, g, f)
    }

    fn line(d: &mut Drawing, y: f64) -> Handle {
        let h = d.new_handle();
        d.model.push(Entity {
            handle: h,
            common: Common::default(),
            kind: EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(10.0, y, 0.0) }),
        });
        h
    }

    fn add(d: &mut Drawing, name: &str, level: LevelId) -> Result<SpaceId> {
        d.add_view_space(ViewKind::Section, name, level, (Vec2::new(0.0, 0.0), Vec2::new(10_000.0, 0.0)), ViewLook::Left, 5000.0, None)
    }

    #[test]
    fn a_view_store_swaps_in_and_out_and_keeps_the_level_objects() {
        let (mut d, g, _f) = two_levels();
        let on_ground = line(&mut d, 1.0);
        let v = add(&mut d, "Section A-A", g).unwrap();
        d.enter_view_space(v).unwrap();
        assert_eq!(d.target(), Some(EditTarget::View(v)));
        assert!(d.model.is_empty(), "the view starts empty");
        let drafted = line(&mut d, 2.0);
        assert!(d.level_store(g).unwrap().contains(on_ground), "the level's objects wait in its slot");
        assert_eq!(d.view_store(v).unwrap().len(), 1);
        assert_eq!(d.entity_count(), 2, "drawing-wide counts see both");
        assert!(d.entity(drafted).is_some() && d.entity(on_ground).is_some());
        // Switching level from a view leaves the view and parks its objects.
        let f = d.level_by_name("First").unwrap().id;
        d.switch_level(f).unwrap();
        assert_eq!(d.active_view, None);
        assert!(d.view_store(v).unwrap().contains(drafted));
        assert!(d.level_store(g).unwrap().contains(on_ground));
        d.enter_view_space(v).unwrap();
        d.leave_view_space().unwrap();
        assert_eq!(d.target(), Some(EditTarget::Level(f)));
    }

    #[test]
    fn names_ids_and_limits_are_checked() {
        let (mut d, g, _) = two_levels();
        let a = add(&mut d, "A", g).unwrap();
        assert!(add(&mut d, "a", g).is_err(), "names are unique, case ignored");
        assert!(add(&mut d, "", g).is_err());
        assert!(add(&mut d, "x", LevelId(999)).is_err());
        let bad =
            |d: &mut Drawing, line: (Vec2, Vec2), depth: f64| d.add_view_space(ViewKind::Section, "Z", g, line, ViewLook::Left, depth, None).is_err();
        assert!(bad(&mut d, (Vec2::ZERO, Vec2::ZERO), 1.0));
        assert!(bad(&mut d, (Vec2::ZERO, Vec2::new(f64::NAN, 1.0)), 1.0));
        assert!(bad(&mut d, (Vec2::ZERO, Vec2::new(1.0, 1.0)), 0.0));
        assert!(bad(&mut d, (Vec2::ZERO, Vec2::new(1.0, 1.0)), f64::INFINITY));
        assert!(bad(&mut d, (Vec2::ZERO, Vec2::new(1e12, 1.0)), 1.0));
        let b = add(&mut d, "B", g).unwrap();
        assert!(b > a, "ids are handed out in order and share the level counter");
        d.remove_view_space(b).unwrap();
        let c = add(&mut d, "C", g).unwrap();
        assert!(c > b, "ids are never reused");
        for i in 0..MAX_VIEW_SPACES {
            if add(&mut d, &format!("v{i}"), g).is_err() {
                break;
            }
        }
        assert_eq!(d.view_spaces.len(), MAX_VIEW_SPACES);
        assert!(add(&mut d, "one too many", g).is_err());
    }

    #[test]
    fn removing_a_level_removes_its_views_even_the_one_being_edited() {
        let (mut d, g, f) = two_levels();
        let on_first = add(&mut d, "On first", f).unwrap();
        let on_ground = add(&mut d, "On ground", g).unwrap();
        d.enter_view_space(on_first).unwrap();
        line(&mut d, 3.0);
        d.remove_level(f).unwrap();
        assert!(d.view_space(on_first).is_none() && d.view_space(on_ground).is_some());
        assert_eq!(d.active_view, None);
        assert_eq!(d.target(), Some(EditTarget::Level(g)));
        assert_eq!(d.entity_count(), 0);
    }

    #[test]
    fn deleting_a_view_cleans_traces_and_the_edit_target() {
        let (mut d, g, _) = two_levels();
        let a = add(&mut d, "A", g).unwrap();
        let b = add(&mut d, "B", g).unwrap();
        let mut rec = d.view_space(a).unwrap().rec.clone();
        rec.traces = vec![b];
        d.update_view_record(rec.clone()).unwrap();
        rec.traces = vec![a];
        assert!(d.update_view_record(rec).is_err(), "a view cannot trace itself");
        d.enter_view_space(b).unwrap();
        d.remove_view_space(b).unwrap();
        assert!(d.view_space(a).unwrap().rec.traces.is_empty());
        assert_eq!(d.active_view, None);
        assert_eq!(d.target(), Some(EditTarget::Level(g)));
    }
}
