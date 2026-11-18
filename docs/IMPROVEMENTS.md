# Possible improvements (not done yet)

Notes on points that were measured or found but not fixed, with the numbers and the options
considered, so we can come back to them later.

## 1. Photos are compressed twice in `hard` / `hard-95`

**Current state.** The page background of `hard` and `hard-95` is an SVG drawn by the `hayro-svg`
library. The library decodes every image in the PDF to pixels and writes it back as PNG (safe for
every image format, color space and transparency mask in PDF). Because PNGs of photos are very
large, `compact_images` in `src/pdf/render.rs` decodes the large PNGs again and compresses them to
JPEG (quality 88, longest side at most 2600 px). The result: an image that is already a JPEG in the
PDF gets JPEG-compressed a second time, and goes through the loop
JPEG → pixels → PNG → pixels → JPEG.

**Numbers** (Faster R-CNN paper, 1506.01497, measured with callgrind, one thread): about 70% of the
`hard-95` convert time goes into this loop: PNG encoding in the library ≈ 12%, PNG decoding ≈ 16%,
JPEG encoding ≈ 40%. Time: 2.4 s (1 thread), 1.2 s (4 threads); `medium`/`friendly` take only
0.7–0.8 s.

Number of images that could keep their original JPEG (JPEG, RGB or gray, no transparency mask):

| Paper | Eligible images / total |
| --- | --- |
| Faster R-CNN (1506.01497) | about 520 / 681 |
| ViT (2010.11929) | 254 / 304 |
| GPT-4 (2303.08774) | 7 / 13 |
| BERT (1810.04805) | 0 / 68 (all JPEG images have a transparency mask) |
| Attention, Adam, ResNet, LIGO | no JPEG images |

**Why we cannot do it from outside the library.** `hayro-svg` does not expose its image drawing
(`SvgRenderer` is an internal type, and images are decoded before they reach any place we can hook
into). `hayro-interpret` does let us get the raw data: `RasterImage::stream()`.

**Options:**

| | Approach | Patch the library? | Pros | Cons |
| --- | --- | --- | --- | --- |
| A | Use a faster JPEG encoder for the recompression step | No | Estimated 20–30% faster on image-heavy papers | Still compresses twice |
| B | Before drawing, replace each JPEG image in the PDF with a 1-pixel image that carries an ID (using an incremental update), draw the SVG, then swap the original JPEG back in | No | Close to D | Complex, fragile with unusual PDFs, hard to maintain |
| C | Send hayro a pull request: an option to embed the original JPEG when possible | No (fix goes upstream) | Cleanest | Depends on whether they accept it |
| D | Copy `hayro-svg` into the repo (`[patch.crates-io]`), change about 30 lines in `draw_image` | Yes | Fast, images identical to the PDF, smaller files | Must reapply the patch on every hayro upgrade |

The license of `hayro-svg` is `Apache-2.0 OR MIT`, so D is allowed if we keep the copyright notice,
include `LICENSE-MIT`/`LICENSE-APACHE` and state clearly which files were changed.

Two trade-offs apply to every way of embedding the original JPEG: colors may shift very slightly when
the PDF attaches an ICC color profile to the image (browsers ignore that profile); CMYK JPEGs must be
excluded (browsers often show wrong colors).

**Current decision:** leave as is. Preferred direction when we come back: A + C.

## 2. Parallelism is incomplete

`-j/--jobs` runs page reading, line building and page/image drawing in parallel. The rest of the
layout analysis and HTML building still run on one thread, so on 4 cores the speedup is only
about 2.5x (see section 10). A paper with a few image-heavy
pages has uneven load (one thread takes the heaviest page).

## 3. Printing pages whose size differs from the paper

When printing `hard`/`hard-95` (Ctrl+P), each page is stretched to the paper width. A page with a
narrower aspect ratio than Letter (for example Faster R-CNN, 567 × 774 pt) is taller than the sheet,
so it spills onto the next sheet: 14 pages become 28 sheets. Fix: in `@media print`, limit the page
by height as well, or set `@page { size: <PDF page size> }`.

## 4. Leftovers in `hard-95`

- The ViT paper (2010.11929) still has about 2381 characters left in the background image instead of
  being HTML text; the cause has not been investigated. Other papers: BERT 0, LIGO 3, Attention 85,
  Faster R-CNN 188.
- Some lines placed by position (`.pl`) do not yet match the original width (Adam 49 lines, ViT 36
  lines).

