# Replacement figure images Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `kakemono convert … --content edited.json` puts a replacement picture (PNG, JPEG or SVG) in place of a figure in every output format, and `kakemono render-svg` rasterizes an SVG the way kakemono does.

**Architecture:** The edited JSON names the replacement in a new input-only field `replace_image` on a `figure` block. Applying it loads the file, stores it in `Document::pictures` and points the figure's `ImageSpec::embedded` at it, keeping the figure's `page` and `rect`. Every format already gets figure pictures through `Pictures::region` (`src/render/assets.rs`), which returns stored pictures first, so they all pick it up; SVG pictures stay vector where the format takes SVG (text turned into paths) and are rasterized with `resvg` where it does not. `hard-95` draws the replacement over the figure's box and drops the figure's text layer.

**Tech Stack:** Rust 2024, `resvg`/`usvg` 0.45 (already a dependency), `clap`, `serde_json`.

**Spec:** `iamcaominhtien/utsushi`, `docs/design/images.md` §4.1 and §4.3 (phase 1 of §8). Two deviations, decided while planning, to be written back into the spec (Task 5):
- The replacement goes in a new field `replace_image`, not in `image`: applying an unedited JSON must change nothing, and kakemono cannot tell its own picture path (which depends on `--image-dir`, `--images`) from a replacement without re-rendering.
- `raster_ratio` (§4.3) is dropped: raster image boxes exist only during analysis (`analysis::page::Page::images`) and are not kept in `Analysis`, whose fields are public API; the utsushi classifier judges it from the picture instead.

## Global Constraints

- No new `FORMAT_VERSION` (`src/render/json/mod.rs:35`): `replace_image` is a new input field; the commit type is `feat:`, not `feat!:`.
- Public API changes are additive only: a new method `Paper::apply_content_in`; `Picture` may now hold SVG data (documented). No field added to a public struct.
- Outputs of documents without `replace_image` do not change: `KAKEMONO_UPDATE_SNAPSHOTS` must not be needed; every snapshot in `tests/data/snapshots/` stays as it is.
- A number with a meaning is a named constant with a doc comment, its name ending with its unit (`_ratio`, `_pt`, `_px`).
- `cargo fmt --check && cargo clippy --all-targets -- -D warnings` clean; `cargo test --release` green.
- Commits: Conventional Commits, English, imperative (`feat(json): …`).

## Review Focus

1. A JSON applied with `--content` from another folder than the JSON's: `replace_image` paths resolve against the JSON file's folder, not the current directory.
2. A replacement whose aspect differs from the figure's box: a warning in the `ContentReport`, and `hard-95` keeps the picture inside the box (`object-fit: contain`), never stretched over the text next to it.
3. A broken replacement (missing file, not PNG/JPEG/SVG, an SVG that does not parse): a warning naming the block, and the figure keeps its original picture; the conversion still succeeds.
4. `replace_image` on a block that is not a `figure` (an equation, a table): a warning, ignored.
5. A Word input (`.docx`), whose figures are already stored pictures: the replacement works the same way.

---

### Task 1: Stored SVG pictures in every format

**Files:**
- Modify: `src/model/mod.rs` (doc of `Picture`, `Document::pictures`, `ImageSpec::embedded`)
- Modify: `src/render/assets.rs` (`Pictures::region`, `ImageStore`, `ImageStore::draw`)
- Modify: `src/render/latex/mod.rs:113`, `src/render/latex/mod.rs:939`, `src/render/docx/mod.rs:129-135` and every other `ImageStore { … }` literal (`grep -rn "ImageStore {" src`)
- Test: `src/render/assets.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub(crate) enum PictureData { Png(Vec<u8>), Jpeg(Vec<u8>), Svg(String) }` in `assets.rs`; `Pictures::region(&self, renderer, spec, scale) -> Option<PictureData>`; `pub(crate) fn picture_data(picture: &Picture) -> Option<PictureData>` (sniffs: `jpeg` flag → Jpeg; PNG signature → Png; UTF-8 text that `usvg` parses → Svg); `ImageStore` gains `pub(crate) vector: bool` (true: an SVG stays SVG; false: rasterized at `opts.scale`).
- Picture docs: `Picture::data` is "PNG, JPEG or SVG data"; `jpeg` stays as it is (SVG is recognised by its content). `ImageSpec::embedded` doc: "a stored picture; for a figure of a PDF whose picture was replaced (`--content`), `page` and `rect` still give its place".

- [ ] **Step 1: Write the failing tests** in `assets.rs`:

