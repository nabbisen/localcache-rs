# RFCs

This directory contains design specifications for `localcache`.
The lifecycle of RFC files (folder layout, states, numbering, status fields,
cross-references) is governed by **[RFC 000](./done/000-rfc-lifecycle-policy.md)**.

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

### Proposed

| RFC | Title | Target |
|-----|-------|--------|
| [009](./proposed/009-reproducible-source-archives-and-release-gates.md) | Reproducible Source Archives and Release Gates | Phase 21 M1/M6 |
