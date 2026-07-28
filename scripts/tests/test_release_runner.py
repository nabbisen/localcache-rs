import argparse
import importlib.util
import os
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path


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

    @staticmethod
    def _version_reference_fixture(root: Path, *, version: str) -> None:
        for relative in RUNNER.VERSION_REFERENCE_TARGETS:
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
        identity = RUNNER.toolchain_identity()
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
                    "--require-job",
                    "matrix=success",
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
                    "--require-job",
                    "matrix=failure",
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
                    "--evidence-manifest",
                    str(Path(directory) / "does-not-exist.json"),
                ]
            )
            self.assertNotEqual(status, 0)
            summary = (output / "summary.log").read_text(encoding="utf-8")
            self.assertIn("cannot read evidence manifest", summary)

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


if __name__ == "__main__":
    unittest.main()
