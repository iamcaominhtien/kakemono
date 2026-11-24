#!/usr/bin/env python3
"""Writes scripts/web-bench/pages.tsv: the web pages of the benchmark (besides the
pages with a ground truth of the Scrapinghub benchmark, which fetch.py downloads).

News sites and blogs change every day: their newest articles are taken from their
feeds (or home pages) once, and the list is kept. Documentation, encyclopedias,
science and government pages are fixed addresses. Run again to make a new list;
the pages already downloaded are kept by fetch.py under their name.

    python3 scripts/web-bench/make-pages.py
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import urljoin, urlparse

HERE = Path(__file__).resolve().parent
USER_AGENT = "Mozilla/5.0 (compatible; kakemono-bench; +https://github.com/iamcaominhtien/kakemono)"

# (category, site, feed or home page, how many articles, pattern of an article's address)
FEEDS = [
    ("news-vi", "vnexpress", "https://vnexpress.net/rss/tin-moi-nhat.rss", 8, r"vnexpress\.net/.+-\d+\.html$"),
    ("news-vi", "thanhnien", "https://thanhnien.vn/rss/home.rss", 6, r"thanhnien\.vn/[\w-]+-\d{10,}\.htm$"),
    ("news-vi", "dantri", "https://dantri.com.vn/rss/home.rss", 6, r"dantri\.com\.vn/.+-\d{10,}\.htm$"),
    ("news-vi", "tuoitre", "https://tuoitre.vn/rss/tin-moi-nhat.rss", 6, r"tuoitre\.vn/[\w-]+-\d{10,}\.htm$"),
    ("news-vi", "vietnamnet", "https://vietnamnet.vn/", 4, r"vietnamnet\.vn/[\w-]+-\d{5,}\.html$"),
    ("news-vi", "genk", "https://genk.vn/", 3, r"genk\.vn/.+-\d+\.chn$"),
    ("news", "bbc", "https://feeds.bbci.co.uk/news/rss.xml", 6, r"bbc\.co(m|\.uk)/news/articles/\w+"),
    ("news", "npr", "https://feeds.npr.org/1001/rss.xml", 5, r"npr\.org/\d{4}/"),
    ("news", "aljazeera", "https://www.aljazeera.com/xml/rss/all.xml", 5, r"aljazeera\.com/\w+/\d{4}/"),
    ("news", "lemonde", "https://www.lemonde.fr/rss/une.xml", 4, r"lemonde\.fr/[\w-]+/article/\d{4}/"),
    ("news", "spiegel", "https://www.spiegel.de/schlagzeilen/index.rss", 4, r"spiegel\.de/.+-[a-f0-9-]{20,}"),
    ("news", "nhk", "https://www3.nhk.or.jp/rss/news/cat0.xml", 3, r"nhk\.or\.jp/news/html/"),
    ("tech", "techcrunch", "https://techcrunch.com/feed/", 4, r"techcrunch\.com/\d{4}/"),
    ("tech", "theverge", "https://www.theverge.com/rss/index.xml", 4, r"theverge\.com/.+/\d+/"),
    ("tech", "arstechnica", "https://arstechnica.com/feed/", 4, r"arstechnica\.com/.+/\d{4}/\d{2}/"),
    ("blog", "medium", "https://medium.com/feed/tag/programming", 5, r"medium\.com/.+-[0-9a-f]{8,}"),
    ("blog", "pragmatic-engineer", "https://newsletter.pragmaticengineer.com/feed", 3, r"pragmaticengineer\.com/p/"),
    ("blog", "wordpress-news", "https://wordpress.org/news/feed/", 3, r"wordpress\.org/news/\d{4}/"),
    ("blog", "ghost", "https://ghost.org/blog/rss/", 3, r"ghost\.org/(changelog|blog)/[\w-]+/(\?.*)?$"),
    ("blog", "devto", "https://dev.to/feed", 4, r"dev\.to/[\w-]+/[\w-]+"),
    ("blog", "simonwillison", "https://simonwillison.net/atom/entries/", 3, r"simonwillison\.net/\d{4}/"),
    ("blog", "jvns", "https://jvns.ca/atom.xml", 3, r"jvns\.ca/blog/\d{4}/"),
    ("blog", "martinfowler", "https://martinfowler.com/feed.atom", 3, r"martinfowler\.com/(articles|bliki)/"),
]

# (category, name, address)
FIXED = [
    ("docs", "python-tutorial-classes", "https://docs.python.org/3/tutorial/classes.html"),
    ("docs", "python-library-re", "https://docs.python.org/3/library/re.html"),
    ("docs", "python-howto-logging", "https://docs.python.org/3/howto/logging.html"),
    ("docs", "mdn-picture", "https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/picture"),
    ("docs", "mdn-flexbox", "https://developer.mozilla.org/en-US/docs/Learn_web_development/Core/CSS_layout/Flexbox"),
    ("docs", "mdn-promise", "https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Promise"),
    ("docs", "rust-book-ownership", "https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html"),
    ("docs", "rust-book-errors", "https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html"),
    ("docs", "rust-std-vec", "https://doc.rust-lang.org/std/vec/struct.Vec.html"),
    ("docs", "react-learn", "https://react.dev/learn"),
    ("docs", "react-state", "https://react.dev/learn/state-a-components-memory"),
    ("docs", "docusaurus-docs", "https://docusaurus.io/docs"),
    ("docs", "fastapi-first-steps", "https://fastapi.tiangolo.com/tutorial/first-steps/"),
    ("docs", "fastapi-path-params", "https://fastapi.tiangolo.com/tutorial/path-params/"),
    ("docs", "requests-quickstart", "https://requests.readthedocs.io/en/latest/user/quickstart/"),
    ("docs", "kubernetes-overview", "https://kubernetes.io/docs/concepts/overview/"),
    ("docs", "kubernetes-pods", "https://kubernetes.io/docs/concepts/workloads/pods/"),
    ("docs", "git-book-branching", "https://git-scm.com/book/en/v2/Git-Branching-Branches-in-a-Nutshell"),
    ("docs", "postgres-select", "https://www.postgresql.org/docs/current/sql-select.html"),
    ("wiki", "wiki-en-fourier", "https://en.wikipedia.org/wiki/Fourier_transform"),
    ("wiki", "wiki-en-gravitational-wave", "https://en.wikipedia.org/wiki/Gravitational_wave"),
    ("wiki", "wiki-en-rust", "https://en.wikipedia.org/wiki/Rust_(programming_language)"),
    ("wiki", "wiki-en-photosynthesis", "https://en.wikipedia.org/wiki/Photosynthesis"),
    ("wiki", "wiki-en-vietnam-war", "https://en.wikipedia.org/wiki/Vietnam_War"),
    ("wiki", "wiki-en-pi", "https://en.wikipedia.org/wiki/Pi"),
    ("wiki", "wiki-vi-hanoi", "https://vi.wikipedia.org/wiki/H%C3%A0_N%E1%BB%99i"),
    ("wiki", "wiki-vi-ho-chi-minh", "https://vi.wikipedia.org/wiki/H%E1%BB%93_Ch%C3%AD_Minh"),
    ("wiki", "wiki-vi-pho", "https://vi.wikipedia.org/wiki/Ph%E1%BB%9F"),
    ("wiki", "wiki-vi-tri-tue-nhan-tao", "https://vi.wikipedia.org/wiki/Tr%C3%AD_tu%E1%BB%87_nh%C3%A2n_t%E1%BA%A1o"),
    ("wiki", "wiki-vi-ly-thuyet-do-thi", "https://vi.wikipedia.org/wiki/L%C3%BD_thuy%E1%BA%BFt_%C4%91%E1%BB%93_th%E1%BB%8B"),
    ("wiki", "wiki-fr-paris", "https://fr.wikipedia.org/wiki/Paris"),
    ("wiki", "wiki-de-berlin", "https://de.wikipedia.org/wiki/Berlin"),
    ("wiki", "wiki-ja-tokyo", "https://ja.wikipedia.org/wiki/%E6%9D%B1%E4%BA%AC%E9%83%BD"),
    ("wiki", "wiki-zh-beijing", "https://zh.wikipedia.org/wiki/%E5%8C%97%E4%BA%AC%E5%B8%82"),
    ("science", "arxiv-attention", "https://arxiv.org/html/1706.03762v7"),
    ("science", "arxiv-mistral", "https://arxiv.org/html/2310.06825v1"),
    ("science", "arxiv-llama3", "https://arxiv.org/html/2407.21783v3"),
    ("science", "arxiv-dpo", "https://arxiv.org/html/2305.18290v3"),
    ("science", "plos-one-article", "https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0230416"),
    ("science", "pmc-article", "https://pmc.ncbi.nlm.nih.gov/articles/PMC7096724/"),
    ("science", "nature-numpy", "https://www.nature.com/articles/s41586-020-2649-2"),
    ("gov", "govuk-vat", "https://www.gov.uk/how-vat-works"),
    ("gov", "govuk-passport", "https://www.gov.uk/apply-renew-passport"),
    ("gov", "usagov-passport", "https://www.usa.gov/passport"),
    ("gov", "chinhphu-home", "https://chinhphu.vn/"),
    ("other", "anthropic-tools", "https://www.anthropic.com/research/writing-tools-for-agents"),
    ("other", "anthropic-building-agents", "https://www.anthropic.com/engineering/building-effective-agents"),
    ("other", "cnn-huang", "https://edition.cnn.com/2026/11/23/politics/trump-administration-jensen-huang-reinstated"),
    ("other", "pg-great-work", "https://paulgraham.com/greatwork.html"),
    ("other", "pg-startup-ideas", "https://paulgraham.com/startupideas.html"),
    ("other", "willison-llms-2024", "https://simonwillison.net/2024/Dec/31/llms-in-2024/"),
    ("other", "hn-front", "https://news.ycombinator.com/"),
    ("other", "vnexpress-home", "https://vnexpress.net/"),
]


def get(url: str) -> str:
    """The text at `url` (curl, which knows the proxy), or '' when it fails."""
    run = subprocess.run(
        ["curl", "-fsSL", "-m", "30", "-A", USER_AGENT, url],
        capture_output=True,
        check=False,
    )
    return run.stdout.decode("utf-8", "replace") if run.returncode == 0 else ""


def links(text: str, base: str) -> list[str]:
    """The addresses in a feed (<link>, <guid>, href) or a page (href), in order."""
    found = re.findall(r"<link>\s*(?:<!\[CDATA\[)?\s*(https?://[^<\]\s]+)", text)
    found += re.findall(r"<link[^>]+href=\"(https?://[^\"]+)\"", text)
    found += re.findall(r"<guid[^>]*>\s*(?:<!\[CDATA\[)?\s*(https?://[^<\]\s]+)", text)
    found += [urljoin(base, href) for href in re.findall(r"href=\"([^\"#]+)\"", text)]
    seen: list[str] = []
    for url in found:
        url = url.split("?")[0] if "medium.com" in url else url
        if url not in seen:
            seen.append(url)
    return seen


def slug(url: str) -> str:
    """A short name from the last part of an address."""
    path = urlparse(url).path.rstrip("/")
    last = path.rsplit("/", 1)[-1] or urlparse(url).netloc
    last = re.sub(r"\.(html?|chn|shtml)$", "", last)
    last = re.sub(r"[^\w-]+", "-", last).strip("-").lower()
    return last[:48] or "page"


def main() -> int:
    rows = []
    for category, site, feed, count, pattern in FEEDS:
        text = get(feed)
        picked = [url for url in links(text, feed) if re.search(pattern, url)][:count]
        if not picked:
            print(f"warning: no article from {site} ({feed})", file=sys.stderr)
        for url in picked:
            rows.append((category, f"{site}-{slug(url)}", url))
    rows += FIXED
    names = set()
    with (HERE / "pages.tsv").open("w", encoding="utf-8") as out:
        out.write("# category\tname\taddress (written by make-pages.py)\n")
        for category, name, url in rows:
            while name in names:
                name += "-2"
            names.add(name)
            out.write(f"{category}\t{name}\t{url}\n")
    print(f"{len(rows)} pages in {HERE / 'pages.tsv'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
