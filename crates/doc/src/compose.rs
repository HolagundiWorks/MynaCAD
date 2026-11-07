//! Compose Views, Master Layouts and Publisher Sets (ArchiCAD-style documentation pipeline).
//!
//! - **Compose View** — named display recipe: which level, optional sheet set, scale, plot style,
//!   traces and optional model window. Distinct from DXF `VIEW` ([`crate::NamedView`]) and from
//!   Phase-6 section/elevation view spaces.
//! - **Master Layout** — shared paper size, orientation and title-block / sheet-template chrome
//!   applied to many layouts.
//! - **Publisher Set** — named list of layouts (or compose views) with output format and folder,
//!   for one-click republish.
//!
//! Persisted under project key [`COMPOSE_KEY`] (`compose1`) so DXF/DWG round-trip with `AAD_PROJECT`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{DocError, Drawing, EntityKind, LevelId, PageSetup, Result, SheetSetId, Trace};

/// Project-data key for compose views, masters and publisher sets.
pub const COMPOSE_KEY: &str = "compose1";
/// Most compose views in one drawing.
pub const MAX_COMPOSE_VIEWS: usize = 256;
/// Most master layouts.
pub const MAX_MASTER_LAYOUTS: usize = 32;
/// Most publisher sets.
pub const MAX_PUBLISHER_SETS: usize = 32;
/// Most layouts (or view names) in one publisher set.
pub const MAX_PUBLISHER_ITEMS: usize = 256;
/// Longest name for a view / master / publisher set.
pub const MAX_COMPOSE_NAME: usize = 64;

/// Stable compose-view identity; never reused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ComposeViewId(pub u32);

/// Stable master-layout identity; never reused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MasterLayoutId(pub u32);

/// Output format for a publisher set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PublishFormat {
    #[default]
    Pdf,
    /// Reserved; exporter may refuse until DWG batch export exists.
    Dwg,
}

/// A named display recipe placed on paper through a viewport (ArchiCAD View Map analogue).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeView {
    pub id: ComposeViewId,
    /// 1..=64 characters, unique among compose views (case-insensitive).
    pub name: String,
    /// Level to show; `None` = first / implicit level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<LevelId>,
    /// Sheet-set annotation over that level; `None` = design geometry only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet_set: Option<SheetSetId>,
    /// Plot scale denominator (`50` means 1:50). Zero means “use layout / fit”.
    #[serde(default)]
    pub scale_denom: u32,
    /// Plot style table name (empty = layout default).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub plot_style_table: String,
    /// Other levels faded under this view (at most [`crate::MAX_TRACES`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traces: Vec<Trace>,
    /// Optional model-space centre for the viewport; `None` = extents when placed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_center: Option<[f64; 2]>,
    /// Optional model-space view height; with `view_center` sets the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_height: Option<f64>,
}

/// Shared paper chrome: size, orientation, title block / sheet template.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterLayout {
    pub id: MasterLayoutId,
    pub name: String,
    pub page: PageSetup,
    /// Block name for the title block (default `AAD-TITLEBLOCK` when empty and applying).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title_block: String,
    /// Optional `sheet.template` name applied with the master.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub template: String,
}

/// One named publish job: which layouts, format and destination folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherSet {
    pub name: String,
    /// Layout names to plot (order preserved).
    #[serde(default)]
    pub layouts: Vec<String>,
    #[serde(default)]
    pub format: PublishFormat,
    /// When true, one multipage PDF (engine may fall back to one file per sheet).
    #[serde(default)]
    pub single_file: bool,
    /// Output folder (empty = drawing folder / Exports).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeBundle {
    #[serde(default)]
    pub views: Vec<ComposeView>,
    #[serde(default)]
    pub masters: Vec<MasterLayout>,
    #[serde(default)]
    pub publishers: Vec<PublisherSet>,
    #[serde(default)]
    pub view_seed: u32,
    #[serde(default)]
    pub master_seed: u32,
    /// Layout name → master layout id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub layout_masters: BTreeMap<String, u32>,
    /// `"layoutName\\handleHex"` → compose view id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub viewport_views: BTreeMap<String, u32>,
}

fn invalid(msg: impl Into<String>) -> DocError {
    DocError::Invalid(msg.into())
}