## 5. `medium` / `friendly`

- Display equations whose structure cannot be rebuilt stay images (about 7% of them, see section 8).

## 6. Very long documents (about 1000 pages)

Tested with "Dive into Deep Learning" (arXiv 2106.11342, 1185 pages, 39.5 MB PDF), on a 4 vCPU
machine:

| Mode | 4 threads | 1 thread | Peak RAM | Output file | Open in Chromium |
| --- | --- | --- | --- | --- | --- |
| `friendly` | 7.9 s | 20.5 s | 0.9–1.1 GB | 105 MB | 5.4 s |
| `medium` | 7.5 s | – | 1.1 GB | 105 MB | – |
| `hard-95` | 10.9 s | 25.1 s | 1.0–1.2 GB | 142 MB | 9.7 s |
| `hard` | 10.3 s | – | 1.7 GB | 481 MB | 13.7 s |

- **RAM:** (since about halved, see section 10) reading and analysis alone need about 900 MB,
  because all characters of all pages are kept in memory at once (each character is a struct with its own `String`). Possible fixes: a more
  compact character representation; drop the raw page data right after the lines are built; write the
  HTML to the file in pieces instead of holding the whole string.
- **Opening the file:** an HTML file of 100–500 MB opens slowly (5–14 s). Possible fix: split a long
  document into several files by chapter, with a table of contents.

## 7. Batch processing

- There is no command for processing many documents. For now you need an outside script, for example
  `ls *.pdf | xargs -P 2 -I{} kakemono convert {} -m hard-95 -j 1 -o out/`.
  We should add a `batch` command: run N documents in parallel, skip documents that are already done,
  log errors.
- Downloads from arXiv are not rate-limited. arXiv asks automated tools to download slowly (their API
  guidelines say about 1 request every 3 seconds); downloading thousands of papers in a row may get
  blocked. We need to add rate limiting, or use arXiv's bulk data store.

## 8. Formulas rebuilt as MathML from PDFs

**Current state.** On 73 arXiv papers, 2607 of 2822 display formulas are written as MathML. They
were checked by overlaying each MathML rendering on the picture of the formula (see "How to measure"
below): 31 formulas score under 0.5 (from 106 before the fixes of that work), median 0.917.

**Errors seen but not fixed:**

- `\underbrace` and brace pieces (`⎪`) are not supported (the formula stays a picture).
- `argmin` with a long limit under it: part of the limit is read as a subscript.
- A `∑` limit merged with text next to it.
- A hyphen in a word inside a formula (`log-sum-exp`) is read as a minus sign.
- Short explanation lines (`where m̃ = m + max{sᵢ}`, `when z = g(…)`) still join the display above
  them: they have too few sentence words (the rule needs four) to be told from a formula.
- Inline fractions in a sentence: the numerators are given back to the sentence, but their text is
  interleaved with the denominators (`v1/2 αt,t i`).
- The picture of some formulas is cropped too tightly (the top of the formula is cut).

**How to measure (scripts kept outside the repository).** Convert many papers twice, with
`--equations mathml` and with `--equations image` (`medium`, `--images files`). Pair the
equations of the two outputs by order. Take a screenshot of each `<math>` in Chromium with the
embedded math font at 20 px, crop the ink of both pictures, scale the screenshot to the box of the
picture, and score how much ink of each is covered by ink of the other, with each 40 px tile
allowed to shift ±30 px sideways and ±4 px vertically (spacing drifts; misplaced scripts do not).
Sort by score, look at the worst, fix by kind of error, run again and compare with the previous
run by picture (only formulas whose MathML changed need new screenshots). Pairing breaks when the
number of equation blocks changes, so pictures must be rebuilt in the same run.

## 9. Formulas from pictures (`kakemono formula`)

**Current state.** Experimental. The ink of the picture is cut into pieces and each is compared
with glyphs of the TeX fonts (Computer Modern at 10/7/5 pt, AMS fonts, `dsfont`, `rsfs`), then the
glyphs go through the same builder as formulas of PDFs. On the 2607 formula pictures of 73 papers
(the answer being the MathML rebuilt from the PDF text): 58% read, 30% give exactly the same text,
93% of the characters of the pictures read are right. Steps that made a difference: shapes from the
grey level of pixels; boxes from the dark core of the ink (the anti-aliased edge shifted thin
glyphs); templates from the real TeX fonts at their optical sizes (from 44%/22% with Latin Modern);
advance boxes from the font metrics; size of flat symbols (`=`, `→`) from their width.

