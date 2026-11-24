# Web benchmark

How well kakemono finds the article of a web page, on some 330 pages, and a PDF to look
through for its mistakes. The pages are downloaded, not committed (they belong to
their sites), into `tests/data/web-bench/`; the results go to `out/web-bench/`.

- **181 pages with a ground truth**: the [Scrapinghub article extraction
  benchmark](https://github.com/scrapinghub/article-extraction-benchmark) (news and
  blog articles, with the text of each article marked by people). trafilatura and
  readability-lxml are scored on them too.
- **About 150 pages of [`pages.tsv`](pages.tsv)**, by category: Vietnamese news,
  international news, tech news, blogs (Medium, Substack, WordPress, Ghost, dev.to,
  personal sites), documentation (Sphinx, MDN, mdBook, Docusaurus, MkDocs, Read the
  Docs, rustdoc), Wikipedia in seven languages, science (arXiv HTML, PLOS, PMC,
  Nature), government sites, and pages that are not articles (home pages, Hacker
  News). Their reference is trafilatura's extraction: a reference, not the truth.
  `make-pages.py` writes the list (the newest articles of each feed, and fixed
  addresses).

The score is the one of the Scrapinghub benchmark: shingles of 4 words, precision
(how much of what kakemono kept belongs to the article), recall (how much of the
article it kept) and F1, averaged over pages. Every page is also written to every
output (Word, LaTeX, Typst, EPUB, JSON, chunks, PDF) to find failures and timeouts.

```bash
python3 -m venv /root/.venvs/web-bench && /root/.venvs/web-bench/bin/pip install -r scripts/web-bench/requirements.txt
cargo build --release
python3 scripts/web-bench/fetch.py                                   # once; pages already there are kept
/root/.venvs/web-bench/bin/python scripts/web-bench/evaluate.py      # scores: out/web-bench/results.json
/root/.venvs/web-bench/bin/python scripts/web-bench/report.py        # review PDFs: out/web-bench/review/*.pdf
```

Each page of a review PDF shows one tested page: its scores, the page as its site
shows it (the saved copy, with the site's style sheets and pictures), kakemono's
friendly HTML at its start, middle and end, and the sentences kakemono left out or
kept against the reference. The lowest scores come first. `report.py` keeps the
screenshots it made: delete `out/web-bench/review/` after a change to kakemono.