/// Whether `name` is usable for a compose view, master or publisher set.
pub fn compose_name_ok(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n.chars().count() <= MAX_COMPOSE_NAME && !n.chars().any(char::is_control)
}

impl Drawing {
    /// Serialise compose data into [`COMPOSE_KEY`] (call before DXF/DWG write).
    pub fn sync_compose_to_project(&mut self) {
        let mut layout_masters = BTreeMap::new();
        let mut viewport_views = BTreeMap::new();
        for layout in &self.layouts {
            if let Some(m) = layout.master {
                layout_masters.insert(layout.name.clone(), m.0);
            }
            for e in layout.entities.iter() {
                if let EntityKind::Viewport(vp) = &e.kind {
                    if let Some(v) = vp.compose_view {
                        viewport_views.insert(format!("{}\\{}", layout.name, e.handle.hex()), v.0);
                    }
                }
            }
        }
        if self.compose_views.is_empty()
            && self.master_layouts.is_empty()
            && self.publisher_sets.is_empty()
            && layout_masters.is_empty()
            && viewport_views.is_empty()
        {
            self.project.remove(COMPOSE_KEY);
            return;
        }
        let bundle = ComposeBundle {
            views: self.compose_views.clone(),
            masters: self.master_layouts.clone(),
            publishers: self.publisher_sets.clone(),
            view_seed: self.compose_view_seed,
            master_seed: self.master_layout_seed,
            layout_masters,
            viewport_views,
        };
        match serde_json::to_string(&bundle) {
            Ok(json) => {
                self.project.insert(COMPOSE_KEY.into(), json);
            }
            Err(_) => {
                self.project.remove(COMPOSE_KEY);
            }
        }
    }