**Confidence.** Each reading has a confidence (likeness of its least sure glyph, lead over the
best other glyph; `formula --scores`). It does not separate right from wrong readings yet: with
thresholds chosen on half the papers for 95%, the other half gets 77% at 5% of the pictures; for
80%, 74% at 19%. So no threshold is applied. The goal was 95% right among the readings given.

**Most frequent misreadings:** `|` read as `]` (56), comma read as `.` (51) or `′` (40), `D` as
`𝒟` (42), `.` as `·` (37), `F` as `Γ` (25), `1` as `I` (25), `)` as `⟩` (18), `f` as `∫` (17). The
paper 1303.5113 (Fraktur, unusual symbols) makes up much of the unread pictures. Pictures in other
fonts (Times, as in the LIGO paper) are not read.

**Ideas, not done:**

- Tell `|` from `]` by the rows of the piece: a straight stroke has rows of equal width centred at
  the same place. A first try made `;` and `]` read as `!`, and was dropped.
- Use context more: `1` against `I`, `D` against `𝒟` by the letters around them; a comma on the
  baseline after a subscript.
- A stronger confidence: draw the glyphs read at their place and compare with the picture, or
  render the MathML and overlay it on the picture (as in section 8, which needs a browser).
- For pictures not set in TeX fonts, a learned model (pix2tex, Texify) would do better than
  templates.

**How to measure (scripts kept outside the repository).** Run `kakemono formula` on the
equation pictures of the papers of section 8 (several pictures per call; one line per picture),
compare the text of the `mi`/`mn`/`mo` elements with that of the MathML from the PDF, counting
look-alikes as equal (`∆`/`Δ`, `φ`/`ϕ`, `‖`/`||`), and list the confusions of readings of the
same length.


## 10. Benchmark (November 2026) and leads for speed and memory

Measured with `scripts/benchmark.py` (release build) on a 4-core Xeon at 2.1 GHz with 16 GB of RAM.
Inputs: the 8 test papers, three books (Pride and Prejudice, The Great Gatsby, Think Python 2),
"Mathematics for Machine Learning" (`mml`, 417 pages), "Dive into Deep Learning" (`d2l`, 1151
pages) and `d2l` joined to itself with `pdfunite` (`d2l-x2`, 2302 pages, 93 MB) as a stress test.
To run it again:

```bash
scripts/benchmark.py formats tests/data/*.pdf book.pdf --formats friendly,medium,hard,hard-95,epub,docx,md,tex --csv
scripts/benchmark.py threads big.pdf
scripts/benchmark.py concurrent paper.pdf --max 32 --jobs 1
```

### One file at a time (default `-j`, i.e. 4 threads)

Wall time / peak resident memory. "Analysis" is the time the CLI reports for reading and analysis;
the rest is rendering and writing.

| PDF | Pages | Analysis | friendly | epub | docx | md / tex | hard | hard-95 |
| --- | ---: | ---: | --- | --- | --- | --- | --- | --- |
| Adam (1412.6980) | 15 | 0.14 s | 0.22 s / 54 MB | 0.38 s / 86 MB | 0.31 s / 57 MB | 0.2 s / 57 MB | 0.2 s / 48 MB | 0.22 s / 44 MB |
| Faster R-CNN (1506.01497) | 14 | 0.25 s | 0.64 s / 101 MB | 0.6 s / 112 MB | 0.59 s / 103 MB | 0.6 s / 103 MB | 1.5 s / 159 MB | 1.68 s / 158 MB |
| Attention (1706.03762) | 15 | 0.13 s | 0.39 s / 91 MB | 0.38 s / 119 MB | 0.41 s / 88 MB | 0.43 s / 89 MB | 0.21 s / 77 MB | 0.24 s / 79 MB |
| GPT-4 (2303.08774) | 100 | 0.85 s | 1.32 s / 239 MB | 1.34 s / 250 MB | 1.3 s / 240 MB | 1.2 s / 240 MB | 1.3 s / 209 MB | 1.34 s / 163 MB |
| Gatsby | 193 | 0.63 s | 0.69 s / 113 MB | 0.72 s / 124 MB | 0.71 s / 111 MB | 0.72 s / 111 MB | 0.87 s / 175 MB | 1.01 s / 113 MB |
| Pride and Prejudice | 479 | 1.7 s | 1.91 s / 248 MB | 1.89 s / 260 MB | 1.84 s / 247 MB | 1.74 s / 247 MB | 2.71 s / 418 MB | 2.64 s / 254 MB |
| Think Python 2 | 244 | 1.1 s | 1.22 s / 194 MB | 1.4 s / 225 MB | 1.13 s / 191 MB | 1.2 s / 189 MB | 1.71 s / 291 MB | 1.81 s / 196 MB |
| mml | 417 | 2.9 s | 3.94 s / 509 MB | 5.02 s / 533 MB | 4.04 s / 515 MB | 4.05 s / 483 MB | 5.15 s / 794 MB | 6.05 s / 651 MB |
| d2l | 1151 | 8.1 s | 10.18 s / 1.20 GB | 10.99 s / 1.24 GB | 10.7 s / 1.24 GB | 10.45 s / 1.14 GB | 12.73 s / 1.76 GB | 13.37 s / 1.36 GB |
| d2l-x2 | 2302 | 20.3 s | 24.44 s / 2.36 GB | 26.12 s / 2.36 GB | 25.5 s / 2.40 GB | 25 s / 2.23 GB | 30.42 s / 3.45 GB | 33.34 s / 2.58 GB |

