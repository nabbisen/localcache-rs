import argparse
import importlib.util
import hashlib
import io
import json
import os
import re
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import unittest
from pathlib import Path
from unittest import mock


SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
SCRIPT = SCRIPTS / "release.py"
SPEC = importlib.util.spec_from_file_location("release_runner", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
RUNNER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = RUNNER
SPEC.loader.exec_module(RUNNER)


class ReleaseRunnerTests(unittest.TestCase):
    def test_dirty_worktree_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = self.repository(Path(directory))
            (root / "tracked.txt").write_text("changed\n", encoding="utf-8")
            with self.assertRaisesRegex(RUNNER.ReleaseError, "requires a clean"):
                RUNNER.require_clean_commit(root)

    def test_clean_worktree_returns_full_commit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = self.repository(Path(directory))
            commit = RUNNER.require_clean_commit(root)
            self.assertRegex(commit, r"^[0-9a-f]{40}$")

    def test_output_boundary_allows_only_external_or_git_exclude(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "repository"
            root.mkdir()
            allowed = RUNNER.require_output_boundary(
                root, root / ".git-exclude/release"
            )
            self.assertEqual(allowed, (root / ".git-exclude/release").resolve())
            external = RUNNER.require_output_boundary(
                root, Path(directory) / "external"
            )
            self.assertEqual(external, (Path(directory) / "external").resolve())
            with self.assertRaisesRegex(RUNNER.ReleaseError, "outside the repository"):
                RUNNER.require_output_boundary(root, root / "release")

    def test_required_layout_rejects_git_and_nested_archive(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative in RUNNER.REQUIRED_PATHS:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("", encoding="utf-8")
            RUNNER.verify_required_layout(root)
            (root / ".git").mkdir()
            with self.assertRaisesRegex(RUNNER.ReleaseError, "forbidden path"):
                RUNNER.verify_required_layout(root)
            (root / ".git").rmdir()
            (root / "localcache-v1.2.3.tar.gz").write_bytes(b"nested")
            with self.assertRaisesRegex(RUNNER.ReleaseError, "nested release archive"):
                RUNNER.verify_required_layout(root)

    def test_smoke_commands_are_package_scoped_and_locked(self) -> None:
        commands = dict(RUNNER.smoke_commands())
        self.assertEqual(
            commands["cargo-metadata"],
            ["cargo", "metadata", "--locked", "--format-version", "1"],
        )
        self.assertIn("-p", commands["library-all-targets"])
        self.assertIn("localcache", commands["library-all-targets"])
        self.assertIn("localcache-cli", commands["cli-all-targets"])
        self.assertIn("--locked", commands["benchmark-compile"])
        self.assertEqual(commands["mdbook"], ["mdbook", "build", "docs"])

    def test_workspace_version_rejects_mismatched_package_versions(self) -> None:
        document = {
            "packages": [
                {"name": "localcache", "version": "1.2.3", "dependencies": []},
                {
                    "name": "localcache-cli",
                    "version": "1.2.4",
                    "dependencies": [{"name": "localcache", "req": "^1.2.3"}],
                },
            ]
        }
        with self.assertRaisesRegex(RUNNER.ReleaseError, "versions differ"):
            RUNNER.workspace_version(document)

    def test_workspace_version_does_not_inspect_cli_dependency_requirement(self) -> None:
        # workspace_version() only compares the two packages' own declared
        # versions; it does not read or gate on the CLI's `localcache`
        # path-dependency requirement at all -- any requirement string, even
        # a wildly incompatible one, must not affect the result.
        document = {
            "packages": [
                {"name": "localcache", "version": "1.2.3", "dependencies": []},
                {
                    "name": "localcache-cli",
                    "version": "1.2.3",
                    "dependencies": [{"name": "localcache", "req": "^99.0.0"}],
                },
            ]
        }
        self.assertEqual(RUNNER.workspace_version(document), "1.2.3")

        del document["packages"][1]["dependencies"]
        self.assertEqual(RUNNER.workspace_version(document), "1.2.3")

    # RFC 022 R3 widened the gate to discover targets by glob rather than a
    # hand list (see RUNNER.version_reference_targets), so the fixture now
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
            RUNNER.verify_version_references(root, "0.20.1")  # must not raise

    def test_verify_version_references_fails_closed_on_stale_version(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            with self.assertRaisesRegex(RUNNER.ReleaseError, "stale version reference"):
                RUNNER.verify_version_references(root, "0.20.2")

    def test_verify_version_references_fails_closed_on_missing_line(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            (root / "README.md").write_text("no install example here\n", encoding="utf-8")
            with self.assertRaisesRegex(
                RUNNER.ReleaseError, "no install-example version line found"
            ):
                RUNNER.verify_version_references(root, "0.20.1")

    def test_verify_version_references_fails_closed_on_missing_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(RUNNER.ReleaseError, "cannot read version reference"):
                RUNNER.verify_version_references(root, "0.20.1")

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
            targets = RUNNER.version_reference_targets(root)
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
            RUNNER.verify_version_references(root, "0.20.1")  # must not raise

    def test_verify_version_references_accepts_the_table_form_declaration(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            (root / "README.md").write_text(
                'localcache = { version = "0.20.1", features = ["watching"] }\n',
                encoding="utf-8",
            )
            RUNNER.verify_version_references(root, "0.20.1")  # must not raise

    def test_verify_version_references_accepts_column_aligned_whitespace(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._version_reference_fixture(root, version="0.20.1")
            (root / "README.md").write_text(
                'localcache         = "0.20.1"\n', encoding="utf-8"
            )
            RUNNER.verify_version_references(root, "0.20.1")  # must not raise

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
                RUNNER.ReleaseError, "unparseable localcache declaration line"
            ):
                RUNNER.verify_version_references(root, "0.20.1")

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
            RUNNER.verify_version_references(root, "0.20.1")  # must not raise

    def test_verify_changelog_has_coming_version_section_passes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "CHANGELOG.md").write_text(
                "# Changelog\n\n## [0.20.1] — Unreleased\n\n### Added\n\n- Something.\n\n"
                "## [0.20.0] — 2026-06-06\n\n### Added\n\n- Older.\n",
                encoding="utf-8",
            )
            RUNNER.verify_changelog_has_coming_version_section(root, "0.20.1")

    def test_verify_changelog_fails_closed_on_missing_section(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "CHANGELOG.md").write_text(
                "# Changelog\n\n## [0.20.0] — 2026-06-06\n\n### Added\n\n- Older.\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RUNNER.ReleaseError, "no section for"):
                RUNNER.verify_changelog_has_coming_version_section(root, "0.20.1")

    def test_verify_changelog_fails_closed_on_empty_section(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "CHANGELOG.md").write_text(
                "# Changelog\n\n## [0.20.1] — Unreleased\n\n## [0.20.0] — 2026-06-06\n\n"
                "### Added\n\n- Older.\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RUNNER.ReleaseError, "is empty"):
                RUNNER.verify_changelog_has_coming_version_section(root, "0.20.1")

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
        root = SCRIPT.resolve().parents[1]
        with (root / "Cargo.toml").open("rb") as file:
            document = tomllib.load(file)
        version = document["workspace"]["package"]["version"]
        RUNNER.verify_version_references(root, version)
        RUNNER.verify_changelog_has_coming_version_section(root, version)

    def test_failed_gate_is_logged_and_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            logger = RUNNER.GateLog(root / "gate.log")
            with self.assertRaisesRegex(RUNNER.ReleaseError, "exit status 7"):
                RUNNER.run_gate(
                    logger,
                    "deliberate-failure",
                    [sys.executable, "-c", "raise SystemExit(7)"],
                    root,
                )
            log = (root / "gate.log").read_text(encoding="utf-8")
            self.assertIn("gate: deliberate-failure", log)
            self.assertIn("exit-status: 7", log)

    def test_run_gate_default_merges_stderr_into_returned_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            logger = RUNNER.GateLog(root / "gate.log")
            script = "import sys; sys.stderr.write('warn\\n'); sys.stdout.write('ok')"
            output = RUNNER.run_gate(
                logger, "merged", [sys.executable, "-c", script], root
            )
            self.assertIn("warn", output)
            self.assertIn("ok", output)

    def test_run_gate_separate_stderr_returns_only_stdout(self) -> None:
        # RC-4: a cold cargo cache writes progress lines to stderr; a caller
        # that parses stdout as structured data (cargo_metadata) must not see
        # them merged in, or the parse breaks non-deterministically depending
        # on cache state.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            logger = RUNNER.GateLog(root / "gate.log")
            script = (
                "import sys; "
                "sys.stderr.write('Updating crates.io index\\n'); "
                "sys.stdout.write('{\"ok\": true}')"
            )
            output = RUNNER.run_gate(
                logger,
                "stderr-emitting",
                [sys.executable, "-c", script],
                root,
                separate_stderr=True,
            )
            self.assertEqual(output, '{"ok": true}')
            log = (root / "gate.log").read_text(encoding="utf-8")
            self.assertIn("stderr:", log)
            self.assertIn("Updating crates.io index", log)

    def test_cargo_metadata_uses_separate_stderr(self) -> None:
        source_text = (SCRIPTS / "release.py").read_text(encoding="utf-8")
        cargo_metadata_text = source_text[source_text.index("def cargo_metadata(") :]
        cargo_metadata_text = cargo_metadata_text[: cargo_metadata_text.index("\n\n\n")]
        self.assertIn("separate_stderr=True", cargo_metadata_text)

    def test_artifact_context_rejects_git_before_running_gates(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".git").mkdir()
            arguments = argparse.Namespace(
                root=root,
                expected_layout=RUNNER.LAYOUT,
                expected_uncompressed_sha256="a" * 64,
                expected_version="1.2.3",
                evidence_dir=root / "evidence",
                target_dir=root / "target-output",
            )
            with self.assertRaisesRegex(RUNNER.ReleaseError, "must not contain .git"):
                RUNNER.artifact_mode(arguments)

    def test_artifact_context_rejects_parent_contract_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            arguments = argparse.Namespace(
                root=root,
                expected_layout="versioned-parent",
                expected_uncompressed_sha256="a" * 64,
                expected_version="1.2.3",
                evidence_dir=root / "evidence",
                target_dir=root / "target-output",
            )
            with self.assertRaisesRegex(
                RUNNER.ReleaseError, "unsupported artifact layout"
            ):
                RUNNER.artifact_mode(arguments)

    def test_failed_artifact_mode_marks_downstream_steps_incomplete(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            evidence = root / "evidence"
            status = RUNNER.main(
                [
                    "artifact",
                    "--root",
                    str(root),
                    "--expected-version",
                    "1.2.3",
                    "--expected-layout",
                    "wrong-layout",
                    "--expected-uncompressed-sha256",
                    "a" * 64,
                    "--evidence-dir",
                    str(evidence),
                    "--target-dir",
                    str(root / "target-output"),
                ]
            )
            self.assertNotEqual(status, 0)
            summary = (evidence / "summary.log").read_text(encoding="utf-8")
            self.assertIn("status: FAIL", summary)
            self.assertIn("required-downstream-steps: NOT COMPLETED", summary)

    def test_unknown_mode_is_nonzero(self) -> None:
        with self.assertRaises(SystemExit) as raised:
            RUNNER.parse_args(["unknown-mode"])
        self.assertNotEqual(raised.exception.code, 0)

    def test_release_implementation_hashes_match_manifest(self) -> None:
        root = SCRIPT.resolve().parents[1]
        with (root / "scripts/release-tools.toml").open("rb") as file:
            document = tomllib.load(file)
        for policy in document["implementations"].values():
            path = root / policy["path"]
            self.assertEqual(RUNNER.sha256_file(path), policy["sha256"])

    def test_verify_implementation_fails_closed_on_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "tool.py"
            target.write_text("print('hello')\n", encoding="utf-8")
            policy = {"path": "tool.py", "sha256": "0" * 64}
            with self.assertRaisesRegex(RUNNER.ReleaseError, "hash mismatch"):
                RUNNER.verify_implementation(root, "tool", policy)

    def test_verify_implementation_passes_on_matching_hash(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "tool.py"
            target.write_text("print('hello')\n", encoding="utf-8")
            digest = RUNNER.sha256_file(target)
            policy = {"path": "tool.py", "sha256": digest}
            observed = RUNNER.verify_implementation(root, "tool", policy)
            self.assertIn(digest, observed)

    def test_verify_named_implementation_reads_from_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            target = root / "scripts/tool.py"
            target.write_text("print('hello')\n", encoding="utf-8")
            digest = RUNNER.sha256_file(target)
            (root / "scripts/release-tools.toml").write_text(
                "schema-version = 1\n\n"
                '[implementations.tool]\n'
                'path = "scripts/tool.py"\n'
                f'sha256 = "{digest}"\n',
                encoding="utf-8",
            )
            observed = RUNNER.verify_named_implementation(root, "tool")
            self.assertIn(digest, observed)

    def test_verify_named_implementation_fails_closed_when_unlisted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            (root / "scripts/release-tools.toml").write_text(
                "schema-version = 1\n\n[implementations]\n", encoding="utf-8"
            )
            with self.assertRaisesRegex(
                RUNNER.ReleaseError, "no implementation policy"
            ):
                RUNNER.verify_named_implementation(root, "does-not-exist")

    def test_verify_named_implementation_does_not_require_extra_tables(self) -> None:
        # Used by gates (security) that only need their own implementation
        # verified. RFC 017 removed [producer]/[canonical-tools]/
        # [supported-host-tools] entirely, so there is nothing else left for
        # this to accidentally depend on any more.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            target = root / "scripts/tool.py"
            target.write_text("print('hello')\n", encoding="utf-8")
            digest = RUNNER.sha256_file(target)
            (root / "scripts/release-tools.toml").write_text(
                "schema-version = 1\n\n"
                '[implementations.tool]\n'
                'path = "scripts/tool.py"\n'
                f'sha256 = "{digest}"\n',
                encoding="utf-8",
            )
            observed = RUNNER.verify_named_implementation(root, "tool")
            self.assertIn(digest, observed)

    def test_verify_implementations_verifies_every_pin(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            target = root / "scripts/tool.py"
            target.write_text("print('hello')\n", encoding="utf-8")
            digest = RUNNER.sha256_file(target)
            (root / "scripts/release-tools.toml").write_text(
                "schema-version = 1\n\n"
                '[implementations.tool]\n'
                'path = "scripts/tool.py"\n'
                f'sha256 = "{digest}"\n',
                encoding="utf-8",
            )
            observed = RUNNER.verify_implementations(root)
            self.assertIn(digest, observed["tool"])

    def test_verify_implementations_fails_closed_on_missing_table(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            (root / "scripts/release-tools.toml").write_text(
                "schema-version = 1\n", encoding="utf-8"
            )
            with self.assertRaisesRegex(
                RUNNER.ReleaseError, "missing table: implementations"
            ):
                RUNNER.verify_implementations(root)

    def test_verify_implementations_fails_closed_on_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            (root / "scripts/tool.py").write_text("print('hello')\n", encoding="utf-8")
            (root / "scripts/release-tools.toml").write_text(
                "schema-version = 1\n\n"
                '[implementations.tool]\n'
                'path = "scripts/tool.py"\n'
                f'sha256 = "{"0" * 64}"\n',
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RUNNER.ReleaseError, "hash mismatch"):
                RUNNER.verify_implementations(root)

    def test_release_tools_toml_has_no_retired_producer_tables(self) -> None:
        # RFC 017 R3/R5: the canonical/noncanonical producer distinction, the
        # image pin, the base-component hashes, and the platform-keyed
        # host-tool tables are all retired -- confirm none survive in the
        # real, committed manifest.
        root = SCRIPT.resolve().parents[1]
        with (root / "scripts/release-tools.toml").open("rb") as file:
            document = tomllib.load(file)
        for retired_table in (
            "producer",
            "canonical-tools",
            "canonical-tool-artifacts",
            "canonical-base-components",
            "supported-platforms",
            "supported-host-tools",
        ):
            self.assertNotIn(retired_table, document)
        self.assertNotIn("canonical-producer", document.get("implementations", {}))

    def test_canonical_producer_script_is_deleted(self) -> None:
        self.assertFalse((SCRIPTS / "canonical-producer.sh").exists())

    def test_toolchain_identity_returns_every_r4_field(self) -> None:
        # RC-3: toolchain_identity() shells out to git/python3/cargo/rustc/mdbook,
        # but the source-integrity CI job that runs this suite does not install
        # a Rust toolchain or mdBook. Stub command_version and (N3 Part B)
        # rustc_target_triple -- the latter shells out to `rustc -vV` on its
        # own, not through command_version -- rather than depend on any of
        # those binaries being present in whatever environment runs this
        # test. The field-presence assertion needs no real subprocess.
        original_command_version = RUNNER.command_version
        original_target_triple = RUNNER.rustc_target_triple
        RUNNER.command_version = lambda command: f"stub {command[0]} 0.0.0"
        RUNNER.rustc_target_triple = lambda: "stub-target-triple"
        try:
            identity = RUNNER.toolchain_identity()
        finally:
            RUNNER.command_version = original_command_version
            RUNNER.rustc_target_triple = original_target_triple
        for field in (
            "platform",
            "target_triple",
            "git_version",
            "python_version",
            "zlib_version",
            "locale",
            "timezone",
            "cargo_version",
            "rustc_version",
            "mdbook_version",
        ):
            self.assertIn(field, identity)
            self.assertTrue(identity[field], f"{field} must not be empty")

    # N3 Part A — command_version must not merge stderr into the parsed/evidence value.

    def test_command_version_ignores_stderr_diagnostics_in_returned_value(self) -> None:
        script = (
            "import sys; "
            "sys.stderr.write('warning: something noisy\\n'); "
            "sys.stdout.write('tool 1.2.3')"
        )
        output = RUNNER.command_version([sys.executable, "-c", script])
        self.assertEqual(output, "tool 1.2.3")

    def test_command_version_failure_message_includes_stderr(self) -> None:
        script = "import sys; sys.stderr.write('boom: missing config\\n'); sys.exit(1)"
        with self.assertRaisesRegex(RUNNER.ReleaseError, "boom: missing config"):
            RUNNER.command_version([sys.executable, "-c", script])

    def test_command_version_fails_closed_when_binary_is_missing(self) -> None:
        with self.assertRaisesRegex(RUNNER.ReleaseError, "required tool is unavailable"):
            RUNNER.command_version(["localcache-nonexistent-tool-xyz"])

    # N3 Part B — target_triple must come from `rustc -vV`'s `host:` line.

    def test_rustc_target_triple_reads_the_host_line(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fake_rustc = Path(directory) / "rustc"
            fake_rustc.write_text(
                "#!/bin/sh\n"
                "echo 'rustc 1.99.0 (deadbeef 2026-01-01)'\n"
                "echo 'binary: rustc'\n"
                "echo 'commit-hash: deadbeef'\n"
                "echo 'host: x86_64-unknown-linux-gnu'\n"
                "echo 'release: 1.99.0'\n",
                encoding="utf-8",
            )
            fake_rustc.chmod(0o755)
            original_path = os.environ.get("PATH", "")
            os.environ["PATH"] = f"{directory}{os.pathsep}{original_path}"
            try:
                triple = RUNNER.rustc_target_triple()
            finally:
                os.environ["PATH"] = original_path
            self.assertEqual(triple, "x86_64-unknown-linux-gnu")

    def test_rustc_target_triple_fails_closed_without_a_host_line(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fake_rustc = Path(directory) / "rustc"
            fake_rustc.write_text(
                "#!/bin/sh\necho 'rustc 1.99.0 (deadbeef 2026-01-01)'\n",
                encoding="utf-8",
            )
            fake_rustc.chmod(0o755)
            original_path = os.environ.get("PATH", "")
            os.environ["PATH"] = f"{directory}{os.pathsep}{original_path}"
            try:
                with self.assertRaisesRegex(RUNNER.ReleaseError, "no host: line"):
                    RUNNER.rustc_target_triple()
            finally:
                os.environ["PATH"] = original_path

    # N3 Part C — rc_eligible's inputs are computed, and the fail-closed shape holds.

    def test_source_mode_step_tracking_matches_required_source_steps_exactly(
        self,
    ) -> None:
        source_text = (SCRIPTS / "release.py").read_text(encoding="utf-8")
        start = source_text.index("def source_mode(")
        end = source_text.index("\n\n\ndef ", start)
        body = source_text[start:end]
        appended = re.findall(r'completed_steps\.append\("([^"]+)"\)', body)
        self.assertEqual(tuple(appended), RUNNER.REQUIRED_SOURCE_STEPS)

    def test_source_mode_writes_manifest_at_most_once_unconditionally(self) -> None:
        # The fail-closed property (no manifest at all on any failure, never
        # one asserting rc_eligible: false) depends on write_manifest being
        # reachable only by falling all the way through the function -- so
        # there must be exactly one call to it, and no try/except anywhere
        # in the function that could catch a failure and continue past it.
        source_text = (SCRIPTS / "release.py").read_text(encoding="utf-8")
        start = source_text.index("def source_mode(")
        end = source_text.index("\n\n\ndef ", start)
        body = source_text[start:end]
        self.assertEqual(body.count("write_manifest("), 1)
        self.assertNotIn("try:", body)
        self.assertNotIn("except", body)

    def test_security_mode_failure_marks_downstream_steps_incomplete(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "evidence"
            args = argparse.Namespace(mode="security", output_dir=output)
            error = RUNNER.ReleaseError("deliberate test failure")
            RUNNER.record_failure_summary(args, error)
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("status: FAIL", summary)
            self.assertIn("deliberate test failure", summary)
            self.assertIn("required-downstream-steps: NOT COMPLETED", summary)

    def test_msrv_mode_failure_marks_downstream_steps_incomplete(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "evidence"
            args = argparse.Namespace(mode="msrv", output_dir=output)
            error = RUNNER.ReleaseError("deliberate test failure")
            RUNNER.record_failure_summary(args, error)
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("status: FAIL", summary)
            self.assertIn("deliberate test failure", summary)
            self.assertIn("required-downstream-steps: NOT COMPLETED", summary)

    def test_doc_package_mode_failure_marks_downstream_steps_incomplete(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "evidence"
            output.mkdir()
            args = argparse.Namespace(mode="doc-package", output_dir=output)
            error = RUNNER.ReleaseError("deliberate test failure")
            RUNNER.record_failure_summary(args, error)
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("status: FAIL", summary)
            self.assertIn("deliberate test failure", summary)
            self.assertIn("required-downstream-steps: NOT COMPLETED", summary)

    def test_doc_package_mode_failure_before_output_created_is_silently_skipped(
        self,
    ) -> None:
        # Mirrors source_mode: if require_clean_commit raises before
        # output.mkdir() ever runs, there is no evidence directory to write
        # into, and record_failure_summary must not fabricate one outside
        # its intended boundary.
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "does-not-exist" / "evidence"
            args = argparse.Namespace(mode="doc-package", output_dir=output)
            error = RUNNER.ReleaseError("deliberate test failure")
            RUNNER.record_failure_summary(args, error)
            self.assertFalse(output.exists())

    def test_verify_declared_toolchain_passes_on_exact_match(self) -> None:
        RUNNER.verify_declared_toolchain(
            "rustc 1.85.0 (4d91de4e4 2025-02-17)",
            "cargo 1.85.0 (d73d2caf9 2024-12-31)",
            "1.85",
        )  # must not raise

    def test_verify_declared_toolchain_fails_closed_on_rustc_mismatch(self) -> None:
        with self.assertRaisesRegex(RUNNER.ReleaseError, "active rustc"):
            RUNNER.verify_declared_toolchain(
                "rustc 1.97.1 (8bab26f4f 2026-07-14)",
                "cargo 1.85.0 (d73d2caf9 2024-12-31)",
                "1.85",
            )

    def test_verify_declared_toolchain_fails_closed_on_cargo_mismatch(self) -> None:
        with self.assertRaisesRegex(RUNNER.ReleaseError, "active cargo"):
            RUNNER.verify_declared_toolchain(
                "rustc 1.85.0 (4d91de4e4 2025-02-17)",
                "cargo 1.97.1 (c980f4866 2026-06-30)",
                "1.85",
            )

    def test_verify_declared_toolchain_rejects_prefix_collision(self) -> None:
        # "1.85" must not match "1.850" or similar -- the trailing dot in the
        # comparison exists specifically to prevent this.
        with self.assertRaisesRegex(RUNNER.ReleaseError, "active rustc"):
            RUNNER.verify_declared_toolchain(
                "rustc 1.850.0 (bogus 2026-01-01)",
                "cargo 1.85.0 (d73d2caf9 2024-12-31)",
                "1.85",
            )

    def test_matching_installed_toolchains_matches_dotted_prefix(self) -> None:
        listing = "1.85.0-x86_64-unknown-linux-gnu (default)\nstable-x86_64-unknown-linux-gnu\n"
        self.assertEqual(
            RUNNER.matching_installed_toolchains(listing, "1.85"), ["1.85.0"]
        )

    def test_matching_installed_toolchains_rejects_prefix_collision(self) -> None:
        # "1.85" must not match "1.850.0-..." -- same hazard as
        # verify_declared_toolchain's own prefix-collision guard.
        listing = "1.850.0-x86_64-unknown-linux-gnu\n"
        self.assertEqual(RUNNER.matching_installed_toolchains(listing, "1.85"), [])

    def test_matching_installed_toolchains_empty_when_absent(self) -> None:
        listing = "stable-x86_64-unknown-linux-gnu\n"
        self.assertEqual(RUNNER.matching_installed_toolchains(listing, "1.85"), [])

    def test_matching_installed_toolchains_reports_every_ambiguous_match(self) -> None:
        listing = "1.85.0-x86_64-unknown-linux-gnu\n1.85.1-x86_64-unknown-linux-gnu\n"
        self.assertEqual(
            RUNNER.matching_installed_toolchains(listing, "1.85"),
            ["1.85.0", "1.85.1"],
        )

    def test_require_declared_toolchain_installed_fails_closed_when_rustup_unavailable(
        self,
    ) -> None:
        original_path = os.environ.get("PATH", "")
        os.environ["PATH"] = ""
        try:
            with self.assertRaisesRegex(
                RUNNER.ReleaseError, "cannot list rustup toolchains"
            ):
                RUNNER.require_declared_toolchain_installed("1.85")
        finally:
            os.environ["PATH"] = original_path

    def test_release_mode_scopes_msrv_gate_to_declared_toolchain_via_rustup_run(
        self,
    ) -> None:
        # RC-2: `release` must invoke `msrv` under the exact resolved MSRV
        # toolchain, independent of the ambient toolchain -- "rustup run"
        # rejects a bare two-component version like "1.85" outright, so this
        # must be the *resolved* toolchain, not the raw declared string --
        # and must fail closed rather than skip if that toolchain is absent.
        source_text = (SCRIPTS / "release.py").read_text(encoding="utf-8")
        # The step list moved into `release_steps` (RFC 023 Q1b), which sits
        # directly above `release_mode`; slicing from it covers both.
        release_mode_text = source_text[source_text.index("def release_steps(") :]
        self.assertIn(
            'command = ["rustup", "run", resolved_toolchain, *command]',
            release_mode_text,
        )
        self.assertIn(
            "resolved_toolchain = require_declared_toolchain_installed(declared)",
            release_mode_text,
        )

    def test_rc_eligibility_requires_all_three_conditions(self) -> None:
        cases = [
            # (clean_worktree, all_required_gates_passed, evidence_complete, expected)
            (True, True, True, True),
            (False, True, True, False),
            (True, False, True, False),
            (True, True, False, False),
            (False, False, False, False),
        ]
        for clean, gates, evidence, expected in cases:
            with self.subTest(clean=clean, gates=gates, evidence=evidence):
                self.assertEqual(
                    RUNNER.rc_eligibility(
                        clean_worktree=clean,
                        all_required_gates_passed=gates,
                        evidence_complete=evidence,
                    ),
                    expected,
                )

    def test_main_finalizes_summary_on_an_unexpected_exception_type(self) -> None:
        # An OSError (or any exception outside the three expected gate-error
        # types) must still finalize summary.log as FAIL, not propagate
        # uncaught and leave it reading "status: RUNNING" forever.
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "evidence"

            def raising_handler(args: argparse.Namespace) -> int:
                append_summary_path = Path(args.output_dir) / "summary.log"
                append_summary_path.parent.mkdir(parents=True, exist_ok=True)
                append_summary_path.write_text("status: RUNNING\n", encoding="utf-8")
                raise OSError("deliberate unexpected failure")

            original_parse_args = RUNNER.parse_args
            RUNNER.parse_args = lambda argv: argparse.Namespace(
                mode="security", output_dir=output, handler=raising_handler
            )
            try:
                status = RUNNER.main(["security", "--output-dir", str(output)])
            finally:
                RUNNER.parse_args = original_parse_args
            self.assertEqual(status, 1)
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("status: FAIL", summary)
            self.assertIn("deliberate unexpected failure", summary)
            self.assertIn("required-downstream-steps: NOT COMPLETED", summary)

    def test_ci_identity_is_all_none_outside_github_actions(self) -> None:
        original = os.environ.pop("GITHUB_ACTIONS", None)
        try:
            identity = RUNNER.ci_identity()
        finally:
            if original is not None:
                os.environ["GITHUB_ACTIONS"] = original
        self.assertEqual(
            identity,
            {"ci_run_id": None, "ci_job": None, "ci_workflow": None, "ci_sha": None},
        )

    def test_ci_identity_reads_github_actions_env_vars(self) -> None:
        saved = {
            key: os.environ.get(key)
            for key in ("GITHUB_ACTIONS", "GITHUB_RUN_ID", "GITHUB_JOB", "GITHUB_WORKFLOW", "GITHUB_SHA")
        }
        os.environ["GITHUB_ACTIONS"] = "true"
        os.environ["GITHUB_RUN_ID"] = "111"
        os.environ["GITHUB_JOB"] = "archive"
        os.environ["GITHUB_WORKFLOW"] = "CI"
        os.environ["GITHUB_SHA"] = "cafef00d"
        try:
            identity = RUNNER.ci_identity()
        finally:
            for key, value in saved.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value
        self.assertEqual(
            identity,
            {
                "ci_run_id": "111",
                "ci_job": "archive",
                "ci_workflow": "CI",
                "ci_sha": "cafef00d",
            },
        )

    @staticmethod
    def _require_job_args(**overrides: str) -> list[str]:
        """`--require-job` args covering every `CI_REQUIRED_JOBS` entry,
        defaulting each to "success" unless overridden by name."""
        args: list[str] = []
        for name in RUNNER.CI_REQUIRED_JOBS:
            result = overrides.get(name, "success")
            args += ["--require-job", f"{name}={result}"]
        return args

    def test_aggregate_ci_passes_when_jobs_succeed_and_evidence_binds(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / "manifest.json"
            manifest.write_text(
                '{"status": "pass", "ci_run_id": "42", "ci_sha": "abc123"}',
                encoding="utf-8",
            )
            output = root / "out"
            status = RUNNER.main(
                [
                    "aggregate-ci",
                    "--output-dir",
                    str(output),
                    "--run-id",
                    "42",
                    "--sha",
                    "abc123",
                    *self._require_job_args(),
                    "--evidence-manifest",
                    str(manifest),
                ]
            )
            self.assertEqual(status, 0)
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("status: PASS", summary)
            self.assertIn("evidence-binding: PASS", summary)

    def test_aggregate_ci_fails_closed_on_a_failed_required_job(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "out"
            status = RUNNER.main(
                [
                    "aggregate-ci",
                    "--output-dir",
                    str(output),
                    "--run-id",
                    "42",
                    "--sha",
                    "abc123",
                    *self._require_job_args(matrix="failure"),
                ]
            )
            self.assertNotEqual(status, 0)
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("status: FAIL", summary)
            self.assertIn("matrix", summary)
            self.assertNotIn("evidence-binding: PASS", summary)

    def test_aggregate_ci_rejects_a_malformed_require_job_value(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "out"
            status = RUNNER.main(
                [
                    "aggregate-ci",
                    "--output-dir",
                    str(output),
                    "--run-id",
                    "42",
                    "--sha",
                    "abc123",
                    "--require-job",
                    "no-equals-sign",
                ]
            )
            self.assertNotEqual(status, 0)

    def test_aggregate_ci_fails_closed_when_a_required_job_is_omitted(self) -> None:
        # RC-1 (2026-07-29 M6e RC-construction review): before this fix,
        # supplying only one --require-job value verified only that job and
        # silently ignored the rest. Reproduces the exact repro from the
        # review, which used to exit 0.
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "out"
            status = RUNNER.main(
                [
                    "aggregate-ci",
                    "--output-dir",
                    str(output),
                    "--run-id",
                    "r1",
                    "--sha",
                    "deadbeef",
                    "--require-job",
                    "source=success",
                ]
            )
            self.assertNotEqual(status, 0)
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("omitted from this aggregation", summary)
            for name in RUNNER.CI_REQUIRED_JOBS:
                self.assertIn(name, summary)

    def test_aggregate_ci_fails_closed_on_run_id_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / "manifest.json"
            manifest.write_text(
                '{"status": "pass", "ci_run_id": "wrong-run", "ci_sha": "abc123"}',
                encoding="utf-8",
            )
            output = root / "out"
            status = RUNNER.main(
                [
                    "aggregate-ci",
                    "--output-dir",
                    str(output),
                    "--run-id",
                    "42",
                    "--sha",
                    "abc123",
                    *self._require_job_args(),
                    "--evidence-manifest",
                    str(manifest),
                ]
            )
            self.assertNotEqual(status, 0)
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("does not match this workflow run", summary)
            self.assertNotIn("evidence-binding: PASS", summary)

    def test_aggregate_ci_accepts_legacy_commit_field_without_ci_sha(self) -> None:
        # `artifact`-mode evidence has no `ci_sha` field (see `artifact_mode`'s
        # manifest); the aggregator must still be able to bind it via `commit`.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / "manifest.json"
            manifest.write_text(
                '{"status": "pass", "ci_run_id": "42", "commit": "abc123"}',
                encoding="utf-8",
            )
            output = root / "out"
            status = RUNNER.main(
                [
                    "aggregate-ci",
                    "--output-dir",
                    str(output),
                    "--run-id",
                    "42",
                    "--sha",
                    "abc123",
                    *self._require_job_args(),
                    "--evidence-manifest",
                    str(manifest),
                ]
            )
            self.assertEqual(status, 0)

    def test_aggregate_ci_fails_closed_on_missing_manifest_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "out"
            status = RUNNER.main(
                [
                    "aggregate-ci",
                    "--output-dir",
                    str(output),
                    "--run-id",
                    "42",
                    "--sha",
                    "abc123",
                    *self._require_job_args(),
                    "--evidence-manifest",
                    str(Path(directory) / "does-not-exist.json"),
                ]
            )
            self.assertNotEqual(status, 0)
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("cannot read evidence manifest", summary)

    def test_ci_required_jobs_matches_release_gate_needs_in_ci_yaml(self) -> None:
        # Guards exactly the drift class CI_REQUIRED_JOBS's docstring warns
        # about: a job present in release-gate's `needs:` but absent from
        # the canonical set would silently stop being required.
        ci_yaml = (
            SCRIPTS.parent / ".github" / "workflows" / "ci.yaml"
        ).read_text(encoding="utf-8")
        match = re.search(
            r"release-gate:.*?needs:\n((?:\s+- [a-z-]+\n)+)", ci_yaml, re.DOTALL
        )
        assert match is not None, "could not locate release-gate's needs: list"
        needs = re.findall(r"- ([a-z-]+)", match.group(1))
        self.assertEqual(set(needs), set(RUNNER.CI_REQUIRED_JOBS))

    def test_release_gates_matches_rc1_specified_order(self) -> None:
        self.assertEqual(
            RUNNER.RELEASE_GATES, ("source", "msrv", "doc-package", "security")
        )

    def test_release_mode_failure_marks_downstream_steps_incomplete(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "evidence"
            args = argparse.Namespace(mode="release", output_dir=output)
            error = RUNNER.ReleaseError("deliberate test failure")
            RUNNER.record_failure_summary(args, error)
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("status: FAIL", summary)
            self.assertIn("deliberate test failure", summary)
            self.assertIn("required-downstream-steps: NOT COMPLETED", summary)

    def test_source_and_security_manifests_embed_ci_identity_fields(self) -> None:
        source_text = (SCRIPTS / "release.py").read_text(encoding="utf-8")
        self.assertIn("**ci_identity()", source_text)

    @staticmethod
    def repository(parent: Path) -> Path:
        root = parent / "repository"
        root.mkdir()
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        subprocess.run(
            ["git", "-C", str(root), "config", "user.name", "Fixture"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(root), "config", "user.email", "fixture@example.invalid"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(root), "config", "commit.gpgsign", "false"],
            check=True,
        )
        (root / "tracked.txt").write_text("original\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(root), "add", "tracked.txt"], check=True)
        subprocess.run(
            ["git", "-C", str(root), "commit", "-qm", "fixture"],
            check=True,
            env={
                **os.environ,
                "GIT_AUTHOR_DATE": "2026-01-02T03:04:05Z",
                "GIT_COMMITTER_DATE": "2026-01-02T03:04:05Z",
            },
        )
        return root


class FreshResolutionTests(unittest.TestCase):
    """RFC 023 R6.2 / Q1b: the fresh-resolution drift check.

    The command runner (`run_gate`), the toolchain probe (`command_version`),
    and the repository root are replaced, so nothing here needs Cargo, a
    network, or the 1.85 toolchain.
    """

    DECLARED = "1.85"
    RUSTC_OK = "rustc 1.85.0 (4d91de4e4 2025-02-17)"
    CARGO_OK = "cargo 1.85.0 (d73d2caf9 2024-12-31)"
    CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"
    MATRIX = ["python3", "scripts/feature_matrix.py", "--run-msrv"]

    @staticmethod
    def fixture_repository(parent: Path) -> Path:
        root = parent / "repository"
        root.mkdir()
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        for key, value in (
            ("user.name", "Fixture"),
            ("user.email", "fixture@example.invalid"),
            ("commit.gpgsign", "false"),
        ):
            subprocess.run(["git", "-C", str(root), "config", key, value], check=True)
        files = {
            "Cargo.toml": '[workspace.package]\nrust-version = "1.85"\n',
            "Cargo.lock": "# repository lockfile\n",
            "crates/a/Cargo.lock": "# nested lockfile\n",
            "crates/a/src/lib.rs": "// lib\n",
            "target/debug/build.txt": "build output\n",
            "gone.txt": "tracked, then deleted from the working tree\n",
        }
        for relative, content in files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
        subprocess.run(["git", "-C", str(root), "add", "-f", "."], check=True)
        subprocess.run(["git", "-C", str(root), "commit", "-qm", "fixture"], check=True)
        (root / "gone.txt").unlink()
        (root / "untracked.txt").write_text("never tracked\n", encoding="utf-8")
        return root

    @classmethod
    def metadata_document(cls) -> dict[str, object]:
        def package(name: str, version: str, source: str | None, rust_version: str | None):
            return {
                "name": name,
                "version": version,
                "source": source,
                "rust_version": rust_version,
            }

        return {
            "packages": [
                package("zeta", "2.0.0", cls.CRATES_IO, None),
                package("declared", "1.0.0", cls.CRATES_IO, "1.70"),
                package("alpha", "0.3.1", cls.CRATES_IO, None),
                package("sparse-undeclared", "0.1.0", "sparse+https://index.crates.io/", None),
                package("workspace-member", "0.21.4", None, None),
                package("git-dep", "0.1.0", "git+https://example.invalid/repo#abc", None),
                package("other-registry", "0.1.0", "registry+https://example.invalid/index", None),
                package("rusqlite", "0.40.1", cls.CRATES_IO, None),
                package("libsqlite3-sys", "0.38.1", cls.CRATES_IO, None),
                package("localcache", "0.21.4", None, "1.85"),
                package("localcache-cli", "0.21.4", None, "1.85"),
            ]
        }

    # -- the tracked copy -----------------------------------------------------

    def test_tracked_copy_leaves_out_every_lockfile_and_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture_repository(Path(directory))
            destination = Path(directory) / "copy"
            copied = RUNNER.copy_tracked_tree(
                root, destination, RUNNER.tracked_files(root)
            )
            present = sorted(
                str(path.relative_to(destination))
                for path in destination.rglob("*")
                if path.is_file()
            )
            self.assertEqual(present, ["Cargo.toml", "crates/a/src/lib.rs"])
            self.assertEqual(copied, 2)
            # Untracked and deleted-tracked files are never copied.
            self.assertFalse((destination / "untracked.txt").exists())
            self.assertFalse((destination / "gone.txt").exists())

    # -- the undeclared-package parser ---------------------------------------

    def test_undeclared_parser_lists_only_crates_io_packages_without_rust_version(
        self,
    ) -> None:
        self.assertEqual(
            RUNNER.undeclared_rust_version_packages(self.metadata_document()),
            [
                ("alpha", "0.3.1"),
                ("libsqlite3-sys", "0.38.1"),
                ("rusqlite", "0.40.1"),
                ("sparse-undeclared", "0.1.0"),
                ("zeta", "2.0.0"),
            ],
        )

    def test_undeclared_parser_fails_closed_on_malformed_metadata(self) -> None:
        with self.assertRaisesRegex(RUNNER.ReleaseError, "expected packages"):
            RUNNER.undeclared_rust_version_packages({})

    def test_resolved_versions_reports_absent_and_multiple(self) -> None:
        document = {
            "packages": [
                {"name": "rusqlite", "version": "0.39.0"},
                {"name": "rusqlite", "version": "0.40.1"},
            ]
        }
        self.assertEqual(
            RUNNER.resolved_versions(document, ("rusqlite", "libsqlite3-sys")),
            {"rusqlite": "0.39.0, 0.40.1", "libsqlite3-sys": None},
        )

    # -- the lockfile guard ---------------------------------------------------

    def test_lockfile_guard_fails_when_the_hash_changes(self) -> None:
        RUNNER.require_lockfile_unchanged("abc", "abc")
        with self.assertRaisesRegex(RUNNER.ReleaseError, "Cargo.lock changed"):
            RUNNER.require_lockfile_unchanged("abc", "abd")

    def test_lockfile_digest_is_the_sha256_or_absent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.assertEqual(RUNNER.lockfile_digest(root), "absent")
            (root / "Cargo.lock").write_bytes(b"lock\n")
            self.assertEqual(
                RUNNER.lockfile_digest(root), hashlib.sha256(b"lock\n").hexdigest()
            )

    # -- msrv --fresh, end to end with the runner mocked ---------------------

    def run_fresh(
        self,
        parent: Path,
        *,
        resolution_fails: bool = False,
        matrix_fails: bool = False,
        matrix_rewrites_repository_lock: bool = False,
        rustc: str | None = None,
    ):
        root = self.fixture_repository(parent)
        output = parent / "evidence"
        calls: list[tuple[str, list[str], Path, bool]] = []

        def fake_run_gate(logger, name, command, cwd, **_kwargs):
            # Recorded at call time: the copy is deleted afterwards.
            calls.append(
                (name, list(command), Path(cwd), (Path(cwd) / "Cargo.lock").exists())
            )
            if name == "fresh-generate-lockfile":
                if resolution_fails:
                    raise RUNNER.ReleaseError("fresh-generate-lockfile failed")
                (Path(cwd) / "Cargo.lock").write_text("# fresh\n", encoding="utf-8")
            elif name == "declared-msrv-matrix-fresh":
                if matrix_rewrites_repository_lock:
                    (root / "Cargo.lock").write_text("# changed\n", encoding="utf-8")
                if matrix_fails:
                    raise RUNNER.ReleaseError(f"{name} failed with exit status 1")
            elif name == "fresh-metadata":
                return json.dumps(self.metadata_document())
            return ""

        def fake_command_version(command):
            return rustc or self.RUSTC_OK if command[0] == "rustc" else self.CARGO_OK

        args = argparse.Namespace(mode="msrv", output_dir=output, fresh=True)
        error = None
        with mock.patch.object(RUNNER, "repository_root", lambda: root), mock.patch.object(
            RUNNER, "run_gate", fake_run_gate
        ), mock.patch.object(RUNNER, "command_version", fake_command_version):
            try:
                RUNNER.msrv_mode(args)
            except RUNNER.ReleaseError as raised:
                error = raised
        return root, output, calls, error

    def summary(self, output: Path) -> str:
        return (output / "summary.log").read_text(encoding="utf-8")

    def test_fresh_passes_and_writes_the_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root, output, calls, error = self.run_fresh(Path(directory))
            self.assertIsNone(error)
            summary = self.summary(output)
            for line in (
                "context: msrv",
                "mode: fresh",
                "fresh-resolution: PASS",
                "declared-msrv-matrix-fresh: PASS",
                "undeclared-rust-version: 5 packages",
                "repository-lockfile-unchanged: PASS",
                "status: PASS",
            ):
                self.assertIn(line, summary)
            self.assertEqual(
                (output / "fresh-Cargo.lock").read_text(encoding="utf-8"), "# fresh\n"
            )
            self.assertIn(
                "rusqlite 0.40.1\n",
                (output / "fresh-undeclared-rust-version.txt").read_text(encoding="utf-8"),
            )
            manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["context"], "msrv")
            self.assertEqual(manifest["status"], "pass")
            self.assertEqual(
                manifest["fresh"],
                {
                    "rusqlite": "0.40.1",
                    "libsqlite3-sys": "0.38.1",
                    "undeclared_rust_version_count": 5,
                },
            )
            # The temporary copy is gone; the repository lockfile is untouched.
            self.assertFalse((output / "fresh-work").exists())
            self.assertEqual(
                (root / "Cargo.lock").read_text(encoding="utf-8"), "# repository lockfile\n"
            )

    def test_fresh_resolves_without_a_lockfile_then_runs_the_one_row_list(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            _root, output, calls, error = self.run_fresh(Path(directory))
            self.assertIsNone(error)
            self.assertEqual(
                [name for name, *_ in calls],
                ["fresh-generate-lockfile", "declared-msrv-matrix-fresh", "fresh-metadata"],
            )
            generate, matrix, _metadata = calls
            # Resolution starts with no Cargo.lock in the copy...
            self.assertFalse(generate[3])
            self.assertEqual(generate[1], ["cargo", "generate-lockfile"])
            # ...the rows run in the copy against the generated lockfile...
            self.assertEqual(matrix[2], output / "fresh-work")
            self.assertTrue(matrix[3])
            # ...and are exactly `feature_matrix.py --run-msrv`: no second list.
            self.assertEqual(matrix[1], self.MATRIX)

    def test_fresh_refuses_any_toolchain_but_the_declared_one(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            _root, output, calls, error = self.run_fresh(
                Path(directory), rustc="rustc 1.98.1 (48a229cea 2026-09-01)"
            )
            self.assertIsNotNone(error)
            self.assertIn("does not match declared MSRV", str(error))
            self.assertEqual(calls, [])
            self.assertFalse((output / "fresh-work").exists())

    def test_fresh_row_failure_still_lists_the_undeclared_packages(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            _root, output, calls, error = self.run_fresh(
                Path(directory), matrix_fails=True
            )
            self.assertIsNotNone(error)
            self.assertIn("declared-msrv-matrix-fresh failed", str(error))
            self.assertIn("declared-msrv-matrix-fresh: FAIL", self.summary(output))
            listed = (output / "fresh-undeclared-rust-version.txt").read_text(encoding="utf-8")
            self.assertIn("rusqlite 0.40.1\n", listed)
            self.assertIn("libsqlite3-sys 0.38.1\n", listed)
            # Only a passing run writes a manifest; the copy is still removed.
            self.assertFalse((output / "manifest.json").exists())
            self.assertFalse((output / "fresh-work").exists())

    def test_fresh_resolution_failure_skips_the_rows(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            _root, output, calls, error = self.run_fresh(
                Path(directory), resolution_fails=True
            )
            self.assertIsNotNone(error)
            self.assertIn("fresh-resolution: FAIL", self.summary(output))
            self.assertEqual([name for name, *_ in calls], ["fresh-generate-lockfile"])
            self.assertFalse((output / "fresh-work").exists())

    def test_fresh_fails_when_the_repository_lockfile_changes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            _root, output, _calls, error = self.run_fresh(
                Path(directory), matrix_rewrites_repository_lock=True
            )
            self.assertIsNotNone(error)
            self.assertIn("Cargo.lock changed", str(error))
            self.assertNotIn("repository-lockfile-unchanged: PASS", self.summary(output))
            self.assertFalse((output / "manifest.json").exists())

    # -- plain msrv, and release ---------------------------------------------

    def test_plain_msrv_runs_no_fresh_step(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture_repository(Path(directory))
            output = Path(directory) / "evidence"
            names: list[str] = []

            def fake_run_gate(logger, name, command, cwd, **_kwargs):
                names.append(name)
                self.assertEqual(list(command), self.MATRIX)
                self.assertEqual(Path(cwd), root)
                return ""

            for args in (
                argparse.Namespace(mode="msrv", output_dir=output / "a"),
                argparse.Namespace(mode="msrv", output_dir=output / "b", fresh=False),
            ):
                with mock.patch.object(RUNNER, "repository_root", lambda: root), mock.patch.object(
                    RUNNER, "run_gate", fake_run_gate
                ), mock.patch.object(
                    RUNNER,
                    "command_version",
                    lambda command: self.RUSTC_OK if command[0] == "rustc" else self.CARGO_OK,
                ):
                    self.assertEqual(RUNNER.msrv_mode(args), 0)
            self.assertEqual(names, ["declared-msrv-matrix", "declared-msrv-matrix"])
            summary = self.summary(output / "a")
            self.assertNotIn("mode: fresh", summary)
            self.assertNotIn("fresh", (output / "a" / "manifest.json").read_text(encoding="utf-8"))

    def test_fresh_flag_defaults_off_and_parses(self) -> None:
        self.assertFalse(RUNNER.parse_args(["msrv", "--output-dir", "x"]).fresh)
        self.assertTrue(RUNNER.parse_args(["msrv", "--fresh", "--output-dir", "x"]).fresh)

    def test_release_steps_run_fresh_right_after_locked_msrv_under_the_toolchain(
        self,
    ) -> None:
        output = Path("/out")
        steps = RUNNER.release_steps("release.py", output, "1.85.0")
        self.assertEqual(
            [name for name, *_ in steps],
            ["source", "msrv", "msrv-fresh", "doc-package", "security"],
        )
        commands = {name: command for name, command, _ in steps}
        self.assertEqual(commands["msrv"][:4], ["rustup", "run", "1.85.0", sys.executable])
        self.assertNotIn("--fresh", commands["msrv"])
        self.assertEqual(
            commands["msrv-fresh"][:4], ["rustup", "run", "1.85.0", sys.executable]
        )
        self.assertIn("--fresh", commands["msrv-fresh"])
        self.assertEqual(commands["msrv-fresh"][commands["msrv-fresh"].index("msrv")], "msrv")
        manifests = {name: path for name, _, path in steps}
        self.assertEqual(manifests["msrv-fresh"], output / "msrv" / "fresh" / "manifest.json")
        # The other gates keep their locations.
        self.assertEqual(manifests["source"], output / "source" / "evidence" / "manifest.json")
        self.assertEqual(manifests["msrv"], output / "msrv" / "manifest.json")

    def run_release(self, parent: Path, *, fresh_fails: bool):
        root = parent / "repository"
        root.mkdir()
        (root / "Cargo.toml").write_text(
            '[workspace.package]\nrust-version = "1.85"\n', encoding="utf-8"
        )
        output = parent / "release-evidence"
        order: list[str] = []
        manifests = {
            "source": {
                "commit": "c" * 40,
                "version": "0.21.4",
                "rc_eligible": True,
                "archive": "a.tar.gz",
                "archive_uncompressed_sha256": "u" * 64,
                "archive_compressed_sha256_advisory": "z" * 64,
                "toolchain_identity": {},
                "tool_versions": {},
            },
            "msrv": {
                "declared_rust_version": "1.85",
                "rustc_version": self.RUSTC_OK,
                "cargo_version": self.CARGO_OK,
            },
            "msrv-fresh": {"fresh": {"rusqlite": "0.39.0", "libsqlite3-sys": "0.37.0"}},
            "doc-package": {"packages": {}},
            "security": {"advisories_evidence": "advisories"},
        }

        def fake_run_gate(logger, name, command, cwd, **_kwargs):
            order.append(name)
            if name == "msrv-fresh" and fresh_fails:
                raise RUNNER.ReleaseError("msrv-fresh failed with exit status 1")
            destination = Path(command[command.index("--output-dir") + 1])
            manifest = (
                destination / "evidence" / "manifest.json"
                if name == "source"
                else destination / "manifest.json"
            )
            manifest.parent.mkdir(parents=True, exist_ok=True)
            manifest.write_text(json.dumps(manifests[name]), encoding="utf-8")
            return ""

        error = None
        args = argparse.Namespace(mode="release", output_dir=output)
        with mock.patch.object(RUNNER, "repository_root", lambda: root), mock.patch.object(
            RUNNER, "run_gate", fake_run_gate
        ), mock.patch.object(
            RUNNER, "require_declared_toolchain_installed", lambda declared: "1.85.0"
        ):
            try:
                RUNNER.release_mode(args)
            except RUNNER.ReleaseError as raised:
                error = raised
        return output, order, error

    def test_release_runs_fresh_after_locked_msrv_and_records_it(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output, order, error = self.run_release(Path(directory), fresh_fails=False)
            self.assertIsNone(error)
            self.assertEqual(
                order, ["source", "msrv", "msrv-fresh", "doc-package", "security"]
            )
            manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(
                manifest["fresh_resolution"],
                {"rusqlite": "0.39.0", "libsqlite3-sys": "0.37.0"},
            )
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("msrv: PASS", summary)
            self.assertIn("msrv-fresh: PASS", summary)

    def test_release_fails_on_fresh_drift_and_runs_nothing_after_it(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output, order, error = self.run_release(Path(directory), fresh_fails=True)
            self.assertIsNotNone(error)
            self.assertIn("msrv-fresh failed", str(error))
            self.assertEqual(order, ["source", "msrv", "msrv-fresh"])
            self.assertFalse((output / "manifest.json").exists())
            self.assertNotIn("msrv-fresh: PASS", (output / "summary.log").read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
