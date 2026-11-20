# CLAUDE.md

`kakemono`: a Rust CLI and library that converts arXiv PDF papers (and Word documents, `.docx`
and `.doc`, read by `src/word/`; laid out in pages as Word does by `src/layout/`, which also
writes them as a PDF) to HTML, in 4 modes, to a
Word document (`.docx`, `src/render/docx/`), to Markdown (`.md`, `src/render/markdown/`), to
LaTeX (`.tex`, `src/render/latex/`), to an EPUB book (`.epub`, `src/render/epub/`), to chunks for
retrieval (`.jsonl`, `src/render/chunks/`) or to editable JSON (`.json`, `src/render/json/`), which `--content` applies back so that any output shows the
edited (for example translated) text:

- `hard`: a copy of every page, with a selectable text layer.
- `hard-95`: like `hard`, but all text is real HTML text placed as in the PDF.
- `medium`: page and column layout kept, text reflowed.
- `friendly`: a single-column article, with a scroll view, a book view and a phone layout.

Read before changing code:

- [ARCHITECTURE.md](ARCHITECTURE.md): pipeline, modules, coding conventions.
- [CONTRIBUTING.md](CONTRIBUTING.md): commit messages.
- [README.md](README.md), the "Testing" section.

## Commands

```bash
cargo build --release
./target/release/kakemono convert <file.pdf | arXiv id> -m friendly -o out/x.friendly.html   # or --all -o out/dir
./target/release/kakemono convert <file.pdf> -o out/x.docx                    # Word document (or -f docx)
./target/release/kakemono convert <file.pdf> -o out/x.md --image-dir images   # Markdown + images/ (or -f md)
./target/release/kakemono convert <file.pdf> -o out/x.tex                     # LaTeX + x_assets/ (or -f tex)
./target/release/kakemono convert <file.pdf> -o out/x.epub                    # EPUB 3 book (or -f epub)
./target/release/kakemono convert <file.pdf> -o out/x.json                    # editable JSON + x_assets/ (or -f json)
./target/release/kakemono convert <file.pdf> -o out/x.jsonl                   # chunks for RAG (or -f chunks; --tokenizer tokenizer.json)
./target/release/kakemono convert <file.pdf> --content out/x.edited.json -m hard-95   # any output with the edited text
./target/release/kakemono convert <file.docx | file.doc> -o out/x.md         # a Word document as input (all 4 HTML modes too)
./target/release/kakemono convert <file.docx | file.doc> -o out/x.pdf        # a Word document laid out as Word does
./target/release/kakemono inspect <file.pdf>        # print the detected structure, for debugging
scripts/fetch-test-pdfs.sh                            # download the 7 test papers (once)
cargo test --release                                  # unit tests + 8 real papers + snapshots
KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test real_papers --test word   # when output changes ON PURPOSE
KAKEMONO_PRINT_METRICS=1 cargo test --release --test real_papers -- --nocapture  # print each paper's metrics
cargo fmt --check && cargo clippy --all-targets -- -D warnings      # the CI lint gate
node tests/browser/check-friendly.js out/*.friendly.html             # browser checks (book view: only when asked)
node tests/browser/check-hard95.js out/*.hard-95.html
node tests/browser/check-stable.js out/*.medium.html out/*.friendly.html
cd python && maturin develop --release && pytest tests             # Python package (in a venv with maturin, pytest)
scripts/fetch-word-corpus.sh 8 && python3 scripts/word-fidelity.py   # Word layout vs Word's own PDFs (CI minimums in ci.yml)
python3 scripts/word-crash-test.py DIR...                            # every output of every Word file: panics, timeouts
```

Chromium and Playwright are preinstalled in the cloud environment. Do not run
`playwright install`.

## Automatic hooks (`.claude/settings.json`)

- **After each Edit/Write** (`.claude/hooks/after-edit.sh`):
  - `.rs`: runs `rustfmt`.
  - `.js`: runs `node --check`.
  - `.json`: checks that the JSON is valid.
  - `.sh`: runs `bash -n`.

  Errors (syntax errors, rustfmt failures) are reported back at once so they can be fixed.
- **At the end of a turn** (`.claude/hooks/on-stop.sh`): runs only if the turn changed a `.rs`
  file.
  1. Runs `cargo fmt`, then `cargo clippy --fix`, then `cargo clippy --all-targets -- -D warnings`.
  2. Warnings that clippy cannot fix itself are sent back to Claude to finish (once per turn).

The hooks do not run the tests, which are slow. Run `cargo test --release` yourself before
committing.

## Workflow for changes to the analysis (`src/analysis`)

- A new threshold must be a documented field in that stage's config struct (`src/config.rs`). The
  field name ends with its unit: `_em`, `_pt` or `_ratio`. No magic numbers in code.
- A rule that fixes one paper easily breaks another. After each change:
  1. Run the tests again and see which snapshots changed.
  2. Compare the output text before and after on as many papers as you can (the 8 test papers,
     plus other varied papers if you have them).
- Snapshots (`tests/data/snapshots/*.txt`) are hashes of each mode's HTML, of the Word document, of the
  Markdown, of the LaTeX, of the EPUB book, of the JSON and of the chunks.
- The editable JSON (`src/render/json/`) is a published format: a field that changes meaning or
  goes away needs a new `FORMAT_VERSION` and a breaking-change commit (`feat!:`); new fields
  do not.
  - Its read-only fields `citation` (references) and `meta.authors` (front matter) are read from
    the text by heuristics (`citation.rs`, `front.rs`): like a rule of the analysis, a change
    that fixes one citation style easily breaks another, so compare them on many papers
    (`json_references_have_citations` holds the lower bounds).
