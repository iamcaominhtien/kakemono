"""Sets the version of every binding package (bindings/) to the one given.

The release workflow bumps only the Rust crate's version; the npm, Maven and NuGet
packages of a release are built with the same version:
`python3 scripts/set-bindings-version.py 0.21.0`.

It sets:
- `bindings/Cargo.toml` (`[workspace.package] version`) and `bindings/wasm/Cargo.toml`;
- `bindings/node/package.json` and `bindings/wasm/package.json`, and the version napi's loader
  `bindings/node/index.js` checks its platform packages against;
- the Maven POMs of `bindings/jvm/` (the project's and its parent's version);
- `bindings/dotnet/Kakemono/Kakemono.csproj` (`<Version>`).
"""

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent / "bindings"

# A version as the release tags have it: 1.2.3, maybe with a pre-release.
VERSION = re.compile(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$")


def replace_once(path: pathlib.Path, pattern: str, replacement: str) -> None:
    """Replaces the first match of `pattern` (multi-line) in `path`, which must match."""
    text = path.read_text(encoding="utf-8")
    new, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise ValueError(f"no version in {path}")
    path.write_text(new, encoding="utf-8")


def set_json(path: pathlib.Path, version: str) -> None:
    """Sets the `version` of a package.json, keeping its layout of two spaces."""
    data = json.loads(path.read_text(encoding="utf-8"))
    data["version"] = version
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def set_versions(root: pathlib.Path, version: str) -> list[pathlib.Path]:
    """Sets `version` in every manifest under `root`; returns the files changed."""
    if not VERSION.match(version):
        raise ValueError(f"not a version: {version!r}")
    changed = []
    workspace = root / "Cargo.toml"
    replace_once(workspace, r'(\[workspace\.package\]\n(?:.*\n)*?version = )"[^"]*"', rf'\g<1>"{version}"')
    changed.append(workspace)
    wasm = root / "wasm" / "Cargo.toml"
    replace_once(wasm, r'(\[package\]\n(?:.*\n)*?version = )"[^"]*"', rf'\g<1>"{version}"')
    changed.append(wasm)
    node = root / "node" / "package.json"
    old = json.loads(node.read_text(encoding="utf-8"))["version"]
    loader = root / "node" / "index.js"
    if loader.exists():
        text = loader.read_text(encoding="utf-8")
        new = text.replace(f"'{old}'", f"'{version}'").replace(f"expected {old} but", f"expected {version} but")
        loader.write_text(new, encoding="utf-8")
        changed.append(loader)
    for package in (node, root / "wasm" / "package.json"):
        set_json(package, version)
        changed.append(package)
    for pom in sorted((root / "jvm").glob("**/pom.xml")):
        if "target" in pom.parts:
            continue
        text = pom.read_text(encoding="utf-8")
        # The project's own version and the version of its parent, both
        # those of io.github.iamcaominhtien artifacts; a version given as a
        # property (`${project.version}`) is left as it is.
        new = re.sub(
            r"(<groupId>io\.github\.iamcaominhtien</groupId>\s*<artifactId>[^<]+</artifactId>\s*<version>)(?!\$\{)[^<]*(</version>)",
            rf"\g<1>{version}\g<2>",
            text,
        )
        if new == text:
            raise ValueError(f"no version in {pom}")
        pom.write_text(new, encoding="utf-8")
        changed.append(pom)
    csproj = root / "dotnet" / "Kakemono" / "Kakemono.csproj"
    replace_once(csproj, r"<Version>[^<]*</Version>", f"<Version>{version}</Version>")
    changed.append(csproj)
    return changed


if __name__ == "__main__":
    for path in set_versions(ROOT, sys.argv[1]):
        print(f"{path.relative_to(ROOT.parent)}: version {sys.argv[1]}")
