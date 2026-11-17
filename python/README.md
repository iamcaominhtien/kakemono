# kakemono for Python

Convert PDF papers and books (a local file or an arXiv id) into clean, readable HTML, Word
documents, Markdown, LaTeX or EPUB books, from Python. The conversion is done by the [kakemono](https://github.com/iamcaominhtien/kakemono) Rust
library, bundled in the wheel: no poppler, mupdf or other system dependency.

```bash
pip install kakemono
```

```python
import kakemono

html = kakemono.convert("paper.pdf")                       # friendly mode, one self-contained page
html = kakemono.convert("1706.03762", mode="medium")       # an arXiv id or URL works too
pages = kakemono.convert_all("paper.pdf")                  # {"hard": ..., "hard-95": ..., "medium": ..., "friendly": ...}
kakemono.to_docx("paper.pdf", "paper.docx")               # a Word document (also returns its bytes)
text = kakemono.to_markdown("paper.pdf", "paper.md")       # Markdown, pictures in paper_assets/
tex = kakemono.to_latex("paper.pdf", "paper.tex")          # LaTeX, pictures in paper_assets/
book = kakemono.to_epub("paper.pdf", "paper.epub")         # an EPUB book (also returns its bytes)
doc = kakemono.analyze(open("paper.pdf", "rb").read())     # the structure: blocks in reading order
```

| Mode | What you get |
| --- | --- |
| `hard` | Each page redrawn as SVG/PNG, with a hidden selectable text layer |
| `hard-95` | The same pages, with visible HTML text placed as in the PDF |
| `medium` | Pages and columns kept, text reflowed |
| `friendly` | One responsive column with themes, contents, a phone layout and a book view |

Options of `convert` and `convert_all`: `tables`, `equations` (`"mathml"` or `"image"`),
`page_image` (`"svg"`, `"svg-only"`, `"png"`), `embed_fonts`, `scale` and `jobs`. They match the
command-line options described in the [main README](https://github.com/iamcaominhtien/kakemono#usage).

`to_docx` takes `tables`, `equations`, `scale` and `jobs`. Formulas whose structure is clear become
Word equations (editable in Word), the others pictures; rebuilt tables become Word tables. With
`save_images=True` the pictures are also written as files to `image_dir` (default
`<name>_assets`, next to the document).

`to_markdown` takes the same options, plus `image_dir` (default `<name>_assets`, next to the
file) and `inline_images`. Formulas whose structure is clear become LaTeX between `$$`, tables
pipe tables. Without a path, or with `inline_images=True`, the pictures are embedded as data URIs.

`to_latex` takes `tables`, `equations`, `scale`, `jobs` and `image_dir`, and returns a LaTeX
document that compiles with pdfLaTeX, XeLaTeX or LuaLaTeX: sections, `tabular` tables, LaTeX
math. LaTeX cannot embed pictures, so they are written to `image_dir` next to the file; without a
path, only the source is returned.

`to_epub` takes `tables`, `equations`, `scale` and `jobs`, plus `cover` (the first page as the
cover, on by default) and `modified` (the date recorded in the book, `YYYY-MM-DDThh:mm:ssZ`;
by default, now). It returns an EPUB 3 book with one chapter per top-level section, a table of
contents, MathML formulas and the pictures inside.

The GIL is released during a conversion, so several PDFs can be converted in parallel threads.

MIT license.
