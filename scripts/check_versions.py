#!/usr/bin/env python3
"""Check that the install examples and the CHANGELOG name the coming version.

RFC 009 R10 and R11, kept by RFC 027. Two checks, standard library only:

* every install example in `README.md` and every `docs/src/**/*.md` page
  names the version (a *declaration line* is a line that starts, after
  optional indentation, with `localcache =` and a plain `"X"` or a
  `{ ... version = "X" ... }` table; prose mentions mid-line are not);
* `CHANGELOG.md` has a non-empty section for that version.

The version is the workspace version in the root `Cargo.toml`, or the one
given with `--expect` (the release workflow passes the tag's version).
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

_DECLARATION_PREFIX_RE = re.compile(r"^[ \t]*localcache[ \t]*=")
_SIMPLE_FORM_RE = re.compile(r'^[ \t]*localcache[ \t]*=[ \t]*"([^"]*)"')
_TABLE_FORM_RE = re.compile(r"^[ \t]*localcache[ \t]*=[ \t]*\{(.*)\}[ \t]*$")
_TABLE_VERSION_RE = re.compile(r'\bversion[ \t]*=[ \t]*"([^"]*)"')


class VersionError(Exception):
    """A version-consistency check failed."""


def workspace_version(root: Path) -> str:
    with (root / "Cargo.toml").open("rb") as file:
        return tomllib.load(file)["workspace"]["package"]["version"]


def version_reference_targets(root: Path) -> tuple[Path, ...]:
    """`README.md` plus every `docs/src/**/*.md`, by glob."""
    return (root / "README.md", *sorted((root / "docs" / "src").glob("**/*.md")))


def _parse_declaration_line(relative: Path, line_no: int, line: str) -> str:
    """The pinned version on one declaration line; a line that cannot be
    parsed fails the check and is never skipped."""
    simple = _SIMPLE_FORM_RE.match(line)
    if simple:
        return simple.group(1)
    table = _TABLE_FORM_RE.match(line)
    if table:
        version = _TABLE_VERSION_RE.search(table.group(1))
        if version:
            return version.group(1)
    raise VersionError(
        f"{relative}:{line_no}: unparseable localcache declaration line: {line!r}"
    )


def verify_version_references(root: Path, expected_version: str) -> None:
    """Fails on: a missing target; a README without a declaration line; a
    declaration line that cannot be parsed; a stale version in any target.
    A docs page with no declaration line is fine."""
    readme = root / "README.md"
    for path in version_reference_targets(root):
        relative = path.relative_to(root)
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as error:
            raise VersionError(f"cannot read version reference {relative}: {error}") from error
        found = [
            (line_no, _parse_declaration_line(relative, line_no, line))
            for line_no, line in enumerate(text.splitlines(), start=1)
            if _DECLARATION_PREFIX_RE.match(line)
        ]
        if not found:
            if path == readme:
                raise VersionError(f"{relative}: no install-example version line found")
            continue
        stale = sorted({v for _, v in found if v != expected_version})
        if stale:
            lines = ", ".join(str(n) for n, v in found if v != expected_version)
            raise VersionError(
                f"{relative}: stale version reference(s) {stale}, "
                f"expected {expected_version!r} (line {lines})"
            )


def verify_changelog_has_coming_version_section(root: Path, expected_version: str) -> None:
    """`CHANGELOG.md` has a non-empty `## [X]` section for the version."""
    text = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    headers = re.findall(r"^## \[(.+?)\](?:\s*—.*)?$", text, re.MULTILINE)
    if expected_version not in headers:
        raise VersionError(
            f"CHANGELOG.md has no section for the coming version {expected_version!r}"
        )
    sections = re.split(r"^## \[.+?\](?:\s*—.*)?$", text, flags=re.MULTILINE)
    # One leading chunk before the first header, then one chunk per header.
    if not sections[headers.index(expected_version) + 1].strip():
        raise VersionError(f"CHANGELOG.md section for {expected_version!r} is empty")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--expect", metavar="VERSION", help="default: the workspace version")
    args = parser.parse_args(argv)
    try:
        version = args.expect or workspace_version(ROOT)
        verify_version_references(ROOT, version)
        verify_changelog_has_coming_version_section(ROOT, version)
    except (VersionError, OSError, KeyError, tomllib.TOMLDecodeError) as error:
        print(f"check_versions: FAILED: {error}", file=sys.stderr)
        return 1
    print(f"check_versions: OK ({version})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
