# Charts and vector pictures (Excel and Word) — design

Date: 2026-11-22. Status: directions decided by the user (2026-11-21/22):
pictures and charts are part of the Excel work, not later ("ko có chuyện để
sau"); SVG is the primary format, PNG only a fallback, for Word too.

## Intent

- A chart of a workbook or of a Word document shows in every output as a
  chart, sharp at any zoom, and its numbers are readable by a person and by
  a model (RAG chunks, JSON).
- A vector picture (SVG, EMF, WMF) stays vector: today `word::pictures`
  rasterizes all three to PNG at 2×, which blurs when zoomed and is larger.

## Goals

1. `model::Picture` can hold an SVG (with a PNG fallback made from it).
2. Word SVG, EMF and WMF pictures are kept as SVG (EMF/WMF through the
   existing `word::metafile::to_svg`). A docx picture with an `asvg:svgBlip`
   extension uses the SVG, not its PNG fallback.
3. A DrawingML chart (`c:chartSpace`, the same part in xlsx and docx) is read
   into a chart model and drawn as SVG by our own renderer. Types drawn:
   bar/column (clustered, stacked, percent stacked; horizontal or vertical),
   line (with or without markers, stacked), area (stacked), pie, doughnut,
   scatter, and a bar + line combination in one plot area (secondary value
   axis allowed). Other types (radar, bubble, stock, surface, 3-D variants
   drawn as their 2-D form where that is honest, else not drawn; `cx:chart`
   chartex: waterfall, treemap, sunburst, funnel, histogram, box) are not
   drawn: they give their title and data table only — or, in Word, the
   fallback picture of their `mc:AlternateContent`, when there is one.
4. Every chart becomes, in the document: a figure (the SVG picture, caption =
   the chart's title, or "Chart N" of the sheet when it has none) **followed
   by a table of its data** (categories as rows, one column per series; the
   series names as the header row) — decided with the user. The data table is
   what chunks/JSON/Markdown readers and models use.
5. Excel: charts of a sheet's drawing (`xdr:graphicFrame` → chart part) take
   their place among the sheet's drawn objects (anchor order, like shapes and
   pictures today); pictures inside groups (`xdr:grpSp`) are found; Excel 365
   pictures in cells ("Place in cell": `xl/richData/`, `vm` attribute of the
   cell) are shown as figures after the cell's table (v1: after the region,
   in cell order — not inside the table cell).
6. Word: charts in `w:drawing` (inline and anchored) give the figure + data
   table at their place in the text (`word/docx.rs` reading); the page
   layout (`src/layout/`) draws the chart SVG as vector in the PDF.

## Outputs

| Output | SVG picture / chart |
| --- | --- |
| HTML (all modes) | `<img>` with the SVG (data URI or file, as today's images); never inline `<svg>` of a foreign file (no script can run in an `<img>`) |
| Markdown | `.svg` file in the image dir |
| docx | `a:blip` → PNG fallback + `asvg:svgBlip` extension → SVG (as Word writes it) |
| EPUB | SVG file (EPUB 3 core media type) in the manifest |
| LaTeX | the SVG converted to a PDF file (`krilla-svg`, vector) and `\includegraphics` of it: works with pdflatex, xelatex and lualatex |
| JSON | the SVG in the assets dir, like other pictures |
| chunks | the figure caption and the data table, as today's tables |
| PDF (Word layout, `src/layout/`) | drawn as vector by krilla's SVG support (`krilla-svg`) |

PNG fallback: drawn from the SVG at 2× (the current `SVG_SCALE`), with the
existing pixel cap; made only where an output needs it (docx), or kept in
`Picture` — builder's choice, within the memory limits.

## Safety (anti-goals)

- A foreign SVG is never written out as it came: it is parsed by `usvg` and
  written back by `usvg`'s writer (scripts, `foreignObject`, event
  attributes, external references, CSS are gone; only data images stay, as
  the current `resolve_data` rules allow). Our own chart SVGs are generated
  from numbers and escaped text, so they need no such pass (but text in them
  is XML-escaped).
