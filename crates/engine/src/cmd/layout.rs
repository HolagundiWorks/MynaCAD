//! Model-space plotting: PLOT, PLOT.WINDOW, PLOTL, PLOTP and EXPORTPDF, plus the plot-style commands.
//!
//! The layout, viewport, page-setup and mspace/pspace commands were removed with paper space
//! (`docs/layout-book.md` 11.1); sheets are made in MynaOps. `Space::Paper` and `Layout` stay as
//! internal data for the DXF/DWG round trip. Plotting goes through the host's
//! [`super::file::IoHooks::plot`] hook so the engine stays I/O-agnostic.

use aad_doc::{EntityKind, Space};
use aad_geom::Vec2;
use serde_json::{Value, json};

use super::file::{base64_encode, io};
use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

mod plotstyle;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("plot.window", "Plot Window", run_plot_window)
            .params("{window?: [x0, y0, x1, y1] model-space corners} → {window} (checks a plot window; typed or from the menu it asks for the two corners, or Extents for the whole drawing, or Display for what is on screen, then the application asks where to save)")
            .noundo()
            .interactive(|_| Ok(Box::new(PlotWindowM::default()))),
        // Model-space window plot on landscape / portrait paper (no layout is created).
        CommandSpec::new("plotl", "Plot Landscape", run_plot_window)
            .alias(&["plot.landscape"])
            .params("{window?: [x0,y0,x1,y1]} → picks a model window, then plots it on landscape paper")
            .noundo()
            .interactive(|_| Ok(Box::new(PlotWindowM::oriented(true, false)))),
        CommandSpec::new("plotp", "Plot Portrait", run_plot_window)
            .alias(&["plot.portrait"])
            .params("{window?: [x0,y0,x1,y1]} → picks a model window, then plots it on portrait paper")
            .noundo()
            .interactive(|_| Ok(Box::new(PlotWindowM::oriented(false, false)))),
        CommandSpec::new("plot", "Print...", |s, p| run_plot(s, p, "plot"))
            .menu(&["File", "Print..."])
            .key("Cmd+P")
            .alias(&["print"])
            .params("{path? (else returns base64 `data`), window?: [x0, y0, x1, y1] (model space: plot this rectangle, not the whole drawing), paper?, landscape?: bool, fit?: bool, scale?, lineweights?: bool, layers?: bool (default true: each drawing layer is a PDF layer the viewer can hide)}")
            .noundo(),
        CommandSpec::new("exportpdf", "Export to PDF...", |s, p| run_plot(s, p, "exportpdf"))
            .menu(&["File", "Export to PDF..."])
            .params("{path? (else returns base64 `data`), window?, paper?, landscape?, fit?, lineweights?, layers?: bool (default true: PDF optional-content layers), format?: pdf|svg|png (default pdf), dpi? (png: dots per inch, 36-1200, default 150; the image is the whole sheet)}")
            .noundo(),
    ]
    .into_iter()
    .chain(plotstyle::specs())
    .collect()
}

fn parse_window(cmd: &str, p: &Value) -> Result<Option<[f64; 4]>> {
    let Some(v) = p.get("window").filter(|v| !v.is_null()) else { return Ok(None) };
    let a: Vec<f64> = v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    match a[..] {
        [x0, y0, x1, y1] if a.iter().all(|n| n.is_finite() && n.abs() < 1e12) && x0 != x1 && y0 != y1 => {
            Ok(Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]))
        }
        _ => Err(bad(cmd, "`window` must be [x0, y0, x1, y1]: two different corners of a rectangle")),
    }
}

fn run_plot_window(_s: &mut Session, p: &Value) -> Result<Value> {
    let w = parse_window("plot.window", p)?;
    Ok(json!({ "window": w }))
}

/// PLOT.WINDOW / PLOTL / PLOTP: what part of the drawing. Two corners make the window; Extents plots the
/// whole drawing and Display what is on screen. The answer is left in [`Session::plot_request`].
#[derive(Default)]
struct PlotWindowM {
    first: Option<Vec2>,
    /// PLOTL (`Some(true)`) and PLOTP (`Some(false)`) choose the paper orientation; PLOT.WINDOW leaves it to the shell.
    landscape: Option<bool>,
    preview: bool,
}

impl PlotWindowM {
    fn oriented(landscape: bool, preview: bool) -> Self {
        PlotWindowM { first: None, landscape: Some(landscape), preview }
    }

    fn request(&self, s: &mut Session, window: Option<[f64; 4]>) -> Result<Step> {
        s.plot_request = Some(crate::PlotRequest { window, landscape: self.landscape, preview: self.preview });
        Ok(Step::Done)
    }
}

