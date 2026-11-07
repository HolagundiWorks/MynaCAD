# Project parameters, company details and presentation import

What exists today and what is not built. Status vocabulary: [documentation-policy.md](documentation-policy.md).

## Project parameters (Project panel, `project`)

Implemented and tested (`crates/engine/tests/project_details.rs`): the panel shows (read only) the
twelve project details (project name, client, location, site address, property ID, site area, plot
use, approving authority, architect, engineer, date, summary) and the slab and beam defaults; each
is edited in the Project Wizard ("Edit project..." or File > Project Wizard). `project` also returns `company`, the company details
the drawing holds. Checked by hand in the running shell; there is no automated UI test.

## Company details (`companyfile`)

The Kivo company file (`document_context`, version 1) gives: company name, address, phone, email,
PAN, GSTIN, COA registration, one architect name, the preparation date, and the project's name, code,
site address, summary and client. The panel shows these read only. The Project Wizard has a "Read company
file" button (`companyfile {refresh: true}`), which needs `OSAS_COMPANY` and a ticket. Tren never asks
for the company admin password.

When the drawing holds company-file identity, those fields are **refresh-only**: `project` and
`project.wizard` refuse changes to Studio-owned keys (`suite.locked_details` in the `project` /
`companyfile` answer). Firm-only apply locks architect and date; a linked project also locks title,
client, site address, property id and summary. Engineer and other local fields stay editable. Change
identity in Studio, then refresh. The local offline `CompanyProfile` store (`projectplan/company.rs`)
refuses save/logo while `OSAS_COMPANY` is set so it cannot become a second writer of firm identity.

Not available from the company file: **a logo** and **more than one architect**. `CompanyProfile`
has a logo path and one architect; neither reaches the panel or the title block when suite-linked.
Both need either an extension of `document_context` (a Studio contract, not in this repository) or a
profile loader that only mirrors Studio.

## Import for presentation: not built

| Wanted | State |
|---|---|
| PNG, JPEG and other raster images | `EntityKind::Image` is read and written in DXF but drawn only as a frame with a cross (`render/lib.rs`). There is no decoder, no texture path (GPU, raster, PDF, SVG) and no attach command. Needs an image pipeline first |
| Other vector files | DXF and DWG open, and `underlay.attach` reads a PDF page as paths. SVG import does not exist |
| Mood boards | Depends on raster images in layouts |
| Tables from CSV or spreadsheets | `EntityKind::Table` exists and is drawn, but nothing creates one from a file |
| Tables linked to project parameters so a parameter change updates them | Not designed. Needs a decision on what a table cell links to |

Open questions for the owner: which image formats and decoder crate (licence and wasm check), whether
images are embedded in the file or referenced by path, and what a linked table cell refers to.
