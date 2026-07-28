#!/usr/bin/env python3
"""Canonical RFC 009 R7 / R12 package-and-feature test matrix.

This module is the single checked-in source of truth for the release gate's
row list. `Makefile.toml` and `.github/workflows/ci.yaml` both invoke it
rather than maintaining independent command lists — adding, removing, or
renaming a row here is the only change needed to update local and CI gates
together.

Adding a new `localcache` Cargo feature without adding a corresponding row
is caught by `--check-coverage`, which fails closed rather than silently
leaving the new feature untested.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

REPO_ROOT = Path(__file__).resolve().parents[1]

# Every optional Cargo feature `crates/localcache/Cargo.toml` declares.
# `--check-coverage` fails closed if this list and the manifest disagree in
# either direction.
LIBRARY_FEATURES: tuple[str, ...] = (
    "async",
    "async-std",
    "smol",
    "compression",
    "json",
    "encryption",
    "tracing",
    "watching",
    "metrics",
    "opentelemetry",
)


class MatrixError(Exception):
    """A matrix row is unknown, miscovered, or failed to run."""


@dataclass(frozen=True)
class Row:
    """One (package, feature-set) gate row.

    `doctest_only` rows have no clippy variant and run
    `cargo test --workspace --doc --all-features --locked` regardless of
    `package`/`features`.
    """

    name: str
    package: str
    features: tuple[str, ...] = ()
    all_features: bool = False
    no_default_features: bool = True
    doctest_only: bool = False

    def cargo_args(self, *, subcommand: str) -> list[str]:
        if self.doctest_only:
            return ["test", "--workspace", "--doc", "--all-features", "--locked"]
        args = [subcommand, "-p", self.package, "--all-targets"]
        if self.all_features:
            args.append("--all-features")
        else:
            if self.no_default_features:
                args.append("--no-default-features")
            if self.features:
                qualified = ",".join(f"{self.package}/{f}" for f in self.features)
                args += ["--features", qualified]
        args.append("--locked")
        if subcommand == "clippy":
            args += ["--", "-D", "warnings"]
        return args


ROWS: tuple[Row, ...] = (
    Row("lib-no-features", "localcache"),
    *(
        Row(f"lib-feature-{feature}", "localcache", features=(feature,))
        for feature in LIBRARY_FEATURES
    ),
    Row("lib-all-features", "localcache", all_features=True),
    # Non-Tokio runtime suites: async-std/smol alone are meaningless without
    # the companion features real callers combine them with, and priority
    # dispatch (RFC 005 DEC-004) means these are the only way to exercise
    # each backend outside of Tokio's `--all-features` priority win.
    Row(
        "lib-async-std-suite",
        "localcache",
        features=(
            "async-std",
            "compression",
            "json",
            "encryption",
            "tracing",
            "watching",
            "metrics",
        ),
    ),
    Row(
        "lib-smol-suite",
        "localcache",
        features=(
            "smol",
            "compression",
            "json",
            "encryption",
            "tracing",
            "watching",
            "metrics",
        ),
    ),
    Row("cli-default", "localcache-cli", no_default_features=False),
    Row("cli-all-features", "localcache-cli", all_features=True),
    Row("workspace-doctest", "localcache", doctest_only=True),
)


def row_by_name(name: str) -> Row:
    for row in ROWS:
        if row.name == name:
            return row
    raise MatrixError(f"unknown matrix row: {name!r}")


def declared_library_features(root: Path = REPO_ROOT) -> set[str]:
    path = root / "crates" / "localcache" / "Cargo.toml"
    try:
        with path.open("rb") as file:
            document = tomllib.load(file)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise MatrixError(f"cannot read {path}: {error}") from error
    features = document.get("features")
    if not isinstance(features, dict):
        raise MatrixError(f"{path} has no [features] table")
    return set(features.keys())


def check_coverage(root: Path = REPO_ROOT) -> None:
    """Fail closed if the declared feature set and the row set disagree."""
    declared = declared_library_features(root)
    covered = set(LIBRARY_FEATURES)
    missing = declared - covered
    if missing:
        raise MatrixError(
            "declared localcache feature(s) have no matrix row: "
            f"{sorted(missing)} — add a row before this is safe to gate on"
        )
    stale = covered - declared
    if stale:
        raise MatrixError(
            f"matrix row references undeclared feature(s): {sorted(stale)}"
        )


def run_row(name: str, mode: str, *, root: Path = REPO_ROOT) -> None:
    row = row_by_name(name)
    if row.doctest_only and mode == "clippy":
        raise MatrixError(f"row {name!r} has no clippy variant")
    command = ["cargo", *row.cargo_args(subcommand=mode)]
    print(f"[{name}/{mode}] {' '.join(command)}", flush=True)
    completed = subprocess.run(command, cwd=root)
    if completed.returncode != 0:
        raise MatrixError(
            f"row {name!r} mode {mode!r} failed with exit status {completed.returncode}"
        )


def run_row_modes(name: str, modes: Sequence[str], *, root: Path = REPO_ROOT) -> None:
    """Run `name` for each of `modes`, silently skipping `clippy` for a
    doctest-only row rather than treating the combination as an error — a
    caller iterating a fixed mode list across every row (CI, `--run-all`)
    should not need to special-case doctest rows itself."""
    row = row_by_name(name)
    for mode in modes:
        if row.doctest_only and mode == "clippy":
            continue
        run_row(name, mode, root=root)


def run_all(modes: Sequence[str], *, root: Path = REPO_ROOT) -> None:
    check_coverage(root)
    failures: list[str] = []
    for row in ROWS:
        for mode in modes:
            if row.doctest_only and mode == "clippy":
                continue
            try:
                run_row(row.name, mode, root=root)
            except MatrixError as error:
                failures.append(str(error))
    if failures:
        raise MatrixError("matrix failures:\n" + "\n".join(failures))


def parse_args(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument(
        "--list", action="store_true", help="print every row name, one per line"
    )
    group.add_argument("--run", metavar="ROW", help="run one row")
    group.add_argument(
        "--run-all", action="store_true", help="run every row, in sequence"
    )
    group.add_argument(
        "--check-coverage",
        action="store_true",
        help="fail if a declared feature has no row, or vice versa",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="with --list, print a JSON array instead of newline-separated names",
    )
    parser.add_argument(
        "--modes",
        nargs="+",
        choices=("clippy", "test"),
        default=("clippy", "test"),
        help="used with --run and --run-all; clippy is silently skipped for a "
        "doctest-only row",
    )
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        if args.list:
            names = [row.name for row in ROWS]
            print(json.dumps(names) if args.json else "\n".join(names))
        elif args.check_coverage:
            check_coverage()
        elif args.run:
            run_row_modes(args.run, args.modes)
        elif args.run_all:
            run_all(args.modes)
    except MatrixError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
