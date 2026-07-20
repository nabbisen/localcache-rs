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
