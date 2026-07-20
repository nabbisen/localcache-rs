import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "release_archive.py"
SPEC = importlib.util.spec_from_file_location("release_archive", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
ARCHIVE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ARCHIVE
SPEC.loader.exec_module(ARCHIVE)


class ReleaseArchiveTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name) / "repository"
        self.root.mkdir()
        self.write("Cargo.toml", '[package]\nname = "sample"\nversion = "1.2.3"\n')
        self.write("src/lib.rs", "pub fn answer() -> u8 { 42 }\n")
        self.write("docs/guide.md", "guide\n")
        executable = self.write("scripts/check.sh", "#!/bin/sh\nexit 0\n")
        executable.chmod(0o755)
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(
            ["git", "-C", str(self.root), "config", "user.name", "Fixture"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(self.root), "config", "user.email", "fixture@example.invalid"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(self.root), "config", "commit.gpgsign", "false"],
            check=True,
        )
        subprocess.run(["git", "-C", str(self.root), "add", "."], check=True)
        env = {
            **os.environ,
            "GIT_AUTHOR_DATE": "2026-01-02T03:04:05Z",
            "GIT_COMMITTER_DATE": "2026-01-02T03:04:05Z",
        }
        subprocess.run(
            ["git", "-C", str(self.root), "commit", "-qm", "fixture"],
            check=True,
            env=env,
        )
        self.commit = (
            subprocess.check_output(
                ["git", "-C", str(self.root), "rev-parse", "HEAD"], text=True
            )
            .strip()
        )
        self.expected = ARCHIVE.expected_manifest(self.root, self.commit)
        self.raw = ARCHIVE.build_git_tar(self.root, self.commit)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_canonical_git_archive_validates_and_extracts(self) -> None:
        members = ARCHIVE.validate_tar(self.raw, self.expected, self.commit)
        destination = Path(self.temporary.name) / "extract"
        ARCHIVE.extract_validated(members, destination)

        self.assertEqual(
            (destination / "src/lib.rs").read_text(encoding="utf-8"),
            "pub fn answer() -> u8 { 42 }\n",
        )
        self.assertTrue((destination / "scripts/check.sh").stat().st_mode & 0o111)
        self.assertFalse((destination / ".git").exists())

    def test_deterministic_gzip_has_no_timestamp_or_filename(self) -> None:
        first = ARCHIVE.compress_tar(self.raw)
        second = ARCHIVE.compress_tar(self.raw)

        self.assertEqual(first, second)
        self.assertEqual(first[3] & 0x1E, 0)
        self.assertEqual(first[4:8], b"\0\0\0\0")
        self.assertEqual(ARCHIVE.decompress_archive(first), self.raw)
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "concatenated"):
            ARCHIVE.decompress_archive(first + second)

    def test_rejects_missing_duplicate_and_mismatched_global_pax(self) -> None:
        records = self.records(self.raw)
        global_record = self.raw[records[0][0] : records[0][1]]
        without_global = self.raw[records[0][1] :]
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "missing canonical global"):
            ARCHIVE.validate_tar(without_global, self.expected, self.commit)
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "exactly once and first"):
            ARCHIVE.validate_tar(
                global_record + self.raw, self.expected, self.commit
            )
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "expected commit comment"):
            ARCHIVE.validate_tar(
                self.raw,
                self.expected,
                "0" * 40 if self.commit != "0" * 40 else "1" * 40,
            )

    def test_rejects_unknown_global_pax_key(self) -> None:
        records = self.records(self.raw)
        start, _end, size = records[0]
        mutated = bytearray(self.raw)
        payload = bytes(mutated[start + 512 : start + 512 + size])
        self.assertIn(b"comment=", payload)
        mutated[start + 512 : start + 512 + size] = payload.replace(
            b"comment=", b"evilkey=", 1
        )

        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "only the independently"):
            ARCHIVE.validate_tar(bytes(mutated), self.expected, self.commit)

    def test_rejects_per_entry_pax_and_gnu_extensions(self) -> None:
        for typeflag, message in ((b"x", "per-entry PAX"), (b"L", "GNU")):
            with self.subTest(typeflag=typeflag):
                hostile = self.mutate_header(self.raw, 1, typeflag=typeflag)
                with self.assertRaisesRegex(ARCHIVE.ArchiveError, message):
                    ARCHIVE.validate_tar(hostile, self.expected, self.commit)

    def test_rejects_links_and_special_entries(self) -> None:
        cases = (
            (b"1", "link entries"),
            (b"2", "link entries"),
            (b"3", "unsupported"),
            (b"4", "unsupported"),
            (b"6", "unsupported"),
            (b"7", "unsupported"),
        )
        for typeflag, message in cases:
            with self.subTest(typeflag=typeflag):
                hostile = self.mutate_header(
                    self.raw, 1, typeflag=typeflag, linkname="../../escape"
                )
                with self.assertRaisesRegex(ARCHIVE.ArchiveError, message):
                    ARCHIVE.validate_tar(hostile, self.expected, self.commit)

    def test_rejects_unsafe_and_ambiguous_paths(self) -> None:
        cases = (
            ("/absolute", "absolute"),
            ("C:/absolute", "absolute platform"),
            ("../traversal", "invalid archive path component"),
            ("dot/./entry", "invalid archive path component"),
            ("back\\slash", "ambiguous platform separator"),
            ("control\x01name", "control character"),
            ("empty//component", "invalid archive path component"),
        )
        for name, message in cases:
            with self.subTest(name=name):
                hostile = self.mutate_header(self.raw, 1, name=name)
                with self.assertRaisesRegex(ARCHIVE.ArchiveError, message):
                    ARCHIVE.validate_tar(hostile, self.expected, self.commit)

    def test_rejects_nul_with_nonzero_suffix(self) -> None:
        records = self.records(self.raw)
        start = records[1][0]
        hostile = bytearray(self.raw)
        hostile[start : start + 100] = b"safe\0evil" + bytes(91)
        self.fix_checksum(hostile, start)

        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "after its NUL"):
            ARCHIVE.validate_tar(bytes(hostile), self.expected, self.commit)

    def test_rejects_duplicate_normalized_path(self) -> None:
        directories = self.directory_indices(self.raw)
        first_name = self.header_name(self.raw, directories[0])
        hostile = self.mutate_header(self.raw, directories[1], name=first_name)
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "duplicate normalized"):
            ARCHIVE.validate_tar(hostile, self.expected, self.commit)

    def test_rejects_directory_trailing_separator_errors(self) -> None:
        directory_index = self.directory_indices(self.raw)[0]
        name = self.header_name(self.raw, directory_index)
        self.assertTrue(name.endswith("/"))
        for hostile_name in (name[:-1], name + "/"):
            with self.subTest(name=hostile_name):
                hostile = self.mutate_header(
                    self.raw, directory_index, name=hostile_name
                )
                with self.assertRaisesRegex(
                    ARCHIVE.ArchiveError, "directory must have exactly one"
                ):
                    ARCHIVE.validate_tar(hostile, self.expected, self.commit)

    def test_rejects_missing_member_and_wrong_order(self) -> None:
        records = self.records(self.raw)
        regular_index = self.first_regular_index(self.raw)
        start, end, _size = records[regular_index]
        missing = self.raw[:start] + self.raw[end:]
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "missing expected member"):
            ARCHIVE.validate_tar(missing, self.expected, self.commit)

        first = records[1]
        second = records[2]
        swapped = (
            self.raw[: first[0]]
            + self.raw[second[0] : second[1]]
            + self.raw[first[0] : first[1]]
            + self.raw[second[1] :]
        )
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "ordering differs"):
            ARCHIVE.validate_tar(swapped, self.expected, self.commit)

    def test_rejects_unexpected_missing_and_mode_mismatch(self) -> None:
        regular_index = self.first_regular_index(self.raw)
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "unexpected archive member"):
            ARCHIVE.validate_tar(
                self.mutate_header(self.raw, regular_index, name="unexpected.txt"),
                self.expected,
                self.commit,
            )
        missing = dict(self.expected)
        missing.pop(self.header_name(self.raw, regular_index))
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "unexpected archive member"):
            ARCHIVE.validate_tar(self.raw, missing, self.commit)
        current_mode = self.header_mode(self.raw, regular_index)
        hostile = self.mutate_header(
            self.raw, regular_index, mode=current_mode ^ 0o111
        )
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "type/mode mismatch"):
            ARCHIVE.validate_tar(hostile, self.expected, self.commit)

    def test_rejects_content_not_bound_to_committed_blob(self) -> None:
        regular_index = self.first_regular_index(self.raw)
        start, _end, size = self.records(self.raw)[regular_index]
        self.assertGreater(size, 0)
        hostile = bytearray(self.raw)
        hostile[start + 512] ^= 1
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "content mismatch"):
            ARCHIVE.validate_tar(bytes(hostile), self.expected, self.commit)

    def test_rejects_nonempty_extraction_destination(self) -> None:
        members = ARCHIVE.validate_tar(self.raw, self.expected, self.commit)
        destination = Path(self.temporary.name) / "extract"
        destination.mkdir()
        (destination / "occupied").write_text("x", encoding="utf-8")
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "not empty"):
            ARCHIVE.extract_validated(members, destination)

    def test_expected_manifest_rejects_tracked_symlink(self) -> None:
        (self.root / "link").symlink_to("src/lib.rs")
        subprocess.run(["git", "-C", str(self.root), "add", "link"], check=True)
        subprocess.run(
            ["git", "-C", str(self.root), "commit", "-qm", "add link"], check=True
        )
        commit = subprocess.check_output(
            ["git", "-C", str(self.root), "rev-parse", "HEAD"], text=True
        ).strip()
        with self.assertRaisesRegex(ARCHIVE.ArchiveError, "unsupported tracked object"):
            ARCHIVE.expected_manifest(self.root, commit)

    def write(self, relative: str, contents: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")
        return path

    @staticmethod
    def records(raw: bytes) -> list[tuple[int, int, int]]:
        records = []
        offset = 0
        while raw[offset : offset + 512] != bytes(512):
            header = raw[offset : offset + 512]
            size_raw = header[124:136].rstrip(b"\0 ").lstrip(b" ")
            size = int(size_raw or b"0", 8)
            end = offset + 512 + ((size + 511) // 512) * 512
            records.append((offset, end, size))
            offset = end
        return records

    @classmethod
    def header_name(cls, raw: bytes, index: int) -> str:
        start = cls.records(raw)[index][0]
        return raw[start : start + 100].split(b"\0", 1)[0].decode()

    @classmethod
    def header_mode(cls, raw: bytes, index: int) -> int:
        start = cls.records(raw)[index][0]
        return int(raw[start + 100 : start + 108].rstrip(b"\0 ").lstrip(b" "), 8)

    @classmethod
    def first_regular_index(cls, raw: bytes) -> int:
        for index, (start, _end, _size) in enumerate(cls.records(raw)):
            if raw[start + 156 : start + 157] in {b"0", b"\0"}:
                return index
        raise AssertionError("fixture has no regular member")

    @classmethod
    def directory_indices(cls, raw: bytes) -> list[int]:
        return [
            index
            for index, (start, _end, _size) in enumerate(cls.records(raw))
            if raw[start + 156 : start + 157] == b"5"
        ]

    @classmethod
    def mutate_header(
        cls,
        raw: bytes,
        index: int,
        *,
        name: str | None = None,
        typeflag: bytes | None = None,
        mode: int | None = None,
        linkname: str | None = None,
    ) -> bytes:
        start = cls.records(raw)[index][0]
        hostile = bytearray(raw)
        if name is not None:
            encoded = name.encode("utf-8")
            if len(encoded) > 100:
                raise AssertionError("test name is too long")
            hostile[start : start + 100] = encoded + bytes(100 - len(encoded))
        if typeflag is not None:
            hostile[start + 156 : start + 157] = typeflag
        if mode is not None:
            hostile[start + 100 : start + 108] = f"{mode:07o}\0".encode("ascii")
        if linkname is not None:
            encoded = linkname.encode("utf-8")
            hostile[start + 157 : start + 257] = encoded + bytes(100 - len(encoded))
        cls.fix_checksum(hostile, start)
        return bytes(hostile)

    @staticmethod
    def fix_checksum(raw: bytearray, start: int) -> None:
        raw[start + 148 : start + 156] = b"        "
        checksum = sum(raw[start : start + 512])
        raw[start + 148 : start + 156] = f"{checksum:06o}\0 ".encode("ascii")


if __name__ == "__main__":
    unittest.main()