The full table (every paper, CPU share, output size) is printed by the `formats` experiment.

### Threads for one file (`-j`, `friendly`)

| PDF | -j 1 | -j 2 | -j 4 |
| --- | --- | --- | --- |
| GPT-4, 100 pages | 2.33 s (analysis 0.99 s), 143 MB | 1.57 s (×1.48) | 1.30 s (×1.79, analysis 0.87 s), 257 MB |
| d2l, 1151 pages | 17.2 s (analysis 9.4 s), 0.92 GB | 12.5 s (×1.38) | 10.3 s (×1.68, analysis 8.0 s), 1.21 GB |

### Several files at once (`friendly`)

| PDF | At once | `-j` each | Total | Throughput | Memory of the machine |
| --- | ---: | --- | ---: | --- | ---: |
| Attention, 15 pages | 1 / 4 / 8 / 16 / 32 | 1 | 0.75 / 0.86 / 1.69 / 3.19 / 6.5 s | 80 / 278 / 284 / 301 / 296 files/min | ~52 MB per process |
| Attention, 15 pages | 1 / 4 / 16 | auto | 0.45 / 0.96 / 3.5 s | 134 / 249 / 274 files/min | ~95 MB per process |
| d2l, 1151 pages | 1 / 2 / 4 / 8 | auto | 11.1 / 12.8 / 17.2 / 35.6 s | 103 / 179 / 268 / 259 pages/s | 1.2 / 2.1 / 4.4 / 8.9 GB |
| d2l-x2, 2302 pages | 1 / 2 / 4 | auto | 24.7 / 29.3 / 38.9 s | 93 / 157 / 237 pages/s | 2.2 / 4.5 / 8.9 GB |

- Throughput stops growing at the number of cores (4); more processes only queue and add memory.
- For many small files, one process per core with `-j 1` is best (~300 papers a minute on 4 cores).
- 16 GB holds about 12 conversions of a 1000-page book at once, but on 4 cores more than 4 bring
  nothing.

### Done after this benchmark

Measured on a 4-vCPU cloud machine (slower than the Xeon above, and noisy: each number is the best
of two runs, old and new build alternating). `d2l` is the 1185-page version of the book (39.5 MB),
`d2l-x2` the same joined to itself (2370 pages). Every output stays byte for byte the same (the
snapshots did not change).

| PDF, mode | Before | After |
| --- | --- | --- |
| d2l, `friendly` | 10.6 s / 1.20 GB | 6.3 s / 645 MB |
| d2l-x2, `friendly` | 25.3 s / 2.32 GB | 13.1 s / 1.24 GB |
| d2l, `hard` | 13.6 s / 1.73 GB | 8.9 s / 1.28 GB |
| d2l, `friendly`, `-j 1` | 18.8 s / 919 MB | 16.6 s / 574 MB |
| GPT-4, `friendly` | 1.33 s / 245 MB | 0.93 s / 208 MB |
| Attention, `friendly` | 0.43 s / 95 MB | 0.39 s / 78 MB |