```rust
#[test]
fn a_stored_svg_stays_vector_with_its_text_as_paths() {
    // An SVG picture with a <text> element, stored and drawn for HTML.
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><text x="2" y="15" font-size="12">Hồi quy</text></svg>"#;
    let (data, mime, ext, _) = draw_stored(svg.as_bytes(), true).unwrap();
    assert_eq!((mime, ext), ("image/svg+xml", "svg"));
    let out = String::from_utf8(data).unwrap();
    assert!(!out.contains("<text"), "text must be paths: {out}");
    assert!(out.contains("<path"));
}

#[test]
fn a_stored_svg_is_rasterized_where_the_format_takes_no_svg() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#;
    let (data, mime, _, _) = draw_stored(svg.as_bytes(), false).unwrap();
    assert_eq!(mime, "image/png");
    assert!(data.starts_with(b"\x89PNG"));
}
```

`draw_stored(bytes, vector)` is a test helper: a `Document` with one stored `Picture { data, jpeg: false }`, an `ImageSpec` with `embedded: Some(0)`, an `ImageStore { opts: &HtmlOptions::default(), jobs: vec![], vector }`, calling `draw` on `ImageJob::Region`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --release --lib a_stored_svg`
Expected: FAIL (does not compile: `vector`, `PictureData` missing).

- [ ] **Step 3: Implement** `PictureData`, `picture_data`, the new `Pictures::region` return type and `ImageStore::vector`. In `draw`, `Svg` with `vector` → `usvg::Tree::from_str` with the fonts of `svg_to_png` (`svg_fonts()`), written back with `tree.to_string(&usvg::WriteOptions::default())` (text written as paths); `Svg` without `vector` → `svg_to_png(&svg, self.opts.scale)`. Set `vector: false` in LaTeX's stores, `true` everywhere else. In `docx/mod.rs`, map `PictureData::Svg` to `svg_to_png(.., opts.scale)` before `fill_images` (which keeps taking `(Vec<u8>, bool)`).

- [ ] **Step 4: Run the tests**

Run: `cargo test --release --lib a_stored_svg && cargo test --release --test word`
Expected: PASS, and no snapshot change (Word pictures are PNG/JPEG and take the same path).

- [ ] **Step 5: Commit**

```bash
git add src/model/mod.rs src/render
git commit -m "feat(render): draw stored SVG pictures, as vector or rasterized by format"
```

### Task 2: `replace_image` in `--content`

**Files:**
- Modify: `src/render/json/apply.rs` (`BlockIn`, `apply_content`, new `apply_replacement`)
- Modify: `src/paper.rs:391` (new `apply_content_in`), `src/main.rs:789` (pass the JSON's folder)
- Modify: `src/render/mod.rs:40` (re-export if the signature changes)
- Test: `tests/cli.rs`

**Interfaces:**
- Consumes: `picture_data(&Picture) -> Option<PictureData>` (Task 1) to validate a loaded file.
- Produces: `pub fn Paper::apply_content_in(&self, analysis: &mut Analysis, json: &str, base: &Path) -> Result<ContentReport>`; `Paper::apply_content` = the same with no base (a relative `replace_image` then gives a warning: "block N: replace_image is a relative path; give the JSON's folder (apply_content_in)"). `ContentReport::edited_blocks` counts a replaced figure. A const `REPLACE_ASPECT_TOLERANCE_RATIO: f32 = 0.02` with its doc: "largest relative difference between the aspect of a replacement and of the figure's box before a warning".

- `replace_image` value: a data URI (`data:image/png;base64,…`, `image/jpeg`, `image/svg+xml`, base64 or URL-encoded) or a path relative to `base`. Loaded and checked with `picture_data`; pushed to `analysis.doc.pictures`; `ImageSpec::embedded = Some(index)`, `page`/`rect` unchanged. Size of a raster from its header (`image::image_dimensions` from memory), of an SVG from `usvg::Tree::size()`.
- Warnings (block keeps its picture): file not found; not PNG, JPEG or SVG; a block that is not a figure or has no picture; aspect off by more than the tolerance (this one still replaces).

- [ ] **Step 1: Write the failing test** in `tests/cli.rs`, on the committed LIGO paper:

```rust
/// A figure's `replace_image` (a path relative to the JSON) replaces its
/// picture in Markdown, Word, friendly HTML and LaTeX; a broken one is a
/// warning and the figure keeps its picture.
#[test]
fn replace_image_swaps_a_figure_in_every_format() {
    let dir = scratch("cli-replace");
    let json = dir.join("ligo.json");
    assert!(kakemono(&["convert", LIGO, "-j", "1", "-o", json.to_str().unwrap()]).status.success());
    let mut content: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    // The first figure gets an SVG of the size of its box; the second a missing file.
    let figures: Vec<usize> = /* indexes of blocks with "type": "figure" */;
    std::fs::create_dir_all(dir.join("vi")).unwrap();
    std::fs::write(dir.join("vi/fig1.svg"), svg_of_size(&content["blocks"][figures[0]])).unwrap();
    content["blocks"][figures[0]]["replace_image"] = "vi/fig1.svg".into();
    content["blocks"][figures[1]]["replace_image"] = "vi/missing.png".into();
    // The third: a 1:1 PNG for a wide box (aspect warning, still replaced); and one table block.
    std::fs::write(dir.join("vi/square.png"), square_png()).unwrap();
    content["blocks"][figures[2]]["replace_image"] = "vi/square.png".into();
    content["blocks"][a_table]["replace_image"] = "vi/square.png".into();
    let edited = dir.join("ligo.vi.json");
    std::fs::write(&edited, serde_json::to_string(&content).unwrap()).unwrap();

    // Run from another folder: the path resolves against the JSON's folder.
    let md = dir.join("out/x.md");
    let out = kakemono_in(&std::env::temp_dir(), &["convert", LIGO, "--content", edited.to_str().unwrap(), "-o", md.to_str().unwrap()]);
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing.png"), "{stderr}");
    assert!(stderr.contains("aspect"), "{stderr}");
    assert!(stderr.contains("not a figure"), "{stderr}");
    let text = std::fs::read_to_string(&md).unwrap();
    assert!(text.contains(".svg"), "the replaced figure is an SVG file: {text}");
    // docx: the replaced figure is a PNG in word/media; friendly: an inline SVG data URI.
    let docx = convert_bytes(&["--content", edited, "-o", "x.docx"]);
    assert!(zip_parts(&docx).iter().any(|(name, data)| name.starts_with("word/media/") && data == &svg_png_at_default_scale));
    let friendly = convert_to_string(&["-m", "friendly", "--content", edited]);
    assert!(friendly.contains("data:image/svg+xml"));
}
```

`svg_of_size(block)` writes `<svg width=W height=H>` with W×H the figure's `layout.bbox` size in points and a `<text>Hình mới</text>`; `kakemono_in(dir, args)` runs the binary with `current_dir(dir)`. `zip_parts` is the helper of `tests/real_papers.rs` (copy it, or move it to a shared test module); `svg_png_at_default_scale` is `kakemono::backend::svg_to_png(svg, 3.0)` of the same SVG; `square_png()` a 10×10 PNG made with `image`; `a_table` the index of a `table` block.

Add to `tests/word.rs`, `replace_image_works_on_a_word_picture`: convert `tests/data/word/sample.docx` to JSON, set `replace_image` (a data URI of `square_png()`) on its first figure, apply it with `-o x.md`, and assert the Markdown's picture file has the PNG's bytes.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --release --test cli replace_image_swaps`
Expected: FAIL (no `.svg` in the Markdown; no warning).

