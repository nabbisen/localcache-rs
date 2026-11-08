# Security Model

This page says what `localcache` protects, from whom, and what it does not. It is
written for someone deciding whether the crate fits a threat model, and it states
limits plainly. For reporting a vulnerability, see the repository's `SECURITY.md`.

## What the crate is, in security terms

`localcache` is a library that keeps a cache in one SQLite file next to your
program. It has no server, no network code and no privileges of its own: it runs
with the permissions of the process that uses it. Its security properties are
therefore about **what a hostile or damaged input can make the library do**, not
about who may use the cache. Access to the cache is access to the file, and the
operating system decides that.

## Trust boundaries

| Input | Trusted? | What the library does with it |
|---|---|---|
| The caller's code and options | yes | Validates options before touching the database (see "Rejected before any effect") |
| Paths you pass | yes, but checked | Resolved to a canonical absolute UTF-8 path; a path that is not valid UTF-8 is `InvalidPath`; a missing file is `FileNotFound` |
| The files you cache | read only | Read for metadata and hashing; never executed; symlinks are not followed by scans |
| The database file | **not trusted to be well-formed** | Its schema is classified before use; an unrecognised or newer schema is refused and left unchanged; every stored payload is decoded defensively (below) |
| An `ExportRecord` you import | **not trusted** | Stored as given after Base64 validation; decoded only when read |
| Index names and glob patterns | checked | Index names must match a fixed identifier grammar; glob patterns have size and depth limits; hostile text is never echoed into an error or into SQL |

The database file is the interesting boundary. Anyone who can **write** it can put
anything in it. The rest of this page is mostly about what that can and cannot do.

## What a hostile database file can do

**Cannot:**

- Run SQL of its choosing through the library. User-controlled text (paths, namespaces,
  index names, glob patterns) reaches SQLite as bound parameters or, for index names,
  only after the identifier grammar check; a name outside the grammar is refused and
  changes nothing.
- Make the library migrate or rewrite a database it does not recognise. A read-only open
  never initializes or migrates; a writable open of a schema it does not recognise
  returns `IncompatibleSchema` and the file is left byte-identical.