- **The super-linear part was `fit_equation_crops`** (`analysis/crops.rs`), not the stages
  suspected below: for each page it went through every line of every block of the document to
  find the glyphs of that page (3.3 s on d2l, 9.9 s on d2l-x2). The blocks of each page are now
  indexed once (0.08 s and 0.15 s). Time per page is now the same for d2l and d2l-x2.
- **Lines are built on `-j` threads** (`build_lines`, `join_math_letters`, `take_ruby`: 3.1 s
  on one thread, 0.9 s on four), each worker freeing the raw glyphs of its page.
- **The lines of a page are dropped once it is laid out**: its blocks hold copies of the lines
  they need, and the later stages only read the page's line count and its glyphs per math font.
- **mimalloc in the command line.** With glibc, memory freed by another thread than the one that
  allocated it stays reserved: dropping the lines built on the workers gave nothing back
  (`MALLOC_ARENA_MAX=1` showed the gain, at twice the time). mimalloc v2 with an immediate purge
  (`purge_delay` 0, set in `main.rs`) gives the memory back and is faster. mimalloc v3, or v2
  with its default delay, made small papers take up to three times the memory of glibc.

- **The HTML is written as its pictures are drawn** (`Paper::write_html`, used by the command
  line). Before, every picture was drawn into memory, then the whole page was built as one more
  string: `hard` on d2l held 348 MB of page pictures and a 472 MB page at once. Now the workers
  draw at most 4 pictures per thread ahead of the one the HTML is written up to
  (`parallel::map_ordered`), and the page goes straight to the file. Peak memory of d2l with
  4 threads: `hard` 1.31 GB → 640 MB, `hard-95` 877 → 680 MB, `friendly` 656 → 582 MB; the time
  does not change. Markdown, LaTeX and EPUB draw their pictures the same way, but still build
  their output in memory.

Where the time of d2l goes now (4 threads, analysis 3.6 s of 6.3 s): reading the PDF 0.45 s,
lines 0.85 s, furniture 0.5 s, floats/reading order/blocks 0.75 s (one thread), classification
0.3 s, building the document 0.4 s, then 2.3 s of rendering.

### The benchmark again: v0.9.0 against these changes

The three experiments of `scripts/benchmark.py`, run with the v0.9.0 binary and then with the new
one, on the same 4-vCPU cloud machine (16 GB), one run each: differences of 0.1–0.2 s on the
papers are noise. Wall time / peak resident memory, v0.9.0 → now:

| PDF | Pages | friendly | hard | hard-95 | epub | docx | md |
| --- | ---: | --- | --- | --- | --- | --- | --- |
| GPT-4 | 100 | 1.86 → 1.06 s / 250 → 203 MB | 1.22 → 1.02 s / 213 → 126 MB | 1.83 → 1.10 s / 170 → 125 MB | 1.89 → 1.13 s / 250 → 235 MB | 1.97 → 0.99 s / 240 → 204 MB | 1.76 → 0.97 s / 238 → 202 MB |
| d2l | 1185 | 13.9 → 7.3 s / 1198 → 573 MB | 14.8 → 9.9 s / 1747 → 631 MB | 17.6 → 10.6 s / 1352 → 655 MB | 13.3 → 8.2 s / 1232 → 894 MB | 13.3 → 7.1 s / 1234 → 633 MB | 12.6 → 6.8 s / 1138 → 615 MB |
| d2l-x2 | 2370 | 30.1 → 13.8 s / 2328 → 1020 MB | 35.1 → 19.9 s / 3426 → 1089 MB | 39.2 → 24.5 s / 2505 → 1170 MB | 28.5 → 16.2 s / 2332 → 1712 MB | 29.5 → 13.5 s / 2370 → 1232 MB | 28.7 → 14.1 s / 2196 → 1099 MB |

- All 80 conversions (the 8 test papers, d2l and d2l-x2, in the 8 formats): 400 s → 227 s.
  Summed over the 8 formats: d2l-x2 1.9× faster, d2l 1.7×, GPT-4 1.6×, the 15-page papers
  1.05–1.5×. The memory of the papers stays within about 10 MB of v0.9.0.
- `medium` and `tex` are like `md` (d2l: 7.7 s / 564 MB and 7.2 s / 611 MB).
- EPUB now takes the most memory (894 MB on d2l): the book and the Word document are still built
  in memory, not written as they are drawn like the HTML.

Threads for one file (d2l, `friendly`):