- The pixel cap and the per-document picture byte budget of the last fixes
  apply to every rasterization (PNG fallback, LaTeX PDF conversion of
  embedded rasters) and to the SVG bytes kept.
- A chart with absurd data (10⁶ points, NaN, inf, empty caches, a 10⁹-long
  category list) must not hang or blow memory: limits in config (points per
  series, series per chart, charts per document), the remainder dropped with
  the data table still complete up to the table limits.
- Formulas in chart refs are not evaluated: the cached values (`c:numCache`,
  `c:strCache`, `c:multiLvlStrCache`) are used; a chart without caches gives
  its title only (plus a note that its data is not stored in the file).

## Chart model and renderer

- New module `src/chart/` (shared by `src/word/` and `src/excel/`):
  - `read.rs`: `c:chartSpace` → `Chart { title, kind groups (type, bar
    direction, grouping, series), axes (titles, number format, min/max when
    fixed, deleted), legend position, plot visible only, style }`; series:
    name (`c:tx`), categories, values, x values (scatter), colour (`c:spPr`
    solid fill, else the theme's accent colour N via `xl/theme/theme1.xml` /
    `word/theme/theme1.xml`, else Office defaults), number format.
  - `svg.rs`: chart → SVG string. Fixed size from the anchor/extent (points),
    clamped. Axis ticks by "nice numbers"; labels formatted with the series
    or axis number format through the existing `excel::numfmt` (dates ISO).
    Legend, axis titles. Text as `<text>` (font-family list with CJK
    fallbacks), so it stays selectable in HTML and vector in PDF.
  - The look follows the file: series colours, chart type, orientation,
    grouping, legend position, axis deletion/reversal. Gridlines as the file
    says (`c:majorGridlines`). No 3-D effects, shadows or gradients (a
    gradient fill → its first stop).
- Thresholds as documented config fields with units (`ChartConfig`, in
  `src/config.rs`): max points per series, max series, max charts per
  document, default size in points.

## Public API and formats

- `model::Picture` changes shape (an SVG variant / a format field instead of
  `jpeg: bool`): a breaking change, released together with the already
  breaking `InputFormat::Xlsx` (`feat!`). `Document::pictures` is not
  serialised (no JSON format change). The JSON output's picture files may now
  be `.svg`: a new value, not a changed field.
- Python: nothing to change unless it exposes `Picture`.

## Tests

- Unit tests: chart reading (each type, caches, multi-level categories,
  missing caches, colours from theme), SVG renderer (well-formed XML parsed by
  roxmltree; points inside the plot area; nice ticks; NaN/inf ignored), SVG
  sanitizing (script, onload, foreignObject, external href removed).
- Samples: `tests/data/excel/` gains a workbook with charts (made by
  LibreOffice from a committed source, or a committed small script — must be
  reproducible); `tests/data/word/` a docx with a chart and an SVG picture.
  Snapshots updated on purpose.
- Corpus: the 7 workbooks of the Excel corpus with charts (34 charts:
  destatis-energy-prices, ine-spain-cpi, ipa-5minutes-shindan,
  mhlw-kokumin-seikatsu-r6-06, poi-withtwocharts, ss-campaign-report,
  ss-kpi-performance-dashboard) — each chart drawn or given as data, checked
  by eye against LibreOffice's rendering (`soffice --convert-to pdf`); the
  crash test over the whole corpus (0 panics/timeouts); memory under the
  hostile repros.
- LaTeX: the chart PDFs compile with pdflatex, xelatex, lualatex.
- Browser: friendly HTML shows the SVG images (no book-view checks unless the
  user asks).

## Left to the builder

- Exact names, whether the PNG fallback is stored or made on demand, how
  theme colours are passed, the sample's production.
- Which rare chart variants are drawn in v1, as long as every chart gives at
  least its title and data table.