    /// Restore compose data from [`COMPOSE_KEY`] after DXF/DWG read. Damaged data is dropped.
    pub fn load_compose_from_project(&mut self) {
        let Some(raw) = self.project.remove(COMPOSE_KEY) else {
            return;
        };
        let Ok(bundle) = serde_json::from_str::<ComposeBundle>(&raw) else {
            return;
        };
        if bundle.views.len() > MAX_COMPOSE_VIEWS
            || bundle.masters.len() > MAX_MASTER_LAYOUTS
            || bundle.publishers.len() > MAX_PUBLISHER_SETS
            || bundle.publishers.iter().any(|p| p.layouts.len() > MAX_PUBLISHER_ITEMS)
            || bundle.layout_masters.len() > 512
            || bundle.viewport_views.len() > 4096
        {
            return;
        }
        self.compose_views = bundle.views;
        self.master_layouts = bundle.masters;
        self.publisher_sets = bundle.publishers;
        self.compose_view_seed = bundle.view_seed;
        self.master_layout_seed = bundle.master_seed;
        for v in &self.compose_views {
            if v.id.0 >= self.compose_view_seed {
                self.compose_view_seed = v.id.0.saturating_add(1);
            }
        }
        for m in &self.master_layouts {
            if m.id.0 >= self.master_layout_seed {
                self.master_layout_seed = m.id.0.saturating_add(1);
            }
        }
        for (name, mid) in bundle.layout_masters {
            if let Some(layout) = self.layouts.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&name)) {
                if self.master_layouts.iter().any(|m| m.id.0 == mid) {
                    layout.master = Some(MasterLayoutId(mid));
                }
            }
        }
        for (key, vid) in bundle.viewport_views {
            let Some((layout_name, hex)) = key.split_once('\\') else { continue };
            if !self.compose_views.iter().any(|v| v.id.0 == vid) {
                continue;
            }
            let Some(handle) = crate::Handle::parse_hex(hex) else { continue };
            if let Some(layout) = self.layouts.iter_mut().find(|l| l.name.eq_ignore_ascii_case(layout_name)) {
                if let Some(arc) = layout.entities.get(handle).cloned() {
                    let mut e = (*arc).clone();
                    if let EntityKind::Viewport(vp) = &mut e.kind {
                        vp.compose_view = Some(ComposeViewId(vid));
                        layout.entities.replace(e);
                    }
                }
            }
        }
    }

    pub fn compose_view(&self, id: ComposeViewId) -> Option<&ComposeView> {
        self.compose_views.iter().find(|v| v.id == id)
    }

    pub fn compose_view_by_name(&self, name: &str) -> Option<&ComposeView> {
        self.compose_views.iter().find(|v| v.name.eq_ignore_ascii_case(name.trim()))
    }

    pub fn master_layout(&self, id: MasterLayoutId) -> Option<&MasterLayout> {
        self.master_layouts.iter().find(|m| m.id == id)
    }

    pub fn master_layout_by_name(&self, name: &str) -> Option<&MasterLayout> {
        self.master_layouts.iter().find(|m| m.name.eq_ignore_ascii_case(name.trim()))
    }

    pub fn publisher_set_by_name(&self, name: &str) -> Option<&PublisherSet> {
        self.publisher_sets.iter().find(|p| p.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Add a compose view. Refuses duplicate names and over-capacity.
    pub fn add_compose_view(&mut self, mut view: ComposeView) -> Result<ComposeViewId> {
        if self.compose_views.len() >= MAX_COMPOSE_VIEWS {
            return Err(invalid(format!("at most {MAX_COMPOSE_VIEWS} compose views")));
        }
        if !compose_name_ok(&view.name) {
            return Err(invalid("compose view name is empty, too long, or has control characters"));
        }
        view.name = view.name.trim().to_string();
        if self.compose_view_by_name(&view.name).is_some() {
            return Err(invalid(format!("compose view `{}` already exists", view.name)));
        }
        if view.traces.len() > crate::MAX_TRACES {
            return Err(invalid(format!("at most {} traces on a compose view", crate::MAX_TRACES)));
        }
        if view.scale_denom > 10_000 {
            return Err(invalid("`scaleDenom` must be 0..=10000"));
        }
        if let Some(h) = view.view_height {
            if !(h.is_finite() && h > 0.0 && h < 1.0e12) {
                return Err(invalid("`viewHeight` must be a finite positive number"));
            }
        }
        if let Some([x, y]) = view.view_center {
            if !(x.is_finite() && y.is_finite() && x.abs() < 1.0e12 && y.abs() < 1.0e12) {
                return Err(invalid("`viewCenter` must be finite"));
            }
        }
        let id = ComposeViewId(self.compose_view_seed);
        self.compose_view_seed = self.compose_view_seed.saturating_add(1);
        view.id = id;
        self.compose_views.push(view);
        Ok(id)
    }

    pub fn remove_compose_view(&mut self, id: ComposeViewId) -> Result<()> {
        let Some(i) = self.compose_views.iter().position(|v| v.id == id) else {
            return Err(invalid(format!("no compose view {id:?}")));
        };
        self.compose_views.remove(i);
        for layout in &mut self.layouts {
            let handles: Vec<_> = layout
                .entities
                .iter()
                .filter(|e| matches!(&e.kind, EntityKind::Viewport(vp) if vp.compose_view == Some(id)))
                .map(|e| e.handle)
                .collect();
            for h in handles {
                if let Some(arc) = layout.entities.get(h).cloned() {
                    let mut e = (*arc).clone();
                    if let EntityKind::Viewport(vp) = &mut e.kind {
                        vp.compose_view = None;
                        layout.entities.replace(e);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn add_master_layout(&mut self, mut master: MasterLayout) -> Result<MasterLayoutId> {
        if self.master_layouts.len() >= MAX_MASTER_LAYOUTS {
            return Err(invalid(format!("at most {MAX_MASTER_LAYOUTS} master layouts")));
        }
        if !compose_name_ok(&master.name) {
            return Err(invalid("master layout name is empty, too long, or has control characters"));
        }
        master.name = master.name.trim().to_string();
        if self.master_layout_by_name(&master.name).is_some() {
            return Err(invalid(format!("master layout `{}` already exists", master.name)));
        }
        if master.title_block.chars().count() > MAX_COMPOSE_NAME || master.template.chars().count() > MAX_COMPOSE_NAME {
            return Err(invalid("title block or template name is too long"));
        }
        let id = MasterLayoutId(self.master_layout_seed);
        self.master_layout_seed = self.master_layout_seed.saturating_add(1);
        master.id = id;
        self.master_layouts.push(master);
        Ok(id)
    }

    pub fn remove_master_layout(&mut self, id: MasterLayoutId) -> Result<()> {
        let Some(i) = self.master_layouts.iter().position(|m| m.id == id) else {
            return Err(invalid(format!("no master layout {id:?}")));
        };
        self.master_layouts.remove(i);
        for layout in &mut self.layouts {
            if layout.master == Some(id) {
                layout.master = None;
            }
        }
        Ok(())
    }

    pub fn add_publisher_set(&mut self, mut set: PublisherSet) -> Result<()> {
        if self.publisher_sets.len() >= MAX_PUBLISHER_SETS {
            return Err(invalid(format!("at most {MAX_PUBLISHER_SETS} publisher sets")));
        }
        if !compose_name_ok(&set.name) {
            return Err(invalid("publisher set name is empty, too long, or has control characters"));
        }
        set.name = set.name.trim().to_string();
        if self.publisher_set_by_name(&set.name).is_some() {
            return Err(invalid(format!("publisher set `{}` already exists", set.name)));
        }
        if set.layouts.len() > MAX_PUBLISHER_ITEMS {
            return Err(invalid(format!("at most {MAX_PUBLISHER_ITEMS} layouts in a publisher set")));
        }
        for n in &set.layouts {
            if n.chars().count() > MAX_COMPOSE_NAME || n.chars().any(char::is_control) {
                return Err(invalid("a layout name in the publisher set is invalid"));
            }
        }
        if set.folder.chars().count() > 512 || set.folder.chars().any(char::is_control) {
            return Err(invalid("`folder` is too long or has control characters"));
        }
        self.publisher_sets.push(set);
        Ok(())
    }

    pub fn remove_publisher_set(&mut self, name: &str) -> Result<()> {
        let Some(i) = self.publisher_sets.iter().position(|p| p.name.eq_ignore_ascii_case(name.trim())) else {
            return Err(invalid(format!("no publisher set `{name}`")));
        };
        self.publisher_sets.remove(i);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Drawing;

    #[test]
    fn compose_round_trips_through_project() {
        let mut d = Drawing::new_metric();
        let id = d
            .add_compose_view(ComposeView {
                id: ComposeViewId(0),
                name: "GF Elec 1:50".into(),
                level: None,
                sheet_set: None,
                scale_denom: 50,
                plot_style_table: "monochrome".into(),
                traces: Vec::new(),
                view_center: Some([0.0, 0.0]),
                view_height: Some(10000.0),
            })
            .unwrap();
        d.add_master_layout(MasterLayout {
            id: MasterLayoutId(0),
            name: "A1 Landscape".into(),
            page: PageSetup { paper: "A1".into(), width_mm: 841.0, height_mm: 594.0, landscape: true, ..PageSetup::default() },
            title_block: "AAD-TITLEBLOCK".into(),
            template: String::new(),
        })
        .unwrap();
        d.add_publisher_set(PublisherSet {
            name: "CD PDF".into(),
            layouts: vec!["Layout1".into()],
            format: PublishFormat::Pdf,
            single_file: true,
            folder: String::new(),
        })
        .unwrap();
        d.sync_compose_to_project();
        let json = d.project.get(COMPOSE_KEY).cloned().unwrap();
        let mut e = Drawing::new_metric();
        e.project.insert(COMPOSE_KEY.into(), json);
        e.load_compose_from_project();
        assert_eq!(e.compose_views.len(), 1);
        assert_eq!(e.compose_views[0].id, id);
        assert_eq!(e.compose_views[0].scale_denom, 50);
        assert_eq!(e.master_layouts.len(), 1);
        assert_eq!(e.publisher_sets.len(), 1);
    }

    #[test]
    fn refuses_duplicate_and_hostile_names() {
        let mut d = Drawing::new_metric();
        d.add_compose_view(ComposeView {
            id: ComposeViewId(0),
            name: "Plan".into(),
            level: None,
            sheet_set: None,
            scale_denom: 100,
            plot_style_table: String::new(),
            traces: Vec::new(),
            view_center: None,
            view_height: None,
        })
        .unwrap();
        assert!(
            d.add_compose_view(ComposeView {
                id: ComposeViewId(0),
                name: "plan".into(),
                level: None,
                sheet_set: None,
                scale_denom: 100,
                plot_style_table: String::new(),
                traces: Vec::new(),
                view_center: None,
                view_height: None,
            })
            .is_err()
        );
        assert!(
            d.add_compose_view(ComposeView {
                id: ComposeViewId(0),
                name: "".into(),
                level: None,
                sheet_set: None,
                scale_denom: 100,
                plot_style_table: String::new(),
                traces: Vec::new(),
                view_center: None,
                view_height: None,
            })
            .is_err()
        );
    }
}