| Build | `-j 1` | `-j 2` | `-j 4` | Speedup at `-j 4` |
| --- | --- | --- | --- | --- |
| v0.9.0 | 22.9 s / 912 MB | 15.4 s | 12.2 s / 1203 MB | 1.87× |
| now | 16.9 s / 499 MB | 11.0 s | 6.9 s / 572 MB | 2.44× |

Several files at once:

| | v0.9.0 | now |
| --- | --- | --- |
| Attention, 4 processes with `-j 1` | 207 papers a minute | 283 papers a minute |
| Attention, 8 processes with `-j 1` | 264 papers a minute | 295 papers a minute |
| d2l, 4 at once (default `-j`) | 21.8 s, 4.4 GB | 18.6 s, 3.1 GB |

**A small machine:** one core (`taskset -c 0`) and a hard limit of 512 MB without swap (a cgroup
v1 memory limit: past it, the kernel kills the process).

| PDF | `friendly` | `hard` |
| --- | --- | --- |
| Attention, GPT-4 | both builds | both builds |
| d2l (1185 pages) | v0.9.0 killed; now done in 18.4 s | both killed |
| d2l-x2 (2370 pages) | both killed | both killed |

A 1000-page book in `friendly` now fits in 512 MB, but only just; longer books, and `hard`, do
not yet. The leads below on memory (glyphs kept in the `Document`, EPUB and Word written as they
are drawn, a memory limit with glyphs written to a temporary file) are what is still needed.

### Leads (not done yet)

- **Drawing the pictures is the rendering.** Writing the HTML of d2l takes 0.15 s; drawing its
  651 pictures takes 9.4 s on one thread (2.4 s on four): 5.1 s to rasterize, 3.0 s to compress
  the PNGs with the `png` crate's default (`Balanced`) level, the rest to try JPEG for photos.
  The `Fast` level (fdeflate) compresses the same PNGs in 0.75 s, but they are 28% larger
  (59 MB instead of 46 MB); `High` takes 26 s for 9% less. The default stays `Balanced`, since
  a file is converted once and read many times; `--png-compression fast` (`Paper::
  with_png_compression`) is there for converting many documents on a slow machine: d2l in
  `friendly` on one thread takes 14.8 s instead of 18.3 s, for an HTML file of 70.7 MB instead of
  55.3 MB. A few more photographs become JPEG with it, since the JPEG is compared with a larger
  PNG. Page backgrounds drawn through resvg (an SVG too large to keep) still use its own encoder.
- **The rest of the per-page analysis** (`layout_page`: floats, reading order, blocks; 0.75 s on
  d2l) could run on `-j` threads like the lines; only the document-wide passes need one thread.
- **EPUB and Word built in memory.** They draw their pictures a few at a time like the HTML, but
  build the whole book or document (and its ZIP) in memory before writing it: EPUB takes 894 MB
  on d2l against 573 MB for `friendly`. Writing the ZIP entries to the file as they are made
  (with data descriptors, and ZIP64 past 4 GB) would bring them to the level of the HTML.
- **Memory of the analysis** is now most of the peak. The `Document` of d2l holds 1.9 million
  glyphs (`Block.lines`, `inner_lines`, `loose`), which only `hard`/`hard-95` read: about 250 of
  its 463 MB (one thread), about 107 bytes a glyph with its `String`. A compact glyph type, and a `Document` without glyphs for the
  reflowed outputs, are the next steps; `model::Line` is public API, so this needs care.
- **Machines with little memory** (for example 512 MB, one core) and documents of any size: a
  memory limit (`--memory-limit`, detected from cgroups and the free memory when not given) that
  picks fewer threads, streamed output and, as a last resort, glyphs of finished pages written
  to a temporary file. Splitting the analysis into chunks of pages was considered and set aside:
  several passes are document-wide (the vocabulary used to remove hyphens, heading levels from
  the table of contents, paragraphs that run over several pages), so chunks could not give the
  same output.
- **EPUB on mml is 1.1 s slower than `friendly`** (5.0 s vs 3.9 s): probably the pictures drawn for
  formulas wider than `max_math_width_em`. Worth checking with a profile.
- **`hard` / `hard-95` on photo-heavy papers** (Faster R-CNN: 1.5–1.7 s vs 0.6 s): the double image
  compression of section 1.
- **Batch command** (section 7): the numbers above give its default, one process per core with
  `-j 1`; with a memory limit it should also run no more processes than fit in memory.