- Make a compressed payload expand without bound. A stored zstd frame is decompressed
  only up to `max_decompressed_bytes` (default 64 MiB) and is then refused with
  `PayloadTooLarge`: a header that declares a larger size is refused before anything is
  allocated, and an undeclared stream is cut off at the limit. A forged declared size is a
  malformed frame, never an allocation. Memory for a refused read stays near the limit
  (measured 1.25 times the limit for frames this crate writes, 2.2 times for a hostile header
  that asks for a 128 MiB decoder window, which is zstd's own constant maximum).
- Make a read abort through a forged length or a count, or overflow the stack through nesting
  (fixed in v0.22.1; it was a Known issue of v0.22.0). The decoder for uncompressed
  (`raw`) payloads is the project's own (RFC 028), in the crate `localcache-format`. It checks every string, byte-buffer, sequence
  and map length against the bytes that remain **before** it allocates or loops, so a payload
  that claims a gigabyte but holds four bytes is a `Serialization` error, and it refuses nesting
  deeper than 128 levels (the limit `serde_json` applies to documents). No allocation is made
  from a claimed length: a string is allocated at its verified length, and a sequence or map is
  built element by element, the standard collections reserving room up front for at most the
  verified count and never more than 1 MiB per open nesting level (address space, not touched
  memory), so at most 128 MiB however large the claim, and never more than the previous decoder
  allocated on the same input. A small hostile payload can briefly hold tens of megabytes
  (measured: 60 MB for a 21 KB payload of a recursive `Vec` type at 63 levels, released when the
  read fails), and a decoded value can be larger than its blob (a one-byte `None` becomes a
  whole `Option<T>`). The counts of all the sequences and maps in one read together may not
  exceed the length of the blob, so the time to decode is proportional to the blob. Writing
  follows the same limits: a value nested deeper than 128 levels is refused by `set`, so the
  cache never stores what it would refuse to read. Two things that v0.22.0 accepted are
  refused: a
  sequence or map of zero-byte elements (`Vec<()>`) whose count is larger than the bytes left,
  because its count cannot be told from a forged one, and a value nested deeper than 128
  levels (a level is a struct, sequence, map, enum, tuple, newtype or `Option` that holds a
  value, inside another). No known payload uses either.
- Make a query read a different schema object than the one intended: the `main` schema wins
  over temporary and attached objects with the same name.

**Can (and what you can do about it):**

- **Make a read fail, or use a lot of time.** A payload can be made to be an error (every
  such read returns `Serialization`; the entry stays in the file), and a payload that really
  contains a large value decodes to a large value: the size of an uncompressed payload is the
  size of the blob, and the decoded value can be larger than the blob (a one-byte `None` becomes
  a whole `Option<T>`). Decoding time is proportional to the blob.
- **Replace or swap entries.** Without a key, the cache holds whatever the file holds. With a
  key (v0.24.0 and later), an attacker who can write the file cannot read a payload, cannot forge
  one, cannot move one to another path, cannot downgrade one to plain and cannot change how a
  sealed entry is decoded (see "Encryption" below). What they can still do is delete entries,
  restore an older valid ciphertext of the same row, and move a payload between namespaces that
  share a key. Before v0.24.0 the ciphertext was not bound to its row, so a payload could be moved to
  another path and a plain entry was read as if it were protected.
- **Fill the disk or the cache.** `max_entries` bounds the number of entries and
  `max_decompressed_bytes` the size of a decompressed payload; the library does not bound the
  size of an uncompressed payload, which is exactly as large as the blob in the file.
- **Make stale data look fresh.** Freshness is decided from file metadata and, depending on the
  change-detection mode, a content hash. `ChangeDetectionMode::MetadataOnly` compares only modification
  time and size, so someone who can change a file and restore its time can fool it;
  `MetadataThenPartialHash` samples the first and last 64 KiB and misses a same-size change confined to the
  middle of a large file. `MetadataThenFullHash` and `StrictFullHash` hash the whole file. Choose a full-hash
  mode when the files can be changed by someone who can also set times.

**Fixed in v0.26.0: the bundled SQLite (record).** Versions of `localcache` through 0.25.1 bundle SQLite 3.53.2 (through
`rusqlite 0.40`). [CVE-2026-119375](https://nvd.nist.gov/vuln/detail/CVE-2026-119375) (published 2026-10-30, fixed in
SQLite 3.54.0) lets a crafted database file, together with a sibling `-wal` file, make ordinary queries return
uninitialized page-cache memory; its CVSS score is 6.8 (v4) and 5.5 (v3.1), with a local attack vector. Those
versions are affected as described. **v0.26.0 bundles SQLite 3.54.1, which fixes it**, and raised the minimum supported
Rust version to 1.89 to do so ([RFC 023](https://github.com/nabbisen/localcache-rs/blob/main/rfcs/accepted/023-msrv-policy.md)
R3.1: the fix exists only in a SQLite that needs the newer toolchain). The v0.25 line gets no backport, because the fix
itself needs the raise.

- **Whom it concerned.** A process that opens a database file, with its `-wal`, that a less trusted party could have
  written: the CLI's `copy --from-db` and any import of a foreign file, a cache in a directory others can write, a cache
  restored from an untrusted backup. A cache that only your own user can create was outside the model.
- **What to do.** Upgrade to v0.26.0 or later (Rust 1.89 or newer is needed). Until you can, the advice stands: keep the
  cache directory writable only by the user of the cache (recommendation 1 below), and do not open a database file, or its
  `-wal`, that someone less trusted could have written.
- **What the schema check does and does not do** (unchanged). It refuses a database file that holds any application object
  this crate did not create (a virtual table, a trigger, a view), which closes the extension-table and view
  vulnerabilities of 2026. It reads through SQLite's own page reader, so it does not protect against a flaw at the
  page level, which is where this one was.

## Rejected before any effect

Invalid configuration is refused before the database is created, opened, migrated or
configured: a rejected `open` leaves the filesystem and an existing database exactly as it
found them. Rejected: read-only with an in-memory database, an encryption key that is not
32 bytes, `max_entries(0)`, a TTL under one second, a payload limit of zero, a `ReadPool` of
size zero or on `:memory:`. Calls with invalid arguments (a malformed glob, an index name
outside the grammar, a `batch_set` or import with more distinct paths than `max_entries`)
are refused before a transaction starts. Each has its own error variant, so a caller can
act on it without reading the message.

## Encryption

With the `encryption` feature and a 32-byte key, payload bytes are encrypted with AES-256-GCM
using a fresh 96-bit nonce per write and stored as nonce followed by ciphertext. GCM
authenticates the ciphertext, so a payload altered without the key fails to decrypt (an
`EncryptionError`) and never reaches the decoder.

**With a key, an entry is readable only if it is sealed under that key and bound to its row**
(v0.24.0). Each payload is sealed with associated data made of a fixed label, the **full encoding
tag** and the **entry's path as stored**, so the ciphertext authenticates which entry it belongs to
and how it is to be decoded. A bound entry carries a tag ending in `-aes256gcm-bound`. With a key,
anything else is an error that names the remedy, never a silent read: a plain entry, or one sealed
by v0.23 or earlier without binding, is `UnsealedEntry`; a bound entry that does not open (wrong
key, damage, or moved from its path) is `EncryptionError`. An engine without a key reads plain
entries as before and names a missing key (`MissingEncryptionKey`) for sealed ones. A query on an
engine with a key never evaluates payloads in SQL. A database written by v0.23 or earlier needs one
`rotate_encryption_key` with the same key, which seals every entry of the namespace bound to its path
(see [Migration](./migration.md#encryption-integrity-v0240)).

**What an attacker who can write the file, but has no key, can and cannot do:**

| | |
|---|---|
| Read a payload | **cannot** (AES-256-GCM) |
| Forge a payload | **cannot** (without the key a ciphertext does not authenticate) |
| Move an entry to another path | **cannot** (the path is authenticated; the entry fails to open) |
| Downgrade an entry to plain | **cannot** (with a key a plain or unbound entry is refused, and editing a bound tag fails to open) |
| Change how a sealed entry is decoded | **cannot** (the tag is authenticated) |
| Delete entries | **can** |
| Restore an older valid ciphertext of the same row | **can** (freshness is not authenticated: the attacker can rewrite the row's metadata too) |
| Move a payload between namespaces that share a key | **can**: the namespace is not bound (binding it would make every raw copy, `import_from` and the CLI's `copy`, re-seal entries, which the CLI cannot do because it takes no key). **Use one key per trust domain** |
| Read all metadata | **can** (paths, namespaces, sizes, times, hashes and the encoding tag are stored in the clear) |

Not protected, and not claimed: an attacker who can read the process's memory.

**The nonce limit.** With random 96-bit nonces, NIST SP 800-38D allows one key at most 2^32
encryptions (about 50 days at 1,000 writes per second). Rotation starts a new key, so rotate before
a key has sealed that many entries.

What it does **not** do:

- It encrypts **payload content only**. Paths, namespaces, sizes, modification times, hashes
  and timestamps are stored in the clear, and the database file itself is not encrypted. Use
  disk encryption for that.
- The key lives in your process. From v0.23.1 the crate does not keep its own copies of the key
  after they are dropped: it holds the key in a type that erases itself on drop, and it erases
  the `Vec<u8>` it takes from the options and from the builder. From v0.26.2 the
  cipher object that seals or opens one entry is overwritten when it is dropped too: the round keys
  and the GHASH key that the cipher library derives from the key live in that object, and the
  library's `zeroize` feature, which the crate now enables, writes zeros over all of it. A test
  checks that wiring and fails if the feature is removed. That is all it claims. Erasure on drop is
  best effort: Rust does not promise that the compiler or the CPU left no other copy (a register, a
  stack temporary, the 32-byte array the key was copied from), and nothing here proves a copy is
  gone, so a copy can remain. The crate cannot erase your own copy of the key (the `Vec` you passed
  in, a clone of the options you keep), other code's stack frames, those temporaries, or the swap
  file, so do not assume the key is gone from the process. Before v0.23.1 the crate erased nothing,
  and up to 0.26.1 the cipher's own round keys were not overwritten. Losing the key makes every
  encrypted entry permanently unreadable. There is no recovery.
- A missing key is an error (`MissingEncryptionKey`), never a silent read of ciphertext.
- Fixed in v0.23.1: `CacheOptions` printed the encryption key with `{:?}`; it now prints
  `Some("[redacted]")`. In v0.23.0 and earlier it still does, so do not log the options of an
  engine that has a key under those versions. No other type of the crate can print the key.
- It does not authenticate **freshness** (see the table), and it does not isolate namespaces that
  share a key.

## Decoding limits at a glance

| Payload kind | Bound | Error |
|---|---|---|
| zstd-compressed (`compression` feature) | decompressed size ≤ `max_decompressed_bytes` (64 MiB default), checked on read and on write | `PayloadTooLarge { limit }` |
| uncompressed binary (`raw`, the legacy `bincode` wire format) | every length and count must fit the bytes that remain, and all counts in one read together the blob's length, checked before anything is allocated; nesting ≤ 128 levels, on write too; the blob's own size is not limited | `Serialization` |
| JSON | the document's size; nesting depth is limited by the JSON parser, a million opening brackets is an error and not a stack overflow | `Serialization` |
| unknown or foreign encoding tag | not decoded | `UnknownEncoding` |
| with a key, an entry that is plain or sealed without binding | not decoded | `UnsealedEntry` |

## Supply chain and releases

- **Dependencies** are checked by `cargo deny` on every push and again at every release: advisories
  (vulnerabilities, unsound and unmaintained crates, yanked versions) and sources (crates.io
  only, no Git dependencies). That check reads the RustSec database: a vulnerability in the C source of the SQLite
  that `libsqlite3-sys` bundles is reported only if RustSec lists it, and the known issue above is not
  something it reports. A standing exception must carry its reason and the condition
  that ends it; there is none at present (see [MSRV & Dependency Security](./dependency_security.md)). The two that
  existed ended in v0.26.0 with the removal of the `async-std` feature (`RUSTSEC-2025-0052` for `async-std` and
  `RUSTSEC-2026-0441` for `pin-utils`, which only `async-std` depended on), and the `bincode` exception ended in
  v0.22.1, when the payload format's reader and writer became the project's own code.
- **Releases** are published by one GitHub Actions workflow, not from a workstation, through
  crates.io Trusted Publishing: the workflow exchanges a short-lived GitHub identity for a
  registry token that lasts 30 minutes. A real run starts only when the owner starts it with
  `dry_run` off, from a signed tag equal to the workspace version, on a commit on `main` with
  green CI. No long-lived registry token is stored in the repository or in CI.
  The source archive of each release is built from that commit and carries a GitHub build
  attestation (`gh attestation verify`). The tag's signature is made on the owner's machine and
  is not verified by CI (see [Project Source Archives](./source_archives.md)).
- **Three crates are published**, always at one version: `localcache-format` (the payload format: the decoders, the decompression bound and the sealing this page describes), `localcache` and `localcache-cli`. `localcache` depends on `localcache-format` at exactly its own version. The format crate is an implementation crate: depend on `localcache`.
- **The build** is read-only by default in CI: no job holds a secret, no workflow uses
  `pull_request_target`, and every third-party action is pinned to a commit.

## Recommendations

1. Keep the database file where only the user of the cache can write it. That one measure
   removes most of this page's "can".
2. Raise `max_decompressed_bytes` only for payloads you know are large and trusted.
3. Import only exports you produced or trust.
4. Treat an `IncompatibleSchema`, `PayloadTooLarge` or `Serialization` error on a read as
   "this entry or file is not what I expected", not as a transient failure to retry.
