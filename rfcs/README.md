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

### Archive

*(none yet)*

### Accepted

| RFC | Title | Target |
|-----|-------|--------|
| [009](./accepted/009-reproducible-source-archives-and-release-gates.md) | Reproducible Source Archives and Release Gates | Phase 21 M1/M6 |
| [010](./accepted/010-transactional-payload-preserving-schema-migrations.md) | Transactional, Payload-Preserving Schema Migrations | Phase 21 M2 |
| [011](./accepted/011-safe-sqlite-identifier-boundary.md) | Safe SQLite Identifier Boundary | Phase 21 M2 |
| [012](./accepted/012-read-only-schema-and-mutation-contract.md) | Read-only Schema and Mutation Contract | Phase 21 M3 |
| [013](./accepted/013-panic-free-path-glob-and-cli-text-handling.md) | Panic-free Path, Glob, and CLI Text Handling | Phase 21 M3 |
| [014](./accepted/014-declared-msrv-and-dependency-security-policy.md) | Declared MSRV and Dependency Security Policy | Phase 21 M4 |
| [015](./accepted/015-async-runtime-and-watcher-failure-safety.md) | Async Runtime and Watcher Failure Safety | Phase 21 M5 |

### Proposed

*(none yet)*
