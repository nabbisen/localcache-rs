# Terms of Use

`localcache` consists of the `localcache` library crate and the `localcache-cli` command-line
tool. Both are distributed under the **Apache License, Version 2.0**.

The license text in [LICENSE](LICENSE) is the only binding document. This page helps you find
your way around it. It does not replace the license, and it is not legal advice.

## Where the license and notices are

- **Repository and project source archives:** [LICENSE](LICENSE) and [NOTICE](NOTICE), at the
  root.
- **Crates on crates.io:** each crate's manifest declares `license = "Apache-2.0"`. The license
  text is at <https://www.apache.org/licenses/LICENSE-2.0>.

## Summary

This summary is not a substitute for the license. Each point names the section of
[LICENSE](LICENSE) it comes from.

- You may use, reproduce, modify, and distribute the work, in source or object form, for any
  purpose (§ 2).
- Each contributor grants a patent license. It terminates if you bring patent litigation
  alleging that the work infringes (§ 3).
- When you redistribute the work or a derivative work (§ 4):
  - give recipients a copy of the license (§ 4(a));
  - make modified files carry prominent notices that you changed them (§ 4(b));
  - retain the copyright, patent, trademark, and attribution notices of the source form (§ 4(c));
  - include the attribution notices from [NOTICE](NOTICE) (§ 4(d)).
- The license grants no rights to the licensor's names or trademarks, beyond customary use in
  describing the origin of the work (§ 6).
- The work is provided "AS IS", without warranties (§ 7), and contributors are not liable for
  damages arising from its use (§ 8).

## Third-party components

The `localcache` source contains no vendored third-party code. Its dependencies are listed in
`Cargo.toml`, with the exact versions in `Cargo.lock`. Cargo fetches them at build time, and each
is governed by its own license.

With the dependency configuration this project ships, `rusqlite` compiles the SQLite library from
source (the `bundled` feature of `libsqlite3-sys`), so SQLite is linked into your builds. See
<https://www.sqlite.org/copyright.html> for SQLite's own terms.

To review every dependency's declared license, run
`cargo metadata --format-version 1` and read each package's `license` field.

## Security

Please report suspected vulnerabilities privately, as described in
[.github/SECURITY.md](.github/SECURITY.md).
