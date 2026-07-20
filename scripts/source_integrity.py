#!/usr/bin/env python3
"""Verify Cargo target sources without invoking Cargo.

This bootstrap preflight intentionally uses only Python's standard library so
it can diagnose a missing manifest target source even when Cargo refuses to
parse or execute workspace commands.
"""

from __future__ import annotations

import argparse
import glob
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any


class IntegrityError(Exception):
    """A source-integrity contract violation."""


@dataclass(frozen=True)
class Target:
    kind: str
    name: str
    source: Path
    manifest: Path


def load_manifest(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as file:
            return tomllib.load(file)
    except FileNotFoundError as error:
        raise IntegrityError(f"missing manifest: {path}") from error
    except tomllib.TOMLDecodeError as error:
        raise IntegrityError(f"invalid TOML in {path}: {error}") from error


def workspace_manifests(root: Path, root_document: dict[str, Any]) -> list[Path]:
    manifests = {root / "Cargo.toml"}
    workspace = root_document.get("workspace")
    if not isinstance(workspace, dict):
        return sorted(manifests)

    members = workspace.get("members", [])
    if not isinstance(members, list) or not all(
        isinstance(member, str) for member in members
    ):
        raise IntegrityError("[workspace].members must be an array of strings")

    excludes = workspace.get("exclude", [])
    if not isinstance(excludes, list) or not all(
        isinstance(exclude, str) for exclude in excludes
    ):
        raise IntegrityError("[workspace].exclude must be an array of strings")

    excluded_paths = {
        path.resolve()
        for pattern in excludes
        for path in root.glob(pattern)
    }
    for pattern in members:
        matches = [Path(path) for path in glob.glob(str(root / pattern))]
        if not matches:
            raise IntegrityError(f"workspace member pattern has no matches: {pattern}")
        for member in matches:
            member = member.resolve()
            if member in excluded_paths:
                continue
            manifest = member if member.name == "Cargo.toml" else member / "Cargo.toml"
            manifests.add(manifest)

    return sorted(manifests)


def explicit_targets(manifest: Path, document: dict[str, Any]) -> list[Target]:
    package_dir = manifest.parent
    package = document.get("package")
    if not isinstance(package, dict):
        return []

    package_name = package.get("name")
    if not isinstance(package_name, str) or not package_name:
        raise IntegrityError(f"{manifest}: [package].name must be a non-empty string")

    targets: list[Target] = []
    lib = document.get("lib")
    if lib is not None:
        if not isinstance(lib, dict):
            raise IntegrityError(f"{manifest}: [lib] must be a table")
        targets.append(
            target_from_table(
                "lib", package_name, lib, package_name, package_dir, manifest
            )
        )
    elif (package_dir / "src/lib.rs").is_file():
        targets.append(Target("lib", package_name, package_dir / "src/lib.rs", manifest))

    for kind in ("bin", "bench", "example", "test"):
        entries = document.get(kind, [])
        if not isinstance(entries, list):
            raise IntegrityError(f"{manifest}: [[{kind}]] entries must be tables")
        for entry in entries:
            if not isinstance(entry, dict):
                raise IntegrityError(f"{manifest}: [[{kind}]] entry must be a table")
            name = entry.get("name")
            if not isinstance(name, str) or not name:
                raise IntegrityError(
                    f"{manifest}: [[{kind}]].name must be a non-empty string"
                )
            targets.append(
                target_from_table(
                    kind, name, entry, package_name, package_dir, manifest
                )
            )

    if not any(target.kind == "bin" for target in targets) and (
        package_dir / "src/main.rs"
    ).is_file():
        targets.append(
            Target("bin", package_name, package_dir / "src/main.rs", manifest)
        )

    build = package.get("build")
    if build is True:
        targets.append(Target("build", "build-script", package_dir / "build.rs", manifest))
    elif isinstance(build, str):
        targets.append(
            Target("build", "build-script", package_dir / build, manifest)
        )
    elif build not in (None, False):
        raise IntegrityError(f"{manifest}: [package].build must be a string or boolean")
    elif (package_dir / "build.rs").is_file():
        targets.append(Target("build", "build-script", package_dir / "build.rs", manifest))

    has_primary_target = any(target.kind in {"lib", "bin"} for target in targets)
    if not has_primary_target:
        raise IntegrityError(
            f"{manifest}: package {package_name!r} has no library or binary target"
        )

    return targets


def target_from_table(
    kind: str,
    name: str,
    table: dict[str, Any],
    package_name: str,
    package_dir: Path,
    manifest: Path,
) -> Target:
    configured_path = table.get("path")
    if configured_path is not None:
        if not isinstance(configured_path, str) or not configured_path:
            raise IntegrityError(f"{manifest}: [[{kind}]].path must be a string")
        return Target(kind, name, package_dir / configured_path, manifest)

    candidates = default_candidates(kind, name, package_name, package_dir)
    existing = [candidate for candidate in candidates if candidate.is_file()]
    source = existing[0] if existing else candidates[0]
    return Target(kind, name, source, manifest)


def default_candidates(
    kind: str, name: str, package_name: str, package_dir: Path
) -> list[Path]:
    if kind == "lib":
        return [package_dir / "src/lib.rs"]
    if kind == "bin":
        candidates = [
            package_dir / f"src/bin/{name}.rs",
            package_dir / f"src/bin/{name}/main.rs",
        ]
        if name == package_name:
            candidates.insert(0, package_dir / "src/main.rs")
        return candidates
    directory = {"bench": "benches", "example": "examples", "test": "tests"}[kind]
    return [
        package_dir / directory / f"{name}.rs",
        package_dir / directory / name / "main.rs",
    ]


def tracked_files(root: Path) -> set[Path]:
    try:
        completed = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z", "--cached"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        detail = (
            error.stderr.decode("utf-8", "replace").strip()
            if isinstance(error, subprocess.CalledProcessError)
            else str(error)
        )
        raise IntegrityError(f"cannot enumerate tracked files: {detail}") from error

    tracked: set[Path] = set()
    for raw_path in completed.stdout.split(b"\0"):
        if not raw_path:
            continue
        try:
            path = raw_path.decode("utf-8", "strict")
        except UnicodeDecodeError as error:
            raise IntegrityError("tracked path is not valid UTF-8") from error
        tracked.add((root / path).resolve())
    return tracked


