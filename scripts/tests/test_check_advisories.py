import importlib.util
import json
import sys
import tempfile
import unittest
from datetime import date
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "check_advisories.py"
SPEC = importlib.util.spec_from_file_location("check_advisories", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
CHECKER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CHECKER
SPEC.loader.exec_module(CHECKER)


class AdvisoryPolicyTests(unittest.TestCase):
    def test_checked_in_policy_has_two_exact_warnings(self) -> None:
        policy = SCRIPT.parents[1] / "security/advisory-policy.json"
        defaults, entries = CHECKER.load_policy(policy)

        self.assertTrue(all(action == "deny" for action in defaults.values()))
        self.assertEqual(
            [entry.finding.key for entry in entries],
            [
                ("RUSTSEC-2025-0052", "async-std", "1.13.2", "unmaintained"),
                ("RUSTSEC-2025-0141", "bincode", "2.0.1", "unmaintained"),
            ],
        )
        # RFC 019: both live entries are now standing dispositions — no expiry.
        self.assertTrue(all(entry.expires is None for entry in entries))

    def test_policy_rejects_unknown_duplicate_and_wildcard_entries(self) -> None:
        for mutate, message in (
            (lambda policy: policy.update({"extra": True}), "keys differ"),
            (
                lambda policy: policy["findings"].append(policy["findings"][0].copy()),
                "duplicate policy tuple",
            ),
            (
                lambda policy: policy["findings"][0].update({"version": "1.*"}),
                "invalid exact version",
            ),
            (
                lambda policy: policy["findings"][0].update({"kind": "yanked"}),
                "unsupported policy finding kind",
            ),
            (
                lambda policy: policy["defaults"].update({"unsound": "warn"}),
                "must be deny",
            ),
            (
                lambda policy: policy["findings"][0].pop("owner"),
                "keys differ",
            ),
        ):
            with self.subTest(message=message), tempfile.TemporaryDirectory() as directory:
                policy = self.policy_document()
                mutate(policy)
                path = Path(directory) / "policy.json"
                path.write_text(json.dumps(policy), encoding="utf-8")
                with self.assertRaisesRegex(CHECKER.AdvisoryGateError, message):
                    CHECKER.load_policy(path)

    def test_policy_allows_same_advisory_for_two_exact_versions(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            policy = self.policy_document()
            second = policy["findings"][0].copy()
            second["version"] = "1.13.3"
            policy["findings"].append(second)
            path = Path(directory) / "policy.json"
            path.write_text(json.dumps(policy), encoding="utf-8")

            _, entries = CHECKER.load_policy(path)

            self.assertEqual(len(entries), 2)

    # RFC 019 R1/R2 — kind-aware expiry.

    def test_unmaintained_entry_without_expires_key_is_accepted_as_standing(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            policy = self.policy_document()
            del policy["findings"][0]["expires"]
            path = Path(directory) / "policy.json"
            path.write_text(json.dumps(policy), encoding="utf-8")

            _, entries = CHECKER.load_policy(path)

            self.assertIsNone(entries[0].expires)

    def test_unmaintained_entry_with_null_expires_is_accepted_as_standing(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            policy = self.policy_document()
            policy["findings"][0]["expires"] = None
            path = Path(directory) / "policy.json"
            path.write_text(json.dumps(policy), encoding="utf-8")

            _, entries = CHECKER.load_policy(path)

            self.assertIsNone(entries[0].expires)

    def test_notice_entry_without_expires_is_accepted_as_standing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            policy = self.policy_document()
            policy["findings"][0]["kind"] = "notice"
            del policy["findings"][0]["expires"]
            path = Path(directory) / "policy.json"
            path.write_text(json.dumps(policy), encoding="utf-8")

            _, entries = CHECKER.load_policy(path)

            self.assertIsNone(entries[0].expires)

    def test_vulnerability_entry_without_expires_is_a_schema_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            policy = self.policy_document()
            policy["findings"][0].update({"kind": "vulnerability", "action": "exception"})
            del policy["findings"][0]["expires"]
            path = Path(directory) / "policy.json"
            path.write_text(json.dumps(policy), encoding="utf-8")

            with self.assertRaisesRegex(
                CHECKER.AdvisoryGateError, "expires is required for kind"
            ):
                CHECKER.load_policy(path)

    def test_unsound_entry_without_expires_is_a_schema_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            policy = self.policy_document()
            policy["findings"][0].update({"kind": "unsound", "action": "exception"})
            del policy["findings"][0]["expires"]
            path = Path(directory) / "policy.json"
            path.write_text(json.dumps(policy), encoding="utf-8")

            with self.assertRaisesRegex(
                CHECKER.AdvisoryGateError, "expires is required for kind"
            ):
                CHECKER.load_policy(path)

    def test_expires_must_still_postdate_approved_when_present_on_a_standing_kind(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            policy = self.policy_document()
            policy["findings"][0]["expires"] = "2026-07-23"  # == approved
            path = Path(directory) / "policy.json"
            path.write_text(json.dumps(policy), encoding="utf-8")

            with self.assertRaisesRegex(
                CHECKER.AdvisoryGateError, "expiry must be after approval"
            ):
                CHECKER.load_policy(path)

    def test_unknown_key_in_a_finding_entry_is_still_an_error(self) -> None:
        # RFC 019's widening (making `expires` optional) must not become a
        # hole: a misspelled or unexpected key is still rejected.
        with tempfile.TemporaryDirectory() as directory:
            policy = self.policy_document()
            policy["findings"][0]["expiration"] = "2026-10-21"
            path = Path(directory) / "policy.json"
            path.write_text(json.dumps(policy), encoding="utf-8")

            with self.assertRaisesRegex(CHECKER.AdvisoryGateError, "keys differ"):
                CHECKER.load_policy(path)

    @staticmethod
    def policy_document() -> dict[str, object]:
        return {
            "schema": 1,
            "defaults": {
                "vulnerability": "deny",
                "unsound": "deny",
                "yanked": "deny",
                "unmaintained": "deny",
                "notice": "deny",
            },
            "findings": [
                {
                    "id": "RUSTSEC-2025-0052",
                    "package": "async-std",
                    "version": "1.13.2",
                    "kind": "unmaintained",
                    "action": "warn",
                    "owner": "maintainers",
                    "approved": "2026-07-23",
                    "expires": "2026-10-21",
                    "reason": "compatibility",
                    "follow-up": "review before expiry",
                }
            ],
        }


class AuditReportTests(unittest.TestCase):
    def test_parses_vulnerability_and_all_warning_categories(self) -> None:
        report = self.report()
        report["vulnerabilities"] = {
            "found": True,
            "count": 1,
            "list": [self.item("RUSTSEC-2026-0204", "crossbeam-epoch", "0.9.18")],
        }
        report["warnings"] = {
            "unmaintained": [
                self.item(
                    "RUSTSEC-2025-0052", "async-std", "1.13.2", "unmaintained"
                )
            ],
            "unsound": [
                self.item("RUSTSEC-2026-0190", "anyhow", "1.0.102", "unsound")
            ],
            "notice": [
                self.item("RUSTSEC-2026-9999", "sample", "1.2.3", "notice")
            ],
        }

        findings = CHECKER.parse_audit_report(json.dumps(report).encode())

        self.assertEqual(
            {finding.kind for finding in findings},
            {"vulnerability", "unmaintained", "unsound", "notice"},
        )

    def test_unknown_warning_kind_is_visible_and_denied_without_policy(self) -> None:
        report = self.report()
        report["warnings"] = {
            "future-kind": [
                self.item(
                    "RUSTSEC-2026-9999", "sample", "1.2.3", "future-kind"
                )
            ]
        }
        findings = CHECKER.parse_audit_report(json.dumps(report).encode())

        lines, denied = CHECKER.classify_findings(findings, [], date(2026, 7, 23))

        self.assertTrue(denied)
        self.assertIn("DENY", lines[0])

    def test_malformed_counts_kinds_and_duplicate_tuples_fail_closed(self) -> None:
        cases = []
        wrong_count = self.report()
        wrong_count["vulnerabilities"]["count"] = 1
        cases.append((wrong_count, "count does not match"))
        wrong_kind = self.report()
        wrong_kind["warnings"] = {
            "unsound": [
                self.item(
                    "RUSTSEC-2026-0190", "anyhow", "1.0.102", "unmaintained"
                )
            ]
        }
        cases.append((wrong_kind, "kind mismatch"))
        duplicate = self.report()
        item = self.item(
            "RUSTSEC-2025-0052", "async-std", "1.13.2", "unmaintained"
        )
        duplicate["warnings"] = {"unmaintained": [item, item]}
        cases.append((duplicate, "duplicate finding"))

        for report, message in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(CHECKER.AdvisoryGateError, message):
                    CHECKER.parse_audit_report(json.dumps(report).encode())

    def test_audit_status_contract_accepts_only_zero_or_one_with_valid_json(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            path.write_text(json.dumps(self.report()), encoding="utf-8")
            for status in (0, 1):
                self.assertTrue(CHECKER.require_audit_result(status, path, "fixture"))
            for status in (2, 3):
                with self.assertRaisesRegex(CHECKER.AdvisoryGateError, "operational"):
                    CHECKER.require_audit_result(status, path, "fixture")
            path.write_text("not json", encoding="utf-8")
            with self.assertRaisesRegex(CHECKER.AdvisoryGateError, "invalid"):
                CHECKER.require_audit_result(0, path, "fixture")

    @staticmethod
    def report() -> dict[str, object]:
        return {
            "database": {},
            "lockfile": {},
            "settings": {},
            "vulnerabilities": {"found": False, "count": 0, "list": []},
            "warnings": {},
        }

    @staticmethod
    def item(
        advisory_id: str, package: str, version: str, kind: str | None = None
    ) -> dict[str, object]:
        item: dict[str, object] = {
            "advisory": {"id": advisory_id, "package": package},
            "package": {"name": package, "version": version},
        }
        if kind is not None:
            item["kind"] = kind
        return item


class ClassificationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.finding = CHECKER.Finding(
            "RUSTSEC-2025-0052", "async-std", "1.13.2", "unmaintained"
        )
        self.entry = CHECKER.PolicyEntry(
            finding=self.finding,
            action="warn",
            owner="maintainers",
            approved=date(2026, 7, 23),
            expires=date(2026, 10, 21),
            reason="compatibility",
            follow_up="review",
        )

    def test_exact_warning_passes_before_expiry_and_fails_on_expiry(self) -> None:
        lines, denied = CHECKER.classify_findings(
            [self.finding], [self.entry], date(2026, 10, 20)
        )
        self.assertFalse(denied)
        self.assertTrue(lines[0].startswith("WARN "))

        lines, denied = CHECKER.classify_findings(
            [self.finding], [self.entry], date(2026, 10, 21)
        )
        self.assertTrue(denied)
        self.assertIn("expired", lines[0])

    def test_each_identity_mismatch_and_stale_policy_fail_closed(self) -> None:
        mismatches = [
            CHECKER.Finding("RUSTSEC-2025-9999", "async-std", "1.13.2", "unmaintained"),
            CHECKER.Finding("RUSTSEC-2025-0052", "other", "1.13.2", "unmaintained"),
            CHECKER.Finding("RUSTSEC-2025-0052", "async-std", "1.13.3", "unmaintained"),
            CHECKER.Finding("RUSTSEC-2025-0052", "async-std", "1.13.2", "notice"),
        ]
        for finding in mismatches:
            with self.subTest(finding=finding):
                lines, denied = CHECKER.classify_findings(
                    [finding], [self.entry], date(2026, 7, 23)
                )
                self.assertTrue(denied)
                self.assertEqual(sum(line.startswith("DENY ") for line in lines), 2)

        lines, denied = CHECKER.classify_findings(
            [], [self.entry], date(2026, 7, 23)
        )
        self.assertTrue(denied)
        self.assertIn("stale", lines[0])

    def test_each_default_finding_kind_is_denied_without_policy(self) -> None:
        for kind in ("vulnerability", "unsound", "unmaintained", "notice"):
            with self.subTest(kind=kind):
                finding = CHECKER.Finding(
                    "RUSTSEC-2026-9999", "sample", "1.2.3", kind
                )
                lines, denied = CHECKER.classify_findings(
                    [finding], [], date(2026, 7, 23)
                )
                self.assertTrue(denied)
                self.assertTrue(lines[0].startswith("DENY "))

    def test_no_findings_and_no_policy_passes(self) -> None:
        lines, denied = CHECKER.classify_findings([], [], date(2026, 7, 23))
        self.assertEqual(lines, [])
        self.assertFalse(denied)

    # RFC 019 — standing dispositions.

    def test_standing_unmaintained_disposition_does_not_cover_a_vulnerability_finding(
        self,
    ) -> None:
        # This is the requirement the whole RFC rests on: the policy key
        # includes `kind`, so a standing `unmaintained` entry must not be
        # mistaken for coverage of a `vulnerability` finding for the same
        # package and version. If this fails, RFC 019's premise is wrong.
        standing_entry = CHECKER.PolicyEntry(
            finding=self.finding,  # kind="unmaintained"
            action="warn",
            owner="maintainers",
            approved=date(2026, 7, 23),
            expires=None,
            reason="compatibility",
            follow_up="reassess if a vulnerability advisory is published",
        )
        vulnerability_finding = CHECKER.Finding(
            "RUSTSEC-2025-0052", "async-std", "1.13.2", "vulnerability"
        )
        lines, denied = CHECKER.classify_findings(
            [vulnerability_finding], [standing_entry], date(2026, 7, 24)
        )
        self.assertTrue(denied)
        # Both the uncovered new finding and the now-unmatched standing
        # entry must be reported.
        self.assertEqual(sum(line.startswith("DENY ") for line in lines), 2)
        self.assertTrue(
            any("no exact policy disposition" in line for line in lines)
        )
        self.assertTrue(any("stale policy entry" in line for line in lines))

    def test_standing_entry_reports_without_expiry_and_names_condition(self) -> None:
        standing_entry = CHECKER.PolicyEntry(
            finding=self.finding,
            action="warn",
            owner="maintainers",
            approved=date(2026, 7, 23),
            expires=None,
            reason="compatibility",
            follow_up="reassess if a maintained fork gains adoption",
        )
        lines, denied = CHECKER.classify_findings(
            [self.finding], [standing_entry], date(2026, 7, 24)
        )
        self.assertFalse(denied)
        self.assertTrue(lines[0].startswith("WARN "))
        self.assertNotIn("until", lines[0])
        self.assertIn("standing disposition", lines[0])
        self.assertIn("reassess if a maintained fork gains adoption", lines[0])

    def test_standing_entry_version_change_denies_both_new_and_stale(self) -> None:
        standing_entry = CHECKER.PolicyEntry(
            finding=self.finding,  # version="1.13.2"
            action="warn",
            owner="maintainers",
            approved=date(2026, 7, 23),
            expires=None,
            reason="compatibility",
            follow_up="reassess if a vulnerability advisory is published",
        )
        new_version_finding = CHECKER.Finding(
            "RUSTSEC-2025-0052", "async-std", "1.14.0", "unmaintained"
        )
        lines, denied = CHECKER.classify_findings(
            [new_version_finding], [standing_entry], date(2026, 7, 24)
        )
        self.assertTrue(denied)
        self.assertEqual(sum(line.startswith("DENY ") for line in lines), 2)
        self.assertTrue(
            any("no exact policy disposition" in line for line in lines)
        )
        self.assertTrue(any("stale policy entry" in line for line in lines))

    def test_vulnerability_entry_with_past_expires_still_denies(self) -> None:
        vulnerability_finding = CHECKER.Finding(
            "RUSTSEC-2026-0001", "sample", "1.0.0", "vulnerability"
        )
        entry = CHECKER.PolicyEntry(
            finding=vulnerability_finding,
            action="exception",
            owner="maintainers",
            approved=date(2026, 7, 1),
            expires=date(2026, 7, 20),
            reason="deferred fix",
            follow_up="patch by expiry",
        )
        lines, denied = CHECKER.classify_findings(
            [vulnerability_finding], [entry], date(2026, 7, 20)
        )
        self.assertTrue(denied)
        self.assertIn("expired", lines[0])


class RegistryTests(unittest.TestCase):
    CHECKSUM = "a" * 64

    def test_sparse_paths_follow_cargo_layout(self) -> None:
        self.assertEqual(CHECKER.sparse_path("a"), "1/a")
        self.assertEqual(CHECKER.sparse_path("AB"), "2/ab")
        self.assertEqual(CHECKER.sparse_path("AbC"), "3/a/abc")
        self.assertEqual(CHECKER.sparse_path("rusqlite"), "ru/sq/rusqlite")

    def test_lockfile_enumerates_crates_io_and_rejects_other_registry(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "Cargo.lock"
            path.write_text(
                self.lock_package("sample", "1.2.3", CHECKER.CRATES_IO_SOURCE)
                + '\n[[package]]\nname = "workspace"\nversion = "0.1.0"\n'
                + '\n[[package]]\nname = "git-package"\nversion = "1.0.0"\n'
                + 'source = "git+https://example.invalid/repository"\n',
                encoding="utf-8",
            )
            packages = CHECKER.load_registry_packages(path)
            self.assertEqual([package.name for package in packages], ["sample"])

            path.write_text(
                self.lock_package(
                    "sample", "1.2.3", "registry+https://example.invalid/index"
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(CHECKER.AdvisoryGateError, "unsupported registry"):
                CHECKER.load_registry_packages(path)

    def test_sparse_validation_covers_checksum_duplicates_malformed_and_yanked(self) -> None:
        package = self.package()
        valid = self.record(yanked=False)
        selected = CHECKER.validate_sparse_response(valid, [package], "fixture")
        self.assertFalse(selected[0]["yanked"])

        yanked = CHECKER.validate_sparse_response(
            self.record(yanked=True), [package], "fixture"
        )
        self.assertTrue(yanked[0]["yanked"])

        cases = (
            (b"", "0 records"),
            (valid + valid, "2 records"),
            (b"not-json\n", "invalid"),
            (self.record(checksum="b" * 64), "checksum mismatch"),
            (self.record(version="9.9.9"), "0 records"),
            (self.record(yanked="false"), "non-Boolean"),
        )
        for body, message in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(CHECKER.AdvisoryGateError, message):
                    CHECKER.validate_sparse_response(body, [package], "fixture")

    def test_snapshot_rejects_one_failed_lookup_and_records_complete_manifest(self) -> None:
        packages = [self.package("alpha"), self.package("beta")]

        def fetch(url: str, _timeout: int):
            name = url.rsplit("/", 1)[-1]
            if name == "beta":
                return 503, {}, b""
            return 200, {"etag": '"fixture"'}, self.record(name=name)

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            with self.assertRaisesRegex(CHECKER.AdvisoryGateError, "HTTP 503"):
                CHECKER.build_registry_snapshot(
                    packages, output, "2026-07-23T00:00:00Z", fetch
                )

        def successful_fetch(url: str, _timeout: int):
            name = url.rsplit("/", 1)[-1]
            return 200, {"date": "fixture"}, self.record(name=name)

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            manifest, selected = CHECKER.build_registry_snapshot(
                packages, output, "2026-07-23T00:00:00Z", successful_fetch
            )
            parsed = json.loads(manifest)
            self.assertEqual(len(parsed["responses"]), 2)
            self.assertEqual(len(selected), 2)
            original = CHECKER.sha256_bytes(manifest)
            path = output / "manifest.json"
            path.write_bytes(manifest)
            loaded = CHECKER.load_registry_snapshot(
                path, original, packages, output
            )
            self.assertEqual(loaded, selected)
            response = output / parsed["responses"][0]["response-file"]
            response.write_bytes(response.read_bytes() + b" ")
            with self.assertRaisesRegex(CHECKER.AdvisoryGateError, "changed"):
                CHECKER.load_registry_snapshot(path, original, packages, output)
            response.write_bytes(response.read_bytes()[:-1])
            path.write_bytes(manifest + b" ")
            with self.assertRaisesRegex(CHECKER.AdvisoryGateError, "changed"):
                CHECKER.load_registry_snapshot(path, original, packages, output)

    def test_live_cli_exposes_no_date_policy_or_snapshot_override(self) -> None:
        for option in ("--today", "--policy", "--snapshot", "--ignore"):
            with self.subTest(option=option), self.assertRaises(SystemExit):
                CHECKER.parse_args([option, "value", "output"])

    def package(self, name: str = "sample"):
        return CHECKER.RegistryPackage(
            CHECKER.CRATES_IO_SOURCE, name, "1.2.3", self.CHECKSUM
        )

    def record(
        self,
        *,
        name: str = "sample",
        version: str = "1.2.3",
        checksum: str | None = None,
        yanked: object = False,
    ) -> bytes:
        return (
            json.dumps(
                {
                    "name": name,
                    "vers": version,
                    "cksum": self.CHECKSUM if checksum is None else checksum,
                    "yanked": yanked,
                }
            ).encode()
            + b"\n"
        )

    def lock_package(self, name: str, version: str, source: str) -> str:
        return (
            'version = 4\n\n[[package]]\n'
            f'name = "{name}"\nversion = "{version}"\nsource = "{source}"\n'
            f'checksum = "{self.CHECKSUM}"\n'
        )


if __name__ == "__main__":
    unittest.main()
