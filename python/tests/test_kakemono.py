"""Tests of the Python package on the LIGO paper (CC BY 4.0), the one test
paper committed to the repository."""

import io
import zipfile
from pathlib import Path

import pytest

import kakemono

LIGO = Path(__file__).resolve().parents[2] / "tests" / "data" / "1602.03837v1.pdf"


def test_version_is_set():
    assert kakemono.__version__.count(".") == 2


def test_convert_from_a_path_gives_friendly_html():
    html = kakemono.convert(LIGO)
    assert html.startswith("<!DOCTYPE html>")
    assert '<meta name="generator" content="kakemono (friendly)">' in html
    assert "Observation of Gravitational Waves" in html


def test_convert_all_analyses_once_for_every_mode():
    pages = kakemono.convert_all(LIGO.read_bytes(), embed_fonts=False, scale=1.0)
    assert list(pages) == list(kakemono.MODES)
    for mode, html in pages.items():
        assert f'content="kakemono ({mode})"' in html


def test_to_docx_gives_a_word_document(tmp_path):
    out = tmp_path / "ligo.docx"
    data = kakemono.to_docx(LIGO, out, scale=1.0)
    assert out.read_bytes() == data
    with zipfile.ZipFile(io.BytesIO(data)) as package:
        assert package.testzip() is None
        document = package.read("word/document.xml").decode()
    assert "Observation of Gravitational Waves" in document
    assert '<w:pStyle w:val="Heading1"/>' in document
    with pytest.raises(ValueError, match="unknown equations"):
        kakemono.to_docx(LIGO, equations="latex")


def test_analyze_gives_blocks_in_reading_order():
    doc = kakemono.analyze(str(LIGO))
    kinds = {block["type"] for block in doc["blocks"]}
    assert {"title", "heading", "paragraph"} <= kinds
    title = next(block for block in doc["blocks"] if block["type"] == "title")
    assert "Gravitational Waves" in "".join(span["text"] for span in title["spans"])


def test_bad_input_raises():
    with pytest.raises(ValueError):
        kakemono.convert(b"not a pdf")
    with pytest.raises(ValueError, match="unknown mode"):
        kakemono.convert(LIGO, mode="soft")
    with pytest.raises(FileNotFoundError):
        kakemono.convert("no-such-file.pdf")


def test_arxiv_ids_are_recognised_without_downloading():
    assert kakemono._kakemono.parse_arxiv_id("https://arxiv.org/abs/1706.03762v7") == "1706.03762v7"
    assert kakemono._kakemono.parse_arxiv_id("paper.pdf") is None
