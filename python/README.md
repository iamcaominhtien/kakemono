# kakemono for Python

Convert PDF papers and books (a local file or an arXiv id) into clean, readable HTML, from
Python. The conversion is done by the [kakemono](https://github.com/iamcaominhtien/kakemono) Rust
library, bundled in the wheel: no poppler, mupdf or other system dependency.

```bash
pip install kakemono
```

```python
import kakemono

html = kakemono.convert("paper.pdf")                       # friendly mode, one self-contained page
html = kakemono.convert("1706.03762", mode="medium")       # an arXiv id or URL works too
pages = kakemono.convert_all("paper.pdf")                  # {"hard": ..., "hard-95": ..., "medium": ..., "friendly": ...}
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

The GIL is released during a conversion, so several PDFs can be converted in parallel threads.

MIT license.
