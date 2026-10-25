#!/usr/bin/env python3
"""Copy the docs of both repositories into the site's source tree and fix their links.

The docs stay where they are (docs/ here, docs/ in open-stomp-manager) so they still read well on GitHub; this
script copies them into site/src/lib and site/src/manager (both git-ignored) and rewrites the links that point
outside the docs: source files and READMEs go to GitHub, links between the two repositories' docs stay on the site.
It also writes site/theme/versions.js (git-ignored): both versions, shown at the top of the sidebar.

usage: site/assemble.py [path to an open-stomp-manager checkout]   (default: ../open-stomp-manager)
"""
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

LIB = Path(__file__).resolve().parent.parent
MANAGER = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else LIB.parent / "open-stomp-manager"
SRC = LIB / "site" / "src"
GH = "https://github.com/JackMechem"

LINK = re.compile(r"\]\(([^)\s]+)\)")


def rewrite(target: str, repo: str) -> str:
    """One link target, as seen from a page in site/src/<lib|manager>/."""
    if target.startswith("#") or target.startswith("mailto:"):
        return target
    path, _, frag = target.partition("#")
    frag = "#" + frag if frag else ""
    m = re.match(GH + r"/open-stomp-lib/blob/main/docs/([^/]+\.md)$", path)
    if m:
        return f"../lib/{m[1]}{frag}"
    m = re.match(GH + r"/open-stomp-manager/blob/main/docs/([^/]+\.md)$", path)
    if m:
        return f"../manager/{m[1]}{frag}"
    if "://" in path:
        return target
    if path in ("../README.md",) and frag == "#install" and repo == "open-stomp-manager":
        return "../install/app.md"
    if path.startswith("../"):  # anything else outside docs/: the file on GitHub
        kind = "tree" if path.endswith("/") or "." not in path.rsplit("/", 1)[-1] else "blob"
        return f"{GH}/{repo}/{kind}/main/{path[3:]}{frag}"
    return target


def copy(docs: Path, dest: Path, repo: str) -> None:
    if dest.exists():
        shutil.rmtree(dest)
    shutil.copytree(docs, dest)
    for md in dest.glob("*.md"):
        text = md.read_text()
        md.write_text(LINK.sub(lambda m: "](" + rewrite(m[1], repo) + ")", text))


def changelog(repo_dir: Path, dest: Path, repo: str) -> None:
    """The CHANGELOG sits at the repository root: docs/x.md is a page next to it, anything else is on GitHub."""
    def fix(target: str) -> str:
        if target.startswith("docs/"):
            return target[len("docs/"):]
        return rewrite("../" + target, repo) if not re.match(r"[a-z]+:|#", target) else target
    text = (repo_dir / "CHANGELOG.md").read_text()
    (dest / "CHANGELOG.md").write_text(LINK.sub(lambda m: "](" + fix(m[1]) + ")", text))


def version(repo_dir: Path) -> dict:
    """The version in the workspace Cargo.toml, and how many commits HEAD is past its release tag
    (None when the tag isn't there, e.g. in a shallow clone)."""
    v = re.search(r'^version = "([^"]+)"', (repo_dir / "Cargo.toml").read_text(), re.M)[1]

    def git(*args: str) -> str | None:
        r = subprocess.run(["git", "-C", str(repo_dir), *args], capture_output=True, text=True)
        return r.stdout.strip() if r.returncode == 0 else None

    ahead = git("rev-list", "--count", f"v{v}..HEAD")
    return {"version": v, "ahead": int(ahead) if ahead is not None else None, "commit": git("rev-parse", "--short", "HEAD")}


if not (MANAGER / "docs").is_dir():
    sys.exit(f"no open-stomp-manager checkout at {MANAGER} (pass its path)")
copy(LIB / "docs", SRC / "lib", "open-stomp-lib")
copy(MANAGER / "docs", SRC / "manager", "open-stomp-manager")
changelog(LIB, SRC / "lib", "open-stomp-lib")
changelog(MANAGER, SRC / "manager", "open-stomp-manager")
versions = {"lib": version(LIB), "manager": version(MANAGER)}
(LIB / "site" / "theme" / "versions.js").write_text(f"window.openStompVersions = {json.dumps(versions)};\n")
print("assembled", SRC / "lib", "and", SRC / "manager")
