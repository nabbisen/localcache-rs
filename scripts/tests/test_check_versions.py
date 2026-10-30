import importlib.util
import io
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[1] / "check_versions.py"
SPEC = importlib.util.spec_from_file_location("check_versions", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
CV = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CV
SPEC.loader.exec_module(CV)


class CheckVersionsTests(unittest.TestCase):
    # RFC 022 R3 widened the gate to discover targets by glob rather than a
    # hand list (see CV.version_reference_targets), so the fixture now
    # names its own fixed set of paths -- README.md plus two docs/src pages
    # -- purely to give the glob something to discover on disk; it is not
    # reading back a target list from the module under test.
    _VERSION_REFERENCE_FIXTURE_TARGETS = (
        Path("README.md"),
        Path("docs/src/getting_started.md"),
        Path("docs/src/introduction.md"),
    )

    @classmethod
    def _version_reference_fixture(cls, root: Path, *, version: str) -> None:
        for relative in cls._VERSION_REFERENCE_FIXTURE_TARGETS:
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(
                f'```toml\n[dependencies]\nlocalcache = "{version}"\n```\n',
                encoding="utf-8",
            )

    def test_verify_version_references_passes_when_all_match(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            CV.verify_version_references(root, "0.20.1")  # must not raise

    def test_verify_version_references_fails_closed_on_stale_version(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            with self.assertRaisesRegex(CV.VersionError, "stale version reference"):
                CV.verify_version_references(root, "0.20.2")

    def test_verify_version_references_fails_closed_on_missing_line(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            (root / "README.md").write_text("no install example here\n", encoding="utf-8")
            with self.assertRaisesRegex(
                CV.VersionError, "no install-example version line found"
            ):
                CV.verify_version_references(root, "0.20.1")

    def test_verify_version_references_fails_closed_on_missing_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(CV.VersionError, "cannot read version reference"):
                CV.verify_version_references(root, "0.20.1")

    def test_version_reference_targets_discovers_docs_pages_by_glob(self) -> None:
        # RFC 022 R3: no hand list -- README.md plus every docs/src/**/*.md,
        # found by glob, so a new doc page is covered automatically.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "README.md").write_text("x\n", encoding="utf-8")
            (root / "docs" / "src" / "nested").mkdir(parents=True)
            (root / "docs" / "src" / "querying.md").write_text("x\n", encoding="utf-8")
            (root / "docs" / "src" / "nested" / "deep.md").write_text("x\n", encoding="utf-8")
            (root / "docs" / "src" / "SUMMARY.md").write_text("x\n", encoding="utf-8")
            targets = CV.version_reference_targets(root)
            self.assertEqual(
                set(targets),
                {
                    root / "README.md",
                    root / "docs" / "src" / "querying.md",
                    root / "docs" / "src" / "nested" / "deep.md",
                    root / "docs" / "src" / "SUMMARY.md",
                },
            )

    def test_verify_version_references_does_not_require_a_declaration_on_every_docs_page(
        self,
    ) -> None:
        # A docs/src page discovered by glob is only held to the version
        # contract once it actually makes an install-example claim; most
        # pages (architecture, querying, SUMMARY.md, ...) never had one and
        # are not required to grow one just because the gate now finds them.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            (root / "docs" / "src" / "architecture.md").write_text(
                "No install example on this page, just prose.\n", encoding="utf-8"
            )
            CV.verify_version_references(root, "0.20.1")  # must not raise

    def test_verify_version_references_accepts_the_table_form_declaration(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            (root / "README.md").write_text(
                'localcache = { version = "0.20.1", features = ["watching"] }\n',
                encoding="utf-8",
            )
            CV.verify_version_references(root, "0.20.1")  # must not raise

    def test_verify_version_references_accepts_column_aligned_whitespace(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            (root / "README.md").write_text(
                'localcache         = "0.20.1"\n', encoding="utf-8"
            )
            CV.verify_version_references(root, "0.20.1")  # must not raise

    def test_verify_version_references_fails_closed_on_an_unparseable_declaration(
        self,
    ) -> None:
        # A declaration line is never silently skipped: it either parses or
        # the gate fails, so a typo that renders the pinned version
        # unreadable cannot slip through as an accidental pass.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            (root / "README.md").write_text("localcache = 0.20.1\n", encoding="utf-8")
            with self.assertRaisesRegex(
                CV.VersionError, "unparseable localcache declaration line"
            ):
                CV.verify_version_references(root, "0.20.1")

    def test_verify_version_references_ignores_a_prose_mention(self) -> None:
        # Anchored on the line's start, not a substring search: a backticked
        # mid-line mention like this one (docs/src/dependency_security.md's
        # real shape) is preceded by other text on the same line, so it must
        # never be treated as a declaration and must never fail the gate.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            (root / "README.md").write_text(
                'Some crates pin to `localcache = "0.19"` for compatibility.\n'
                'localcache = "0.20.1"\n',
                encoding="utf-8",
            )
            CV.verify_version_references(root, "0.20.1")  # must not raise

    def test_verify_changelog_has_coming_version_section_passes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "CHANGELOG.md").write_text(
                "# Changelog\n\n## [0.20.1] — Unreleased\n\n### Added\n\n- Something.\n\n"
                "## [0.20.0] — 2026-06-06\n\n### Added\n\n- Older.\n",
                encoding="utf-8",
            )
            CV.verify_changelog_has_coming_version_section(root, "0.20.1")

    def test_verify_changelog_fails_closed_on_missing_section(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "CHANGELOG.md").write_text(
                "# Changelog\n\n## [0.20.0] — 2026-06-06\n\n### Added\n\n- Older.\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(CV.VersionError, "no section for"):
                CV.verify_changelog_has_coming_version_section(root, "0.20.1")

    def test_verify_changelog_fails_closed_on_empty_section(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "CHANGELOG.md").write_text(
                "# Changelog\n\n## [0.20.1] — Unreleased\n\n## [0.20.0] — 2026-06-06\n\n"
                "### Added\n\n- Older.\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(CV.VersionError, "is empty"):
                CV.verify_changelog_has_coming_version_section(root, "0.20.1")

    def test_real_repo_version_references_and_changelog_match_workspace(self) -> None:
        # Demonstrates the fixed defect directly: before M6d this failed
        # because README.md/docs said 0.20.1 while Cargo.toml said 0.20.0.
        #
        # RFC 022 R3 (Q0d) widened the gate from a three-file hand list to
        # every docs/src/**/*.md page, and it correctly failed on five
        # pre-existing stale "0.19" declaration lines across three pages
        # (docs/src/async.md, docs/src/cookbook.md, docs/src/features.md)
        # that the narrow gate never covered. Q0d deliberately left them
        # unfixed and this test deliberately asserted that failure; Q0e
        # (RFC 022 R5) fixed those five lines, so this asserts success again.
        root = CV.ROOT
        version = CV.workspace_version(root)
        CV.verify_version_references(root, version)
        CV.verify_changelog_has_coming_version_section(root, version)

    def test_workspace_version_reads_the_root_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text(
                '[workspace.package]\nversion = "1.2.3"\n', encoding="utf-8"
            )
            self.assertEqual(CV.workspace_version(root), "1.2.3")

    def test_a_stale_reference_names_the_file_and_the_line(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            (root / "docs" / "src" / "introduction.md").write_text(
                'text\n\n```toml\nlocalcache = "0.19.0"\n```\n', encoding="utf-8"
            )
            with self.assertRaisesRegex(
                CV.VersionError, r"docs/src/introduction.md: stale .*\(line 4\)"
            ):
                CV.verify_version_references(root, "0.20.1")

    def _run_main(self, root: Path, *argv: str) -> tuple[int, str]:
        err = io.StringIO()
        with mock.patch.object(CV, "ROOT", root), redirect_stdout(io.StringIO()), redirect_stderr(err):
            status = CV.main(list(argv))
        return status, err.getvalue()

    def _consistent_repository(self, root: Path, version: str) -> None:
        self._version_reference_fixture(root, version=version)
        (root / "Cargo.toml").write_text(
            f'[workspace.package]\nversion = "{version}"\n', encoding="utf-8"
        )
        (root / "CHANGELOG.md").write_text(
            f"# Changelog\n\n## [{version}] — Unreleased\n\n- Something.\n", encoding="utf-8"
        )

    def test_main_passes_for_the_workspace_version_and_for_a_matching_expect(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._consistent_repository(root, "0.20.1")
            self.assertEqual(self._run_main(root)[0], 0)
            self.assertEqual(self._run_main(root, "--expect", "0.20.1")[0], 0)

    def test_main_fails_for_an_expect_that_the_tree_does_not_name(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._consistent_repository(root, "0.20.1")
            status, error = self._run_main(root, "--expect", "0.20.2")
            self.assertEqual(status, 1)
            self.assertIn("stale version reference", error)

    def test_main_fails_when_the_workspace_version_is_unreadable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            status, error = self._run_main(Path(directory))
            self.assertEqual(status, 1)
            self.assertIn("check_versions: FAILED", error)


if __name__ == "__main__":
    unittest.main()