- The LaTeX output must compile: after a change to `src/render/latex/`, compile a few papers with
  `pdflatex` (and `xelatex` or `lualatex`) when TeX is installed; `tests/cli.rs` compiles the LIGO
  paper when `pdflatex` is on the `PATH`.
  - Changing the renderer's CSS, JS or assets also changes the snapshots, because they are
    embedded with `include_str!`.
  - Update the snapshots only when the output change is intended, and commit them with the code.
- The limits in `tests/real_papers.rs` (broken paragraphs, words still hyphenated, leftover
  characters, ...) matter: when a number gets better, tighten the limit; when it gets worse, it is
  a regression.
- A test for a specific bug goes in `tests/real_papers.rs` and is named after the behaviour, for
  example `ligo_radical_is_a_square_root`.

## Word input (`src/word`)

- Both readers (`docx.rs` for `.docx`, `doc/` for `.doc`) fill the same intermediate form
  (`ir.rs`); `build.rs` alone decides what each paragraph is. A rule about styles goes there.
- The test samples are `tests/data/word/sample.docx` (pandoc, from `sample.md`) and `sample.doc`
  (LibreOffice); a change to the reading shows in `tests/data/snapshots/word-sample.txt`.
- A Word document has no pages (`Document::pages` is empty) and its pictures are stored in
  `Document::pictures` (`ImageSpec::embedded`): renderers must not assume a page to draw from.
- The page layout (`src/word/fmt/` → `src/layout/`) is a second reading of the same file, with
  every formatting property: a layout rule (line height, row height, page break) is checked
  against Word's own PDFs with `scripts/word-fidelity.py` on the corpus of
  `tests/data/word-corpus.tsv` (downloaded, not committed). A change that raises the totals
  should raise the minimums in `.github/workflows/ci.yml` too; one that lowers them is a
  regression. `scripts/compare-layout.py REF.pdf OURS.pdf --diff` shows the lines that differ.

## Public API and packages

- The crate is published on crates.io and the Python package (`python/`) on PyPI, both by the
  release workflows. Only what `src/lib.rs` re-exports, plus `config`, `model` and `source`, is
  the public API; changing it is a breaking change (`feat!:`/`fix!:`). The `#[doc(hidden)]`
  modules are internals.
- `python/` is a separate crate, not a workspace member: run its `cargo fmt`/`clippy` from inside
  it. Its version is set from the release tag when wheels are built
  (`scripts/set-python-version.py`).
- The package on crates.io holds only what `include` in `Cargo.toml` lists.

## Renderer JS and CSS

- `src/render/friendly/{friendly.js,book.js,friendly.css}` and `src/render/hard95/` are written
  compactly on purpose. Do not run prettier or reformat whole files: the diff would be huge and
  every snapshot would change.
- The book view (`book.js`) measures layout with CSS columns. `check-friendly.js` checks it for
  near-empty pages, captions apart from their figures, and orphaned headings.
- **Test the book view only when the user asks for it.** Do not run `check-friendly.js` or other
  book-view checks (screenshots, pagination timing) on your own, even after a change to
  `book.js` or to page breaking; mention instead that the book view was not tested.

## Constraints

- **PDFs:** only the LIGO paper (`tests/data/1602.03837v1.pdf`, CC BY 4.0) is committed. Other
  arXiv papers may not be redistributed: download them with `scripts/fetch-test-pdfs.sh` and keep
  them out of git (`.gitignore` already has `*.pdf`).
- **Clippy:** CI uses a newer clippy than the local one, so it can report extra lints (for example
  `needless_bool`). Check the CI log if the "fmt + clippy" job fails.
- **Commits:** Conventional Commits (`fix(tables): ...`, `feat: ...`), in English, imperative
  mood.
- **Google Drive:** everything uploaded for this project (test corpora, evaluation kits, results)
  goes in the `kakemono` folder at the root of My Drive
  (id `1P1p1y-8wzjb_LNIIK9Y5zYBJiiUVLKRj`), in one subfolder per purpose
  (`kakemono-excel-corpus`, `kakemono-qasper-eval`, ...). Reuse a subfolder that fits; never
  create a project folder at the root of the Drive.

## Pull requests

- Title and description in English.
- **Assignee:** always `iamcaominhtien`.
- **Labels:** add them yourself from what the PR contains, one or more. Use the repository's
  existing labels:

  | PR content | Label |
  | --- | --- |
  | New feature or an improvement to the output (`feat`, `perf`) | `enhancement` |
  | Bug fix (`fix`) | `bug` |
  | Documentation (`docs`, README, ARCHITECTURE, CLAUDE.md) | `documentation` |

  A PR that mixes kinds gets every label that applies. If a PR fits none of these (for example
  CI only, or refactoring only), create a short, lowercase label that names it (`ci`,
  `refactor`, `tests`) rather than leaving the PR unlabelled.
- **SonarQube Cloud issues:** after the PR's checks finish, read the new issues SonarQube Cloud
  reports on it and fix them all in the PR, without waiting to be asked. They are public:
  `curl -s "https://sonarcloud.io/api/issues/search?componentKeys=iamcaominhtien_rust-pdf-parser&pullRequest=<N>&resolved=false"`.
  Recurring ones: `rust:S2208` (a wildcard `use x::*` outside a test module: import each name).
