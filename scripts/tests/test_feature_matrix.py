import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "feature_matrix.py"
SPEC = importlib.util.spec_from_file_location("feature_matrix", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MATRIX = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MATRIX
SPEC.loader.exec_module(MATRIX)


class FeatureMatrixTests(unittest.TestCase):
    def test_row_names_are_unique(self) -> None:
        names = [row.name for row in MATRIX.ROWS]
        self.assertEqual(len(names), len(set(names)), "duplicate row name")

    def test_every_declared_feature_has_exactly_one_individual_row(self) -> None:
        individual_rows = [
            row
            for row in MATRIX.ROWS
            if row.package == "localcache"
            and len(row.features) == 1
            and not row.all_features
            and not row.doctest_only
        ]
        covered = {row.features[0] for row in individual_rows}
        self.assertEqual(covered, set(MATRIX.LIBRARY_FEATURES))
        self.assertEqual(len(individual_rows), len(MATRIX.LIBRARY_FEATURES))

    def test_row_by_name_fails_closed_on_unknown_row(self) -> None:
        with self.assertRaisesRegex(MATRIX.MatrixError, "unknown matrix row"):
            MATRIX.row_by_name("does-not-exist")

    def test_row_by_name_finds_a_real_row(self) -> None:
        row = MATRIX.row_by_name("lib-all-features")
        self.assertEqual(row.package, "localcache")
        self.assertTrue(row.all_features)

    def test_no_features_row_cargo_args(self) -> None:
        row = MATRIX.row_by_name("lib-no-features")
        self.assertEqual(
            row.cargo_args(subcommand="test"),
            ["test", "-p", "localcache", "--all-targets", "--no-default-features", "--locked"],
        )

    def test_single_feature_row_cargo_args_are_package_qualified(self) -> None:
        row = MATRIX.row_by_name("lib-feature-json")
        args = row.cargo_args(subcommand="clippy")
        self.assertEqual(
            args,
            [
                "clippy",
                "-p",
                "localcache",
                "--all-targets",
                "--no-default-features",
                "--features",
                "localcache/json",
                "--locked",
                "--",
                "-D",
                "warnings",
            ],
        )

    def test_all_features_row_omits_no_default_features(self) -> None:
        row = MATRIX.row_by_name("lib-all-features")
        args = row.cargo_args(subcommand="test")
        self.assertIn("--all-features", args)
        self.assertNotIn("--no-default-features", args)

    def test_cli_default_row_has_no_feature_flags(self) -> None:
        row = MATRIX.row_by_name("cli-default")
        args = row.cargo_args(subcommand="test")
        self.assertEqual(
            args,
            ["test", "-p", "localcache-cli", "--all-targets", "--locked"],
        )

    def test_cli_rows_never_select_the_library_package(self) -> None:
        cli_rows = [row for row in MATRIX.ROWS if row.name.startswith("cli-")]
        self.assertTrue(cli_rows)
        for row in cli_rows:
            self.assertEqual(row.package, "localcache-cli")

    def test_doctest_row_ignores_package_and_features(self) -> None:
        row = MATRIX.row_by_name("workspace-doctest")
        args = row.cargo_args(subcommand="test")
        self.assertEqual(
            args, ["test", "--workspace", "--doc", "--all-features", "--locked"]
        )

    def test_run_row_rejects_clippy_on_doctest_only_row(self) -> None:
        with self.assertRaisesRegex(MATRIX.MatrixError, "no clippy variant"):
            MATRIX.run_row("workspace-doctest", "clippy", root=Path("/nonexistent"))

    def test_run_row_modes_silently_skips_clippy_for_doctest_row(self) -> None:
        # No subprocess should be attempted for the clippy mode; only the
        # test mode may run. Redirect to a directory with no Cargo project
        # so a real cargo invocation would fail loudly if one were attempted
        # for the skipped mode.
        calls: list[tuple[str, str]] = []

        def fake_run_row(name: str, mode: str, *, root: Path) -> None:
            calls.append((name, mode))

        original = MATRIX.run_row
        MATRIX.run_row = fake_run_row
        try:
            MATRIX.run_row_modes(
                "workspace-doctest", ["clippy", "test"], root=Path("/nonexistent")
            )
        finally:
            MATRIX.run_row = original
        self.assertEqual(calls, [("workspace-doctest", "test")])

    def test_declared_library_features_reads_the_real_manifest(self) -> None:
        declared = MATRIX.declared_library_features()
        self.assertEqual(declared, set(MATRIX.LIBRARY_FEATURES))

    def test_check_coverage_passes_against_the_real_manifest(self) -> None:
        MATRIX.check_coverage()  # must not raise

    def test_check_coverage_fails_closed_on_a_missing_row(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "crates/localcache").mkdir(parents=True)
            (root / "crates/localcache/Cargo.toml").write_text(
                "[features]\n"
                + "\n".join(f'{name} = []' for name in MATRIX.LIBRARY_FEATURES)
                + '\nnew-feature-nobody-added-a-row-for = []\n',
                encoding="utf-8",
            )
            with self.assertRaisesRegex(MATRIX.MatrixError, "no matrix row"):
                MATRIX.check_coverage(root)

    def test_check_coverage_fails_closed_on_a_stale_row(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "crates/localcache").mkdir(parents=True)
            remaining = list(MATRIX.LIBRARY_FEATURES)[:-1]  # drop one
            (root / "crates/localcache/Cargo.toml").write_text(
                "[features]\n" + "\n".join(f'{name} = []' for name in remaining) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(MATRIX.MatrixError, "undeclared feature"):
                MATRIX.check_coverage(root)

    def test_declared_library_features_fails_closed_without_features_table(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "crates/localcache").mkdir(parents=True)
            (root / "crates/localcache/Cargo.toml").write_text(
                "[package]\nname = \"localcache\"\n", encoding="utf-8"
            )
            with self.assertRaisesRegex(MATRIX.MatrixError, "no \\[features\\] table"):
                MATRIX.declared_library_features(root)

    def test_run_all_aggregates_failures_rather_than_stopping_at_the_first(self) -> None:
        failures: list[str] = []

        def fake_run_row(name: str, mode: str, *, root: Path) -> None:
            failures.append(f"{name}/{mode}")
            raise MATRIX.MatrixError(f"deliberate failure for {name}/{mode}")

        original_run_row = MATRIX.run_row
        original_check_coverage = MATRIX.check_coverage
        MATRIX.run_row = fake_run_row
        MATRIX.check_coverage = lambda root: None
        try:
            with self.assertRaisesRegex(MATRIX.MatrixError, "matrix failures"):
                MATRIX.run_all(["test"], root=Path("/nonexistent"))
        finally:
            MATRIX.run_row = original_run_row
            MATRIX.check_coverage = original_check_coverage
        # Every non-doctest-duplicate row/mode combination should have been
        # attempted even though each one failed.
        expected_count = sum(
            1
            for row in MATRIX.ROWS
            if not (row.doctest_only)
        ) + 1  # +1 for the doctest row's single "test" mode
        self.assertEqual(len(failures), expected_count)

    def test_main_list_json_is_a_json_array_of_row_names(self) -> None:
        import io
        from contextlib import redirect_stdout

        buffer = io.StringIO()
        with redirect_stdout(buffer):
            status = MATRIX.main(["--list", "--json"])
        self.assertEqual(status, 0)
        import json

        names = json.loads(buffer.getvalue())
        self.assertEqual(names, [row.name for row in MATRIX.ROWS])

    def test_main_run_unknown_row_is_nonzero_and_reports_to_stderr(self) -> None:
        import io
        from contextlib import redirect_stderr

        buffer = io.StringIO()
        with redirect_stderr(buffer):
            status = MATRIX.main(["--run", "does-not-exist"])
        self.assertNotEqual(status, 0)
        self.assertIn("unknown matrix row", buffer.getvalue())

    def test_parser_rejects_ambiguous_abbreviations(self) -> None:
        with self.assertRaises(SystemExit):
            MATRIX.parse_args(["--run", "cli-default", "--mode", "test"])


if __name__ == "__main__":
    unittest.main()
