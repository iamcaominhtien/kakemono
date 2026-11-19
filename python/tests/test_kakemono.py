"""Tests of the Python package on the LIGO paper (CC BY 4.0), the one test
paper committed to the repository."""

import io
import json
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


def test_to_docx_can_save_its_pictures(tmp_path):
    out = tmp_path / "ligo.docx"
    kakemono.to_docx(LIGO, out, scale=1.0, save_images=True, image_dir="images")
    assert sorted(p.name for p in (tmp_path / "images").iterdir())[0] == "001-fig.png"
    with pytest.raises(ValueError, match="needs a path"):
        kakemono.to_docx(LIGO, save_images=True)


def test_to_epub_gives_a_book(tmp_path):
    out = tmp_path / "ligo.epub"
    data = kakemono.to_epub(LIGO, out, scale=1.0, modified="2026-01-31T12:00:00Z")
    assert out.read_bytes() == data
    with zipfile.ZipFile(io.BytesIO(data)) as book:
        assert book.testzip() is None
        assert book.namelist()[0] == "mimetype"
        package = book.read("EPUB/package.opf").decode()
        nav = book.read("EPUB/nav.xhtml").decode()
    assert "<dc:title>Observation of Gravitational Waves" in package
    assert '<meta property="dcterms:modified">2026-01-31T12:00:00Z</meta>' in package
    assert 'properties="cover-image"' in package
    assert 'epub:type="toc"' in nav
    with pytest.raises(ValueError, match="modified"):
        kakemono.to_epub(LIGO, modified="yesterday")


def test_to_markdown_writes_text_and_pictures(tmp_path):
    out = tmp_path / "md" / "ligo.md"
    text = kakemono.to_markdown(LIGO, out, scale=1.0)
    assert out.read_text(encoding="utf-8") == text
    assert "# Observation of Gravitational Waves from a Binary Black Hole Merger" in text
    assert "](ligo_assets/001-fig.png)" in text
    assert (tmp_path / "md" / "ligo_assets" / "001-fig.png").is_file()
    # Without a path, the pictures are embedded.
    assert "](data:image/png;base64," in kakemono.to_markdown(LIGO, scale=1.0)
    with pytest.raises(ValueError, match="relative folder"):
        kakemono.to_markdown(LIGO, out, image_dir="../x")


def test_to_latex_writes_source_and_pictures(tmp_path):
    out = tmp_path / "tex" / "ligo.tex"
    text = kakemono.to_latex(LIGO, out, scale=1.0)
    assert out.read_text(encoding="utf-8") == text
    assert text.startswith("% Converted from a PDF by kakemono.")
    assert "{ligo_assets/001-fig.png}" in text
    assert (tmp_path / "tex" / "ligo_assets" / "001-fig.png").is_file()
    assert text.endswith("\\end{document}\n")
    with pytest.raises(ValueError, match="relative folder"):
        kakemono.to_latex(LIGO, out, image_dir="../x")


def test_analyze_gives_blocks_in_reading_order():
    doc = kakemono.analyze(str(LIGO))
    kinds = {block["type"] for block in doc["blocks"]}
    assert {"title", "heading", "paragraph"} <= kinds
    title = next(block for block in doc["blocks"] if block["type"] == "title")
    assert "Gravitational Waves" in "".join(span["text"] for span in title["spans"])


def test_png_compression_trades_size_for_time():
    fast = kakemono.convert(LIGO, png_compression="fast", embed_fonts=False)
    balanced = kakemono.convert(LIGO, png_compression="balanced", embed_fonts=False)
    assert balanced == kakemono.convert(LIGO, embed_fonts=False)
    assert fast != balanced
    assert "data:image/png;base64," in fast
    with pytest.raises(ValueError, match="unknown png_compression"):
        kakemono.convert(LIGO, png_compression="max")


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


def test_to_json_gives_editable_content(tmp_path):
    out = tmp_path / "json" / "ligo.json"
    content = kakemono.to_json(LIGO, out, scale=1.0)
    assert json.loads(out.read_text(encoding="utf-8")) == content
    assert content["format"] == "kakemono-content"
    assert content["meta"]["title"].startswith("Observation of Gravitational Waves")
    abstract = next(block for block in content["blocks"] if block.get("abstract"))
    assert '<m id="1">' in abstract["text"]
    assert (tmp_path / "json" / "ligo_assets" / "001-fig.png").is_file()
    # Read-only fields for search and data extraction.
    assert "<" not in abstract["plain"]
    assert "$" in abstract["plain"]
    assert content["meta"]["doi"] == "10.1103/PhysRevLett.116.061102"
    assert content["meta"]["authors"][0]["name"] == "B. P. Abbott"


def test_edited_content_reaches_every_output(tmp_path):
    content = kakemono.to_json(LIGO, scale=1.0)
    title = next(block for block in content["blocks"] if block["type"] == "title")
    title["text"] = "Quan sát sóng hấp dẫn"
    path = tmp_path / "ligo.vi.json"
    path.write_text(json.dumps(content, ensure_ascii=False), encoding="utf-8")
    # The edited JSON as a dict, as text or as a path.
    html = kakemono.convert(LIGO, "hard-95", content=content, embed_fonts=False, scale=1.0)
    assert "Quan sát sóng hấp dẫn" in html
    assert "Quan sát sóng hấp dẫn" in kakemono.to_markdown(LIGO, content=str(path), scale=1.0)
    assert "Quan sát sóng hấp dẫn" in kakemono.to_latex(LIGO, content=path.read_text(encoding="utf-8"))
    with zipfile.ZipFile(io.BytesIO(kakemono.to_docx(LIGO, content=path, scale=1.0))) as package:
        assert "Quan sát sóng hấp dẫn" in package.read("word/document.xml").decode()
    assert kakemono.to_json(LIGO, content=content, scale=1.0)["meta"]["title"] == "Quan sát sóng hấp dẫn"
    with pytest.raises(ValueError, match="hard mode"):
        kakemono.convert(LIGO, "hard", content=content)


def test_unreadable_edited_text_warns_and_keeps_the_original():
    content = kakemono.to_json(LIGO, scale=1.0)
    title = next(block for block in content["blocks"] if block["type"] == "title")
    title["text"] = "<blink>Quan sát</blink>"
    with pytest.warns(UserWarning, match="unknown tag"):
        text = kakemono.to_markdown(LIGO, content=content, scale=1.0)
    assert "# Observation of Gravitational Waves" in text
    other = dict(content, source={"format": "pdf", "sha256": "0" * 64})
    with pytest.raises(ValueError, match="another file"):
        kakemono.to_markdown(LIGO, content=other)
