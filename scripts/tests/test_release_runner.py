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

    def test_workspace_version_rejects_stale_cli_dependency(self) -> None:
        document = {
            "packages": [
                {"name": "localcache", "version": "1.2.3", "dependencies": []},
                {
                    "name": "localcache-cli",
                    "version": "1.2.3",
                    "dependencies": [{"name": "localcache", "req": "^1.2.2"}],
                },
            ]
        }
        with self.assertRaisesRegex(RUNNER.ReleaseError, "does not match"):
            RUNNER.workspace_version(document)

        document["packages"][1]["dependencies"][0]["req"] = "^1.2.3"
        self.assertEqual(RUNNER.workspace_version(document), "1.2.3")

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
                expected_sha256="a" * 64,
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
                expected_sha256="a" * 64,
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
                    "--expected-sha256",
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

    def test_verify_named_implementation_does_not_check_host_tools(self) -> None:
        # Unlike verify_tool_manifest, this must not require the canonical
        # producer's pinned rustc/cargo/python/git/mdbook versions — it is
        # used by gates (security) that run on any CI runner.
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
            # No [producer], [canonical-tools], or [supported-host-tools]
            # table exists in this fixture; a passing result proves those
            # tables were never consulted.
            observed = RUNNER.verify_named_implementation(root, "tool")
            self.assertIn(digest, observed)

    def test_current_platform_key_is_lowercase_os_dash_machine(self) -> None:
        key = RUNNER.current_platform_key()
        self.assertRegex(key, r"^[a-z0-9]+-[a-z0-9_]+$")

    def test_verify_tool_policy_requires_hash_for_canonical_entries(self) -> None:
        policy = {"command": "python3", "version": "irrelevant"}
        with self.assertRaisesRegex(RUNNER.ReleaseError, "incomplete tool policy"):
            RUNNER.verify_tool_policy("python", policy, require_hash=True)

    def test_verify_tool_policy_rejects_hash_pin_on_noncanonical_entry(self) -> None:
        # A hash field on a [supported-host-tools] entry would silently
        # reintroduce the single-workstation-pin defect this item exists to
        # remove -- reject it outright rather than ignoring it.
        policy = {"command": "python3", "version": "irrelevant", "sha256": "0" * 64}
        with self.assertRaisesRegex(RUNNER.ReleaseError, "must not pin a binary hash"):
            RUNNER.verify_tool_policy("python", policy, require_hash=False)

    def test_verify_tool_policy_fails_closed_on_version_mismatch(self) -> None:
        policy = {"command": "python3", "version": "Python 0.0.0"}
        with self.assertRaisesRegex(RUNNER.ReleaseError, "version mismatch"):
            RUNNER.verify_tool_policy("python", policy, require_hash=False)

    def test_verify_tool_manifest_noncanonical_happy_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            platform_key = RUNNER.current_platform_key()
            cargo_version = RUNNER.command_version(["cargo", "--version"])
            rust_version = RUNNER.command_version(["rustc", "--version"])
            python_version = RUNNER.command_version(["python3", "--version"])
            (root / "scripts/release-tools.toml").write_text(
                "schema-version = 1\n\n"
                "[producer]\n"
                f'cargo = "{cargo_version}"\n'
                f'rust = "{rust_version}"\n\n'
                "[implementations]\n\n"
                "[supported-platforms]\n"
                f'claimed = ["{platform_key}"]\n\n'
                f"[supported-host-tools.{platform_key}.python]\n"
                'command = "python3"\n'
                f'version = "{python_version}"\n',
                encoding="utf-8",
            )
            observed = RUNNER.verify_tool_manifest(root, canonical=False)
            self.assertEqual(observed["python"], python_version)
            self.assertEqual(observed["cargo"], cargo_version)
            self.assertEqual(observed["rustc"], rust_version)

    def test_verify_tool_manifest_fails_closed_for_unclaimed_platform(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            (root / "scripts/release-tools.toml").write_text(
                "schema-version = 1\n\n"
                "[producer]\n\n"
                "[implementations]\n\n"
                "[supported-platforms]\n"
                'claimed = ["nonexistent-platform-9999"]\n',
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RUNNER.ReleaseError, "not in the claimed"):
                RUNNER.verify_tool_manifest(root, canonical=False)

    def test_verify_tool_manifest_fails_closed_on_empty_claimed_list(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            (root / "scripts/release-tools.toml").write_text(
                "schema-version = 1\n\n"
                "[producer]\n\n"
                "[implementations]\n\n"
                "[supported-platforms]\nclaimed = []\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RUNNER.ReleaseError, "non-empty claimed"):
                RUNNER.verify_tool_manifest(root, canonical=False)

    def test_verify_tool_manifest_fails_closed_without_claimed_table(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            (root / "scripts/release-tools.toml").write_text(
                "schema-version = 1\n\n[producer]\n\n[implementations]\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RUNNER.ReleaseError, "non-empty claimed"):
                RUNNER.verify_tool_manifest(root, canonical=False)

    def test_verify_tool_manifest_fails_closed_on_missing_platform_tools_table(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            platform_key = RUNNER.current_platform_key()
            (root / "scripts/release-tools.toml").write_text(
                "schema-version = 1\n\n"
                "[producer]\n\n"
                "[implementations]\n\n"
                "[supported-platforms]\n"
                f'claimed = ["{platform_key}"]\n\n'
                "[supported-host-tools.some-other-platform]\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                RUNNER.ReleaseError, "no supported-host-tools policy"
            ):
                RUNNER.verify_tool_manifest(root, canonical=False)

    def test_release_tools_toml_claims_this_platform(self) -> None:
        # The real, committed policy must be usable on the platform this
        # test suite is actually running on -- otherwise the noncanonical
        # path is unusable everywhere, repeating the defect this item fixes.
        root = SCRIPT.resolve().parents[1]
        RUNNER.verify_tool_manifest(root, canonical=False)

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

    def test_rc_eligibility_requires_both_signals(self) -> None:
        cases = [
            # (noncanonical, env value, expected)
            (False, "1", True),
            (True, "1", False),  # --noncanonical always disqualifies
            (False, None, False),  # no wrapper attestation
            (False, "0", False),  # wrong value is not truthy
            (False, "true", False),  # only the exact string "1" counts
            (True, None, False),
        ]
        for noncanonical, env_value, expected in cases:
            with self.subTest(noncanonical=noncanonical, env_value=env_value):
                original = os.environ.pop("RFC009_RC_ELIGIBLE", None)
                try:
                    if env_value is not None:
                        os.environ["RFC009_RC_ELIGIBLE"] = env_value
                    self.assertEqual(
                        RUNNER.rc_eligibility(noncanonical=noncanonical), expected
                    )
                finally:
                    os.environ.pop("RFC009_RC_ELIGIBLE", None)
                    if original is not None:
                        os.environ["RFC009_RC_ELIGIBLE"] = original

    def test_canonical_wrapper_sets_rc_eligible_exclusively(self) -> None:
        wrapper = (SCRIPTS / "canonical-producer.sh").read_text(encoding="utf-8")
        self.assertIn("RFC009_RC_ELIGIBLE=1", wrapper)

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

    def test_canonical_wrapper_has_no_release_action(self) -> None:
        wrapper = (SCRIPTS / "canonical-producer.sh").read_text(encoding="utf-8")
        self.assertIn(
            "docker.io/library/rust@sha256:"
            "389c1ae98c20fbcadca68a685482749267cec3c90893ae4671c5a37cc894c416",
            wrapper,
        )
        for forbidden in ("cargo publish", "git push", "git tag", "gh release"):
            self.assertNotIn(forbidden, wrapper)

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
