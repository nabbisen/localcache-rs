#!/usr/bin/env python3
"""Canonical RFC 009 source/archive runner and Git-free artifact verifier."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shlex
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
import zlib
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

import release_archive
import source_integrity


LAYOUT = "archive-root"
REQUIRED_PATHS = (
    "Cargo.toml",
    "Cargo.lock",
    "crates/localcache/Cargo.toml",
    "crates/localcache/src/lib.rs",
    "crates/localcache/benches/cache_bench.rs",
    "crates/cli/Cargo.toml",
    "crates/cli/src/main.rs",
    "docs/book.toml",
    "docs/src/SUMMARY.md",
    "rfcs/README.md",
    "README.md",
    "CHANGELOG.md",
    "ROADMAP.md",
    "LICENSE",
    "NOTICE",
)

# RFC 009 R12/R14, RC-1 (2026-07-29 M6e RC-construction review): the
# canonical CI job set `aggregate-ci` requires — not whatever the caller
# happens to pass via `--require-job`. Must track `.github/workflows/ci.yaml`
# `release-gate`'s `needs:` list exactly; a job present there but absent here
# would silently stop being required.
CI_REQUIRED_JOBS: tuple[str, ...] = (
    "source-integrity",
    "fmt",
    "matrix",
    "bench-compile",
    "msrv",
    "dependency-security",
    "archive",
    "doc-package",
)

# RFC 009 R12: the gates `release` (the canonical entry point) invokes, in
# order, as subprocesses of this same script — not re-implemented. Each is
# independently runnable and independently testable; `release` only adds
# fail-fast orchestration and one consolidated R14 summary.
RELEASE_GATES: tuple[str, ...] = ("source", "msrv", "doc-package", "security")

# RFC 009 R10/R11: install examples that must name the exact coming version.
# Deliberately a fixed, narrow target list rather than a broad version-string
# scan — historical CHANGELOG entries, compatibility ranges, and schema-era
# prose (e.g. "v0.18.0+", "the v0.20.1 schema") mention other versions on
# purpose and must never be treated as stale or rewritten.
VERSION_REFERENCE_TARGETS: tuple[str, ...] = (
    "README.md",
    "docs/src/getting_started.md",
    "docs/src/introduction.md",
)
VERSION_REFERENCE_PATTERN = re.compile(r'^localcache = "([^"]+)"$', re.MULTILINE)


class ReleaseError(Exception):
    """A release gate or orchestration invariant failed."""


@dataclass
class GateLog:
    path: Path

    def record(
        self,
        *,
        name: str,
        command: Sequence[str],
        cwd: Path,
        status: int,
        output: str,
    ) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with self.path.open("a", encoding="utf-8") as file:
            file.write(f"gate: {name}\n")
            file.write(f"command: {shlex.join(command)}\n")
            file.write(f"cwd: {cwd}\n")
            file.write(f"exit-status: {status}\n")
            file.write("output:\n")
            file.write(output)
            if output and not output.endswith("\n"):
                file.write("\n")
            file.write("---\n")


def run_gate(
    logger: GateLog,
    name: str,
    command: Sequence[str],
    cwd: Path,
    *,
    environment: dict[str, str] | None = None,
    echo_output: bool = True,
) -> str:
    print(f"[{name}] {shlex.join(command)}")
    env = os.environ.copy()
    if environment:
        env.update(environment)
    try:
        completed = subprocess.run(
            list(command),
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
    except OSError as error:
        logger.record(
            name=name, command=command, cwd=cwd, status=127, output=str(error)
        )
        raise ReleaseError(f"{name}: cannot execute {command[0]!r}: {error}") from error
    logger.record(
        name=name,
        command=command,
        cwd=cwd,
        status=completed.returncode,
        output=completed.stdout,
    )
    if completed.stdout and echo_output:
        print(completed.stdout, end="" if completed.stdout.endswith("\n") else "\n")
    elif completed.stdout:
        print(f"[{name}] output captured in {logger.path}")
    if completed.returncode:
        raise ReleaseError(f"{name} failed with exit status {completed.returncode}")
    return completed.stdout


def repository_root() -> Path:
    return Path(__file__).resolve().parents[1]


def git_output(root: Path, *args: str) -> str:
    try:
        return subprocess.check_output(
            ["git", "-C", str(root), *args],
            text=True,
            stderr=subprocess.PIPE,
        ).strip()
    except (OSError, subprocess.CalledProcessError) as error:
        detail = (
            error.stderr.strip()
            if isinstance(error, subprocess.CalledProcessError)
            else str(error)
        )
        raise ReleaseError(f"git {' '.join(args)} failed: {detail}") from error


def require_clean_commit(root: Path) -> str:
    status = git_output(root, "status", "--porcelain=v1", "--untracked-files=all")
    if status:
        first = status.splitlines()[0]
        raise ReleaseError(f"source context requires a clean tracked tree: {first}")
    commit = git_output(root, "rev-parse", "HEAD")
    if not release_archive.COMMIT_RE.fullmatch(commit):
        raise ReleaseError(f"HEAD is not a full commit ID: {commit!r}")
    return commit


def require_output_boundary(root: Path, output: Path) -> Path:
    root = root.resolve()
    output = output.resolve()
    try:
        relative = output.relative_to(root)
    except ValueError:
        return output
    if not relative.parts or relative.parts[0] != ".git-exclude":
        raise ReleaseError(
            "output must be outside the repository or below the ignored "
            ".git-exclude/ boundary"
        )
    return output


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def load_tool_manifest(root: Path) -> dict[str, object]:
    path = root / "scripts/release-tools.toml"
    try:
        with path.open("rb") as file:
            document = tomllib.load(file)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ReleaseError(f"cannot load producer-tool manifest: {error}") from error
    if document.get("schema-version") != 1:
        raise ReleaseError("unsupported producer-tool manifest schema")
    return document


def verify_implementation(root: Path, name: str, policy: object) -> str:
    if not isinstance(policy, dict):
        raise ReleaseError(f"invalid implementation policy: {name}")
    path_value = policy.get("path")
    expected_sha256 = policy.get("sha256")
    if not isinstance(path_value, str) or not isinstance(expected_sha256, str):
        raise ReleaseError(f"incomplete implementation policy: {name}")
    path = root / path_value
    digest = sha256_file(path)
    if digest != expected_sha256:
        raise ReleaseError(
            f"release implementation hash mismatch for {path_value}: {digest}"
        )
    return f"{path_value}; sha256={digest}"


def verify_named_implementation(root: Path, name: str) -> str:
    """Verify one `[implementations.<name>]` hash pin in isolation.

    A gate that only needs its own implementation verified (for example
    `security`) should call this instead of `verify_implementations`.
    """
    document = load_tool_manifest(root)
    implementations = document.get("implementations")
    if not isinstance(implementations, dict):
        raise ReleaseError("producer-tool manifest is missing table: implementations")
    policy = implementations.get(name)
    if policy is None:
        raise ReleaseError(f"no implementation policy for {name!r}")
    return verify_implementation(root, name, policy)


def verify_implementations(root: Path) -> dict[str, str]:
    """Verify every `[implementations]` hash pin (RFC 009 R12 supply-chain
    integrity for the gate scripts themselves).

    RFC 017 retired the canonical/noncanonical producer distinction this
    function used to police in addition to the implementation pins — there is
    no more environment to verify against, only the scripts. Toolchain
    identity (platform, git, Python, zlib, locale, timezone, compiler
    versions) is now recorded, not gated; see `toolchain_identity`.
    """
    document = load_tool_manifest(root)
    implementations = document.get("implementations")
    if not isinstance(implementations, dict):
        raise ReleaseError("producer-tool manifest is missing table: implementations")
    return {
        name: verify_implementation(root, name, policy)
        for name, policy in implementations.items()
    }


def toolchain_identity() -> dict[str, object]:
    """RFC 017 R4: record toolchain and host identity per run.

    This is descriptive, not a gate — RFC 017 retired the pinned-environment
    contract entirely. If a future run's uncompressed-tar digest differs from
    a prior run's for the same commit, these are the first values to compare;
    an explainable difference is the goal, not an impossible one.
    """
    return {
        "platform": platform.platform(),
        "target_triple": f"{platform.machine()}-{platform.system().lower()}",
        "git_version": command_version(["git", "--version"]),
        "python_version": command_version(["python3", "--version"]),
        "zlib_version": zlib.ZLIB_RUNTIME_VERSION,
        "locale": os.environ.get("LC_ALL") or os.environ.get("LANG") or "unset",
        "timezone": os.environ.get("TZ") or time.tzname[0],
        "cargo_version": command_version(["cargo", "--version"]),
        "rustc_version": command_version(["rustc", "--version"]),
        "mdbook_version": command_version(["mdbook", "--version"]),
    }


def command_version(command: Sequence[str]) -> str:
    try:
        completed = subprocess.run(
            list(command),
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise ReleaseError(f"required tool is unavailable: {command[0]}") from error
    return completed.stdout.strip()


def cargo_metadata(
    root: Path, logger: GateLog, *, gate_name: str = "cargo-metadata"
) -> tuple[str, dict[str, object]]:
    output = run_gate(
        logger,
        gate_name,
        ["cargo", "metadata", "--locked", "--format-version", "1"],
        root,
        echo_output=False,
    )
    try:
        document = json.loads(output)
    except json.JSONDecodeError as error:
        raise ReleaseError("Cargo metadata was not valid JSON") from error
    return workspace_version(document), document


def workspace_version(document: dict[str, object]) -> str:
    """Return the shared `localcache`/`localcache-cli` package version.

    This only checks that the two packages' own declared versions agree
    (both use `version.workspace = true`, so any disagreement is a Cargo
    metadata anomaly, not a real state). It does not inspect the CLI's
    `localcache` path-dependency requirement — that is a separate, looser
    concern the workspace manifest itself owns.
    """
    try:
        packages = document["packages"]
        versions = {
            package["name"]: package["version"]
            for package in packages
            if package["name"] in {"localcache", "localcache-cli"}
        }
    except (KeyError, TypeError) as error:
        raise ReleaseError("Cargo metadata did not contain expected packages") from error
    if set(versions) != {"localcache", "localcache-cli"}:
        raise ReleaseError("Cargo metadata is missing a workspace package")
    if len(set(versions.values())) != 1:
        raise ReleaseError(f"workspace package versions differ: {versions}")
    return versions["localcache"]


def verify_version_references(root: Path, expected_version: str) -> None:
    """RFC 009 R10/R11: every install example names the exact coming version.

    Fails closed on a missing target file, a target with no matching install
    line, or a stale version — this is what caught README.md/docs claiming
    0.20.1 while both packages still said 0.20.0 before M6d.
    """
    for relative in VERSION_REFERENCE_TARGETS:
        path = root / relative
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as error:
            raise ReleaseError(f"cannot read version reference {relative}: {error}") from error
        matches = VERSION_REFERENCE_PATTERN.findall(text)
        if not matches:
            raise ReleaseError(f"{relative}: no install-example version line found")
        stale = sorted({match for match in matches if match != expected_version})
        if stale:
            raise ReleaseError(
                f"{relative}: stale version reference(s) {stale}, "
                f"expected {expected_version!r}"
            )


def verify_changelog_has_coming_version_section(root: Path, expected_version: str) -> None:
    """RFC 009 R10/R11: `CHANGELOG.md` has a non-empty coming-version section."""
    path = root / "CHANGELOG.md"
    text = path.read_text(encoding="utf-8")
    headers = re.findall(r"^## \[(.+?)\](?:\s*—.*)?$", text, re.MULTILINE)
    if expected_version not in headers:
        raise ReleaseError(
            f"CHANGELOG.md has no section for the coming version {expected_version!r}"
        )
    sections = re.split(r"^## \[.+?\](?:\s*—.*)?$", text, flags=re.MULTILINE)
    # `re.split` on this pattern produces one leading chunk (before the first
    # header) then one chunk per header, in header order; headers[0]'s body
    # is sections[1].
    index = headers.index(expected_version)
    body = sections[index + 1].strip()
    if not body:
        raise ReleaseError(f"CHANGELOG.md section for {expected_version!r} is empty")


def verify_required_layout(root: Path) -> None:
    for relative in REQUIRED_PATHS:
        path = root / relative
        if not path.is_file():
            raise ReleaseError(f"artifact is missing required file: {relative}")
    forbidden = (
        root / ".git",
        root / ".git-exclude",
        root / "target",
        root / "docs/book",
    )
    for path in forbidden:
        if path.exists():
            raise ReleaseError(f"artifact contains forbidden path: {path.relative_to(root)}")
    nested = list(root.glob("localcache-v*.tar.gz"))
    if nested:
        raise ReleaseError(f"artifact contains nested release archive: {nested[0].name}")


def run_source_integrity(
    root: Path, logger: GateLog, *, require_tracked: bool
) -> None:
    command = [sys.executable, "scripts/source_integrity.py"]
    if require_tracked:
        command.append("--require-tracked")
    run_gate(logger, "source-integrity", command, root)


def smoke_commands() -> tuple[tuple[str, list[str]], ...]:
    return (
        (
            "cargo-metadata",
            ["cargo", "metadata", "--locked", "--format-version", "1"],
        ),
        (
            "library-all-targets",
            [
                "cargo",
                "check",
                "-p",
                "localcache",
                "--all-targets",
                "--all-features",
                "--locked",
            ],
        ),
        (
            "cli-all-targets",
            [
                "cargo",
                "check",
                "-p",
                "localcache-cli",
                "--all-targets",
                "--all-features",
                "--locked",
            ],
        ),
        (
            "benchmark-compile",
            [
                "cargo",
                "bench",
                "-p",
                "localcache",
                "--bench",
                "cache_bench",
                "--no-run",
                "--features",
                "localcache/json",
                "--locked",
            ],
        ),
        ("mdbook", ["mdbook", "build", "docs"]),
    )


def run_m1_smoke(
    root: Path,
    logger: GateLog,
    target_dir: Path,
    docs_dir: Path,
) -> None:
    target_dir.mkdir(parents=True, exist_ok=True)
    docs_dir.mkdir(parents=True, exist_ok=True)
    environment = {
        "CARGO_TARGET_DIR": str(target_dir.resolve()),
        "MDBOOK_BUILD__BUILD_DIR": str(docs_dir.resolve()),
        "TMPDIR": str(target_dir.resolve()),
    }
    for name, command in smoke_commands():
        run_gate(
            logger,
            name,
            command,
            root,
            environment=environment,
            echo_output=name != "cargo-metadata",
        )


def write_manifest(path: Path, values: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as file:
        json.dump(values, file, indent=2, sort_keys=True)
        file.write("\n")


def append_summary(path: Path, line: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as file:
        file.write(f"{line}\n")


def record_failure_summary(args: argparse.Namespace, error: Exception) -> None:
    if getattr(args, "mode", None) == "source":
        output = Path(args.output_dir).resolve()
        if not output.exists():
            return
        summary = output / "evidence/summary.log"
    elif getattr(args, "mode", None) == "artifact":
        summary = Path(args.evidence_dir).resolve() / "summary.log"
    elif getattr(args, "mode", None) == "security":
        summary = Path(args.output_dir).resolve() / "summary.log"
    elif getattr(args, "mode", None) == "aggregate-ci":
        summary = Path(args.output_dir).resolve() / "summary.log"
    elif getattr(args, "mode", None) == "msrv":
        summary = Path(args.output_dir).resolve() / "summary.log"
    elif getattr(args, "mode", None) == "doc-package":
        output = Path(args.output_dir).resolve()
        if not output.exists():
            return
        summary = output / "summary.log"
    elif getattr(args, "mode", None) == "release":
        summary = Path(args.output_dir).resolve() / "summary.log"
    else:
        return
    append_summary(summary, "status: FAIL")
    append_summary(summary, f"failure: {error}")
    append_summary(summary, "required-downstream-steps: NOT COMPLETED")


def artifact_mode(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    evidence = Path(args.evidence_dir).resolve()
    summary = evidence / "summary.log"
    append_summary(summary, "context: artifact")
    append_summary(summary, "status: RUNNING")
    if args.expected_layout != LAYOUT:
        raise ReleaseError(
            f"unsupported artifact layout {args.expected_layout!r}; expected {LAYOUT!r}"
        )
    if not re_full_sha256(args.expected_uncompressed_sha256):
        raise ReleaseError(
            "expected uncompressed-tar SHA-256 must be 64 lowercase hex digits"
        )
    if (root / ".git").exists():
        raise ReleaseError("artifact context must not contain .git/")

    target = Path(args.target_dir).resolve()
    logger = GateLog(evidence / "smoke.log")
    verify_required_layout(root)
    run_source_integrity(root, logger, require_tracked=False)
    version, _metadata = cargo_metadata(root, logger, gate_name="version-metadata")
    if version != args.expected_version:
        raise ReleaseError(
            f"artifact version {version!r} does not match parent value "
            f"{args.expected_version!r}"
        )
    run_m1_smoke(root, logger, target, target / "mdbook")
    write_manifest(
        evidence / "manifest.json",
        {
            "archive_uncompressed_sha256": args.expected_uncompressed_sha256,
            "context": "artifact",
            "layout": args.expected_layout,
            "status": "pass",
            "version": args.expected_version,
        },
    )
    append_summary(summary, "source-integrity: PASS")
    append_summary(summary, "version-contract: PASS")
    append_summary(summary, "m1-smoke: PASS")
    append_summary(summary, "status: PASS")
    return 0


def security_mode(args: argparse.Namespace) -> int:
    """RFC 009 R13: dependency-security gate with fail-closed aggregation.

    Wraps `scripts/check_advisories.py` (verified against its
    `release-tools.toml` hash pin before it runs) via `run_gate`, so a
    nonzero exit — a denied finding or an operational failure — raises
    `ReleaseError` the same way every other gate in this module does. R14's
    policy/advisory-database digests are already emitted by the checker
    itself into the nested `advisories/` evidence directory; this wrapper
    adds no separate digest capture.
    """
    root = repository_root()
    output = require_output_boundary(root, args.output_dir)
    summary = output / "summary.log"
    append_summary(summary, "context: security")
    append_summary(summary, "status: RUNNING")
    logger = GateLog(output / "gate.log")
    verify_named_implementation(root, "check-advisories")
    append_summary(summary, "tool-manifest: PASS")
    advisories_dir = output / "advisories"
    run_gate(
        logger,
        "dependency-security",
        ["python3", "scripts/check_advisories.py", str(advisories_dir)],
        root,
    )
    write_manifest(
        output / "manifest.json",
        {
            "context": "security",
            "status": "pass",
            "advisories_evidence": "advisories",
            **ci_identity(),
        },
    )
    append_summary(summary, "dependency-security: PASS")
    append_summary(summary, "status: PASS")
    return 0


def verify_declared_toolchain(rustc_version: str, cargo_version: str, declared: str) -> None:
    """Fail closed unless `rustc`/`cargo --version` exactly match the
    declared MSRV (`[workspace.package].rust-version`).

    The declared-MSRV gate exists to prove that *specific* toolchain
    compiles the workspace; running it under any other toolchain (for
    example accidentally under stable) would silently defeat its purpose
    rather than merely skip it.
    """
    if not rustc_version.startswith(f"rustc {declared}."):
        raise ReleaseError(
            f"active rustc {rustc_version!r} does not match declared MSRV {declared!r}"
        )
    if not cargo_version.startswith(f"cargo {declared}."):
        raise ReleaseError(
            f"active cargo {cargo_version!r} does not match declared MSRV {declared!r}"
        )


def declared_toolchain_installed(toolchain_list_output: str, declared: str) -> bool:
    """Pure predicate over `rustup toolchain list` output, split out of
    `require_declared_toolchain_installed` so it is testable without a real
    `rustup` invocation (not every environment running this test suite has
    Rust installed at all)."""
    return any(
        line.startswith(f"{declared}.") for line in toolchain_list_output.splitlines()
    )


def require_declared_toolchain_installed(declared: str) -> None:
    """RC-2: `release` mode must fail closed, not silently skip, if the
    declared MSRV toolchain — used to run the `msrv` gate via `rustup run`,
    independent of whatever toolchain happens to be ambient — is not
    installed.
    """
    try:
        completed = subprocess.run(
            ["rustup", "toolchain", "list"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise ReleaseError(f"cannot list rustup toolchains: {error}") from error
    if not declared_toolchain_installed(completed.stdout, declared):
        raise ReleaseError(
            f"declared MSRV toolchain {declared!r} is not installed; "
            f"run `rustup toolchain install {declared}`"
        )


def msrv_mode(args: argparse.Namespace) -> int:
    """RFC 009 R8/M6e item 10: declared-MSRV matrix with toolchain evidence."""
    root = repository_root()
    output = require_output_boundary(root, args.output_dir)
    summary = output / "summary.log"
    append_summary(summary, "context: msrv")
    append_summary(summary, "status: RUNNING")
    logger = GateLog(output / "gate.log")

    with (root / "Cargo.toml").open("rb") as file:
        workspace_document = tomllib.load(file)
    try:
        declared = workspace_document["workspace"]["package"]["rust-version"]
    except (KeyError, TypeError) as error:
        raise ReleaseError("Cargo.toml has no [workspace.package].rust-version") from error

    rustc_version = command_version(["rustc", "--version"])
    cargo_version = command_version(["cargo", "--version"])
    verify_declared_toolchain(rustc_version, cargo_version, declared)
    append_summary(summary, f"declared-toolchain: PASS ({rustc_version})")

    run_gate(
        logger,
        "declared-msrv-matrix",
        ["python3", "scripts/feature_matrix.py", "--run-msrv"],
        root,
    )
    append_summary(summary, "declared-msrv-matrix: PASS")

    write_manifest(
        output / "manifest.json",
        {
            "context": "msrv",
            "status": "pass",
            "declared_rust_version": declared,
            "rustc_version": rustc_version,
            "cargo_version": cargo_version,
            **ci_identity(),
        },
    )
    append_summary(summary, "status: PASS")
    return 0


def doc_package_mode(args: argparse.Namespace) -> int:
    """RFC 009 R9/M6e items 7-8: rustdoc, mdBook, and joint package
    verification.

    Runs `cargo doc`, `mdbook build docs`, and `cargo package --workspace
    --locked` against a clean committed tree — no `--allow-dirty`, no
    `--no-verify` — then opens each produced `.crate` and records its
    normalized manifest digest and complete file list as evidence. R9
    requires the gate to inspect both, not just observe a zero exit status.
    """
    root = repository_root()
    commit = require_clean_commit(root)
    output = require_output_boundary(root, Path(args.output_dir))
    if output.exists() and any(output.iterdir()):
        raise ReleaseError(f"output directory is not empty: {output}")
    output.mkdir(parents=True, exist_ok=True)

    summary = output / "summary.log"
    append_summary(summary, "context: doc-package")
    append_summary(summary, f"commit: {commit}")
    append_summary(summary, "status: RUNNING")
    logger = GateLog(output / "gate.log")

    doc_target = output / "target-doc"
    run_gate(
        logger,
        "cargo-doc",
        ["cargo", "doc", "--workspace", "--no-deps", "--all-features", "--locked"],
        root,
        environment={"CARGO_TARGET_DIR": str(doc_target.resolve())},
    )
    append_summary(summary, "cargo-doc: PASS")

    mdbook_target = output / "mdbook"
    run_gate(
        logger,
        "mdbook-build",
        ["mdbook", "build", "docs"],
        root,
        environment={"MDBOOK_BUILD__BUILD_DIR": str(mdbook_target.resolve())},
    )
    append_summary(summary, "mdbook-build: PASS")

    version, _metadata = cargo_metadata(root, logger, gate_name="version-metadata")

    package_target = output / "target-package"
    run_gate(
        logger,
        "cargo-package",
        ["cargo", "package", "--workspace", "--locked"],
        root,
        environment={"CARGO_TARGET_DIR": str(package_target.resolve())},
    )
    append_summary(summary, "cargo-package: PASS")

    package_dir = package_target / "package"
    packages: dict[str, object] = {}
    for package_name in ("localcache", "localcache-cli"):
        crate_path = package_dir / f"{package_name}-{version}.crate"
        if not crate_path.is_file():
            raise ReleaseError(f"expected package artifact missing: {crate_path}")
        with tarfile.open(crate_path, mode="r:gz") as archive:
            files = sorted(
                member.name for member in archive.getmembers() if member.isfile()
            )
            manifest_member = f"{package_name}-{version}/Cargo.toml"
            if manifest_member not in files:
                raise ReleaseError(f"{crate_path.name} is missing its normalized Cargo.toml")
            extracted = archive.extractfile(manifest_member)
            if extracted is None:
                raise ReleaseError(f"{crate_path.name}: normalized Cargo.toml could not be read")
            normalized_manifest = extracted.read()
        packages[package_name] = {
            "crate": crate_path.name,
            "sha256": sha256_file(crate_path),
            "file_count": len(files),
            "files": files,
            "normalized_manifest_sha256": hashlib.sha256(normalized_manifest).hexdigest(),
        }
    append_summary(summary, "joint-package-verification: PASS")

    write_manifest(
        output / "manifest.json",
        {
            "commit": commit,
            "context": "doc-package",
            "packages": packages,
            "status": "pass",
            "version": version,
            **ci_identity(),
        },
    )
    append_summary(summary, "status: PASS")
    print(f"doc+package verified: {', '.join(str(p['crate']) for p in packages.values())}")
    return 0


def aggregate_ci_mode(args: argparse.Namespace) -> int:
    """RFC 009 M6c item 3 / RC-1: fail-closed final CI aggregator.

    Fails closed when a required job did not succeed, when the canonical
    required-job set (`CI_REQUIRED_JOBS`) is not fully covered by the
    supplied `--require-job` values, or when a required evidence manifest is
    missing, unreadable, not a pass, or bound to a different workflow run or
    commit than the one aggregating it. This is the only gate that reads
    `needs.*.result`/`github.run_id`/`github.sha` — every other gate is
    CI-agnostic and runs identically locally.

    RC-1 (2026-07-29 M6e RC-construction review): the *set* of required job
    names must come from this tool, not from whatever the caller happens to
    pass. Before this fix, invoking `aggregate-ci` with only one
    `--require-job` value verified only that job and silently ignored the
    rest — `msrv`, `doc-package`, and `security` could all be unrun and the
    aggregation would still exit 0. Explicit `--require-job` values still
    supply each job's actual result (this tool has no other way to learn a
    GitHub Actions `needs.<job>.result`); what they may no longer do is
    narrow *which* jobs are required.
    """
    root = repository_root()
    output = require_output_boundary(root, args.output_dir)
    summary = output / "summary.log"
    append_summary(summary, "context: aggregate-ci")
    append_summary(summary, f"run-id: {args.run_id}")
    append_summary(summary, f"sha: {args.sha}")
    append_summary(summary, "status: RUNNING")

    failures: list[str] = []

    supplied: dict[str, str] = {}
    for requirement in args.require_job:
        name, separator, result = requirement.partition("=")
        if not separator or not name or not result:
            raise ReleaseError(f"malformed --require-job value: {requirement!r}")
        supplied[name] = result

    missing_jobs = sorted(set(CI_REQUIRED_JOBS) - set(supplied))
    if missing_jobs:
        raise ReleaseError(
            "required jobs omitted from this aggregation (RFC 009 R12/R14 "
            f"fail-closed default set): {missing_jobs}"
        )

    for name, result in supplied.items():
        if result != "success":
            failures.append(
                f"required job {name!r} did not succeed (result={result!r})"
            )

    for manifest_path in args.evidence_manifest:
        label = str(manifest_path)
        manifest_failures: list[str] = []
        try:
            with manifest_path.open("rb") as file:
                document = json.load(file)
        except (OSError, json.JSONDecodeError) as error:
            failures.append(f"{label}: cannot read evidence manifest: {error}")
            continue
        if not isinstance(document, dict):
            failures.append(f"{label}: evidence manifest is not a JSON object")
            continue
        if document.get("status") != "pass":
            manifest_failures.append(
                f"{label}: status is {document.get('status')!r}, not 'pass'"
            )
        if document.get("ci_run_id") != args.run_id:
            manifest_failures.append(
                f"{label}: ci_run_id {document.get('ci_run_id')!r} does not match "
                f"this workflow run {args.run_id!r}"
            )
        observed_sha = document.get("ci_sha") or document.get("commit")
        if observed_sha != args.sha:
            manifest_failures.append(
                f"{label}: commit {observed_sha!r} does not match this checkout "
                f"{args.sha!r}"
            )
        if manifest_failures:
            failures.extend(manifest_failures)
        else:
            append_summary(summary, f"evidence-binding: PASS ({label})")

    if failures:
        # `status: FAIL` and the failure detail are appended uniformly by
        # `record_failure_summary` in `main()`, the same as every other mode
        # — this function does not self-finalize the summary.
        raise ReleaseError(
            "CI provenance aggregation failed:\n" + "\n".join(failures)
        )

    write_manifest(
        output / "manifest.json",
        {
            "context": "aggregate-ci",
            "status": "pass",
            "run_id": args.run_id,
            "sha": args.sha,
            "required_jobs": list(args.require_job),
            "evidence_manifests": [str(path) for path in args.evidence_manifest],
        },
    )
    append_summary(summary, "status: PASS")
    return 0


def release_mode(args: argparse.Namespace) -> int:
    """RFC 009 R12: the canonical release entry point.

    Orchestrates `RELEASE_GATES` in order, each as a subprocess of this same
    script — not re-implemented — so every gate stays independently runnable
    and independently testable. Fails fast with nonzero status on the first
    failing gate; `run_gate` already raises `ReleaseError` on a nonzero exit,
    so no gate after the first failure runs. On success, writes one
    consolidated manifest folding in the R14 fields each gate produced
    (archive identity, toolchain identity, declared-MSRV versions, package
    file lists, and the pointer to the nested security evidence) rather than
    leaving them one directory down from each other.

    RC-2: `msrv` runs under the declared MSRV toolchain explicitly, via
    `rustup run`, regardless of whatever toolchain is ambient. Every other
    gate runs under the ambient toolchain (stable, by convention and in CI).
    This split matters because `cargo package --workspace --locked` (part of
    `doc-package`) verifies each workspace member in isolation against the
    real crates.io index rather than the just-packaged sibling — under a
    cargo old enough for that to matter (1.85 cannot see the sibling the way
    newer cargo can), the CLI's `localcache` dependency resolves to the
    older published version instead of the one just packaged, dragging in a
    `libsqlite3-sys` that needs a newer Rust than 1.85 to build. Packaging
    is release tooling, not an MSRV assertion; `msrv_mode`'s `cargo check`
    matrix is what proves 1.85 compatibility. Running both gates under one
    toolchain was RC-1's composition assumption, and that assumption was the
    defect.
    """
    root = repository_root()
    output = require_output_boundary(root, Path(args.output_dir))
    if output.exists() and any(output.iterdir()):
        raise ReleaseError(f"output directory is not empty: {output}")
    output.mkdir(parents=True, exist_ok=True)

    summary = output / "summary.log"
    append_summary(summary, "context: release")
    append_summary(summary, f"gates: {', '.join(RELEASE_GATES)}")
    append_summary(summary, "status: RUNNING")
    logger = GateLog(output / "gate.log")

    with (root / "Cargo.toml").open("rb") as file:
        workspace_document = tomllib.load(file)
    try:
        declared = workspace_document["workspace"]["package"]["rust-version"]
    except (KeyError, TypeError) as error:
        raise ReleaseError("Cargo.toml has no [workspace.package].rust-version") from error
    require_declared_toolchain_installed(declared)

    script = str(Path(__file__).resolve())
    manifests: dict[str, dict[str, object]] = {}
    for gate in RELEASE_GATES:
        gate_output = output / gate
        command = [sys.executable, script, gate, "--output-dir", str(gate_output)]
        if gate == "msrv":
            command = ["rustup", "run", declared, *command]
        run_gate(logger, gate, command, root)
        append_summary(summary, f"{gate}: PASS")
        manifest_path = (
            gate_output / "evidence" / "manifest.json"
            if gate == "source"
            else gate_output / "manifest.json"
        )
        with manifest_path.open("rb") as file:
            manifests[gate] = json.load(file)

    source_manifest = manifests["source"]
    msrv_manifest = manifests["msrv"]
    doc_package_manifest = manifests["doc-package"]
    security_manifest = manifests["security"]

    write_manifest(
        output / "manifest.json",
        {
            "context": "release",
            "status": "pass",
            "gates": list(RELEASE_GATES),
            "commit": source_manifest["commit"],
            "version": source_manifest["version"],
            "rc_eligible": source_manifest["rc_eligible"],
            "archive": source_manifest["archive"],
            "archive_uncompressed_sha256": source_manifest["archive_uncompressed_sha256"],
            "archive_compressed_sha256_advisory": source_manifest[
                "archive_compressed_sha256_advisory"
            ],
            "toolchain_identity": source_manifest["toolchain_identity"],
            "tool_versions": source_manifest["tool_versions"],
            "declared_rust_version": msrv_manifest["declared_rust_version"],
            "declared_rustc_version": msrv_manifest["rustc_version"],
            "declared_cargo_version": msrv_manifest["cargo_version"],
            "packages": doc_package_manifest["packages"],
            "advisories_evidence": f"security/{security_manifest['advisories_evidence']}",
            **ci_identity(),
        },
    )
    append_summary(summary, "status: PASS")
    print(f"release evidence: {output}")
    return 0


def re_full_sha256(value: str) -> bool:
    return len(value) == 64 and all(character in "0123456789abcdef" for character in value)


def ci_identity() -> dict[str, object]:
    """RFC 009 R14 "CI workflow run ID, job identity, and commit binding".

    Populated only under GitHub Actions (`GITHUB_ACTIONS=true`); all fields
    are `None` for a local run, so evidence never implies CI provenance it
    doesn't have. `scripts/release.py aggregate-ci` is the fail-closed check
    that these fields, once present, actually match the aggregating job's own
    run ID and commit — this function only captures them.
    """
    if os.environ.get("GITHUB_ACTIONS") != "true":
        return {"ci_run_id": None, "ci_job": None, "ci_workflow": None, "ci_sha": None}
    return {
        "ci_run_id": os.environ.get("GITHUB_RUN_ID"),
        "ci_job": os.environ.get("GITHUB_JOB"),
        "ci_workflow": os.environ.get("GITHUB_WORKFLOW"),
        "ci_sha": os.environ.get("GITHUB_SHA"),
    }


def rc_eligibility(
    *, clean_worktree: bool, all_required_gates_passed: bool, evidence_complete: bool
) -> bool:
    """RFC 017 R3: RC eligibility depends on gates, not environment.

    True only when the tree was clean at commit time, every required gate in
    this release run passed, and the evidence bundle is complete with no
    skipped required step. There is no environmental signal any more — RFC
    017 retired the canonical/noncanonical producer distinction and the
    `RFC009_RC_ELIGIBLE` wrapper attestation it required (M6c item 5), since
    there is no longer an environmental claim left to attest externally.
    """
    return clean_worktree and all_required_gates_passed and evidence_complete


def source_mode(args: argparse.Namespace) -> int:
    root = repository_root()
    commit = require_clean_commit(root)
    tool_versions = verify_implementations(root)
    output = require_output_boundary(root, Path(args.output_dir))
    if output.exists() and any(output.iterdir()):
        raise ReleaseError(f"output directory is not empty: {output}")
    output.mkdir(parents=True, exist_ok=True)

    evidence = output / "evidence"
    summary = evidence / "summary.log"
    append_summary(summary, "context: source")
    append_summary(summary, f"commit: {commit}")
    append_summary(summary, "status: RUNNING")
    logger = GateLog(evidence / "checkout-smoke.log")
    run_source_integrity(root, logger, require_tracked=True)
    append_summary(summary, "tracked-source-integrity: PASS")
    version, _metadata = cargo_metadata(root, logger, gate_name="version-metadata")
    append_summary(summary, f"version-contract: PASS ({version})")
    verify_version_references(root, version)
    append_summary(summary, "version-reference-consistency: PASS")
    verify_changelog_has_coming_version_section(root, version)
    append_summary(summary, "changelog-coming-version: PASS")
    with tempfile.TemporaryDirectory(prefix="checkout-", dir=output) as temporary:
        checkout_temporary = Path(temporary)
        run_m1_smoke(
            root,
            logger,
            checkout_temporary / "target",
            checkout_temporary / "docs",
        )
    append_summary(summary, "checkout-m1-smoke: PASS")

    expected = release_archive.expected_manifest(root, commit)
    raw_first = release_archive.build_git_tar(root, commit)
    members = release_archive.validate_tar(raw_first, expected, commit)
    append_summary(summary, "structured-archive-validation: PASS")
    raw_second = release_archive.build_git_tar(root, commit)
    uncompressed_sha256 = release_archive.sha256_bytes(raw_first)
    # RFC 017 R2: per-host determinism is gated on the *uncompressed* tar
    # digest, the identity that now matters — not on compressed bytes, which
    # depend on zlib's version and are no longer part of the contract.
    if release_archive.sha256_bytes(raw_second) != uncompressed_sha256:
        raise ReleaseError(
            "two archive constructions from one commit differ "
            "(uncompressed-tar digest)"
        )
    append_summary(summary, "same-commit-determinism: PASS")

    archive_gzip = release_archive.compress_tar(raw_first)
    archive_name = f"localcache-v{version}.tar.gz"
    archive_path = output / archive_name
    with archive_path.open("xb") as file:
        file.write(archive_gzip)
    archive_size = len(archive_gzip)
    # RFC 017 R1: the compressed digest is retained but advisory; it is never
    # gated on and never asserted as reproducible across hosts.
    archive_compressed_sha256 = release_archive.sha256_bytes(archive_gzip)
    release_archive.validate_archive_file(archive_path, expected, commit)

    archive_evidence = evidence / "archive"
    archive_evidence.mkdir(parents=True)
    with (archive_evidence / "members.txt").open("x", encoding="utf-8") as file:
        for member in members:
            mode = "executable" if member.executable else "non-executable"
            file.write(f"{member.kind}\t{mode}\t{member.path}\n")
    (archive_evidence / "sha256.txt").write_text(
        f"{uncompressed_sha256}  {archive_name} (uncompressed-tar, primary)\n"
        f"{archive_compressed_sha256}  {archive_name} (compressed, advisory)\n",
        encoding="utf-8",
    )

    with tempfile.TemporaryDirectory(prefix="extract-", dir=output) as temporary:
        extraction = Path(temporary) / "source"
        release_archive.extract_validated(members, extraction)
        verify_required_layout(extraction)
        artifact_evidence = evidence / "artifact"
        artifact_target = Path(temporary) / "target"
        command = [
            sys.executable,
            str(extraction / "scripts/release.py"),
            "artifact",
            "--root",
            str(extraction),
            "--expected-version",
            version,
            "--expected-layout",
            LAYOUT,
            "--expected-uncompressed-sha256",
            uncompressed_sha256,
            "--evidence-dir",
            str(artifact_evidence),
            "--target-dir",
            str(artifact_target),
        ]
        run_gate(logger, "artifact-context", command, extraction)
        append_summary(summary, "git-free-artifact-m1-smoke: PASS")
        # Re-assert the R4/R5 layout *after* the artifact smoke run, not only
        # before it. `--target-dir`/evidence-dir already place build output
        # outside `extraction` by construction, but this proves that stayed
        # true rather than assuming it: a gate that leaked a `target/` or
        # `docs/book/` directory into the extracted source tree would be
        # caught here before it ever reaches evidence as a pass.
        verify_required_layout(extraction)
    append_summary(summary, "post-smoke-layout-reassertion: PASS")

    write_manifest(
        evidence / "manifest.json",
        {
            "archive": archive_name,
            "archive_uncompressed_sha256": uncompressed_sha256,
            "archive_compressed_sha256_advisory": archive_compressed_sha256,
            "archive_compressed_size_advisory": archive_size,
            "commit": commit,
            "context": "source",
            "layout": LAYOUT,
            "member_count": len(members),
            "status": "pass",
            # RFC 017 R3: reaching this point already proves the tree was
            # clean (require_clean_commit, above) and every required gate in
            # this straight-line function passed (any failure would have
            # raised before this write_manifest call) -- so all three
            # conditions are literally true here, not merely assumed.
            "rc_eligible": rc_eligibility(
                clean_worktree=True,
                all_required_gates_passed=True,
                evidence_complete=True,
            ),
            "release_tool_manifest_sha256": sha256_file(
                root / "scripts/release-tools.toml"
            ),
            "tool_versions": tool_versions,
            "toolchain_identity": toolchain_identity(),
            "version": version,
            **ci_identity(),
        },
    )
    append_summary(summary, f"archive-uncompressed-sha256: {uncompressed_sha256}")
    append_summary(
        summary, f"archive-compressed-sha256-advisory: {archive_compressed_sha256}"
    )
    append_summary(summary, "status: PASS")
    print(f"verified archive: {archive_path}")
    print(f"uncompressed-tar sha256 (primary): {uncompressed_sha256}")
    print(f"compressed sha256 (advisory): {archive_compressed_sha256}")
    return 0


def validate_mode(args: argparse.Namespace) -> int:
    root = repository_root()
    commit = args.expected_commit or git_output(root, "rev-parse", "HEAD")
    expected = release_archive.expected_manifest(root, commit)
    members = release_archive.validate_archive_file(
        Path(args.archive), expected, commit
    )
    print(f"archive-validation: OK ({len(members)} logical members, commit {commit})")
    return 0


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="mode", required=True)

    source = subparsers.add_parser(
        "source", help="construct and verify an archive from a clean committed checkout"
    )
    source.add_argument("--output-dir", type=Path, required=True)
    source.set_defaults(handler=source_mode)

    artifact = subparsers.add_parser(
        "artifact", help="run Git-free M1 gates inside an extracted archive"
    )
    artifact.add_argument("--root", type=Path, default=Path.cwd())
    artifact.add_argument("--expected-version", required=True)
    artifact.add_argument("--expected-layout", required=True)
    artifact.add_argument("--expected-uncompressed-sha256", required=True)
    artifact.add_argument("--evidence-dir", type=Path, required=True)
    artifact.add_argument("--target-dir", type=Path, required=True)
    artifact.set_defaults(handler=artifact_mode)

    validate = subparsers.add_parser(
        "validate-archive", help="validate an archive against a committed Git tree"
    )
    validate.add_argument("archive", type=Path)
    validate.add_argument("--expected-commit")
    validate.set_defaults(handler=validate_mode)

    security = subparsers.add_parser(
        "security", help="run the RFC 009 R13 dependency-security gate"
    )
    security.add_argument("--output-dir", type=Path, required=True)
    security.set_defaults(handler=security_mode)

    msrv = subparsers.add_parser(
        "msrv", help="run the declared-MSRV matrix under the active toolchain"
    )
    msrv.add_argument("--output-dir", type=Path, required=True)
    msrv.set_defaults(handler=msrv_mode)

    doc_package = subparsers.add_parser(
        "doc-package",
        help="cargo doc, mdbook build, and joint cargo package --workspace verification",
    )
    doc_package.add_argument("--output-dir", type=Path, required=True)
    doc_package.set_defaults(handler=doc_package_mode)

    aggregate = subparsers.add_parser(
        "aggregate-ci",
        help="fail closed unless every required CI job and evidence manifest "
        "is bound to this workflow run and commit (RFC 009 M6c item 3)",
    )
    aggregate.add_argument("--output-dir", type=Path, required=True)
    aggregate.add_argument("--run-id", required=True)
    aggregate.add_argument("--sha", required=True)
    aggregate.add_argument(
        "--require-job",
        action="append",
        default=[],
        metavar="NAME=RESULT",
        help="a needs.<job>.result value that must equal 'success'; repeatable",
    )
    aggregate.add_argument(
        "--evidence-manifest",
        type=Path,
        action="append",
        default=[],
        metavar="PATH",
        help="a downloaded manifest.json that must show status=pass and match "
        "--run-id/--sha; repeatable",
    )
    aggregate.set_defaults(handler=aggregate_ci_mode)

    release = subparsers.add_parser(
        "release",
        help="RFC 009 R12 canonical release entry point: source, msrv, "
        "doc-package, security in order, fail-fast, one consolidated summary",
    )
    release.add_argument("--output-dir", type=Path, required=True)
    release.set_defaults(handler=release_mode)

    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        return args.handler(args)
    except (
        ReleaseError,
        release_archive.ArchiveError,
        source_integrity.IntegrityError,
    ) as error:
        record_failure_summary(args, error)
        print(f"release: FAILED: {error}", file=sys.stderr)
        return 1
    except Exception as error:
        # An expected gate failure always raises one of the three types
        # above. Anything else (OSError, a third-party library's own
        # exception type, ...) is unexpected, but must still finalize
        # `summary.log` rather than leave it reading `status: RUNNING`
        # forever — R14 requires the summary to never disagree with the
        # actual (failed) outcome. `BaseException` subclasses that are not
        # `Exception` (`KeyboardInterrupt`, `SystemExit`) intentionally
        # propagate unfinalized: those are not gate failures.
        record_failure_summary(args, error)
        print(f"release: FAILED (unexpected {type(error).__name__}): {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
