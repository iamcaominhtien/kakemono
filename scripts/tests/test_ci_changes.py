"""scripts/ci_changes.py: which pull requests change only the documentation."""

import sys
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import ci_changes

CARGO = """[package]
name = "kakemono"
version = "0.16.0"
description = "Old"
include = ["/src/**"]

[dependencies]
serde = { version = "1" }
"""

LOCK = """version = 4

[[package]]
name = "kakemono"
version = "0.16.0"
dependencies = ["serde"]

[[package]]
name = "serde"
version = "1.0.200"
checksum = "aaa"
"""

PYPROJECT = """[project]
name = "kakemono"
description = "Old"
keywords = ["pdf"]
requires-python = ">=3.9"
"""


def code(files: dict) -> list:
    """The code changes among `files`, a map path -> (old text, new text)."""

    def read(path):
        old, new = files[path]
        return tomllib.loads(old), tomllib.loads(new)

    return ci_changes.code_changes(list(files), read)


def test_markdown_at_the_root_and_docs_are_documentation():
    for path in ["README.md", "CHANGELOG.md", "docs/GUIDE.md", "docs/a/b.png", "LICENSE"]:
        assert ci_changes.is_doc(path), path
    assert ci_changes.is_doc("python/README.md")


def test_markdown_read_by_the_tests_is_code():
    for path in ["tests/data/word/sample.md", "src/render/fonts/README.md", "src/lib.rs"]:
        assert not ci_changes.is_doc(path), path


def test_a_release_of_a_documentation_change_is_documentation():
    files = {
        "README.md": ("", ""),
        "CHANGELOG.md": ("", ""),
        "Cargo.toml": (CARGO, CARGO.replace("0.16.0", "0.16.1").replace("Old", "New")),
        "Cargo.lock": (LOCK, LOCK.replace('version = "0.16.0"', 'version = "0.16.1"')),
        "python/pyproject.toml": (
            PYPROJECT,
            PYPROJECT.replace("Old", "New").replace('["pdf"]', '["pdf", "docx"]'),
        ),
    }
    assert code(files) == []


def test_a_dependency_change_is_code():
    files = {
        "Cargo.toml": (CARGO, CARGO.replace('version = "1"', 'version = "1.1"')),
        "Cargo.lock": (LOCK, LOCK.replace("1.0.200", "1.0.201")),
    }
    assert code(files) == ["Cargo.toml", "Cargo.lock"]


def test_what_goes_into_the_package_is_code():
    files = {"Cargo.toml": (CARGO, CARGO.replace('"/src/**"', '"/src/**", "/docs/**"'))}
    assert code(files) == ["Cargo.toml"]


def test_the_python_requirements_are_code():
    files = {"python/pyproject.toml": (PYPROJECT, PYPROJECT.replace("3.9", "3.10"))}
    assert code(files) == ["python/pyproject.toml"]


def test_invalid_toml_is_code():
    files = {"Cargo.toml": (CARGO, CARGO + "[broken")}
    assert code(files) == ["Cargo.toml"]


def test_commits_are_checked_before_git_reads_them():
    head = ci_changes.commit("HEAD")
    assert ci_changes.COMMIT.fullmatch(head)
    assert ci_changes.commit(head) == head
    for rev in ["--output=/tmp/x", "-p", "HEAD..main", "a b", "", "HEAD;ls"]:
        try:
            ci_changes.commit(rev)
        except ValueError:
            continue
        raise AssertionError(rev)
