#!/usr/bin/env python3
"""Check that every declared dependency requirement admits the newest stable version.

Q25. Dependency freshness is controlled by this gate at release time, not by a
timer. For every dependency of every workspace member (normal, build and
development; optional and target-specific included; workspace-inherited
requirements resolved by `cargo metadata`; workspace members themselves are
skipped), the declared requirement is compared with the newest version on the
crates.io sparse index that is not yanked and not a pre-release.

* it passes when the requirement admits the newest version;
* or when `requirements-held.toml` has a row for that exact (crate, requirement)
  whose `latest` is the newest version now: the row says why the requirement is
  held and what ends the hold, and a newer upstream version makes the row fail,
  so an upstream event, not a date, brings the decision back;
* a row for a requirement that now passes, or that is no longer declared anywhere,
  fails (delete it, as `advisory-not-detected` does for an advisory);
* it fails closed: an index that cannot be fetched or parsed, a requirement that
  names a pre-release or uses a form this script does not understand, a crate with
  no stable version. There is no cached copy and no fallback.

The table "Requirements held back" in `docs/src/dependency_security.md` is generated
from `requirements-held.toml` (between two marker comments) and checked against it
here: `--write-book` rewrites the block, `--book-only` checks the block alone and
needs no network.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
import urllib.request
from pathlib import Path
from typing import Callable

ROOT = Path(__file__).resolve().parents[1]
HELD_FILE = "requirements-held.toml"
BOOK_FILE = "docs/src/dependency_security.md"
BOOK_BEGIN = "<!-- requirements-held:begin (generated from requirements-held.toml by scripts/check_requirements.py --write-book) -->"
BOOK_END = "<!-- requirements-held:end -->"
INDEX_URL = "https://index.crates.io"
USER_AGENT = "localcache-rs-requirements-gate (https://github.com/nabbisen/localcache-rs)"
HELD_KEYS = ("crate", "requirement", "latest", "why", "ends")


class RequirementError(Exception):
    """A check failed, or could not be decided (which is a failure)."""


# ---------------------------------------------------------------------------
# Versions and requirements (Cargo's rules, for the forms a manifest can hold)
# ---------------------------------------------------------------------------

_FULL = re.compile(r"(\d+)\.(\d+)\.(\d+)")
_PART = re.compile(r"(\d+)(?:\.(\d+))?(?:\.(\d+))?")
_COMPARATOR = re.compile(r"(\^|~|=|>=|<=|>|<)?\s*(\d+(?:\.\d+){0,2}|\*)(\.\*)?")


def parse_version(text: str) -> tuple[int, int, int]:
    """A plain `X.Y.Z`. A pre-release or build metadata is not one."""
    match = _FULL.fullmatch(text)
    if not match:
        raise RequirementError(f"cannot read version {text!r} (only plain X.Y.Z is understood)")
    major, minor, patch = (int(part) for part in match.groups())
    return major, minor, patch


def _comparator(text: str) -> tuple[str, int | None, int | None, int | None]:
    text = text.strip()
    if "-" in text or "+" in text:
        raise RequirementError(f"requirement {text!r} names a pre-release or build metadata")
    match = _COMPARATOR.fullmatch(text)
    if not match:
        raise RequirementError(f"requirement form {text!r} is not understood")
    operator, number, wildcard = match.group(1) or "^", match.group(2), match.group(3)
    if number == "*":
        if operator != "^" or text != "*":
            raise RequirementError(f"requirement form {text!r} is not understood")
        return "*", None, None, None
    part = _PART.fullmatch(number)
    assert part is not None  # _COMPARATOR guarantees it
    major, minor, patch = (None if p is None else int(p) for p in part.groups())
    if wildcard:
        if operator != "^" or patch is not None:
            raise RequirementError(f"requirement form {text!r} is not understood")
        return "w", major, minor, None
    return operator, major, minor, patch


def admits(requirement: str, version: str) -> bool:
    """Whether every comma-separated comparator of `requirement` admits `version`."""
    found = parse_version(version)
    if not requirement.strip():
        raise RequirementError("empty requirement")
    for text in requirement.split(","):
        operator, major, minor, patch = _comparator(text)
        low = (major or 0, minor or 0, patch or 0)
        if operator == "*":
            ok = True
        elif operator == "w":
            ok = found[0] == major and (minor is None or found[1] == minor)
        elif operator == "=":
            ok = (
                found[0] == major
                and (minor is None or found[1] == minor)
                and (patch is None or found[2] == patch)
            )
        elif operator == ">=":
            ok = found >= low
        elif operator == "<":
            ok = found < low
        elif operator == ">":
            if patch is not None:
                ok = found > low
            elif minor is not None:
                ok = found >= (major, minor + 1, 0)
            else:
                ok = found >= (major + 1, 0, 0)  # type: ignore[operator]
        elif operator == "<=":
            if patch is not None:
                ok = found <= low
            elif minor is not None:
                ok = found < (major, minor + 1, 0)
            else:
                ok = found < (major + 1, 0, 0)  # type: ignore[operator]
        elif operator == "~":
            ok = found >= low and found[0] == major and (minor is None or found[1] == minor)
        elif operator == "^":
            if major != 0 or minor is None:
                ok = found >= low and found[0] == major
            elif minor != 0 or patch is None:
                ok = found >= low and found[0] == 0 and found[1] == minor
            else:
                ok = found == low
        else:  # pragma: no cover - _comparator returns only the operators above
            raise RequirementError(f"operator {operator!r} is not understood")
        if not ok:
            return False
    return True


# ---------------------------------------------------------------------------
# The crates.io sparse index
# ---------------------------------------------------------------------------


def index_path(name: str) -> str:
    n = name.lower()
    if len(n) == 1:
        return f"1/{n}"
    if len(n) == 2:
        return f"2/{n}"
    if len(n) == 3:
        return f"3/{n[0]}/{n}"
    return f"{n[:2]}/{n[2:4]}/{n}"


def newest_stable(name: str, index_text: str) -> str:
    """The highest version in a sparse-index file that is not yanked and not a pre-release."""
    best: tuple[tuple[int, int, int], str] | None = None
    for number, line in enumerate(index_text.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            record = json.loads(line)
            version = record["vers"]
            yanked = record["yanked"]
        except (ValueError, KeyError, TypeError) as error:
            raise RequirementError(f"index of {name}: line {number} cannot be parsed ({error})") from error
        if yanked is not False and yanked is not True:
            raise RequirementError(f"index of {name}: line {number}: 'yanked' is not a boolean")
        if yanked or "-" in version or "+" in version:
            continue
        key = parse_version(version)
        if best is None or key > best[0]:
            best = (key, version)
    if best is None:
        raise RequirementError(f"index of {name}: no stable, unyanked version")
    return best[1]


def fetch_index_live(name: str) -> str:
    """The index file of `name` from crates.io. Any failure is an error: no cache, no fallback."""
    request = urllib.request.Request(f"{INDEX_URL}/{index_path(name)}", headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.read().decode("utf-8")
    except (OSError, ValueError) as error:  # URLError, HTTPError, timeouts and bad bytes
        raise RequirementError(f"cannot fetch the crates.io index of {name}: {error}") from error


def fetch_index_dir(directory: Path) -> Callable[[str], str]:
    """The index read from a directory laid out like the sparse index (for tests)."""

    def fetch(name: str) -> str:
        try:
            return (directory / index_path(name)).read_text(encoding="utf-8")
        except OSError as error:
            raise RequirementError(f"cannot read the index of {name}: {error}") from error

    return fetch


# ---------------------------------------------------------------------------
# What the workspace declares
# ---------------------------------------------------------------------------


def load_metadata_live() -> dict:
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--locked", "--format-version", "1"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RequirementError(f"cargo metadata failed: {result.stderr.strip()[:300]}")
    return json.loads(result.stdout)


def declared_requirements(metadata: dict) -> dict[tuple[str, str], list[str]]:
    """(crate, requirement) -> who declares it, for every registry dependency of a workspace member."""
    try:
        members = set(metadata["workspace_members"])
        found: dict[tuple[str, str], set[str]] = {}
        for package in metadata["packages"]:
            if package["id"] not in members:
                continue
            for dependency in package["dependencies"]:
                if dependency.get("source") is None:
                    continue  # a path dependency: another workspace member
                key = (dependency["name"], dependency["req"])
                kind = dependency.get("kind") or "normal"
                optional = ", optional" if dependency.get("optional") else ""
                found.setdefault(key, set()).add(f"{package['name']} ({kind}{optional})")
    except (KeyError, TypeError) as error:
        raise RequirementError(f"unexpected `cargo metadata` shape: {error!r}") from error
    if not found:
        raise RequirementError("no registry dependency found in the workspace metadata")
    return {key: sorted(users) for key, users in found.items()}


# ---------------------------------------------------------------------------
# The held-back file
# ---------------------------------------------------------------------------


def load_held(path: Path) -> dict[tuple[str, str], dict[str, str]]:
    """Rows of `requirements-held.toml`, keyed by (crate, requirement). Schema checked."""
    try:
        with path.open("rb") as file:
            data = tomllib.load(file)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise RequirementError(f"{path.name}: cannot be read ({error})") from error
    unknown_top = sorted(set(data) - {"held"})
    if unknown_top:
        raise RequirementError(f"{path.name}: unknown top-level key(s) {unknown_top}")
    rows = data.get("held", [])
    if not isinstance(rows, list):
        raise RequirementError(f"{path.name}: `held` must be an array of tables ([[held]])")
    held: dict[tuple[str, str], dict[str, str]] = {}
    for number, row in enumerate(rows, start=1):
        where = f"{path.name}: row {number}"
        if not isinstance(row, dict):
            raise RequirementError(f"{where}: not a table")
        unknown = sorted(set(row) - set(HELD_KEYS))
        if unknown:
            raise RequirementError(f"{where}: unknown key(s) {unknown}")
        for key in HELD_KEYS:
            value = row.get(key)
            if not isinstance(value, str) or not value.strip():
                raise RequirementError(f"{where}: `{key}` must be a non-empty string")
        parse_version(row["latest"])
        identity = (row["crate"], row["requirement"])
        if identity in held:
            raise RequirementError(f"{where}: duplicate row for {identity[0]} {identity[1]}")
        held[identity] = row
    return held


# ---------------------------------------------------------------------------
# The check
# ---------------------------------------------------------------------------


def evaluate(
    declared: dict[tuple[str, str], list[str]],
    fetch: Callable[[str], str],
    held: dict[tuple[str, str], dict[str, str]],
) -> list[str]:
    """One line per failure, each with its exact next step. An empty list is a pass.
    Raises RequirementError when something cannot be decided."""
    failures: list[str] = []
    newest: dict[str, str] = {}
    for (name, requirement), users in sorted(declared.items()):
        if name not in newest:
            newest[name] = newest_stable(name, fetch(name))
        latest = newest[name]
        passes = admits(requirement, latest)
        row = held.get((name, requirement))
        who = "; ".join(users)
        if passes and row is not None:
            failures.append(
                f"{name} {requirement}: admits the newest version {latest}, so its row in "
                f"{HELD_FILE} no longer applies: delete the row"
            )
        elif not passes and row is None:
            failures.append(
                f"{name} {requirement} (declared by {who}): does not admit the newest version "
                f"{latest}: update the requirement, or add a row to {HELD_FILE} with why it is "
                f"held and what ends the hold (latest = \"{latest}\")"
            )
        elif not passes and row is not None and row["latest"] != latest:
            failures.append(
                f"{name} {requirement}: its row was written against {row['latest']}, the newest "
                f"is now {latest}: decide again, then set `latest = \"{latest}\"` (or update the "
                f"requirement and delete the row)"
            )
    for (name, requirement), row in sorted(held.items()):
        if (name, requirement) not in declared:
            failures.append(
                f"{name} {requirement}: a row in {HELD_FILE} for a requirement no workspace "
                f"member declares: delete the row"
            )
    return failures


# ---------------------------------------------------------------------------
# The book's table
# ---------------------------------------------------------------------------


def _cell(text: str) -> str:
    return " ".join(text.split()).replace("|", "\\|")


def render_book_block(held: dict[tuple[str, str], dict[str, str]]) -> str:
    lines = [
        BOOK_BEGIN,
        "",
        "| Requirement | Newest seen | Why it is held | What ends the hold |",
        "|---|---|---|---|",
    ]
    for (name, requirement), row in sorted(held.items()):
        lines.append(
            f"| `{name} {requirement}` | `{row['latest']}` | {_cell(row['why'])} | {_cell(row['ends'])} |"
        )
    if not held:
        lines.append("| *(none)* | | | |")
    lines += ["", BOOK_END]
    return "\n".join(lines)


def _split_book(text: str) -> tuple[str, str, str]:
    begin, end = text.find(BOOK_BEGIN), text.find(BOOK_END)
    if begin < 0 or end < 0 or end < begin or text.count(BOOK_BEGIN) != 1 or text.count(BOOK_END) != 1:
        raise RequirementError(
            f"{BOOK_FILE}: the two marker comments of the generated table are missing or repeated"
        )
    end += len(BOOK_END)
    return text[:begin], text[begin:end], text[end:]


def check_book(root: Path, held: dict[tuple[str, str], dict[str, str]]) -> None:
    try:
        text = (root / BOOK_FILE).read_text(encoding="utf-8")
    except OSError as error:
        raise RequirementError(f"{BOOK_FILE}: cannot be read ({error})") from error
    _, current, _ = _split_book(text)
    if current != render_book_block(held):
        raise RequirementError(
            f"{BOOK_FILE}: the table of held requirements differs from {HELD_FILE}: "
            "run `python3 scripts/check_requirements.py --write-book`"
        )


def write_book(root: Path, held: dict[tuple[str, str], dict[str, str]]) -> None:
    path = root / BOOK_FILE
    before, _, after = _split_book(path.read_text(encoding="utf-8"))
    path.write_text(before + render_book_block(held) + after, encoding="utf-8")


# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--book-only", action="store_true", help="check the book's table against the file; no network")
    parser.add_argument("--write-book", action="store_true", help="rewrite the book's table from the file")
    parser.add_argument("--metadata", type=Path, help="test input: a `cargo metadata` JSON file")
    parser.add_argument("--index-dir", type=Path, help="test input: a directory laid out like the sparse index")
    parser.add_argument("--root", type=Path, default=ROOT, help="test input: the repository root")
    args = parser.parse_args(argv)
    try:
        held = load_held(args.root / HELD_FILE)
        if args.write_book:
            write_book(args.root, held)
            print(f"check_requirements: wrote the table in {BOOK_FILE}")
            return 0
        check_book(args.root, held)
        if args.book_only:
            print(f"check_requirements: the book's table matches {HELD_FILE} ({len(held)} row(s))")
            return 0
        metadata = json.loads(args.metadata.read_text(encoding="utf-8")) if args.metadata else load_metadata_live()
        fetch = fetch_index_dir(args.index_dir) if args.index_dir else fetch_index_live
        declared = declared_requirements(metadata)
        failures = evaluate(declared, fetch, held)
    except (RequirementError, OSError, ValueError) as error:
        print(f"check_requirements: FAILED: {error}", file=sys.stderr)
        return 1
    if failures:
        for line in failures:
            print(f"check_requirements: {line}", file=sys.stderr)
        print(f"check_requirements: FAILED: {len(failures)} requirement(s) to decide", file=sys.stderr)
        return 1
    print(
        f"check_requirements: OK ({len(declared)} requirements; {len(held)} held, each with its reason)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
