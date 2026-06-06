# RFCs

This directory contains design specifications for `localcache`.
The lifecycle of RFC files (folder layout, states, numbering, status fields,
cross-references) is governed by **[RFC 000](./done/000-rfc-lifecycle-policy.md)**.

## Templates

**Lightweight** (small / unambiguous scope):

```markdown
# RFC NNNN — Title
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
# RFC NNNN — Title
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
| [0001](./done/0001-recursive-directory-watching.md) | Recursive Directory Watching | v0.17.0 |
| [0002](./done/0002-query-index-hints.md) | Query Index Hints and Explain Plan | v0.17.0 |
| [0003](./done/0003-opentelemetry-spans.md) | OpenTelemetry Spans | v0.17.0 |
| [0004](./done/0004-shared-memory-db.md) | Read-only Shared-memory DB Mode | v0.17.0 |
| [0005](./done/0005-async-std-smol.md) | async-std / smol Feature Variants | v0.17.0 |

### Archive

*(none yet)*

### Proposed

*(none — all pending RFCs shipped in v0.17.0)*