impl Interactive for PlotWindowM {
    fn name(&self) -> &'static str {
        "PLOT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        // A window is picked in model space; anything else plots the whole drawing.
        if matches!(s.space(), Space::Model) { Ok(Step::Continue) } else { self.request(s, None) }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.first {
            None => Prompt::new("Specify first corner of the plot window", Accept::POINT).kw(&["Extents", "Display"]).default("Extents"),
            Some(a) => Prompt::new("Specify opposite corner", Accept::POINT).base(a),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.first, i) {
            (_, Input::Cancel) => Ok(Step::Cancel),
            (None, Input::Enter) => self.request(s, None),
            (None, Input::Keyword(k)) if k.eq_ignore_ascii_case("Extents") => self.request(s, None),
            (None, Input::Keyword(k)) if k.eq_ignore_ascii_case("Display") => {
                let v = s.state()?.view();
                let aspect = s.viewport_px.0 / s.viewport_px.1.max(1.0);
                let (h, w) = (v.height, v.height * aspect);
                self.request(s, Some([v.center.x - w / 2.0, v.center.y - h / 2.0, v.center.x + w / 2.0, v.center.y + h / 2.0]))
            }
            (None, Input::Point(a)) => {
                self.first = Some(a);
                Ok(Step::Continue)
            }
            (Some(a), Input::Point(c)) => {
                let w = parse_window("plot", &json!({ "window": [a.x, a.y, c.x, c.y] }))
                    .map_err(|_| crate::EngineError::Other("The two corners must differ in both x and y.".into()))?;
                self.request(s, w)
            }
            _ => Err(crate::EngineError::Other("Pick the corners of the plot window, or choose Extents or Display.".into())),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.first.map(|a| vec![super::helpers::lwpoly(super::helpers::rect_vertices(a, c), true)]).unwrap_or_default()
    }
}

// ---------- plotting ----------

/// A plot ready to run on a worker thread. Plotting only reads the drawing, so [`begin_plot`]
/// (on the session's thread, where the arguments are checked) and [`PlotJob::run`] (anywhere) are
/// all it takes.
pub struct PlotJob {
    hook: super::file::PlotHook,
    doc: std::sync::Arc<aad_doc::Drawing>,
    space: Space,
    opts: Value,
    path: Option<String>,
}

pub fn begin_plot(s: &Session, p: &Value, cmd: &str) -> Result<PlotJob> {
    // Layout plotting was removed with paper space: only model space is plotted.
    match str_param(p, "layout") {
        None => {}
        Some(n) if n.eq_ignore_ascii_case("model") => {}
        Some(_) => return Err(bad(cmd, "layouts are not available: sheets are made in MynaOps; only model space is plotted")),
    }
    let space = Space::Model;
    let hook = io().and_then(|h| h.plot).ok_or_else(|| bad(cmd, "plotting is not available in this build"))?;
    // A plot window is checked here (a malformed one is an error, not a silent whole-drawing plot).
    let window = parse_window(cmd, p)?;
    let mut opts = if p.is_object() { p.clone() } else { json!({}) };
    if let (Some(w), Some(o)) = (window, opts.as_object_mut()) {
        o.insert("window".into(), json!(w));
    }
    Ok(PlotJob { hook, doc: s.state()?.doc.clone(), space, opts, path: str_param(p, "path").map(str::to_string) })
}

impl PlotJob {
    /// Build the PDF and, given a `path`, write it there; without one the result carries base64 `data`.
    pub fn run(&self) -> std::result::Result<Value, String> {
        let fail = |e: String| e;
        let bytes = (self.hook)(&self.doc, &self.space, &self.opts).map_err(fail)?;
        let layout = match &self.space {
            Space::Model => "Model".to_string(),
            Space::Paper(n) => n.clone(),
        };
        match self.path.as_deref() {
            Some(_path) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let path = _path;
                    let tmp = format!("{path}.aad-tmp");
                    std::fs::write(&tmp, &bytes).map_err(|e| fail(format!("{path}: {e}")))?;
                    std::fs::rename(&tmp, path).map_err(|e| fail(format!("{path}: {e}")))?;
                    Ok(json!({ "path": path, "bytes": bytes.len(), "layout": layout }))
                }
                #[cfg(target_arch = "wasm32")]
                Err(fail("paths are not available on the web; omit `path` to get the PDF as base64 `data`".into()))
            }
            None => Ok(json!({ "data": base64_encode(&bytes), "bytes": bytes.len(), "layout": layout })),
        }
    }
}

fn run_plot(s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    begin_plot(s, p, cmd)?.run().map_err(|e| bad(cmd, e))
}
