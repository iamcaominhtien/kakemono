"""Tests of scripts/set-bindings-version.py on a copy of bindings/."""

import importlib.util
import pathlib
import shutil

import pytest

SCRIPTS = pathlib.Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("set_bindings_version", SCRIPTS / "set-bindings-version.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

MANIFESTS = [
    "Cargo.toml",
    "wasm/Cargo.toml",
    "node/package.json",
    "wasm/package.json",
    "jvm/pom.xml",
    "jvm/kakemono/pom.xml",
    "jvm/kakemono-langchain4j/pom.xml",
    "dotnet/Kakemono/Kakemono.csproj",
]


@pytest.fixture
def bindings(tmp_path):
    """A copy of the manifests of bindings/."""
    for name in MANIFESTS:
        target = tmp_path / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(module.ROOT / name, target)
    return tmp_path


def test_every_manifest_gets_the_version(bindings):
    changed = module.set_versions(bindings, "1.2.3")
    assert len(changed) == len(MANIFESTS)
    for name in MANIFESTS:
        assert "1.2.3" in (bindings / name).read_text(encoding="utf-8"), name
    # Other versions stay: dependencies, plugins.
    pom = (bindings / "jvm/pom.xml").read_text(encoding="utf-8")
    assert "<version>3.14.0</version>" in pom
    node = (bindings / "node/package.json").read_text(encoding="utf-8")
    assert '"@napi-rs/cli": "3.11.1"' in node
    cargo = (bindings / "Cargo.toml").read_text(encoding="utf-8")
    assert 'serde = { version = "1"' in cargo


def test_a_child_pom_gets_its_parent_version(bindings):
    module.set_versions(bindings, "2.0.0-rc.1")
    child = (bindings / "jvm/kakemono-langchain4j/pom.xml").read_text(encoding="utf-8")
    assert child.count("2.0.0-rc.1") == 1  # the parent's; its dependency uses ${project.version}


def test_a_version_must_look_like_one(bindings):
    with pytest.raises(ValueError):
        module.set_versions(bindings, "v1.2")
