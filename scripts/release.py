#!/usr/bin/env python3
"""Canonical RFC 009 source/archive runner and Git-free artifact verifier."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib
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

    Unlike `verify_tool_manifest`, this does not check the canonical
    producer's pinned host tools, Rust/Cargo versions, or base components —
    those describe the archive-production environment specifically. A gate
    that only needs its own implementation verified (for example `security`,
    which must run on any CI runner, not just the canonical producer) should
    call this instead of the full manifest verification.
    """
    document = load_tool_manifest(root)
    implementations = document.get("implementations")
    if not isinstance(implementations, dict):
        raise ReleaseError("producer-tool manifest is missing table: implementations")
    policy = implementations.get(name)
    if policy is None:
        raise ReleaseError(f"no implementation policy for {name!r}")
    return verify_implementation(root, name, policy)


def current_platform_key() -> str:
    return f"{platform.system().lower()}-{platform.machine().lower()}"


def verify_tool_policy(name: str, policy: object, *, require_hash: bool) -> str:
    """Verify one `command`/`version`(/`sha256`) tool policy entry.

    `require_hash` is `True` only for `[canonical-tools]`, where R16 demands
    exact byte identity with the pinned Docker image's binaries. Non-canonical
    host tools (`[supported-host-tools.<platform>]`) verify command identity
    and exact version only — the underlying bytes of a system package are
    platform- and package-manager-specific and were never meant to be
    portable across hosts.
    """
    if not isinstance(policy, dict):
        raise ReleaseError(f"invalid tool policy: {name}")
    command = policy.get("command")
    expected_version = policy.get("version")
    expected_sha256 = policy.get("sha256")
    if not isinstance(command, str) or not isinstance(expected_version, str):
        raise ReleaseError(f"incomplete tool policy: {name}")
    if require_hash and not isinstance(expected_sha256, str):
        raise ReleaseError(f"incomplete tool policy: {name}")
    if not require_hash and expected_sha256 is not None:
        raise ReleaseError(
            f"non-canonical tool policy must not pin a binary hash: {name}"
        )
    executable = shutil.which(command)
    if executable is None:
        raise ReleaseError(f"required producer tool is unavailable: {command}")
    try:
        completed = subprocess.run(
            [executable, "--version"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise ReleaseError(f"cannot inspect producer tool: {command}") from error
    version = completed.stdout.strip()
    if version != expected_version:
        raise ReleaseError(
            f"producer tool version mismatch for {name}: {version!r}"
        )
    if require_hash:
        digest = sha256_file(Path(executable).resolve())
        if digest != expected_sha256:
            raise ReleaseError(
                f"producer tool hash mismatch for {name}: sha256={digest}"
            )
        return f"{version}; sha256={digest}"
    return version


def verify_tool_manifest(root: Path, *, canonical: bool) -> dict[str, str]:
    document = load_tool_manifest(root)
    producer = document.get("producer")
    implementations = document.get("implementations")
    if not isinstance(producer, dict) or not isinstance(implementations, dict):
        raise ReleaseError("producer-tool manifest is missing a required table")

    observed: dict[str, str] = {}

    if canonical:
        tools = document.get("canonical-tools")
        if not isinstance(tools, dict):
            raise ReleaseError(
                "producer-tool manifest is missing a required table: canonical-tools"
            )
        for name, policy in tools.items():
            observed[name] = verify_tool_policy(name, policy, require_hash=True)
    else:
        claimed = document.get("supported-platforms", {})
        claimed_list = claimed.get("claimed") if isinstance(claimed, dict) else None
        if not isinstance(claimed_list, list) or not claimed_list:
            raise ReleaseError(
                "producer-tool manifest has no non-empty claimed noncanonical "
                "platform list"
            )
        if not all(isinstance(entry, str) for entry in claimed_list):
            raise ReleaseError("claimed noncanonical platform list must be strings")
        platform_key = current_platform_key()
        if platform_key not in claimed_list:
            raise ReleaseError(
                f"this platform ({platform_key!r}) is not in the claimed "
                f"noncanonical platform list: {claimed_list}"
            )
        tools_by_platform = document.get("supported-host-tools")
        if not isinstance(tools_by_platform, dict):
            raise ReleaseError(
                "producer-tool manifest is missing a required table: "
                "supported-host-tools"
            )
        tools = tools_by_platform.get(platform_key)
        if not isinstance(tools, dict):
            raise ReleaseError(
                f"no supported-host-tools policy for claimed platform: {platform_key!r}"
            )
        for name, policy in tools.items():
            observed[name] = verify_tool_policy(name, policy, require_hash=False)

    for name, policy in implementations.items():
        observed[name] = verify_implementation(root, name, policy)

    cargo_version = command_version(["cargo", "--version"])
    rust_version = command_version(["rustc", "--version"])
    if cargo_version != producer.get("cargo") or rust_version != producer.get("rust"):
        raise ReleaseError(
            "Rust/Cargo do not match the pinned producer-tool manifest"
        )
    observed["cargo"] = cargo_version
    observed["rustc"] = rust_version

    if canonical:
        components = document.get("canonical-base-components")
        if not isinstance(components, dict):
            raise ReleaseError(
                "producer-tool manifest is missing canonical base components"
            )
        for name, policy in components.items():
            if not isinstance(policy, dict):
                raise ReleaseError(f"invalid canonical base component: {name}")
            path_value = policy.get("path")
            expected_sha256 = policy.get("sha256")
            if not isinstance(path_value, str) or not isinstance(
                expected_sha256, str
            ):
                raise ReleaseError(
                    f"incomplete canonical base component policy: {name}"
                )
            digest = sha256_file(Path(path_value))
            if digest != expected_sha256:
                raise ReleaseError(
                    f"canonical base component mismatch for {path_value}: {digest}"
                )
            observed[name] = f"{path_value}; sha256={digest}"

        expected_image = producer.get("image")
        if os.environ.get("RFC009_PRODUCER_IMAGE") != expected_image:
            raise ReleaseError(
                "canonical production requires RFC009_PRODUCER_IMAGE bound "
                "to the pinned platform digest"
            )
        if platform.system() != "Linux" or platform.machine() not in {
            "x86_64",
            "amd64",
        }:
            raise ReleaseError("canonical producer platform must be linux/amd64")
        if os.environ.get("LC_ALL") != producer.get("locale"):
            raise ReleaseError("canonical producer requires LC_ALL=C.UTF-8")
        if os.environ.get("TZ") != producer.get("timezone"):
            raise ReleaseError("canonical producer requires TZ=UTC")
    return observed


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
    version = versions["localcache"]
    try:
        cli = next(package for package in packages if package["name"] == "localcache-cli")
        dependency = next(
            dependency
            for dependency in cli["dependencies"]
            if dependency["name"] == "localcache"
        )
        requirement = dependency["req"]
    except (KeyError, StopIteration, TypeError) as error:
        raise ReleaseError("CLI metadata is missing its localcache dependency") from error
    expected_requirement = f"^{version}"
    if requirement != expected_requirement:
        raise ReleaseError(
            "CLI localcache dependency does not match the workspace version: "
            f"expected {expected_requirement!r}, observed {requirement!r}"
        )
    return version


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
    if not re_full_sha256(args.expected_sha256):
        raise ReleaseError("expected archive SHA-256 must be 64 lowercase hex digits")
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
            "archive_sha256": args.expected_sha256,
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


def aggregate_ci_mode(args: argparse.Namespace) -> int:
    """RFC 009 M6c item 3: fail-closed final CI aggregator.

    Fails closed when a required job did not succeed, or when a required
    evidence manifest is missing, unreadable, not a pass, or bound to a
    different workflow run or commit than the one aggregating it. This is
    the only gate that reads `needs.*.result`/`github.run_id`/`github.sha` —
    every other gate is CI-agnostic and runs identically locally.
    """
    root = repository_root()
    output = require_output_boundary(root, args.output_dir)
    summary = output / "summary.log"
    append_summary(summary, "context: aggregate-ci")
    append_summary(summary, f"run-id: {args.run_id}")
    append_summary(summary, f"sha: {args.sha}")
    append_summary(summary, "status: RUNNING")

    failures: list[str] = []

    for requirement in args.require_job:
        name, separator, result = requirement.partition("=")
        if not separator or not name or not result:
            raise ReleaseError(f"malformed --require-job value: {requirement!r}")
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


def rc_eligibility(*, noncanonical: bool) -> bool:
    """Whether this source-context run may become a release candidate.

    Externally bound (RFC 009 M6c item 5), not self-asserted: requires both
    that this run was not invoked with `--noncanonical` *and* that
    `RFC009_RC_ELIGIBLE=1` was set by `scripts/canonical-producer.sh` (the
    wrapper) — the runner never sets this variable about itself. Neither
    signal alone is sufficient. Reaching this check with `noncanonical=False`
    already means `verify_tool_manifest`'s canonical branch independently
    confirmed the producer image, platform, locale, and base-component
    identity; this adds a signal the runner's own arguments cannot supply.
    """
    return not noncanonical and os.environ.get("RFC009_RC_ELIGIBLE") == "1"


def source_mode(args: argparse.Namespace) -> int:
    root = repository_root()
    commit = require_clean_commit(root)
    tool_versions = verify_tool_manifest(root, canonical=not args.noncanonical)
    output = require_output_boundary(root, Path(args.output_dir))
    if output.exists() and any(output.iterdir()):
        raise ReleaseError(f"output directory is not empty: {output}")
    output.mkdir(parents=True, exist_ok=True)

    evidence = output / "evidence"
    summary = evidence / "summary.log"
    append_summary(summary, "context: source")
    append_summary(summary, f"commit: {commit}")
    append_summary(
        summary,
        f"producer-class: {'supported-noncanonical' if args.noncanonical else 'canonical'}",
    )
    append_summary(summary, "status: RUNNING")
    logger = GateLog(evidence / "checkout-smoke.log")
    run_source_integrity(root, logger, require_tracked=True)
    append_summary(summary, "tracked-source-integrity: PASS")
    version, _metadata = cargo_metadata(root, logger, gate_name="version-metadata")
    append_summary(summary, f"version-contract: PASS ({version})")
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
    first_gzip = release_archive.compress_tar(raw_first)
    second_gzip = release_archive.compress_tar(raw_second)
    if raw_first != raw_second or first_gzip != second_gzip:
        raise ReleaseError("two archive constructions from one commit differ")
    append_summary(summary, "same-commit-determinism: PASS")

    archive_name = f"localcache-v{version}.tar.gz"
    archive_path = output / archive_name
    with archive_path.open("xb") as file:
        file.write(first_gzip)
    archive_size = len(first_gzip)
    archive_sha256 = release_archive.sha256_bytes(first_gzip)
    release_archive.validate_archive_file(archive_path, expected, commit)

    archive_evidence = evidence / "archive"
    archive_evidence.mkdir(parents=True)
    with (archive_evidence / "members.txt").open("x", encoding="utf-8") as file:
        for member in members:
            mode = "executable" if member.executable else "non-executable"
            file.write(f"{member.kind}\t{mode}\t{member.path}\n")
    (archive_evidence / "sha256.txt").write_text(
        f"{archive_sha256}  {archive_name}\n", encoding="utf-8"
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
            "--expected-sha256",
            archive_sha256,
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
            "archive_sha256": archive_sha256,
            "archive_size": archive_size,
            "commit": commit,
            "context": "source",
            "layout": LAYOUT,
            "member_count": len(members),
            "status": "pass",
            "producer_class": (
                "supported-noncanonical" if args.noncanonical else "canonical"
            ),
            "producer_image": (
                None if args.noncanonical else os.environ["RFC009_PRODUCER_IMAGE"]
            ),
            "rc_eligible": rc_eligibility(noncanonical=args.noncanonical),
            "release_tool_manifest_sha256": sha256_file(
                root / "scripts/release-tools.toml"
            ),
            "tool_versions": tool_versions,
            "version": version,
            **ci_identity(),
        },
    )
    append_summary(summary, f"archive-sha256: {archive_sha256}")
    append_summary(summary, "status: PASS")
    print(f"verified archive: {archive_path}")
    print(f"sha256: {archive_sha256}")
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
    source.add_argument(
        "--noncanonical",
        action="store_true",
        help=(
            "run behavioral/content-equivalence evidence only; output is not "
            "eligible to become a release candidate"
        ),
    )
    source.set_defaults(handler=source_mode)

    artifact = subparsers.add_parser(
        "artifact", help="run Git-free M1 gates inside an extracted archive"
    )
    artifact.add_argument("--root", type=Path, default=Path.cwd())
    artifact.add_argument("--expected-version", required=True)
    artifact.add_argument("--expected-layout", required=True)
    artifact.add_argument("--expected-sha256", required=True)
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
