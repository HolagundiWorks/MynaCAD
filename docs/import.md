# Import (File > Import)

`import {path, ...}` brings a file into the open drawing. The kind is chosen by the extension. One undo step.

| Kind | Extensions | What arrives |
|---|---|---|
| Drawing | `.dxf`, `.dwg` | The objects, layers and blocks of the file, merged at their own coordinates. Names this drawing already has keep this drawing's definition. `at` is not used. |
| PDF | `.pdf` | The page's vector paths, text and fills as geometry: the same as `pdf.import` ([pdf-underlay.md](pdf-underlay.md)). `page`, `scale`, `at` apply. |
| SVG | `.svg` | Paths (all commands, curves and arcs flattened), lines, polylines, polygons, rectangles, circles, ellipses, groups and `transform`, as polylines on layer `SVG-IMPORT`. Sized from `width`/`height`/`viewBox` (96 pixels per inch when unitless). Text, pictures, `use`, gradients, clip paths and styles are not read; the result counts what was left out. |
| Picture | `.png`, `.jpg`, `.jpeg`, `.tif`, `.tiff` | The dark lines of the picture traced to closed polylines on layer `RASTER-IMPORT` (pixels darker than `threshold`, default 128; `invert` for light lines on a dark ground). The pixels themselves are not placed in the drawing, because the renderer draws an image only as a frame. |

Parameters: `scale` (drawing mm per paper mm, default 1), `at` (lower left, default [0, 0]), `layer`, `page` (PDF), `dpi` (picture, default 96), `threshold`, `invert`. SVG and pictures are registered like an underlay, so `underlay.calibrate` fixes the scale from two points and a real distance and `underlay.remove` takes them out. A picture has no known scale: calibrate before measuring.

Limits: files up to 256 MB; at most 200 000 SVG polylines; a picture larger than 2000 pixels on its longer side is reduced first; a picture that gives more than 100 000 outlines (a photograph) is refused with a hint. Native builds only: the web build has no file paths.

Code: `crates/engine/src/cmd/import.rs` (dispatch, picture tracing), `svgimport.rs` (SVG reader), `crates/io/src/raster.rs` (PNG, JPEG and TIFF decoding, installed as the host's `set_raster_reader`).

Not yet done: the File > Import... menu item in the WinUI shell (the command exists and the engine lists it in the File menu), a file dialog filter for the nine extensions, and test-plan cases.
