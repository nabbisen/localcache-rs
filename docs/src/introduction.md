# Introduction

`localcache` is a Rust library that makes it easy to cache the results of
expensive computations that are derived from local files.

## The problem it solves

Many data-processing pipelines repeat the same expensive operations — parsing
a document, generating an embedding, extracting features — every time they run,
even when the input files have not changed.

`localcache` solves this by:

1. Storing the result alongside a fingerprint of the source file (mtime, size,
   and optionally a BLAKE3 hash).
2. On subsequent runs, checking whether the file has changed before deciding
   to use the cached result or recompute it.

## Design goals

- **Simple public API** — a handful of methods on `CacheEngine<T>`.
- **Zero infrastructure** — a single SQLite file; no daemon, no network.
- **Safe by default** — atomic writes, foreign-key cascade deletes, no
  implicit background work.
- **Flexible payload type** — any `T: Serialize + DeserializeOwned`.
