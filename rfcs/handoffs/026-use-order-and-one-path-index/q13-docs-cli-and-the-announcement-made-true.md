# Q13 Handoff — The CLI, the Book, the README and the CHANGELOG for Schema v6 (RFC 026 R2, R4)

RFC: `rfcs/accepted/026-use-order-and-one-path-index.md`. Starts after Q12 is **committed locally** (it rewrites text about code that must exist).
Order: Q12 → **Q13** → v0.23.0 release preparation. **Local commits; do not push.**
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

Everything outside `crates/localcache/src` that the new meaning touches, and the one visible CLI change. No library behaviour changes here; if you find that one is wrong, stop and file a design request.

## 1. The change

1. **CLI.** `localcache list` printed `never` for `last_accessed_at == 0` (`crates/cli/src/commands/read.rs:40-43`). After Q12 a stored entry never reports 0: remove the branch and rename the column header to what it now is
   (`LAST_USED`); update the CLI tests and `docs/src/cli.md`. Check `explain`, `stats` and `export` output for the same word or meaning.
2. **The book (RFC 026 R4), using your step 0 readers table (§ 5 of request 052):** `architecture.md` (the schema text, "reflects the last **read**", the index list, the eviction order), `builder.md` (the `max_entries` policy, the read-only note),
   `api.md` (the read-only paragraph), `querying.md` (`SortKey::LastAccessed`), `portability.md` (the export record and the export order), `migration.md` (the announcement becomes the real section: what v6 does, the one-way door, export first, the disk
   space, the read-only behaviour), `performance.md` (the two `EXPLAIN` outputs now name `sqlite_autoindex_files_1`; the measured effect from Q12 § 3, with its limits). Search for the old words with
   `git grep -n -i "never.read\|least recently \*\*read\|least recently read\|last read\|idx_files_namespace_path\|idx_files_lru" -- docs/src README.md crates` and report the final output; every remaining hit is justified (history rows only).
   Rustdoc on public items is Q12's; if you find one left, fix it and say so.
3. **`README.md`**: the LRU bullet states the policy as it now is (the sentence "From v0.23.0 a write also counts as a use (announced; see the CHANGELOG)" becomes plain present tense).
4. **`CHANGELOG.md`**: a `## [Unreleased]` section above `## [0.22.2]`, **Breaking** entries: the use order and its meaning of `last_accessed_at`; the import range rule and the export order; schema v6 (the migration, the one-way door, the disk space, the rollback by export);
   the dropped index and the measured effect; `list`'s column. The release preparation turns it into `[0.23.0]` with a consolidated migration list; write each entry so it can be listed.
5. **Nothing else.** In particular, not the version, not `ROADMAP.md`, not `docs/src/roadmap.md`.

## 2. Gates and the request

As in Q12 § 4 (all of them), plus the search above. One review request `.git-exclude/review-request/NNN-dev-q13-<date>.md`; commit locally after approval. Re-read each page you touch in full and say what else you found untrue.

## 3. Not yours

The library code (Q12), the version, the date, the roadmap pages.
