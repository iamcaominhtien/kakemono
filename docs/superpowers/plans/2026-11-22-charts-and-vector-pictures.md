# Charts and vector pictures — plan

Spec: `docs/superpowers/specs/2026-11-22-charts-and-vector-pictures-design.md`.
Branch `ccr-cc06214d-55t4sc`. Each task: test first (RED shown), then GREEN;
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test --release` green; snapshot changes listed with their reason;
Conventional Commits; no book-view checks.

## Task 1 — SVG pictures end to end

- `model::Picture`: a format (PNG, JPEG, SVG) instead of `jpeg: bool`; SVG
  pictures sanitized by a `usvg` parse + `usvg` write (new helper next to
  `backend::svg_to_png`, with its tests: script, on* attributes,
  foreignObject, external href, CSS gone; data images kept within the caps).
- Word: SVG kept as SVG; EMF/WMF → `metafile::to_svg` → SVG; `asvg:svgBlip`
  preferred over its PNG fallback.
- Every output per the spec table: HTML `<img>` (data URI / file), Markdown
  `.svg`, docx svgBlip + PNG fallback, EPUB manifest `image/svg+xml`, LaTeX
  via `krilla-svg` → PDF + `\includegraphics`, JSON assets, Word layout PDF
  as vector (krilla SVG).
- Done when: the Word sample and a new docx with an SVG and an EMF give
  `.svg` files / svgBlip / vector PDF; the LaTeX compiles with the three
  engines; the hostile SVG repros stay under their memory bounds.

## Task 2 — chart reader (`src/chart/read.rs`)

- `c:chartSpace` → chart model (spec "Chart model"); caches only; theme
  accent colours; limits in `ChartConfig` (`src/config.rs`, units in names).
- Done when: unit tests for each listed type, multi-level categories, missing
  caches, theme colours, absurd sizes; all 34 charts of the corpus workbooks
  read without panic (report: type, series, points per chart).

## Task 3 — chart SVG renderer (`src/chart/svg.rs`)

- Bar/column (clustered/stacked/percent, both directions), line, area, pie,
  doughnut, scatter, bar + line combo with a secondary axis; nice ticks;
  number formats via `excel::numfmt`; legend; titles; CJK-safe fonts.
- Load the `dataviz` skill for axis/legend/label layout guidance; colours
  come from the file (the spec), not from a palette of ours.
- Done when: unit tests (well-formed SVG, geometry inside the plot area,
  ticks, NaN/inf skipped); PNG renders of the 34 corpus charts put side by
  side with LibreOffice's PDF of the same sheets, in the scratchpad, for a
  look by eye; the obvious differences fixed or listed.

## Task 4 — charts in the documents

- Excel: `xdr:graphicFrame` → chart part → figure (SVG) + caption + data
  table, in anchor order with shapes and pictures; pictures inside `grpSp`;
  Excel 365 in-cell pictures (`xl/richData/`, cell `vm`).
- Word: charts in `w:drawing` (inline, anchored, `mc:AlternateContent` with
  chartex → fallback picture) → figure + data table; the layout draws them.
- Chunks: the data table chunks like other tables (caption repeated).
- Samples: an Excel sample with charts (reproducible source committed) and a
  docx sample with a chart; snapshots updated on purpose.
- Done when: corpus crash test 0 panics/timeouts; the 7 chart workbooks show
  their charts in friendly HTML (Chromium screenshot, not the book view), md,
  docx (opened by LibreOffice → PDF), tex (3 engines), epub; docs updated
  (`docs/EXCEL-INPUT.md`, README, ARCHITECTURE, CLAUDE.md).
