#!/usr/bin/env python3
"""RFC 009 archive construction, structural validation, and safe extraction."""

from __future__ import annotations

import gzip
import hashlib
import io
import re
import subprocess
import zlib
from dataclasses import dataclass
from pathlib import Path, PurePosixPath


BLOCK_SIZE = 512
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
FORBIDDEN_TOP_LEVEL = {".git", ".git-exclude", "target"}
RELEASE_ARCHIVE_RE = re.compile(r"^localcache-v[0-9]+\.[0-9]+\.[0-9]+\.tar\.gz$")


class ArchiveError(Exception):
    """The archive violates the RFC 009 structural contract."""


@dataclass(frozen=True)
class ExpectedMember:
    path: str
    kind: str
    executable: bool
    mtime: int
    object_id: str | None


@dataclass(frozen=True)
class ParsedMember:
    path: str
    kind: str
    executable: bool
    data: bytes
    mtime: int


def _git(root: Path, *args: str) -> bytes:
    try:
        completed = subprocess.run(
            ["git", "-C", str(root), *args],
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
        raise ArchiveError(f"git {' '.join(args)} failed: {detail}") from error
    return completed.stdout


def expected_manifest(root: Path, commit: str) -> dict[str, ExpectedMember]:
    """Derive the exact exported path/type/executable manifest from a commit."""
    if not COMMIT_RE.fullmatch(commit):
        raise ArchiveError(f"expected commit must be 40 lowercase hex digits: {commit!r}")

    output = _git(root, "ls-tree", "-rz", "-t", "--full-tree", commit)
    try:
        commit_mtime = int(_git(root, "show", "-s", "--format=%ct", commit))
    except ValueError as error:
        raise ArchiveError(f"commit timestamp is not an integer: {commit}") from error
    expected: dict[str, ExpectedMember] = {}
    for record in output.split(b"\0"):
        if not record:
            continue
        try:
            metadata, raw_path = record.split(b"\t", 1)
            raw_mode, raw_kind, raw_object_id = metadata.split(b" ", 2)
            mode = int(raw_mode, 8)
            kind = raw_kind.decode("ascii")
            object_id = raw_object_id.decode("ascii")
            path = raw_path.decode("utf-8", "strict")
        except (ValueError, UnicodeDecodeError) as error:
            raise ArchiveError(f"invalid git tree record: {record!r}") from error

        normalized = validate_member_name(path, directory=False)
        if excluded_export_path(normalized):
            raise ArchiveError(
                f"forbidden path is tracked at {commit}: {normalized}; "
                "remove it from the commit rather than silently exporting it"
            )
        if kind == "tree":
            member = ExpectedMember(
                normalized, "directory", True, commit_mtime, None
            )
        elif kind == "blob" and raw_mode in {b"100644", b"100755"}:
            if not COMMIT_RE.fullmatch(object_id):
                raise ArchiveError(f"invalid Git blob ID for {normalized}: {object_id}")
            member = ExpectedMember(
                normalized, "file", mode == 0o100755, commit_mtime, object_id
            )
        else:
            raise ArchiveError(
                f"unsupported tracked object {kind!r} mode {raw_mode!r}: {normalized}"
            )
        if normalized in expected:
            raise ArchiveError(f"duplicate path in Git tree: {normalized}")
        expected[normalized] = member

    if not expected:
        raise ArchiveError("Git export manifest is empty")
    return expected


def excluded_export_path(path: str) -> bool:
    parts = PurePosixPath(path).parts
    return (
        bool(parts)
        and (
            parts[0] in FORBIDDEN_TOP_LEVEL
            or parts[:2] == ("docs", "book")
            or RELEASE_ARCHIVE_RE.fullmatch(parts[-1]) is not None
        )
    )


def build_git_tar(root: Path, commit: str) -> bytes:
    """Build the canonical uncompressed committed-source tar stream."""
    return _git(root, "-c", "tar.umask=0022", "archive", "--format=tar", commit)


def compress_tar(raw_tar: bytes) -> bytes:
    """Apply deterministic gzip framing without filename or wall-clock time."""
    output = io.BytesIO()
    with gzip.GzipFile(
        filename="", mode="wb", fileobj=output, compresslevel=9, mtime=0
    ) as compressor:
        compressor.write(raw_tar)
    return output.getvalue()


def decompress_archive(archive: bytes) -> bytes:
    if len(archive) < 10 or archive[:3] != b"\x1f\x8b\x08":
        raise ArchiveError("archive is not a gzip stream")
    flags = archive[3]
    mtime = int.from_bytes(archive[4:8], "little")
    if flags & 0x1E:
        raise ArchiveError("gzip header must not contain filename, comment, or extra data")
    if mtime != 0:
        raise ArchiveError("gzip header contains a wall-clock timestamp")
    try:
        decompressor = zlib.decompressobj(16 + zlib.MAX_WBITS)
        raw_tar = decompressor.decompress(archive)
        raw_tar += decompressor.flush()
    except zlib.error as error:
        raise ArchiveError(f"invalid gzip stream: {error}") from error
    if not decompressor.eof or decompressor.unused_data:
        raise ArchiveError("gzip stream is truncated or contains concatenated data")
    return raw_tar


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _field(header: bytes, start: int, length: int, label: str) -> bytes:
    raw = header[start : start + length]
    nul = raw.find(b"\0")
    if nul >= 0:
        if any(raw[nul + 1 :]):
            raise ArchiveError(f"{label} contains bytes after its NUL terminator")
        raw = raw[:nul]
    return raw


def _octal(header: bytes, start: int, length: int, label: str) -> int:
    raw = header[start : start + length]
    if raw and raw[0] & 0x80:
        raise ArchiveError(f"base-256 {label} is not permitted")
    value = raw.rstrip(b"\0 ").lstrip(b" ")
    if not value:
        return 0
    if any(byte not in b"01234567" for byte in value):
        raise ArchiveError(f"invalid octal {label}: {raw!r}")
    return int(value, 8)


def _decode_name(raw: bytes, label: str) -> str:
    try:
        return raw.decode("utf-8", "strict")
    except UnicodeDecodeError as error:
        raise ArchiveError(f"{label} is not valid UTF-8") from error


def _header_name(header: bytes) -> str:
    name = _field(header, 0, 100, "member name")
    prefix = _field(header, 345, 155, "member prefix")
    raw = prefix + (b"/" if prefix and name else b"") + name
    return _decode_name(raw, "member name")


def _verify_checksum(header: bytes) -> None:
    stored = _octal(header, 148, 8, "checksum")
    calculated = sum(header[:148]) + (8 * ord(" ")) + sum(header[156:])
    if stored != calculated:
        raise ArchiveError(
            f"tar header checksum mismatch: expected {stored}, calculated {calculated}"
        )


def parse_pax(payload: bytes) -> dict[str, str]:
    headers: dict[str, str] = {}
    offset = 0
    while offset < len(payload):
        space = payload.find(b" ", offset)
        if space <= offset:
            raise ArchiveError("malformed PAX record length")
        raw_length = payload[offset:space]
        if not raw_length.isdigit() or raw_length.startswith(b"0"):
            raise ArchiveError("malformed PAX record length")
        length = int(raw_length)
        end = offset + length
        if end > len(payload) or payload[end - 1 : end] != b"\n":
            raise ArchiveError("truncated or unterminated PAX record")
        record = payload[space + 1 : end - 1]
        if b"=" not in record:
            raise ArchiveError("malformed PAX key/value record")
        raw_key, raw_value = record.split(b"=", 1)
        try:
            key = raw_key.decode("utf-8", "strict")
            value = raw_value.decode("utf-8", "strict")
        except UnicodeDecodeError as error:
            raise ArchiveError("PAX key/value is not valid UTF-8") from error
        if not key or key in headers:
            raise ArchiveError(f"empty or duplicate PAX key: {key!r}")
        headers[key] = value
        offset = end
    if offset != len(payload):
        raise ArchiveError("PAX payload has trailing bytes")
    return headers


def validate_member_name(name: str, *, directory: bool) -> str:
    if directory:
        if not name.endswith("/") or name.endswith("//"):
            raise ArchiveError(
                f"directory must have exactly one format trailing slash: {name!r}"
            )
        name = name[:-1]
    elif name.endswith("/"):
        raise ArchiveError(f"regular-file name has a trailing slash: {name!r}")

    if not name:
        raise ArchiveError("archive member name is empty")
    if name.startswith("/") or name.startswith("\\"):
        raise ArchiveError(f"absolute archive member path: {name!r}")
    if "\\" in name:
        raise ArchiveError(f"ambiguous platform separator in archive path: {name!r}")
    if any(ord(character) < 32 or ord(character) == 127 for character in name):
        raise ArchiveError(f"control character in archive path: {name!r}")

    components = name.split("/")
    if any(component in {"", ".", ".."} for component in components):
        raise ArchiveError(f"invalid archive path component: {name!r}")
    if re.fullmatch(r"[A-Za-z]:", components[0]):
        raise ArchiveError(f"absolute platform archive path: {name!r}")
    normalized = PurePosixPath(*components).as_posix()
    if normalized != name:
        raise ArchiveError(f"archive path is not normalized: {name!r}")
    if excluded_export_path(normalized):
        raise ArchiveError(f"forbidden archive member: {normalized}")
    return normalized


def validate_tar(
    raw_tar: bytes,
    expected: dict[str, ExpectedMember],
    expected_commit: str,
) -> list[ParsedMember]:
    """Validate raw tar headers and exact export manifest before extraction."""
    if not COMMIT_RE.fullmatch(expected_commit):
        raise ArchiveError("expected commit must be 40 lowercase hex digits")
    if not expected:
        raise ArchiveError("expected export manifest is empty")
    if len(raw_tar) % BLOCK_SIZE:
        raise ArchiveError("tar stream length is not block-aligned")

    parsed: dict[str, ParsedMember] = {}
    offset = 0
    global_pax_seen = False
    logical_seen = False
    zero_blocks = 0

    while offset < len(raw_tar):
        header = raw_tar[offset : offset + BLOCK_SIZE]
        offset += BLOCK_SIZE
        if header == bytes(BLOCK_SIZE):
            zero_blocks += 1
            remainder = raw_tar[offset:]
            if not any(remainder):
                zero_blocks += len(remainder) // BLOCK_SIZE
                break
            continue
        if zero_blocks:
            raise ArchiveError("nonzero tar header after end-of-archive marker")

        _verify_checksum(header)
        if header[257:263] != b"ustar\0" or header[263:265] != b"00":
            raise ArchiveError("only canonical POSIX ustar headers are permitted")

        name = _header_name(header)
        mode = _octal(header, 100, 8, "mode")
        uid = _octal(header, 108, 8, "uid")
        gid = _octal(header, 116, 8, "gid")
        size = _octal(header, 124, 12, "size")
        mtime = _octal(header, 136, 12, "mtime")
        typeflag = header[156:157]
        linkname = _field(header, 157, 100, "link target")
        uname = _field(header, 265, 32, "user name")
        gname = _field(header, 297, 32, "group name")
        if uid != 0 or gid != 0 or uname != b"root" or gname != b"root":
            raise ArchiveError(
                f"noncanonical ownership metadata for {name!r}: "
                f"uid={uid} gid={gid} uname={uname!r} gname={gname!r}"
            )
        data_end = offset + size
        if data_end > len(raw_tar):
            raise ArchiveError(f"truncated payload for {name!r}")
        payload = raw_tar[offset:data_end]
        offset += ((size + BLOCK_SIZE - 1) // BLOCK_SIZE) * BLOCK_SIZE

        if typeflag == b"g":
            if logical_seen or global_pax_seen:
                raise ArchiveError("global PAX record must occur exactly once and first")
            if name != "pax_global_header":
                raise ArchiveError(f"unexpected global PAX header name: {name!r}")
            if linkname:
                raise ArchiveError("global PAX record has a link target")
            if mode != 0o666:
                raise ArchiveError("global PAX record has noncanonical mode")
            expected_mtime = next(iter(expected.values())).mtime
            if mtime != expected_mtime:
                raise ArchiveError(
                    "global PAX record mtime does not match the commit timestamp"
                )
            headers = parse_pax(payload)
            if headers != {"comment": expected_commit}:
                raise ArchiveError(
                    "global PAX record must contain only the independently "
                    "expected commit comment"
                )
            global_pax_seen = True
            continue

        logical_seen = True
        if typeflag in {b"x", b"X"}:
            raise ArchiveError("per-entry PAX records are forbidden")
        if typeflag in {b"L", b"K", b"S"}:
            raise ArchiveError("GNU archive extension records are forbidden")
        if typeflag in {b"1", b"2"}:
            target = _decode_name(linkname, "link target")
            if "\0" in target:
                raise ArchiveError("NUL in link target")
            raise ArchiveError(f"link entries are forbidden: {name!r} -> {target!r}")
        if typeflag not in {b"\0", b"0", b"5"}:
            raise ArchiveError(
                f"unsupported archive entry type {typeflag!r} for {name!r}"
            )
        if linkname:
            raise ArchiveError(f"non-link member has a link target: {name!r}")

        kind = "directory" if typeflag == b"5" else "file"
        path = validate_member_name(name, directory=kind == "directory")
        if path in parsed:
            raise ArchiveError(f"duplicate normalized archive path: {path}")
        if kind == "directory" and size != 0:
            raise ArchiveError(f"directory has a payload: {path}")

        canonical_mode = 0o755 if kind == "directory" or mode & 0o111 else 0o644
        if mode != canonical_mode:
            raise ArchiveError(
                f"noncanonical mode for {path}: {mode:o}; expected {canonical_mode:o}"
            )
        actual = ParsedMember(path, kind, bool(mode & 0o111), payload, mtime)
        wanted = expected.get(path)
        if wanted is None:
            raise ArchiveError(f"unexpected archive member: {path}")
        if (actual.kind, actual.executable) != (wanted.kind, wanted.executable):
            raise ArchiveError(
                f"type/mode mismatch for {path}: "
                f"expected {wanted.kind} executable={wanted.executable}, "
                f"found {actual.kind} executable={actual.executable}"
            )
        if actual.mtime != wanted.mtime:
            raise ArchiveError(
                f"mtime mismatch for {path}: expected {wanted.mtime}, "
                f"found {actual.mtime}"
            )
        if actual.kind == "file":
            blob_prefix = f"blob {len(payload)}\0".encode("ascii")
            object_id = hashlib.sha1(
                blob_prefix + payload, usedforsecurity=False
            ).hexdigest()
            if object_id != wanted.object_id:
                raise ArchiveError(
                    f"content mismatch for {path}: expected Git blob "
                    f"{wanted.object_id}, found {object_id}"
                )
        parsed[path] = actual

    if zero_blocks < 2:
        raise ArchiveError("tar stream lacks two zero end-of-archive blocks")
    if not global_pax_seen:
        raise ArchiveError("missing canonical global PAX commit record")
    missing = sorted(set(expected) - set(parsed))
    if missing:
        raise ArchiveError(f"archive is missing expected member: {missing[0]}")
    if len(parsed) != len(expected):
        raise ArchiveError("archive member count does not match export manifest")
    if list(parsed) != list(expected):
        raise ArchiveError("archive member ordering differs from the Git export manifest")
    return list(parsed.values())


def extract_validated(members: list[ParsedMember], destination: Path) -> None:
    destination = destination.resolve()
    if destination.exists():
        if not destination.is_dir() or any(destination.iterdir()):
            raise ArchiveError(f"extraction destination is not empty: {destination}")
    else:
        destination.mkdir(parents=True, mode=0o700)

    for member in members:
        target = destination.joinpath(*PurePosixPath(member.path).parts)
        try:
            target.resolve().relative_to(destination)
        except ValueError as error:
            raise ArchiveError(f"extraction path escaped destination: {member.path}") from error
        if member.kind == "directory":
            target.mkdir(parents=True, exist_ok=True)
            target.chmod(0o755)
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("xb") as file:
                file.write(member.data)
            target.chmod(0o755 if member.executable else 0o644)


def write_archive(path: Path, raw_tar: bytes) -> tuple[int, str]:
    archive = compress_tar(raw_tar)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as file:
        file.write(archive)
    return len(archive), sha256_bytes(archive)


def validate_archive_file(
    path: Path,
    expected: dict[str, ExpectedMember],
    expected_commit: str,
) -> list[ParsedMember]:
    try:
        archive = path.read_bytes()
    except OSError as error:
        raise ArchiveError(f"cannot read archive {path}: {error}") from error
    return validate_tar(decompress_archive(archive), expected, expected_commit)
