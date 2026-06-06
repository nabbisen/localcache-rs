# RFCs

This directory contains design specifications for planned features.
Each file covers one roadmap theme and serves as both the external API
design and the internal implementation guide for the developer picking
up the work.

## Template

**Lightweight** (small / unambiguous scope):

```markdown
# RFC NNNN — Title
| Field | Value |
|-------|-------|
| Status | Proposed / Accepted / Implemented |
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

The "Background" section is optional in both templates; include it when
historical context materially helps the implementor.

---

## Index

| RFC | Title | Status |
|-----|-------|--------|
| [0001](./0001-recursive-directory-watching.md) | Recursive Directory Watching | Proposed |
| [0002](./0002-query-index-hints.md) | Query Index Hints and Explain Plan | Proposed |
| [0003](./0003-opentelemetry-spans.md) | OpenTelemetry Spans | Proposed |
| [0004](./0004-shared-memory-db.md) | Read-only Shared-memory DB Mode | Proposed |
| [0005](./0005-async-std-smol.md) | async-std / smol Feature Variants | Proposed |