def verify(
    root: Path, *, require_tracked: bool = False
) -> tuple[list[Path], list[Target]]:
    root = root.resolve()
    root_manifest = root / "Cargo.toml"
    root_document = load_manifest(root_manifest)
    manifests = workspace_manifests(root, root_document)
    targets: list[Target] = []

    for manifest in manifests:
        document = root_document if manifest == root_manifest else load_manifest(manifest)
        targets.extend(explicit_targets(manifest, document))

    errors: list[str] = []
    tracked = tracked_files(root) if require_tracked else None
    for target in targets:
        source = target.source.resolve()
        try:
            source.relative_to(root)
        except ValueError:
            errors.append(
                f"{target.manifest}: {target.kind} target {target.name!r} "
                f"resolves outside workspace: {target.source}"
            )
            continue
        if not source.is_file():
            errors.append(
                f"{target.manifest}: missing {target.kind} target source "
                f"for {target.name!r}: {target.source}"
            )
        elif tracked is not None and source not in tracked:
            errors.append(
                f"{target.manifest}: untracked {target.kind} target source "
                f"for {target.name!r}: {target.source}"
            )

    if errors:
        raise IntegrityError("\n".join(errors))
    return manifests, targets


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="workspace root (defaults to the script's repository)",
    )
    parser.add_argument(
        "--require-tracked",
        action="store_true",
        help="also require every target source to be tracked by Git",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        manifests, targets = verify(args.root, require_tracked=args.require_tracked)
    except IntegrityError as error:
        print(f"source-integrity: FAILED\n{error}", file=sys.stderr)
        return 1

    print(
        "source-integrity: OK "
        f"({len(manifests)} manifests, {len(targets)} manifest targets)"
    )
    for target in targets:
        print(f"  {target.kind:7} {target.name}: {target.source}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
