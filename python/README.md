# kakemono for Python

Convert PDF papers and books (a local file or an arXiv id) into clean, readable HTML, Word
documents, Markdown, LaTeX, EPUB books, editable JSON or chunks for retrieval, and Word documents
into PDFs, from Python. Word documents (`.docx`, `.doc`), Excel workbooks (`.xlsx`), EPUB books (`.epub`) and web pages (an
`.html` file, or an `http(s)` address: the article of the page, with its pictures) are inputs too. The conversion is done by the [kakemono](https://github.com/iamcaominhtien/kakemono) Rust
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
typ = kakemono.to_typst("book.pdf", "book.typ")            # Typst, pictures in book_assets/
book = kakemono.to_epub("paper.pdf", "paper.epub")         # an EPUB book (also returns its bytes)
md = kakemono.to_markdown("https://en.wikipedia.org/wiki/Fourier_transform", "ft.md")  # a web page
content = kakemono.to_json("paper.pdf", "paper.json")      # the editable JSON, as a dict
doc = kakemono.analyze(open("paper.pdf", "rb").read())     # the structure: blocks in reading order
kakemono.to_pdf("report.docx", "report.pdf")              # a Word document laid out in pages as Word does
sheet = kakemono.to_markdown("sales.xlsx", header_rows=1)  # an Excel workbook: one table per region of each sheet
png = kakemono.render_svg("figure.svg", "figure.png", scale=2)  # an SVG drawn as a PNG
math = kakemono.read_formula("formula.png")               # a formula picture read as MathML (or None)
```

Each function reads and analyses its input again. To write several outputs of one document, open
it once as a `Document`; its methods take the same options, less those of the analysis (`tables`,
`jobs`, `png_compression`, `content`), which are given when it is opened. Writing six outputs of a
paper this way takes up to half less time:

```python
doc = kakemono.Document("paper.pdf")
content = doc.to_json("paper.json")
# ... edit content ...
vi = kakemono.Document("paper.pdf", content=content)
vi.convert("hard-95"); vi.to_docx("paper.vi.docx"); vi.to_epub("paper.vi.epub")
```

| Mode | What you get |
| --- | --- |
| `hard` | Each page redrawn as SVG/PNG, with a hidden selectable text layer |
| `hard-95` | The same pages, with visible HTML text placed as in the PDF |
| `medium` | Pages and columns kept, text reflowed |
| `friendly` | One responsive column with themes, contents, a phone layout and a book view |

Options of `convert` and `convert_all`: `tables`, `equations` (`"mathml"` or `"image"`),
`page_image` (`"svg"`, `"svg-only"`, `"png"`), `embed_fonts`, `scale`, `jobs` and
`png_compression`. They match the command-line options described in the
[main README](https://github.com/iamcaominhtien/kakemono#usage).

Every conversion function but `analyze`, `to_chunks` and `to_pdf` also takes `png_compression`: `"balanced"` (default), `"fast"` (pictures drawn
several times faster, PNG files about a quarter larger: for many documents on a slow machine) or
`"high"` (about a tenth smaller, ten times slower). The pixels are the same at every level.

`to_docx` takes `tables`, `equations`, `scale` and `jobs`. Formulas whose structure is clear become
Word equations (editable in Word), the others pictures; rebuilt tables become Word tables. With
`save_images=True` the pictures are also written as files to `image_dir` (default
`<name>_assets`, next to the document).

`to_markdown` takes the same options, plus `image_dir` (default `<name>_assets`, next to the
file), `inline_images` and `mode`. Formulas whose structure is clear become LaTeX between `$$`,
tables pipe tables: with `mode="friendly"` (the default) every table, its header rows joined and
its spanning cells repeated; with `mode="medium"` a table with spanning cells stays an HTML table.
Without a path, or with `inline_images=True`, the pictures are embedded as data URIs.

`to_latex` takes `tables`, `equations`, `scale`, `jobs` and `image_dir`, and returns a LaTeX
document that compiles with pdfLaTeX, XeLaTeX or LuaLaTeX: sections, `tabular` tables, LaTeX
math. LaTeX cannot embed pictures, so they are written to `image_dir` next to the file; without a
path, only the source is returned.

`to_typst` takes the same, and `layout` (`"auto"`, `"article"` or `"book"`), and returns a Typst
document that compiles with `typst compile`: a printed book (title page, chapters on right-hand
pages, running heads, footnotes on the page of their call) for a document with chapters, an
article otherwise; its settings (page size, fonts, language) are at the top of the file.

`to_epub` takes `tables`, `equations`, `scale` and `jobs`, plus `max_math_width_em` (displayed
formulas wider than this, in multiples of the text size, become pictures; default 20), `cover`
(the first page as the cover, on by default) and `modified` (the date recorded in the book, `YYYY-MM-DDThh:mm:ssZ`;
by default, now). It returns an EPUB 3 book with one chapter per top-level section, a table of
contents, MathML formulas and the pictures inside.

`to_json` takes the options of `to_markdown` but `mode`, and returns the editable JSON as a dict: the blocks
in reading order, each with its `type`, `section`, `layout` and `text`, a string whose tags
(`<i>`, `<b>`, `<a href>`, ...) keep the styles and links and where `<m id="n">` stands for a
formula. Every conversion function takes `content`: an edited copy of that JSON for the same
PDF (the dict, its text or a path to it), whose text the output then shows. Translating a paper
is editing the `text` fields:

```python
content = kakemono.to_json("paper.pdf")
for block in content["blocks"]:
    if "text" in block:
        block["text"] = my_translate(block["text"])   # keep the tags
html = kakemono.convert("paper.pdf", "hard-95", content=content)   # the pages, translated
kakemono.to_docx("paper.pdf", "paper.vi.docx", content=content)
```

The JSON also has read-only fields for search and data extraction: `plain` on every block (the
text without tags, formulas as `$LaTeX$`), `citation` on every reference (`authors`, `title`,
`venue`, `year`, `volume`, `pages`, `doi`, `arxiv`, `url`, as far as they could be read) and, in
`meta`, the `authors` (with indices into `affiliations` and `author_notes`, and their `emails`),
`arxiv_category`, `date`, `doi`, `keywords` and `abstract`:

```python
content = kakemono.to_json("1706.03762")
print([a["name"] for a in content["meta"]["authors"]])
refs = [b["citation"] for b in content["blocks"] if b.get("citation")]
print(refs[0]["title"], refs[0]["year"])
```

Blocks left unchanged keep their text; a block whose edited text cannot be read (an unknown tag)
keeps its own, with a `UserWarning`. The `hard` mode cannot show edited text. A figure's
`replace_image` that is a relative path is read from the folder of the edited JSON file (from the
current folder when `content` is a dict or text).

`to_pdf` lays out a Word document (`.docx`, `.doc`) or an EPUB book in pages as Word does and returns the PDF
(with `content`, the edited document); a PDF input is a `ValueError`.

`render_svg(svg, path=None, *, scale=1.0, resources_dir=None)` draws an SVG (a path, or the SVG
as text or bytes) as a PNG, as `kakemono render-svg` does: the same fonts (the system's and those
of `KAKEMONO_FONT_DIRS`, loaded once per process), pictures named by relative paths read from the
SVG's folder or `resources_dir`. It returns the PNG's bytes, and raises a `ValueError` saying what
is wrong in an SVG it cannot draw.

`read_formula(image)` reads a picture of a formula (PNG or JPEG bytes or path, black on white as
LaTeX renders it), as `kakemono formula` does, and returns its MathML, or `None` when it cannot be
read with confidence.

The GIL is released during a conversion, so several PDFs can be converted in parallel threads.

Every conversion can be stopped: `cancel=` takes a `kakemono.CancelToken`, whose `cancel()`,
called from another thread, makes it raise `kakemono.Cancelled`, and `timeout=` (seconds) raises
`TimeoutError`. The work stops at its next stopping point, normally within a page, and writes no
file:

```python
token = kakemono.CancelToken()
threading.Timer(5, token.cancel).start()       # or from a "Stop" button
md = kakemono.to_markdown("paper.pdf", cancel=token)
md = kakemono.to_markdown("paper.pdf", timeout=30)
```

`kakemono.aio` has the same functions to be awaited in asyncio programs: each runs in a worker
thread, so the event loop is not blocked while a document is fetched, read and written, and
several run in parallel. `aio.Document.open` reads a document once. Cancelling a task (or
`asyncio.wait_for`, `asyncio.timeout` from Python 3.11, `timeout=`) stops its conversion, and the
cancellation is raised once it has stopped (at once for a conversion still waiting for a thread):

```python
import asyncio
from kakemono import aio

async def main(paths):
    books = await asyncio.gather(*(aio.to_markdown(p) for p in paths))
    async with asyncio.timeout(60):
        doc = await aio.Document.open("paper.pdf")
        await asyncio.gather(doc.to_docx("out/paper.docx"), doc.to_epub("out/paper.epub"))
    # A document that is not trusted: in a process of its own, killed at once when cancelled
    md = await aio.to_markdown("upload.docx", isolate=True, timeout=30)
```

With `isolate=True` a function runs in a Python process of its own, killed by a cancellation or
its `timeout`: a hard limit, even for a document that would make the library hang or crash
(`aio.ProcessError`). It costs the start of a process (about a tenth of a second) and the copy of
the arguments and of the result; a killed conversion may leave a file it was writing half written.

### Many documents at once

`aio.convert_many` converts many documents, a number at a time, each into every format asked for
(it is read and analysed once for all of them), and yields a result for each one as soon as it is
done:

```python
results = aio.convert_many(
    paths,                              # paths, arXiv ids, web addresses, bytes or (name, source)
    "out",                              # out/<name>.docx, out/<name>.md, out/<name>_assets/, ...
    ["docx", "epub", "md", "pdf"],      # also html, friendly, medium, hard, hard-95, tex, typ, json, jsonl
    limit=50,                           # documents at once (default: one per core)
    isolate=False,                      # True: each document in a process of its own
    timeout=120,                        # seconds per document, from when it starts
)
async for result in results:
    if result.ok:
        print(result.source, result.outputs)          # {"docx": "out/x.docx", ...}
    else:
        print(result.source, result.error or result.errors)
```

A document that fails does not stop the others: `result.error` is what stopped it (it could not
be read, `TimeoutError`, `kakemono.Cancelled`) and `result.errors` the formats it has not (a PDF
has no `pdf` output; a Word document has no `medium`, `hard` or `hard-95` mode). `options` passes
the options of each format, and those of `Document` under `"document"`:
`options={"document": {"tables": False}, "md": {"mode": "medium"}}`. Cancelling the task that
iterates (or a `cancel=` token) stops every document.

`convert_many` sets up its threads itself: `limit` threads of its own, and each document gets the
cores shared out among them (`jobs`, one thread each when `limit` is at least the number of cores).
With `isolate=True`, `limit` Python processes run at once: mind the memory.

### How many conversions run at once

The other functions of `kakemono.aio` run in the event loop's default executor, which has at most
`min(32, os.cpu_count() + 4)` threads: more conversions started at once wait for a thread. To run
more at once, give the loop a larger executor, and give each conversion fewer threads of its own
(`jobs`, by default one per core):

```python
from concurrent.futures import ThreadPoolExecutor

loop = asyncio.get_running_loop()
loop.set_default_executor(ThreadPoolExecutor(max_workers=50))
gate = asyncio.Semaphore(50)

async def one(path):
    async with gate:
        return await aio.to_markdown(path, jobs=1, timeout=120)
```

Isolated conversions (`isolate=True`) do not use the executor: each starts its process at once
and uses every core, so bound them with an `asyncio.Semaphore` like this one.

MIT license.
