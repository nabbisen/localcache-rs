#!/usr/bin/env python3
"""Fail-closed RFC 014 RustSec and crates.io yanked-package gate."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import time
import tomllib
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import date, datetime, timezone
from pathlib import Path
from typing import Callable, Iterable, Mapping, Sequence


AUDIT_VERSION = "cargo-audit-audit 0.22.2"
CRATES_IO_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
SPARSE_BASE = "https://index.crates.io/"
PACKAGE_RE = re.compile(r"[A-Za-z0-9_-]{1,64}\Z")
ADVISORY_RE = re.compile(r"RUSTSEC-[0-9]{4}-[0-9]{4}\Z")
VERSION_RE = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?\Z")
SHA256_RE = re.compile(r"[0-9a-f]{64}\Z")
MAX_POLICY_BYTES = 1024 * 1024
MAX_LOCK_BYTES = 16 * 1024 * 1024
MAX_AUDIT_BYTES = 64 * 1024 * 1024
MAX_RESPONSE_BYTES = 16 * 1024 * 1024
MAX_SNAPSHOT_BYTES = 256 * 1024 * 1024
FETCH_TIMEOUT_SECONDS = 30
FETCH_DEADLINE_SECONDS = 15 * 60
# N3 Part E (RFC 014 M4 review H2): bounded retry for transient sparse-index
# failures only -- a flaky security gate gets disabled, and a disabled gate
# is the real risk.
MAX_FETCH_ATTEMPTS = 3
RETRY_BACKOFF_BASE_SECONDS = 1.0


class AdvisoryGateError(Exception):
    """An input, tool, provenance, or policy invariant failed closed."""


class TransientFetchError(AdvisoryGateError):
    """A sparse-index fetch failure believed to be transient -- a network
    condition (connection refused, timeout, DNS failure, ...), raised only
    by `live_fetch`'s own network-error handler.

    Phase 23 P0 Part C1 (N3 review §4.1 deferred register): `fetch_with_retry`
    used to catch the broad `AdvisoryGateError`, correct only because
    `live_fetch` happened to have no other failure mode. A substitute `Fetch`
    raising `AdvisoryGateError` for a non-transient reason would have been
    retried three times regardless. The layer that knows whether a failure
    is transient -- the fetcher -- now says so through the exception type,
    and the retry wrapper catches only this type instead of inferring
    transience from a broad exception class.
    """


@dataclass(frozen=True, order=True)
class Finding:
    advisory_id: str
    package: str
    version: str
    kind: str

    @property
    def key(self) -> tuple[str, str, str, str]:
        return (self.advisory_id, self.package, self.version, self.kind)


@dataclass(frozen=True)
class PolicyEntry:
    finding: Finding
    action: str
    owner: str
    approved: date
    # RFC 019: `None` is a *standing disposition* — valid until the version,
    # kind, or dependency graph changes, with no expiry date. Only permitted
    # for `unmaintained`/`notice`; `vulnerability`/`unsound` still require a
    # real date, enforced in `load_policy`.
    expires: date | None
    reason: str
    follow_up: str


@dataclass(frozen=True, order=True)
class RegistryPackage:
    source: str
    name: str
    version: str
    checksum: str


@dataclass(frozen=True, order=True)
class ExcludedPackage:
    """N3 Part F (RFC 014 M4 review H3): a Cargo.lock package with no
    crates.io registry source -- a path or git dependency -- excluded from
    advisory-gate coverage. Recorded in evidence so completeness is
    self-evidencing rather than requiring a manual Cargo.lock cross-check.
    """

    name: str
    version: str
    reason: str  # "path" or "git"


def repository_root() -> Path:
    return Path(__file__).resolve().parents[1]


def read_bounded(path: Path, maximum: int, description: str) -> bytes:
    try:
        size = path.stat().st_size
    except OSError as error:
        raise AdvisoryGateError(f"cannot inspect {description}: {error}") from error
    if size > maximum:
        raise AdvisoryGateError(f"{description} exceeds {maximum} bytes")
    try:
        return path.read_bytes()
    except OSError as error:
        raise AdvisoryGateError(f"cannot read {description}: {error}") from error


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(read_bounded(path, MAX_SNAPSHOT_BYTES, str(path)))


def require_digest(path: Path, expected: str, description: str) -> None:
    if sha256_file(path) != expected:
        raise AdvisoryGateError(f"{description} changed during the gate")


def parse_json_bytes(value: bytes, description: str) -> object:
    try:
        return json.loads(value.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AdvisoryGateError(f"invalid {description} JSON: {error}") from error


def require_object(value: object, description: str) -> dict[str, object]:
    if not isinstance(value, dict) or not all(
        isinstance(key, str) for key in value
    ):
        raise AdvisoryGateError(f"{description} must be a JSON object")
    return value


def require_exact_keys(
    value: Mapping[str, object], expected: set[str], description: str
) -> None:
    actual = set(value)
    if actual != expected:
        missing = sorted(expected - actual)
        unknown = sorted(actual - expected)
        raise AdvisoryGateError(
            f"{description} keys differ: missing={missing} unknown={unknown}"
        )


POLICY_FINDING_REQUIRED_FIELDS = {
    "id",
    "package",
    "version",
    "kind",
    "action",
    "owner",
    "approved",
    "reason",
    "follow-up",
}
POLICY_FINDING_OPTIONAL_FIELDS = {"expires"}
STANDING_DISPOSITION_KINDS = {"unmaintained", "notice"}


def require_policy_finding_keys(value: Mapping[str, object], description: str) -> None:
    """RFC 019: `expires` is the one optional key on a policy finding --
    every other field is still required, and no entry may carry an unknown
    key. Deliberately its own function rather than a parameter threaded
    through `require_exact_keys`, so the "no unknown keys" guarantee cannot
    be accidentally widened by a future caller reusing the wrong set.
    """
    actual = set(value)
    allowed = POLICY_FINDING_REQUIRED_FIELDS | POLICY_FINDING_OPTIONAL_FIELDS
    missing = sorted(POLICY_FINDING_REQUIRED_FIELDS - actual)
    unknown = sorted(actual - allowed)
    if missing or unknown:
        raise AdvisoryGateError(
            f"{description} keys differ: missing={missing} unknown={unknown}"
        )


def require_string(value: object, description: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise AdvisoryGateError(f"{description} must be a non-empty string")
    return value


def parse_iso_date(value: object, description: str) -> date:
    text = require_string(value, description)
    try:
        parsed = date.fromisoformat(text)
    except ValueError as error:
        raise AdvisoryGateError(f"{description} is not an ISO date: {text!r}") from error
    if parsed.isoformat() != text:
        raise AdvisoryGateError(f"{description} is not a canonical ISO date: {text!r}")
    return parsed


def load_policy(path: Path) -> tuple[dict[str, str], list[PolicyEntry]]:
    document = require_object(
        parse_json_bytes(read_bounded(path, MAX_POLICY_BYTES, "advisory policy"), "policy"),
        "policy",
    )
    require_exact_keys(document, {"schema", "defaults", "findings"}, "policy")
    if document["schema"] != 1:
        raise AdvisoryGateError("unsupported advisory policy schema")

    defaults_value = require_object(document["defaults"], "policy defaults")
    expected_defaults = {
        "vulnerability",
        "unsound",
        "yanked",
        "unmaintained",
        "notice",
    }
    require_exact_keys(defaults_value, expected_defaults, "policy defaults")
    defaults: dict[str, str] = {}
    for kind, action in defaults_value.items():
        if action != "deny":
            raise AdvisoryGateError(f"default action for {kind} must be deny")
        defaults[kind] = action

    findings_value = document["findings"]
    if not isinstance(findings_value, list):
        raise AdvisoryGateError("policy findings must be an array")
    entries: list[PolicyEntry] = []
    seen: set[tuple[str, str, str, str]] = set()
    for index, raw_entry in enumerate(findings_value):
        entry = require_object(raw_entry, f"policy finding {index}")
        require_policy_finding_keys(entry, f"policy finding {index}")
        advisory_id = require_string(entry["id"], f"policy finding {index} id")
        package = require_string(entry["package"], f"policy finding {index} package")
        version = require_string(entry["version"], f"policy finding {index} version")
        kind = require_string(entry["kind"], f"policy finding {index} kind")
        action = require_string(entry["action"], f"policy finding {index} action")
        if not ADVISORY_RE.fullmatch(advisory_id):
            raise AdvisoryGateError(f"invalid advisory ID in policy: {advisory_id!r}")
        if not PACKAGE_RE.fullmatch(package):
            raise AdvisoryGateError(f"invalid package name in policy: {package!r}")
        if not VERSION_RE.fullmatch(version):
            raise AdvisoryGateError(f"invalid exact version in policy: {version!r}")
        if kind not in {"vulnerability", "unsound", "unmaintained", "notice"}:
            raise AdvisoryGateError(f"unsupported policy finding kind: {kind!r}")
        if kind in {"unmaintained", "notice"} and action != "warn":
            raise AdvisoryGateError(f"{kind} policy action must be warn")
        if kind in {"vulnerability", "unsound"} and action != "exception":
            raise AdvisoryGateError(f"{kind} policy action must be exception")
        approved = parse_iso_date(entry["approved"], f"policy finding {index} approved")
        raw_expires = entry.get("expires")
        if kind in STANDING_DISPOSITION_KINDS:
            # RFC 019 R1: omitted or explicit `null` means a standing
            # disposition -- no expiry date.
            expires = (
                None
                if raw_expires is None
                else parse_iso_date(raw_expires, f"policy finding {index} expires")
            )
        else:
            # RFC 019 R2: vulnerability/unsound keep mandatory expiry.
            if raw_expires is None:
                raise AdvisoryGateError(
                    f"policy finding {index} expires is required for kind {kind!r}"
                )
            expires = parse_iso_date(raw_expires, f"policy finding {index} expires")
        if expires is not None and expires <= approved:
            raise AdvisoryGateError(
                f"policy finding {index} expiry must be after approval"
            )
        finding = Finding(advisory_id, package, version, kind)
        if finding.key in seen:
            raise AdvisoryGateError(f"duplicate policy tuple: {finding.key}")
        seen.add(finding.key)
        entries.append(
            PolicyEntry(
                finding=finding,
                action=action,
                owner=require_string(entry["owner"], f"policy finding {index} owner"),
                approved=approved,
                expires=expires,
                reason=require_string(entry["reason"], f"policy finding {index} reason"),
                follow_up=require_string(
                    entry["follow-up"], f"policy finding {index} follow-up"
                ),
            )
        )
    return defaults, entries


def _report_finding(raw: object, kind: str, description: str) -> Finding:
    item = require_object(raw, description)
    advisory = require_object(item.get("advisory"), f"{description} advisory")
    package = require_object(item.get("package"), f"{description} package")
    advisory_id = require_string(advisory.get("id"), f"{description} advisory id")
    package_name = require_string(package.get("name"), f"{description} package name")
    package_version = require_string(
        package.get("version"), f"{description} package version"
    )
    advisory_package = require_string(
        advisory.get("package"), f"{description} advisory package"
    )
    if advisory_package != package_name:
        raise AdvisoryGateError(f"{description} advisory/package mismatch")
    if not ADVISORY_RE.fullmatch(advisory_id):
        raise AdvisoryGateError(f"{description} has invalid advisory ID")
    if not PACKAGE_RE.fullmatch(package_name) or not VERSION_RE.fullmatch(
        package_version
    ):
        raise AdvisoryGateError(f"{description} has invalid package identity")
    item_kind = item.get("kind")
    if item_kind is not None and item_kind != kind:
        raise AdvisoryGateError(f"{description} warning kind mismatch")
    return Finding(advisory_id, package_name, package_version, kind)


def parse_audit_report(value: bytes) -> list[Finding]:
    report = require_object(parse_json_bytes(value, "cargo-audit report"), "audit report")
    require_exact_keys(
        report,
        {"database", "lockfile", "settings", "vulnerabilities", "warnings"},
        "audit report",
    )
    require_object(report["database"], "audit database")
    require_object(report["lockfile"], "audit lockfile")
    require_object(report["settings"], "audit settings")
    vulnerabilities = require_object(report["vulnerabilities"], "vulnerabilities")
    require_exact_keys(vulnerabilities, {"found", "count", "list"}, "vulnerabilities")
    raw_list = vulnerabilities["list"]
    if not isinstance(raw_list, list):
        raise AdvisoryGateError("vulnerabilities list must be an array")
    if not isinstance(vulnerabilities["found"], bool):
        raise AdvisoryGateError("vulnerabilities found must be Boolean")
    if not isinstance(vulnerabilities["count"], int) or isinstance(
        vulnerabilities["count"], bool
    ):
        raise AdvisoryGateError("vulnerabilities count must be an integer")
    if vulnerabilities["count"] != len(raw_list):
        raise AdvisoryGateError("vulnerability count does not match list")
    if vulnerabilities["found"] != bool(raw_list):
        raise AdvisoryGateError("vulnerability found flag does not match list")

    findings = [
        _report_finding(item, "vulnerability", f"vulnerability {index}")
        for index, item in enumerate(raw_list)
    ]
    warnings = require_object(report["warnings"], "warnings")
    for kind, raw_warnings in warnings.items():
        if not isinstance(raw_warnings, list):
            raise AdvisoryGateError(f"warning category {kind!r} must be an array")
        for index, item in enumerate(raw_warnings):
            findings.append(_report_finding(item, kind, f"{kind} warning {index}"))
    if len({finding.key for finding in findings}) != len(findings):
        raise AdvisoryGateError("audit report contains duplicate finding tuples")
    return sorted(findings)


def classify_findings(
    findings: Sequence[Finding], entries: Sequence[PolicyEntry], today: date
) -> tuple[list[str], bool]:
    policy = {entry.finding.key: entry for entry in entries}
    observed = {finding.key for finding in findings}
    lines: list[str] = []
    denied = False
    for finding in sorted(findings):
        entry = policy.get(finding.key)
        identity = "/".join(finding.key)
        if entry is None:
            lines.append(f"DENY {identity}: no exact policy disposition")
            denied = True
            continue
        if entry.expires is not None and today >= entry.expires:
            lines.append(
                f"DENY {identity}: policy expired on {entry.expires.isoformat()}"
            )
            denied = True
            continue
        # N3 Part D (RFC 014 M4 review H1): a knowingly accepted
        # vulnerability/unsound finding must never render as a bare PASS --
        # `action` is validated elsewhere to be exactly "warn" or
        # "exception", so this lookup cannot land on a third case.
        classification = {"warn": "WARN", "exception": "EXCEPTION"}[entry.action]
        if entry.expires is not None:
            lines.append(
                f"{classification} {identity}: {entry.action} until "
                f"{entry.expires.isoformat()} ({entry.owner})"
            )
        else:
            # RFC 019 R4: no expiry to report; name the re-raise condition
            # instead, so a reader can tell the two forms apart without
            # opening the policy file. Phase 23 P0 Part C2: `follow_up` is
            # emitted verbatim -- it is a self-describing sentence in its
            # own right, not a fragment this format string completes. A
            # consumer other than this report format (an evidence viewer,
            # a different reporter) must be able to read it standalone.
            lines.append(
                f"{classification} {identity}: {entry.action}, standing "
                f"disposition ({entry.owner}) — {entry.follow_up}"
            )
    for entry in sorted(entries, key=lambda item: item.finding.key):
        if entry.finding.key not in observed:
            lines.append(
                "DENY " + "/".join(entry.finding.key) + ": stale policy entry"
            )
            denied = True
    return lines, denied


def load_registry_packages(
    path: Path,
) -> tuple[list[RegistryPackage], list[ExcludedPackage]]:
    raw = read_bounded(path, MAX_LOCK_BYTES, "Cargo.lock")
    try:
        document = tomllib.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise AdvisoryGateError(f"invalid Cargo.lock: {error}") from error
    packages = document.get("package")
    if not isinstance(packages, list):
        raise AdvisoryGateError("Cargo.lock package array is missing")
    eligible: list[RegistryPackage] = []
    excluded: list[ExcludedPackage] = []
    for index, raw_package in enumerate(packages):
        if not isinstance(raw_package, dict):
            raise AdvisoryGateError(f"Cargo.lock package {index} is malformed")
        source = raw_package.get("source")
        if source is None or (isinstance(source, str) and source.startswith("git+")):
            name = raw_package.get("name")
            version = raw_package.get("version")
            if not isinstance(name, str) or not PACKAGE_RE.fullmatch(name):
                raise AdvisoryGateError(f"invalid excluded package name: {name!r}")
            if not isinstance(version, str) or not VERSION_RE.fullmatch(version):
                raise AdvisoryGateError(f"invalid excluded package version: {version!r}")
            reason = "git" if isinstance(source, str) else "path"
            excluded.append(ExcludedPackage(name, version, reason))
            continue
        if source != CRATES_IO_SOURCE:
            raise AdvisoryGateError(
                f"unsupported registry source in Cargo.lock package {index}: {source!r}"
            )
        name = raw_package.get("name")
        version = raw_package.get("version")
        checksum = raw_package.get("checksum")
        if not isinstance(name, str) or not PACKAGE_RE.fullmatch(name):
            raise AdvisoryGateError(f"invalid crates.io package name: {name!r}")
        if not isinstance(version, str) or not VERSION_RE.fullmatch(version):
            raise AdvisoryGateError(f"invalid crates.io package version: {version!r}")
        if not isinstance(checksum, str) or not SHA256_RE.fullmatch(checksum):
            raise AdvisoryGateError(f"invalid crates.io checksum for {name} {version}")
        eligible.append(RegistryPackage(source, name, version, checksum))
    if len(set(eligible)) != len(eligible):
        raise AdvisoryGateError("Cargo.lock contains duplicate registry package identities")
    if len(set(excluded)) != len(excluded):
        raise AdvisoryGateError("Cargo.lock contains duplicate excluded package identities")
    return sorted(eligible), sorted(excluded)


def sparse_path(name: str) -> str:
    if not PACKAGE_RE.fullmatch(name):
        raise AdvisoryGateError(f"invalid package name for sparse URL: {name!r}")
    lowered = name.lower()
    if len(lowered) == 1:
        return f"1/{lowered}"
    if len(lowered) == 2:
        return f"2/{lowered}"
    if len(lowered) == 3:
        return f"3/{lowered[0]}/{lowered}"
    return f"{lowered[:2]}/{lowered[2:4]}/{lowered}"


def validate_sparse_response(
    body: bytes, packages: Sequence[RegistryPackage], description: str
) -> list[dict[str, object]]:
    records: list[dict[str, object]] = []
    for line_number, raw_line in enumerate(body.splitlines(), start=1):
        if not raw_line:
            raise AdvisoryGateError(f"{description} has an empty line")
        record = require_object(
            parse_json_bytes(raw_line, f"{description} line {line_number}"),
            f"{description} line {line_number}",
        )
        name = record.get("name")
        version = record.get("vers")
        checksum = record.get("cksum")
        yanked = record.get("yanked")
        if not isinstance(name, str) or not PACKAGE_RE.fullmatch(name):
            raise AdvisoryGateError(f"{description} line {line_number} has invalid name")
        if not isinstance(version, str) or not VERSION_RE.fullmatch(version):
            raise AdvisoryGateError(
                f"{description} line {line_number} has invalid version"
            )
        if not isinstance(checksum, str) or not SHA256_RE.fullmatch(checksum):
            raise AdvisoryGateError(
                f"{description} line {line_number} has invalid checksum"
            )
        if not isinstance(yanked, bool):
            raise AdvisoryGateError(
                f"{description} line {line_number} has non-Boolean yanked state"
            )
        records.append(record)

    selected: list[dict[str, object]] = []
    for package in packages:
        matches = [
            record
            for record in records
            if record.get("name") == package.name
            and record.get("vers") == package.version
        ]
        if len(matches) != 1:
            raise AdvisoryGateError(
                f"{description} has {len(matches)} records for "
                f"{package.name} {package.version}"
            )
        record = matches[0]
        if record["cksum"] != package.checksum:
            raise AdvisoryGateError(
                f"{description} checksum mismatch for {package.name} {package.version}"
            )
        selected.append(
            {
                "source": package.source,
                "name": package.name,
                "version": package.version,
                "checksum": package.checksum,
                "yanked": record["yanked"],
            }
        )
    return selected


Fetch = Callable[[str, int], tuple[int, Mapping[str, str], bytes]]


def live_fetch(url: str, timeout: int) -> tuple[int, Mapping[str, str], bytes]:
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": "localcache-rfc014-security-gate/0.21.2",
            "Cache-Control": "no-cache",
            "Pragma": "no-cache",
            "Accept": "application/octet-stream",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            status = response.status
            headers = {key.lower(): value for key, value in response.headers.items()}
            body = response.read(MAX_RESPONSE_BYTES + 1)
    except (OSError, urllib.error.URLError) as error:
        raise TransientFetchError(f"sparse-index request failed for {url}: {error}") from error
    return status, headers, body


Sleep = Callable[[float], None]


def fetch_with_retry(
    fetch: Fetch,
    url: str,
    timeout: int,
    deadline: float,
    sleep: Sleep = time.sleep,
) -> tuple[int, Mapping[str, str], bytes, int]:
    """N3 Part E (RFC 014 M4 review H2); narrowed in Phase 23 P0 Part C1.
    Retry a transient sparse-index failure -- a network error, raised by
    `live_fetch` as `TransientFetchError` -- or an HTTP 5xx, up to
    `MAX_FETCH_ATTEMPTS` times with exponential backoff.

    Catches only `TransientFetchError`, not the broad `AdvisoryGateError`:
    the fetcher declares transience through the exception type instead of
    the wrapper inferring it, so a substitute `Fetch` raising a plain
    `AdvisoryGateError` for a non-transient reason propagates immediately
    rather than being retried.

    Never retries a non-200/non-5xx status, a validation failure, or a
    size-limit breach -- those are genuine gate failures, not flakiness, and
    retrying them would mask a real problem behind a slow failure instead of
    a fast one. Returns the attempt count alongside the result so it can be
    recorded in evidence.
    """
    last_error: AdvisoryGateError | None = None
    for attempt in range(1, MAX_FETCH_ATTEMPTS + 1):
        if time.monotonic() >= deadline:
            raise AdvisoryGateError("registry snapshot exceeded overall deadline")
        try:
            status, headers, body = fetch(url, timeout)
        except TransientFetchError as error:
            last_error = error
        else:
            if status == 200:
                return status, headers, body, attempt
            if not 500 <= status < 600:
                raise AdvisoryGateError(
                    f"sparse-index request returned HTTP {status}: {url}"
                )
            last_error = AdvisoryGateError(
                f"sparse-index request returned HTTP {status}: {url}"
            )
        if attempt < MAX_FETCH_ATTEMPTS:
            sleep(RETRY_BACKOFF_BASE_SECONDS * (2 ** (attempt - 1)))
    assert last_error is not None
    raise last_error


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        + "\n"
    ).encode("ascii")


def build_registry_snapshot(
    packages: Sequence[RegistryPackage],
    excluded: Sequence[ExcludedPackage],
    output: Path,
    fetched_at: str,
    fetch: Fetch = live_fetch,
    sleep: Sleep = time.sleep,
) -> tuple[bytes, list[dict[str, object]]]:
    groups: dict[str, list[RegistryPackage]] = {}
    for package in packages:
        groups.setdefault(package.name.lower(), []).append(package)
    responses_dir = output / "registry-responses"
    responses_dir.mkdir()
    started = time.monotonic()
    deadline = started + FETCH_DEADLINE_SECONDS
    total = 0
    response_manifest: list[dict[str, object]] = []
    selected: list[dict[str, object]] = []
    for lowered in sorted(groups):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise AdvisoryGateError("registry snapshot exceeded overall deadline")
        timeout = max(1, min(FETCH_TIMEOUT_SECONDS, int(remaining)))
        url = SPARSE_BASE + sparse_path(lowered)
        status, headers, body, attempts = fetch_with_retry(
            fetch, url, timeout, deadline, sleep
        )
        if len(body) > MAX_RESPONSE_BYTES:
            raise AdvisoryGateError(f"sparse-index response exceeds limit: {url}")
        total += len(body)
        if total > MAX_SNAPSHOT_BYTES:
            raise AdvisoryGateError("registry snapshot exceeds aggregate limit")
        response_name = sha256_bytes(lowered.encode("ascii")) + ".json"
        response_path = responses_dir / response_name
        response_path.write_bytes(body)
        records = validate_sparse_response(body, groups[lowered], url)
        selected.extend(records)
        response_manifest.append(
            {
                "package-key": lowered,
                "url": url,
                "response-file": f"registry-responses/{response_name}",
                "response-sha256": sha256_bytes(body),
                "etag": headers.get("etag"),
                "last-modified": headers.get("last-modified"),
                "date": headers.get("date"),
                "attempts": attempts,
            }
        )
    manifest = {
        "schema": 1,
        "fetched-at": fetched_at,
        "responses": response_manifest,
        "excluded": [
            {"name": item.name, "version": item.version, "reason": item.reason}
            for item in sorted(excluded)
        ],
        "selected": sorted(
            selected,
            key=lambda item: (
                str(item["source"]),
                str(item["name"]),
                str(item["version"]),
            ),
        ),
    }
    return canonical_json(manifest), selected


def load_registry_snapshot(
    manifest_path: Path,
    expected_digest: str,
    packages: Sequence[RegistryPackage],
    excluded: Sequence[ExcludedPackage],
    output: Path,
) -> list[dict[str, object]]:
    require_digest(manifest_path, expected_digest, "registry manifest")
    manifest = require_object(
        parse_json_bytes(
            read_bounded(manifest_path, MAX_SNAPSHOT_BYTES, "registry manifest"),
            "registry manifest",
        ),
        "registry manifest",
    )
    require_exact_keys(
        manifest,
        {"schema", "fetched-at", "responses", "selected", "excluded"},
        "registry manifest",
    )
    if manifest["schema"] != 1:
        raise AdvisoryGateError("unsupported registry manifest schema")
    require_string(manifest["fetched-at"], "registry manifest fetched-at")
    raw_responses = manifest["responses"]
    raw_selected = manifest["selected"]
    raw_excluded = manifest["excluded"]
    if (
        not isinstance(raw_responses, list)
        or not isinstance(raw_selected, list)
        or not isinstance(raw_excluded, list)
    ):
        raise AdvisoryGateError("registry manifest arrays are malformed")

    excluded_fields = {"name", "version", "reason"}
    parsed_excluded: list[tuple[str, str, str]] = []
    for index, raw_item in enumerate(raw_excluded):
        item = require_object(raw_item, f"excluded registry record {index}")
        require_exact_keys(item, excluded_fields, f"excluded registry record {index}")
        excluded_name = require_string(
            item["name"], f"excluded registry record {index} name"
        )
        excluded_version = require_string(
            item["version"], f"excluded registry record {index} version"
        )
        reason = item["reason"]
        if reason not in {"path", "git"}:
            raise AdvisoryGateError(
                f"excluded registry record {index} has invalid reason: {reason!r}"
            )
        if not PACKAGE_RE.fullmatch(excluded_name):
            raise AdvisoryGateError(f"excluded registry record {index} has invalid name")
        if not VERSION_RE.fullmatch(excluded_version):
            raise AdvisoryGateError(f"excluded registry record {index} has invalid version")
        parsed_excluded.append((excluded_name, excluded_version, reason))
    expected_excluded = [(item.name, item.version, item.reason) for item in excluded]
    if sorted(parsed_excluded) != sorted(expected_excluded):
        raise AdvisoryGateError("excluded registry records do not match Cargo.lock exactly")

    groups: dict[str, list[RegistryPackage]] = {}
    for package in packages:
        groups.setdefault(package.name.lower(), []).append(package)
    responses: dict[str, dict[str, object]] = {}
    response_fields = {
        "package-key",
        "url",
        "response-file",
        "response-sha256",
        "etag",
        "last-modified",
        "date",
        "attempts",
    }
    for index, raw_response in enumerate(raw_responses):
        response = require_object(raw_response, f"registry response {index}")
        require_exact_keys(response, response_fields, f"registry response {index}")
        key = require_string(response["package-key"], f"registry response {index} key")
        if key not in groups or key in responses or key != key.lower():
            raise AdvisoryGateError(f"unexpected or duplicate registry response key: {key}")
        expected_name = sha256_bytes(key.encode("ascii")) + ".json"
        expected_relative = f"registry-responses/{expected_name}"
        if response["response-file"] != expected_relative:
            raise AdvisoryGateError(f"invalid registry response path for {key}")
        if response["url"] != SPARSE_BASE + sparse_path(key):
            raise AdvisoryGateError(f"invalid registry response URL for {key}")
        digest = response["response-sha256"]
        if not isinstance(digest, str) or not SHA256_RE.fullmatch(digest):
            raise AdvisoryGateError(f"invalid registry response digest for {key}")
        for header in ("etag", "last-modified", "date"):
            if response[header] is not None and not isinstance(response[header], str):
                raise AdvisoryGateError(f"invalid registry response header for {key}")
        attempts = response["attempts"]
        if (
            not isinstance(attempts, int)
            or isinstance(attempts, bool)
            or not 1 <= attempts <= MAX_FETCH_ATTEMPTS
        ):
            raise AdvisoryGateError(f"invalid registry response attempt count for {key}")
        response_path = output / expected_relative
        require_digest(response_path, digest, f"registry response {key}")
        responses[key] = response
    if set(responses) != set(groups):
        raise AdvisoryGateError("registry response manifest does not cover every package")

    selected: list[dict[str, object]] = []
    selected_fields = {"source", "name", "version", "checksum", "yanked"}
    for index, raw_record in enumerate(raw_selected):
        record = require_object(raw_record, f"selected registry record {index}")
        require_exact_keys(record, selected_fields, f"selected registry record {index}")
        if record["source"] != CRATES_IO_SOURCE:
            raise AdvisoryGateError("selected registry record has unsupported source")
        name = record["name"]
        version = record["version"]
        checksum = record["checksum"]
        if not isinstance(name, str) or not PACKAGE_RE.fullmatch(name):
            raise AdvisoryGateError("selected registry record has invalid name")
        if not isinstance(version, str) or not VERSION_RE.fullmatch(version):
            raise AdvisoryGateError("selected registry record has invalid version")
        if not isinstance(checksum, str) or not SHA256_RE.fullmatch(checksum):
            raise AdvisoryGateError("selected registry record has invalid checksum")
        if not isinstance(record["yanked"], bool):
            raise AdvisoryGateError("selected registry record has invalid yanked state")
        selected.append(dict(record))

    expected_identities = [
        (package.source, package.name, package.version, package.checksum)
        for package in packages
    ]
    actual_identities = [
        (
            str(record["source"]),
            str(record["name"]),
            str(record["version"]),
            str(record["checksum"]),
        )
        for record in selected
    ]
    if sorted(actual_identities) != sorted(expected_identities):
        raise AdvisoryGateError("selected registry records do not cover Cargo.lock exactly")

    reparsed: list[dict[str, object]] = []
    for key in sorted(groups):
        relative = str(responses[key]["response-file"])
        body = read_bounded(output / relative, MAX_RESPONSE_BYTES, f"registry response {key}")
        reparsed.extend(validate_sparse_response(body, groups[key], f"frozen response {key}"))
    if canonical_json(sorted(selected, key=lambda item: (str(item["name"]), str(item["version"])))) != canonical_json(
        sorted(reparsed, key=lambda item: (str(item["name"]), str(item["version"])))
    ):
        raise AdvisoryGateError("selected registry records differ from frozen responses")
    return selected


def run_capture(
    command: Sequence[str], cwd: Path, stdout_path: Path, stderr_path: Path
) -> int:
    try:
        with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
            completed = subprocess.run(
                list(command),
                cwd=cwd,
                stdout=stdout,
                stderr=stderr,
                timeout=FETCH_DEADLINE_SECONDS,
            )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise AdvisoryGateError(f"cannot execute {' '.join(command)}: {error}") from error
    if stdout_path.stat().st_size > MAX_AUDIT_BYTES:
        raise AdvisoryGateError(f"command output exceeds limit: {' '.join(command)}")
    return completed.returncode


def run_text(command: Sequence[str], cwd: Path) -> str:
    try:
        completed = subprocess.run(
            list(command),
            cwd=cwd,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=60,
        )
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        raise AdvisoryGateError(f"cannot execute {' '.join(command)}: {error}") from error
    return completed.stdout.strip()


def require_audit_result(status: int, path: Path, description: str) -> bytes:
    if status not in {0, 1}:
        raise AdvisoryGateError(f"{description} returned operational status {status}")
    raw = read_bounded(path, MAX_AUDIT_BYTES, description)
    parse_audit_report(raw)
    return raw


def advisory_database() -> Path:
    # N3 Part G (RFC 014 M4 review H4): this deliberately mirrors
    # cargo-audit's own `CARGO_HOME` resolution (default `~/.cargo`, or
    # `$CARGO_HOME` if set) rather than hard-coding a path, so the identity
    # check below actually describes the same advisory database `cargo
    # audit` used for the scan, not a different one that happens to exist.
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    return cargo_home / "advisory-db"


def database_identity(path: Path, root: Path) -> dict[str, str]:
    return {
        "path": str(path.resolve()),
        "remote": run_text(["git", "-C", str(path), "remote", "get-url", "origin"], root),
        "commit": run_text(["git", "-C", str(path), "rev-parse", "HEAD"], root),
    }


def prepare_output(path: Path, root: Path) -> Path:
    output = path.expanduser().resolve()
    if output == root.resolve():
        raise AdvisoryGateError("evidence output cannot be the repository root")
    if output.exists():
        if not output.is_dir():
            raise AdvisoryGateError("evidence output exists and is not a directory")
        if any(output.iterdir()):
            raise AdvisoryGateError("evidence output directory must be empty")
    else:
        output.mkdir(parents=True)
    return output


def execute(output_arg: Path) -> int:
    root = repository_root()
    output = prepare_output(output_arg, root)
    summary_path = output / "summary.txt"
    status_path = output / "exit-status.txt"
    scan_time = datetime.now(timezone.utc).replace(microsecond=0)
    (output / "scan-utc.txt").write_text(
        scan_time.isoformat().replace("+00:00", "Z") + "\n", encoding="utf-8"
    )
    try:
        version = run_text(["cargo", "audit", "--version"], root)
        (output / "cargo-audit-version.txt").write_text(version + "\n", encoding="utf-8")
        if version != AUDIT_VERSION:
            raise AdvisoryGateError(
                f"cargo-audit version mismatch: expected {AUDIT_VERSION!r}, got {version!r}"
            )

        refresh_path = output / "audit-refresh.json"
        refresh_status = run_capture(
            ["cargo", "audit", "--json", "--no-yanked"],
            root,
            refresh_path,
            output / "audit-refresh.stderr.txt",
        )
        require_audit_result(refresh_status, refresh_path, "cargo-audit refresh scan")

        database = advisory_database()
        before = database_identity(database, root)
        (output / "rustsec-database.json").write_bytes(canonical_json(before))

        lock_path = root / "Cargo.lock"
        policy_path = root / "security/advisory-policy.json"
        lock_digest = sha256_file(lock_path)
        policy_digest = sha256_file(policy_path)
        (output / "Cargo.lock.sha256").write_text(lock_digest + "\n", encoding="ascii")
        (output / "advisory-policy.sha256").write_text(
            policy_digest + "\n", encoding="ascii"
        )

        packages, excluded = load_registry_packages(lock_path)
        manifest_bytes, _ = build_registry_snapshot(
            packages,
            excluded,
            output,
            scan_time.isoformat().replace("+00:00", "Z"),
        )
        manifest_path = output / "registry-manifest.json"
        manifest_path.write_bytes(manifest_bytes)
        manifest_digest = sha256_bytes(manifest_bytes)
        (output / "registry-manifest.sha256").write_text(
            manifest_digest + "\n", encoding="ascii"
        )

        authoritative_path = output / "audit-authoritative.json"
        authoritative_status = run_capture(
            ["cargo", "audit", "--json", "--no-fetch", "--no-yanked"],
            root,
            authoritative_path,
            output / "audit-authoritative.stderr.txt",
        )
        authoritative = require_audit_result(
            authoritative_status, authoritative_path, "authoritative cargo-audit scan"
        )
        after = database_identity(database, root)
        if before != after:
            raise AdvisoryGateError("RustSec database identity changed during the gate")
        selected = load_registry_snapshot(
            manifest_path, manifest_digest, packages, excluded, output
        )

        _, entries = load_policy(policy_path)
        findings = parse_audit_report(authoritative)
        lines, denied = classify_findings(findings, entries, scan_time.date())
        yanked = [item for item in selected if item["yanked"] is True]
        for item in yanked:
            lines.append(
                f"DENY yanked/{item['name']}/{item['version']}/{item['checksum']}"
            )
            denied = True
        lines.append(
            f"PASS registry coverage: {len(selected)} locked crates.io packages, "
            f"{len(excluded)} excluded (path/git), {len(yanked)} yanked"
        )
        lines.append(
            f"RESULT findings={len(findings)} warnings="
            f"{sum(line.startswith('WARN ') for line in lines)} "
            f"exceptions={sum(line.startswith('EXCEPTION ') for line in lines)} "
            f"denied={sum(line.startswith('DENY ') for line in lines)}"
        )
        load_registry_snapshot(manifest_path, manifest_digest, packages, excluded, output)
        summary = "\n".join(lines) + "\n"
        summary_path.write_text(summary, encoding="utf-8")
        print(summary, end="")
        status = 1 if denied else 0
    except AdvisoryGateError as error:
        summary = f"DENY operational: {error}\n"
        summary_path.write_text(summary, encoding="utf-8")
        print(summary, end="", file=sys.stderr)
        status = 2
    status_path.write_text(f"{status}\n", encoding="ascii")
    return status


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run the fixed RFC 014 dependency-security gate."
    )
    parser.add_argument(
        "output",
        type=Path,
        help="new or empty directory for raw reports and registry evidence",
    )
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    arguments = parse_args(sys.argv[1:] if argv is None else argv)
    return execute(arguments.output)


if __name__ == "__main__":
    raise SystemExit(main())