- [ ] **Step 3: Implement** `apply_replacement(analysis, idx, value, base, report)` in `apply.rs` (keep `apply_content` under the `too_many_lines` limit), `Paper::apply_content_in`, and in `main.rs` call it with `path.parent()`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --release --test cli && cargo test --release --test real_papers content_json_round_trips`
Expected: PASS; the round trip is unchanged (an unedited JSON has no `replace_image`).

- [ ] **Step 5: Commit**

```bash
git add src tests/cli.rs
git commit -m "feat(json): replace a figure's picture with replace_image in --content"
```

### Task 3: Replaced figures in hard-95

**Files:**
- Modify: `src/render/hard95/mod.rs` (`page_html`, the figure-text paths at `:340-355` and `:373-400`, `figure_regions` at `:406`)
- Modify: `src/render/hard95/hard95.css` (one rule; compact style, no reformatting)
- Test: `tests/cli.rs` (extend Task 2's test, or a new one using its edited JSON)

**Interfaces:**
- Consumes: a figure block whose `ImageSpec::embedded` is `Some` while the document has pages = a replaced figure (Task 2).
- Produces: in the page's text layer, `<div class="b" data-kind="figure-picture" data-block="N" style="left/top/width/height of the figure's rect, in the page units the layer uses"><img src="…" alt=""></div>`; CSS: `[data-kind=figure-picture]{background:#fff}[data-kind=figure-picture] img{width:100%;height:100%;object-fit:contain}`. For that block: no `data-kind="figure-text"` div, and rotated runs inside its region are not added to the figure-text layer either.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn hard95_draws_a_replaced_figure_over_its_box_without_its_old_text() {
    // Same edited JSON as replace_image_swaps_a_figure_in_every_format.
    let html = convert_to_string(&["-m", "hard-95", "--content", edited]);
    let id = /* the replaced figure's block id */;
    assert!(html.contains(&format!(r#"data-kind="figure-picture" data-block="{id}""#)));
    assert!(!html.contains(&format!(r#"data-kind="figure-text" data-block="{id}""#)));
    assert!(html.contains("data:image/svg+xml"));
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --release --test cli hard95_draws_a_replaced`
Expected: FAIL.

- [ ] **Step 3: Implement** in `hard95/mod.rs`: a replaced figure is placed with `images.region(spec, "fig")` in a box built the way `table_box` builds its position; its figure-text and in-region rotated runs are skipped. The page picture is not changed (the opaque box covers the original figure; a smaller page picture is not worth a second rendering path).

- [ ] **Step 4: Run the tests**

Run: `cargo test --release --test cli && cargo test --release --test real_papers`
Expected: PASS, no snapshot change.

- [ ] **Step 5: Commit**

```bash
git add src/render/hard95 tests/cli.rs
git commit -m "feat(hard95): draw a replaced figure over its box"
```

### Task 4: `kakemono render-svg`

**Files:**
- Modify: `src/main.rs` (`enum Command`, `main`)
- Modify: `src/backend/render.rs:310` (a variant of `svg_to_png` that resolves relative `href`s against a folder: `usvg::Options::resources_dir`)
- Test: `tests/cli.rs`

**Interfaces:**
- Produces: `kakemono render-svg <INPUT> -o <OUTPUT.png> [--scale <f32, default 1.0>]`. INPUT is an SVG (relative `href`s from its folder; embedded raster images allowed). Exit 0 and a PNG of `ceil(size × scale)` pixels; a non-zero exit with "cannot read <INPUT> as SVG: <reason>" otherwise. `pub fn svg_to_png_in(svg: &str, scale: f32, resources: Option<&Path>) -> Option<Vec<u8>>`, `svg_to_png` calling it with `None`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn render_svg_writes_a_png_of_the_scaled_size() {
    let dir = scratch("cli-render-svg");
    std::fs::write(dir.join("a.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#).unwrap();
    let out = kakemono(&["render-svg", dir.join("a.svg").to_str().unwrap(), "-o", dir.join("a.png").to_str().unwrap(), "--scale", "2"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(image::image_dimensions(dir.join("a.png")).unwrap(), (80, 40));

    std::fs::write(dir.join("bad.svg"), "not svg").unwrap();
    let out = kakemono(&["render-svg", dir.join("bad.svg").to_str().unwrap(), "-o", dir.join("b.png").to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));
}
```

(If `image` is not a dev-dependency yet, read the PNG size from its IHDR bytes 16..24 instead.)

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --release --test cli render_svg`
Expected: FAIL (unknown subcommand).

- [ ] **Step 3: Implement** the subcommand and `svg_to_png_in`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --release --test cli render_svg`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/backend/render.rs tests/cli.rs
git commit -m "feat(cli): add render-svg to rasterize an SVG as kakemono does"
```

### Task 5: Documentation, full check

**Files:**
- Modify: `README.md` (the editable JSON section near `:226-233`: `replace_image`; the CLI table: `render-svg`), `ARCHITECTURE.md` (the pictures paragraph: stored pictures may replace a PDF region and may be SVG)
- Modify (other repo): `iamcaominhtien/utsushi` `docs/design/images.md` §4.1 and §4.3, the two deviations listed at the top of this plan, on its branch `claude/confident-rubin-8hux62`

- [ ] **Step 1: Write the documentation** (README: what `replace_image` accepts, that it is relative to the JSON's folder, and what each format does with an SVG).
- [ ] **Step 2: Full check**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --release`
Expected: all green, `git status` shows no change under `tests/data/snapshots/`.

- [ ] **Step 3: Commit and push both repositories**

```bash
git add README.md ARCHITECTURE.md && git commit -m "docs: document replace_image and render-svg"
git push -u origin claude/confident-rubin-8hux62
```
