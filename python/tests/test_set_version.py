"""Tests of scripts/set-python-version.py, which gives the wheels of a release
the version of the release."""

import importlib.util
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "set-python-version.py"
spec = importlib.util.spec_from_file_location("set_python_version", SCRIPT)
set_python_version = importlib.util.module_from_spec(spec)
spec.loader.exec_module(set_python_version)


def test_only_the_package_version_changes(tmp_path):
    manifest = tmp_path / "Cargo.toml"
    manifest.write_text('[package]\nname = "x"\nversion = "0.3.0"\n\n[dependencies]\nfoo = { version = "1" }\n')
    set_python_version.set_version(manifest, "0.4.0")
    assert manifest.read_text() == '[package]\nname = "x"\nversion = "0.4.0"\n\n[dependencies]\nfoo = { version = "1" }\n'


def test_a_manifest_without_version_is_an_error(tmp_path):
    manifest = tmp_path / "Cargo.toml"
    manifest.write_text('[package]\nname = "x"\n')
    with pytest.raises(ValueError, match="no version line"):
        set_python_version.set_version(manifest, "0.4.0")


def test_the_script_points_at_the_python_package():
    assert set_python_version.MANIFEST == Path(__file__).resolve().parents[1] / "Cargo.toml"
