import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "source_integrity.py"
SPEC = importlib.util.spec_from_file_location("source_integrity", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
SOURCE_INTEGRITY = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = SOURCE_INTEGRITY
SPEC.loader.exec_module(SOURCE_INTEGRITY)


class SourceIntegrityTests(unittest.TestCase):
    def test_accepts_virtual_workspace_with_nested_crates(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write(
                root / "Cargo.toml",
                """
[workspace]
members = ["crates/library", "crates/cli"]
""",
            )
            self.write(
                root / "crates/library/Cargo.toml",
                """
[package]
name = "sample"
version = "0.1.0"

[[bench]]
name = "throughput"
""",
            )
            self.write(root / "crates/library/src/lib.rs", "")
            self.write(root / "crates/library/benches/throughput.rs", "")
            self.write(
                root / "crates/cli/Cargo.toml",
                """
[package]
name = "sample-cli"
version = "0.1.0"
""",
            )
            self.write(root / "crates/cli/src/main.rs", "")

            manifests, targets = SOURCE_INTEGRITY.verify(root)

            self.assertEqual(len(manifests), 3)
            self.assertEqual(
                {(target.kind, target.name) for target in targets},
                {
                    ("lib", "sample"),
                    ("bench", "throughput"),
                    ("bin", "sample-cli"),
                },
            )

    def test_accepts_workspace_with_explicit_targets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write(
                root / "Cargo.toml",
                """
[workspace]
members = [".", "cli"]

[package]
name = "sample"
version = "0.1.0"

[[bench]]
name = "throughput"

[[example]]
name = "demo"
""",
            )
            self.write(root / "src/lib.rs", "")
            self.write(root / "benches/throughput.rs", "")
            self.write(root / "examples/demo/main.rs", "")
            self.write(
                root / "cli/Cargo.toml",
                """
[package]
name = "sample-cli"
version = "0.1.0"

[[bin]]
name = "sample"
path = "src/entry.rs"
""",
            )
            self.write(root / "cli/src/entry.rs", "")

            manifests, targets = SOURCE_INTEGRITY.verify(root)

            self.assertEqual(len(manifests), 2)
            self.assertEqual(
                {(target.kind, target.name) for target in targets},
                {
                    ("lib", "sample"),
                    ("bench", "throughput"),
                    ("example", "demo"),
                    ("bin", "sample"),
                },
            )

    def test_rejects_missing_benchmark_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write(
                root / "Cargo.toml",
                """
[package]
name = "sample"
version = "0.1.0"

[[bench]]
name = "missing"
""",
            )
            self.write(root / "src/lib.rs", "")

            with self.assertRaisesRegex(
                SOURCE_INTEGRITY.IntegrityError,
                "missing bench target source.*benches/missing.rs",
            ):
                SOURCE_INTEGRITY.verify(root)

    def test_rejects_target_outside_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "workspace"
            self.write(
                root / "Cargo.toml",
                """
[package]
name = "sample"
version = "0.1.0"

[[bin]]
name = "escape"
path = "../escape.rs"
""",
            )
            self.write(Path(directory) / "escape.rs", "")

            with self.assertRaisesRegex(
                SOURCE_INTEGRITY.IntegrityError,
                "resolves outside workspace",
            ):
                SOURCE_INTEGRITY.verify(root)

    def test_named_binary_does_not_fall_back_to_package_main(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write(
                root / "Cargo.toml",
                """
[package]
name = "sample"
version = "0.1.0"

[[bin]]
name = "admin"
""",
            )
            self.write(root / "src/main.rs", "")

            with self.assertRaisesRegex(
                SOURCE_INTEGRITY.IntegrityError,
                "missing bin target source.*src/bin/admin.rs",
            ):
                SOURCE_INTEGRITY.verify(root)

    def test_rejects_package_without_library_or_binary(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write(
                root / "Cargo.toml",
                """
[package]
name = "sample"
version = "0.1.0"
""",
            )

            with self.assertRaisesRegex(
                SOURCE_INTEGRITY.IntegrityError,
                "has no library or binary target",
            ):
                SOURCE_INTEGRITY.verify(root)

    def test_rejects_unmatched_workspace_member(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write(
                root / "Cargo.toml",
                """
[workspace]
members = ["missing-*"]
""",
            )

            with self.assertRaisesRegex(
                SOURCE_INTEGRITY.IntegrityError,
                "workspace member pattern has no matches",
            ):
                SOURCE_INTEGRITY.verify(root)

    def test_require_tracked_rejects_untracked_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write(
                root / "Cargo.toml",
                """
[package]
name = "sample"
version = "0.1.0"

[[bench]]
name = "throughput"
""",
            )
            self.write(root / "src/lib.rs", "")
            self.write(root / "benches/throughput.rs", "")
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(
                ["git", "-C", str(root), "add", "Cargo.toml", "src/lib.rs"],
                check=True,
            )

            with self.assertRaisesRegex(
                SOURCE_INTEGRITY.IntegrityError,
                "untracked bench target source.*benches/throughput.rs",
            ):
                SOURCE_INTEGRITY.verify(root, require_tracked=True)

    @staticmethod
    def write(path: Path, contents: str) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents.lstrip(), encoding="utf-8")


if __name__ == "__main__":
    unittest.main()
