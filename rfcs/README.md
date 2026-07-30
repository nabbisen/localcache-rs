# RFCs

This directory contains design specifications for `localcache`.
The lifecycle of RFC files (folder layout, states, numbering, status fields,
cross-references) is governed by **[RFC 000](./done/000-rfc-lifecycle-policy.md)**.
localcache uses RFC 000's five-folder variant: an RFC moves from `proposed/`
to `accepted/` only after independent review and explicit owner approval.
Implementation must not begin while an RFC remains Proposed.

## Templates

**Lightweight** (small / unambiguous scope):

```markdown
# RFC NNN — Title
| Field | Value |
|-------|-------|
| Status | Proposed |
| Feature | cargo feature name or *(core)* |
| Touches | affected source files |

## Summary
## Motivation
## Design
## Test plan
```

**Full** (medium / large scope — add as needed):

```markdown
# RFC NNN — Title
…
## Requirements
## Design
## Test plan
## Security considerations
## Open questions
```

The "Background" section is optional in both templates.

---

## Index

### Implemented

| RFC | Title | Shipped in |
|-----|-------|------------|
| [001](./done/001-recursive-directory-watching.md) | Recursive Directory Watching | v0.17.0 |
| [002](./done/002-query-index-hints.md) | Query Index Hints and Explain Plan | v0.17.0 |
| [003](./done/003-opentelemetry-spans.md) | OpenTelemetry Spans | v0.17.0 |
| [004](./done/004-shared-memory-db.md) | Read-only Shared-memory DB Mode | v0.17.0 |
| [005](./done/005-async-std-smol.md) | async-std / smol Feature Variants | v0.17.0 |
| [006](./done/006-directory-scoped-query-predicates.md) | Directory-scoped Query Predicates | v0.18.0 |
| [007](./done/007-read-only-connection-pool.md) | Read-only Connection Pool (`ReadPool<T>`) | v0.19.0 |
| [008](./done/008-compatibility-guarantees.md) | Compatibility Guarantees: Payload Wire Format and Path Semantics | v0.19.0 |
| [009](./done/009-reproducible-source-archives-and-release-gates.md) | Reproducible Source Archives and Release Gates | v0.20.1 |
| [010](./done/010-transactional-payload-preserving-schema-migrations.md) | Transactional, Payload-Preserving Schema Migrations | v0.20.1 |
| [011](./done/011-safe-sqlite-identifier-boundary.md) | Safe SQLite Identifier Boundary | v0.20.1 |
| [012](./done/012-read-only-schema-and-mutation-contract.md) | Read-only Schema and Mutation Contract | v0.20.1 |
| [013](./done/013-panic-free-path-glob-and-cli-text-handling.md) | Panic-free Path, Glob, and CLI Text Handling | v0.20.1 |
| [014](./done/014-declared-msrv-and-dependency-security-policy.md) | Declared MSRV and Dependency Security Policy | v0.20.1 |
| [015](./done/015-async-runtime-and-watcher-failure-safety.md) | Async Runtime and Watcher Failure Safety | v0.20.1 |
| [017](./done/017-content-reproducible-archives-without-a-container-producer.md) | Content-Reproducible Archives Without a Container Producer (amends RFC 009) | v0.20.1 |

### Accepted

*(none — all Phase 21 RFCs shipped in v0.20.1)*

### Proposed

*(none)*

### Archive

| RFC | Title | Reason |
|-----|-------|--------|
| [016](./archive/016-published-crate-legal-file-completeness.md) | Published Crate Legal-File Completeness | Withdrawn 2026-07-28 — its Apache-2.0 §4 premise was false; root-only is sufficient |
