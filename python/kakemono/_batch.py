"""One document of :func:`kakemono.aio.convert_many`: read once, written in
every format asked for. Run in a worker thread, or in the process of an
isolated conversion (:mod:`kakemono._isolated`)."""

from __future__ import annotations

import os
import re
import urllib.parse
from typing import Any, Dict, List, Optional, Sequence, Tuple

from . import MODES, CancelToken, Cancelled, Document, Source, _kakemono, _write

#: The formats of :func:`kakemono.aio.convert_many`: the extension of the
#: file each one writes, after the document's name. ``html`` is the friendly
#: mode; the HTML modes by name write ``<name>.<mode>.html``, as
#: ``kakemono convert --all`` does.
FORMATS: Dict[str, str] = {
    "html": "html",
    **{mode: f"{mode}.html" for mode in MODES},
    "docx": "docx",
    "md": "md",
    "tex": "tex",
    "typ": "typ",
    "epub": "epub",
    "json": "json",
    "jsonl": "jsonl",
    "pdf": "pdf",
}

#: The formats that write their pictures to a folder next to their file.
_WITH_PICTURES = ("md", "tex", "typ", "json")

#: The method of :class:`kakemono.Document` that writes each format (the
#: HTML modes are written by :meth:`Document.convert`).
_METHODS = {
    "docx": "to_docx",
    "md": "to_markdown",
    "tex": "to_latex",
    "typ": "to_typst",
    "epub": "to_epub",
    "json": "to_json",
    "jsonl": "to_chunks",
    "pdf": "to_pdf",
}


def check_formats(formats: Sequence[str]) -> List[str]:
    """`formats` without repeats, each known (a ``ValueError`` names the
    first one that is not)."""
    if isinstance(formats, str):
        formats = [formats]
    seen: List[str] = []
    for name in formats:
        if name not in FORMATS:
            raise ValueError(f"unknown format {name!r} (expected one of {', '.join(FORMATS)})")
        if name not in seen:
            seen.append(name)
    if not seen:
        raise ValueError("give at least one format")
    return seen


def output_names(sources: Sequence[Any]) -> List[Tuple[str, Source]]:
    """The name each output file of `sources` starts with, and the source:
    the file's name without its extension, an arXiv id, the last part of a
    web address, or ``document-<n>`` for bytes; a ``(name, source)`` pair
    names its own. A name already taken gets ``-2``, ``-3``, ..."""
    taken: Dict[str, int] = {}
    out = []
    for index, item in enumerate(sources, start=1):
        if isinstance(item, tuple) and len(item) == 2 and isinstance(item[0], str):
            name, source = item
        else:
            name, source = _name_of(item, index), item
        name = re.sub(r"[^\w.-]+", "_", name).strip("._") or f"document-{index}"
        count = taken.get(name.lower(), 0) + 1
        taken[name.lower()] = count
        out.append((name if count == 1 else f"{name}-{count}", source))
    return out


def _name_of(source: Any, index: int) -> str:
    """The name of the output files of `source`, before it is made safe."""
    if isinstance(source, (bytes, bytearray, memoryview)):
        return f"document-{index}"
    text = os.fspath(source).strip()
    if re.match(r"(?i)https?://", text) and not _kakemono.parse_arxiv_id(text):
        address = urllib.parse.urlsplit(text)
        last = address.path.rstrip("/").rsplit("/", 1)[-1]
        return os.path.splitext(last)[0] if last else address.hostname or ""
    if not os.path.exists(text) and _kakemono.parse_arxiv_id(text):
        return _kakemono.parse_arxiv_id(text) or text
    return os.path.splitext(os.path.basename(text))[0]


def convert_one(
    source: Source,
    out_dir: str,
    name: str,
    formats: Sequence[str],
    jobs: int,
    options: Optional[Dict[str, Dict[str, Any]]] = None,
    cancel: Optional[CancelToken] = None,
) -> Tuple[Dict[str, str], Dict[str, BaseException], Optional[BaseException]]:
    """Reads `source` once and writes it in each of `formats` to `out_dir`.

    Returns the path written for each format, the error of each format that
    failed (a PDF has no ``pdf`` output, for example), and the error that
    stopped the document: reading it, its cancellation or its time limit
    (the formats written before it are kept). `options` gives the options of
    each format by name, and those of :class:`kakemono.Document` under
    ``"document"``.
    """
    options = options or {}
    os.makedirs(out_dir, exist_ok=True)
    written: Dict[str, str] = {}
    failed: Dict[str, BaseException] = {}
    try:
        doc = Document(source, jobs=jobs, cancel=cancel, **options.get("document", {}))
        pictures = sum(fmt in _WITH_PICTURES for fmt in formats) > 1
        for fmt in formats:
            path = os.path.join(out_dir, f"{name}.{FORMATS[fmt]}")
            kwargs = dict(options.get(fmt, {}))
            if pictures and fmt in _WITH_PICTURES:
                # Each format its own folder: they may draw a picture apart.
                kwargs.setdefault("image_dir", f"{name}_{fmt}_assets")
            try:
                if fmt == "html" or fmt in MODES:
                    page = doc.convert("friendly" if fmt == "html" else fmt, cancel=cancel, **kwargs)
                    _write(path, page)
                else:
                    getattr(doc, _METHODS[fmt])(path, cancel=cancel, **kwargs)
            except (Cancelled, TimeoutError):
                raise
            except Exception as error:  # this format only
                failed[fmt] = error
                continue
            written[fmt] = path
    except Exception as error:  # reading it, or its cancellation or time limit
        return written, failed, error
    return written, failed, None
