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
- Make a query read a different schema object than the one intended: the `main` schema wins
  over temporary and attached objects with the same name.

**Can (and what you can do about it):**

- **Abort a reader through a forged length prefix** (known issue in v0.22.0, fixed in
  v0.22.1). A stored uncompressed (`raw`, bincode) payload with a forged length prefix can
  make `get` abort the process with an allocation failure when the payload type contains a
  `String` or another type that bincode decodes through its own string and byte-buffer path.
  The entry is not lost and nothing is corrupted. It needs write access to the file or an
  imported export. Entries encrypted with a key are not affected (the forgery fails
  authentication first), nor are `Codec::Json` payloads. Until the fix: restrict who can
  write the file, import only exports you trust, and prefer an encryption key or the JSON codec
  where strings are in the payload.
- **Replace or swap entries.** Without a key, the cache holds whatever the file holds. With a
  key, an attacker cannot read or forge a payload, but the ciphertext is not bound to its row,
  so someone with write access can move an encrypted payload from one path to another. Binding
  ciphertext to its row is planned with the next stored-format change (v0.23.0).
- **Fill the disk or the cache.** `max_entries` bounds the number of entries and
  `max_decompressed_bytes` the size of a decompressed payload; the library does not bound the
  size of an uncompressed payload, which is exactly as large as the blob in the file.
- **Make stale data look fresh.** Freshness is decided from file metadata and, depending on the
  change-detection mode, a content hash. `ChangeDetectionMode::MetadataOnly` compares only modification
  time and size, so someone who can change a file and restore its time can fool it;
  `MetadataThenPartialHash` samples the first and last 64 KiB and misses a same-size change confined to the
  middle of a large file. `MetadataThenFullHash` and `StrictFullHash` hash the whole file. Choose a full-hash
  mode when the files can be changed by someone who can also set times.

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

What it does **not** do:

- It encrypts **payload content only**. Paths, namespaces, sizes, modification times, hashes
  and timestamps are stored in the clear, and the database file itself is not encrypted. Use
  disk encryption for that.
- The key lives in your process. The library copies it into engine memory and does not zero it
  on drop; do not assume it is erased. Losing the key makes every encrypted entry permanently
  unreadable. There is no recovery.
- A missing key is an error (`MissingEncryptionKey`), never a silent read of ciphertext.
- Authentication does not bind a payload to its path (see above).

## Decoding limits at a glance

| Payload kind | Bound | Error |
|---|---|---|
| zstd-compressed (`compression` feature) | decompressed size ≤ `max_decompressed_bytes` (64 MiB default), checked on read and on write | `PayloadTooLarge { limit }` |
| uncompressed bincode (`raw`) | none beyond the blob's size; see the known issue above for forged lengths | `Serialization` or an abort for the known issue |
| JSON | the document's size; nesting depth is limited by the JSON parser, a million opening brackets is an error and not a stack overflow | `Serialization` |
| unknown or foreign encoding tag | not decoded | `UnknownEncoding` |

## Supply chain and releases

- **Dependencies** are checked by `cargo deny` on every push and every week: advisories
  (vulnerabilities, unsound and unmaintained crates, yanked versions) and sources (crates.io
  only, no Git dependencies). A standing exception must carry its reason and the condition
  that ends it; the two current ones are the `async-std` runtime feature and the `bincode`
  payload format (see [MSRV & Dependency Security](./dependency_security.md)).
- **Releases** are published by one GitHub Actions workflow, not from a workstation, through
  crates.io Trusted Publishing: the workflow exchanges a short-lived GitHub identity for a
  registry token that lasts 30 minutes. A real run starts only when the owner starts it with
  `dry_run` off, from a signed tag equal to the workspace version, on a commit on `main` with
  green CI. No long-lived registry token is stored in the repository or in CI.
  The source archive of each release is built from that commit and carries a GitHub build
  attestation (`gh attestation verify`). The tag's signature is made on the owner's machine and
  is not verified by CI (see [Project Source Archives](./source_archives.md)).
- **The build** is read-only by default in CI: no job holds a secret, no workflow uses
  `pull_request_target`, and every third-party action is pinned to a commit.

## Recommendations

1. Keep the database file where only the user of the cache can write it. That one measure
   removes most of this page's "can".
2. For payloads that contain strings, use an encryption key or the JSON codec until v0.22.1.
3. Raise `max_decompressed_bytes` only for payloads you know are large and trusted.
4. Import only exports you produced or trust.
5. Treat an `IncompatibleSchema` or `PayloadTooLarge` error as "this entry or file is not
   what I expected", not as a transient failure to retry.
