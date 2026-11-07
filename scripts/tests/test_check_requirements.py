"""Tests of scripts/check_requirements.py (Q25). No network: a fixture index, fixture metadata."""

import importlib.util
import io
import json
import sys
import tempfile
import unittest
import urllib.error
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[1] / "check_requirements.py"
SPEC = importlib.util.spec_from_file_location("check_requirements", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
CR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CR
SPEC.loader.exec_module(CR)


def index_line(version, yanked=False):
    return json.dumps({"name": "x", "vers": version, "yanked": yanked, "deps": [], "cksum": "0" * 64, "features": {}})


def index_text(*versions):
    """versions: "1.2.3" or ("1.2.3", True) for a yanked one."""
    return "\n".join(index_line(*((v,) if isinstance(v, str) else v)) for v in versions) + "\n"


def metadata(*dependencies, member="localcache"):
    """dependencies: (name, req) or (name, req, kind, optional, source)."""
    deps = []
    for dep in dependencies:
        name, req, kind, optional, source = (tuple(dep) + (None, False, "registry+https://github.com/rust-lang/crates.io-index")[len(dep) - 2 :])[:5]
        deps.append({"name": name, "req": req, "kind": kind, "optional": optional, "source": source})
    return {
        "workspace_members": ["path+file:///w#1"],
        "packages": [
            {"id": "path+file:///w#1", "name": member, "dependencies": deps},
            {"id": "registry+x#other", "name": "not-a-member", "dependencies": [{"name": "zzz", "req": "^0", "kind": None, "optional": False, "source": "registry+x"}]},
        ],
    }


def held(*rows):
    out = {}
    for crate, requirement, latest in rows:
        out[(crate, requirement)] = {"crate": crate, "requirement": requirement, "latest": latest, "why": "w", "ends": "e"}
    return out


def fetcher(**indexes):
    def fetch(name):
        try:
            return indexes[name.replace("-", "_")]
        except KeyError:
            raise CR.RequirementError(f"no fixture index for {name}")

    return fetch


def run_gate(meta, indexes, held_rows):
    return CR.evaluate(CR.declared_requirements(meta), fetcher(**indexes), held_rows)


class MatcherTests(unittest.TestCase):
    CASES = [
        ("^0.40.2", "0.41.1", False), ("^0.40.2", "0.40.9", True), ("^0.40.2", "0.40.1", False),
        ("^1", "1.8.7", True), ("^1", "2.0.0", False), ("^1.13", "1.13.2", True), ("^1.23", "1.54.1", True),
        ("^0.0.3", "0.0.3", True), ("^0.0.3", "0.0.4", False), ("^0.0", "0.0.9", True), ("^0", "0.9.9", True),
        ("^0", "1.0.0", False), ("=0.25.1", "0.25.1", True), ("=0.25.1", "0.25.2", False),
        ("~1.2.3", "1.2.9", True), ("~1.2.3", "1.3.0", False), (">=1,<2", "1.5.0", True), (">=1,<2", "2.0.0", False),
        ("*", "9.9.9", True), ("1.*", "1.4.0", True), ("1.*", "2.0.0", False), (">1", "2.0.0", True),
        (">1", "1.9.9", False), ("<=1", "1.9.9", True), ("<=1", "2.0.0", False), ("1.2", "1.9.0", True),
    ]

    def test_cargo_rules(self):
        for requirement, version, expected in self.CASES:
            with self.subTest(requirement=requirement, version=version):
                self.assertEqual(CR.admits(requirement, version), expected)

    def test_forms_it_does_not_understand_fail_closed(self):
        for requirement in ["^1 || 2", "1.2.3.4", "^^1", "~", "", "git+x", "^1.0.0-rc.1", "1.2.3+build", ">=*", "1.2.3.*"]:
            with self.subTest(requirement=requirement), self.assertRaises(CR.RequirementError):
                CR.admits(requirement, "1.0.0")

    def test_a_version_that_is_not_plain_fails_closed(self):
        with self.assertRaises(CR.RequirementError):
            CR.admits("^1", "1.0.0-rc.1")


class IndexTests(unittest.TestCase):
    def test_path_layout(self):
        self.assertEqual(CR.index_path("a"), "1/a")
        self.assertEqual(CR.index_path("ab"), "2/ab")
        self.assertEqual(CR.index_path("abc"), "3/a/abc")
        self.assertEqual(CR.index_path("serde_json"), "se/rd/serde_json")

    def test_a_yanked_newest_version_is_skipped(self):
        self.assertEqual(CR.newest_stable("x", index_text("1.0.0", "1.1.0", ("1.2.0", True))), "1.1.0")

    def test_a_prerelease_is_never_the_newest(self):
        self.assertEqual(CR.newest_stable("x", index_text("1.0.0", "2.0.0-rc.1", "1.0.1")), "1.0.1")

    def test_numeric_not_textual_order(self):
        self.assertEqual(CR.newest_stable("x", index_text("1.9.0", "1.10.0")), "1.10.0")

    def test_no_stable_version_fails(self):
        for text in [index_text(("1.0.0", True)), index_text("1.0.0-rc.1"), ""]:
            with self.subTest(text=text), self.assertRaises(CR.RequirementError):
                CR.newest_stable("x", text)

    def test_a_malformed_line_fails(self):
        for text in ["not json\n", '{"vers": "1.0.0"}\n', '{"vers": "1.0.0", "yanked": "no"}\n']:
            with self.subTest(text=text), self.assertRaises(CR.RequirementError):
                CR.newest_stable("x", text)

    def test_the_live_fetch_turns_every_failure_into_an_error(self):
        for error in [urllib.error.URLError("unreachable"), TimeoutError("slow"), ConnectionResetError("reset")]:
            with self.subTest(error=error), mock.patch.object(CR.urllib.request, "urlopen", side_effect=error):
                with self.assertRaisesRegex(CR.RequirementError, "cannot fetch the crates.io index of serde"):
                    CR.fetch_index_live("serde")


class GateTests(unittest.TestCase):
    def test_stale_and_unlisted_fails(self):
        failures = run_gate(metadata(("rusqlite", "^0.40.2")), {"rusqlite": index_text("0.40.2", "0.41.1")}, {})
        self.assertEqual(len(failures), 1)
        self.assertIn("rusqlite ^0.40.2", failures[0])
        self.assertIn("0.41.1", failures[0])
        self.assertIn("update the requirement, or add a row", failures[0])

    def test_a_requirement_that_admits_the_newest_passes(self):
        self.assertEqual(run_gate(metadata(("serde", "^1")), {"serde": index_text("1.0.0", "1.0.229")}, {}), [])

    def test_listed_with_the_current_latest_passes(self):
        failures = run_gate(
            metadata(("rusqlite", "^0.40.2")), {"rusqlite": index_text("0.40.2", "0.41.1")}, held(("rusqlite", "^0.40.2", "0.41.1"))
        )
        self.assertEqual(failures, [])

    def test_listed_with_an_older_latest_fails(self):
        failures = run_gate(
            metadata(("rusqlite", "^0.40.2")), {"rusqlite": index_text("0.40.2", "0.41.1", "0.42.0")}, held(("rusqlite", "^0.40.2", "0.41.1"))
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("written against 0.41.1", failures[0])
        self.assertIn("0.42.0", failures[0])

    def test_listed_but_now_satisfied_fails(self):
        failures = run_gate(metadata(("rusqlite", "^0.41")), {"rusqlite": index_text("0.41.1")}, held(("rusqlite", "^0.41", "0.41.1")))
        self.assertEqual(len(failures), 1)
        self.assertIn("delete the row", failures[0])

    def test_a_row_for_a_requirement_nobody_declares_fails(self):
        failures = run_gate(metadata(("serde", "^1")), {"serde": index_text("1.0.1")}, held(("gone", "^1", "2.0.0")))
        self.assertEqual(len(failures), 1)
        self.assertIn("no workspace member declares", failures[0])

    def test_a_row_for_another_requirement_of_the_same_crate_does_not_cover_it(self):
        failures = run_gate(metadata(("rusqlite", "^0.40.2")), {"rusqlite": index_text("0.41.1")}, held(("rusqlite", "^0.40", "0.41.1")))
        self.assertEqual(len(failures), 2)

    def test_a_yanked_newest_is_skipped(self):
        meta = metadata(("serde", "^1.0.5"))
        self.assertEqual(run_gate(meta, {"serde": index_text("1.0.5", ("2.0.0", True))}, {}), [])
        self.assertEqual(len(run_gate(meta, {"serde": index_text("1.0.5", "2.0.0")}, {})), 1)

    def test_index_unreachable_fails(self):
        with self.assertRaises(CR.RequirementError):
            run_gate(metadata(("serde", "^1")), {}, {})

    def test_an_unknown_operator_fails(self):
        with self.assertRaises(CR.RequirementError):
            run_gate(metadata(("serde", "^1 || 2")), {"serde": index_text("1.0.0")}, {})

    def test_a_prerelease_requirement_fails(self):
        with self.assertRaises(CR.RequirementError):
            run_gate(metadata(("serde", "^1.0.0-rc.1")), {"serde": index_text("1.0.0")}, {})

    def test_workspace_members_and_other_packages_are_not_checked(self):
        meta = metadata(("localcache-format", "=0.25.1", None, False, None), ("serde", "^1"))
        self.assertEqual(CR.declared_requirements(meta), {("serde", "^1"): ["localcache (normal)"]})

    def test_every_kind_optional_and_target_is_covered(self):
        meta = metadata(("a", "^1", "dev", False), ("b", "^1", "build", False), ("c", "^1", None, True))
        names = sorted(name for name, _ in CR.declared_requirements(meta))
        self.assertEqual(names, ["a", "b", "c"])

    def test_no_registry_dependency_at_all_fails_closed(self):
        with self.assertRaises(CR.RequirementError):
            CR.declared_requirements(metadata())

    def test_a_metadata_of_an_unexpected_shape_fails_closed(self):
        with self.assertRaises(CR.RequirementError):
            CR.declared_requirements({"packages": []})


HELD_OK = """\
[[held]]
crate = "rusqlite"
requirement = "^0.40.2"
latest = "0.41.1"
why = "w | with a pipe"
ends = "e"
"""


class HeldFileTests(unittest.TestCase):
    def load(self, text):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "requirements-held.toml"
            path.write_text(text, encoding="utf-8")
            return CR.load_held(path)

    def test_a_valid_file_loads(self):
        self.assertEqual(list(self.load(HELD_OK)), [("rusqlite", "^0.40.2")])

    def test_an_empty_file_is_no_rows(self):
        self.assertEqual(self.load(""), {})

    def test_schema_violations_fail(self):
        bad = {
            "unknown key": HELD_OK + 'extra = "x"\n',
            "unknown top-level key": HELD_OK + '[other]\na = 1\n',
            "missing key": HELD_OK.replace('ends = "e"\n', ""),
            "empty reason": HELD_OK.replace('why = "w | with a pipe"', 'why = " "'),
            "not a string": HELD_OK.replace('latest = "0.41.1"', "latest = 41"),
            "latest not a plain version": HELD_OK.replace('"0.41.1"', '"0.41.1-rc.1"'),
            "duplicate row": HELD_OK + "\n" + HELD_OK,
            "not toml": "[[held\n",
        }
        for label, text in bad.items():
            with self.subTest(label), self.assertRaises(CR.RequirementError):
                self.load(text)


class BookTests(unittest.TestCase):
    def page(self, held_rows, *, block=None):
        block = CR.render_book_block(held_rows) if block is None else block
        return f"# Title\n\nbefore\n\n{block}\n\nafter\n"

    def check(self, text, held_rows):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs/src").mkdir(parents=True)
            (root / CR.BOOK_FILE).write_text(text, encoding="utf-8")
            CR.check_book(root, held_rows)

    def test_a_matching_block_passes(self):
        rows = held(("rusqlite", "^0.40.2", "0.41.1"))
        self.check(self.page(rows), rows)

    def test_a_changed_cell_fails(self):
        rows = held(("rusqlite", "^0.40.2", "0.41.1"))
        with self.assertRaisesRegex(CR.RequirementError, "differs"):
            self.check(self.page(rows).replace("`0.41.1`", "`0.41.0`"), rows)

    def test_a_row_missing_from_the_page_fails(self):
        rows = held(("rusqlite", "^0.40.2", "0.41.1"), ("sha2", "^0.10", "0.11.1"))
        with self.assertRaises(CR.RequirementError):
            self.check(self.page(held(("rusqlite", "^0.40.2", "0.41.1"))), rows)

    def test_missing_or_repeated_markers_fail(self):
        rows = held(("rusqlite", "^0.40.2", "0.41.1"))
        block = CR.render_book_block(rows)
        for label, text in {
            "none": "# T\n\nno table\n",
            "no end": "# T\n" + CR.BOOK_BEGIN + "\n",
            "twice": self.page(rows) + block,
            "reversed": CR.BOOK_END + "\n" + CR.BOOK_BEGIN + "\n",
        }.items():
            with self.subTest(label), self.assertRaises(CR.RequirementError):
                self.check(text, rows)

    def test_write_book_rewrites_only_the_block(self):
        rows = held(("rusqlite", "^0.40.2", "0.41.1"))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs/src").mkdir(parents=True)
            old = self.page(held(("sha2", "^0.10", "0.11.1")))
            (root / CR.BOOK_FILE).write_text(old, encoding="utf-8")
            CR.write_book(root, rows)
            new = (root / CR.BOOK_FILE).read_text(encoding="utf-8")
            self.assertTrue(new.startswith("# Title\n\nbefore\n\n"))
            self.assertTrue(new.endswith("\n\nafter\n"))
            CR.check_book(root, rows)

    def test_a_pipe_in_a_reason_is_escaped_in_the_table(self):
        rows = {("a", "^1"): {"crate": "a", "requirement": "^1", "latest": "2.0.0", "why": "x | y", "ends": "z"}}
        self.assertIn("x \\| y", CR.render_book_block(rows))


class MainTests(unittest.TestCase):
    """The command line, end to end, on a temporary repository root and a fixture index."""

    def tree(self, directory, *, held_text, deps, indexes):
        root = Path(directory)
        (root / "docs/src").mkdir(parents=True)
        (root / CR.HELD_FILE).write_text(held_text, encoding="utf-8")
        rows = CR.load_held(root / CR.HELD_FILE)
        (root / CR.BOOK_FILE).write_text(f"x\n{CR.render_book_block(rows)}\ny\n", encoding="utf-8")
        (root / "meta.json").write_text(json.dumps(metadata(*deps)), encoding="utf-8")
        for name, text in indexes.items():
            path = root / "index" / CR.index_path(name)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
        return ["--root", str(root), "--metadata", str(root / "meta.json"), "--index-dir", str(root / "index")]

    def run_main(self, args):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = CR.main(args)
        return code, out.getvalue(), err.getvalue()

    def test_pass_and_fail_exit_codes(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.tree(directory, held_text=HELD_OK, deps=[("rusqlite", "^0.40.2")], indexes={"rusqlite": index_text("0.41.1")})
            code, out, _ = self.run_main(args)
            self.assertEqual(code, 0, out)
            self.assertIn("OK (1 requirements; 1 held", out)
        with tempfile.TemporaryDirectory() as directory:
            args = self.tree(directory, held_text="", deps=[("rusqlite", "^0.40.2")], indexes={"rusqlite": index_text("0.41.1")})
            code, _, err = self.run_main(args)
            self.assertEqual(code, 1)
            self.assertIn("update the requirement, or add a row", err)

    def test_an_unreachable_index_is_exit_one(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.tree(directory, held_text="", deps=[("serde", "^1")], indexes={})
            code, _, err = self.run_main(args)
            self.assertEqual(code, 1)
            self.assertIn("FAILED", err)

    def test_book_only_needs_no_metadata_and_no_index(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs/src").mkdir(parents=True)
            (root / CR.HELD_FILE).write_text(HELD_OK, encoding="utf-8")
            (root / CR.BOOK_FILE).write_text(CR.render_book_block(CR.load_held(root / CR.HELD_FILE)), encoding="utf-8")
            code, out, _ = self.run_main(["--root", str(root), "--book-only"])
            self.assertEqual(code, 0, out)
            (root / CR.BOOK_FILE).write_text("no markers", encoding="utf-8")
            self.assertEqual(self.run_main(["--root", str(root), "--book-only"])[0], 1)


if __name__ == "__main__":
    unittest.main()
