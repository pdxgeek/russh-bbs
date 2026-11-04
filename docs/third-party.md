# Third-party crates: default choices

The listed crate is the **default** for its need under the library-first decision (D47); a slice uses it unless it records why not. Each adoption pins the version, confirms one ratatui version in `cargo tree`, passes `cargo deny`, and justifies the dependency in its PR. Where the license is not yet confirmed, the entry says "verify license on adoption". Any crate with system-clipboard access has it disabled, because on the server it would be the server's clipboard.

The security rules still win: the shared sanitizer is the only place untrusted text is cleaned, and no library may render member or external content with styles it carries.

## Terminal UI

| Need | Crate | Slice | Notes |
|---|---|---|---|
| Key input from raw SSH bytes, buffering sequences split across packets | termwiz `InputParser` (large, documented as unstable), `anes` (with the `parser` feature), `term-parser` | FIX-M1-1 | FIX-M1-1 is adopting one now |
| Single-line inputs | `tui-input` | M2-A | Evaluated in M2-A and not adopted yet (no new input; existing entry is core state machines with no ratatui dependency, D54). Default again for the M2-D/E forms |
| Multi-line editor | `tui-textarea-2` | M3 | Decision D48 |
| Popups; scroll views | `tui-popup`; `tui-scrollview` (both in `tui-widgets`) | M2-E, M2-F, M3 | |
| Trees | `tui-tree-widget` | M2-D (invite tree), M3 | |
| Banner text | `tui-big-text` | M2-A, M3 | Evaluated in M2-A and not needed (no banner drawn, D54); M3 themes |
| Sysop-supplied ANSI art | `ansi-to-tui` | M3 themes | **Never use it on member or external content**: it turns escapes into styles, so content could imitate the reserved marker style (D39) |
| Markdown display | `tui-markdown` | M3 | It must never produce the reserved marker style (bold+reverse); disable `highlight-code`/syntect unless needed |
| Wrapping and width | `textwrap`, `unicode-width`, `unicode-segmentation` | any | |
| Fuzzy search | `nucleo` | any | |

## Email

| Need | Crate | Slice | Notes |
|---|---|---|---|
| Internal mail | none needed | M3 | |
| Inbound parsing | `mail-parser` (Stalwart) | M6 | Apache-2.0 OR MIT per its source |
| SPF/DKIM/DMARC | `mail-auth` (Stalwart) | M6 | Verify license on adoption |
| Outbound via relay | `lettre` or `mail-send` | M6 | Verify licenses on adoption |
| Inbound SMTP | A real MTA (Postfix or Stalwart) in front, handing mail to us | M6 | D9: never run a hand-written MTA |
| PGP | Local `gpg`, as decided; `rpgp` (MIT/Apache) is the pure-Rust fallback | M6 | Sequoia is LGPL, which is not on the allow list |

## RSS

| Need | Crate | Slice | Notes |
|---|---|---|---|
| Parse | `feed-rs` | M3 | MIT; RSS 0.x/1.0/2.0, Atom, JSON Feed |
| Fetch | `reqwest` with rustls | M3 | |
| SSRF guard | Thin in-house wrapper | M3 | The justified hand-written exception. A custom DNS resolver rejects private, loopback, link-local and metadata addresses; re-check on every redirect; cap size and time |
| Feed HTML | `html2text`, then the shared sanitizer | M3 | Never render feed styling |
| RSS out | `rss` / `atom_syndication` | M3 | Verify licenses on adoption |
| Polling schedule | `croner` | M3 | Verify license on adoption |

## Higher functions

| Need | Crate | Slice | Notes |
|---|---|---|---|
| SFTP server | `russh-sftp` | M4 | Apache-2.0 |
| Blobs | `object_store` 0.14 (D30, D106; S3 client only: features `aws-base`, `reqwest`, `ring`), `reqwest` 0.13 with `rustls-no-provider`, `tokio-util` (`io`), `futures-util`, `bytes` | M4-F1 | SHA-256 via `sha2`, not `blake3` (D106). No aws-lc. The local backend is hand-written. |
| Content type detection | `infer` | M4 | |
| Archive inspection | `zip`, `tar` | M4 | With our own expansion caps |
| AV scanning | The clamd socket protocol, via a small client crate if a good one exists | M4 | |
| Password hashing | `argon2` (RustCrypto), argon2id | M3 | |
| Rate limiting | `governor` | later (LIB-1) | MIT; candidate to replace M1-C's hand-written limiter |
| Search | Database-native FTS5 and Postgres full-text search (D24) | M3 | No search crate |
| Markdown parsing | `pulldown-cmark` | M3 | |
| String templating | `minijinja` | M3 | Only if string packs need placeholders |
| GraphQL | `async-graphql` | M5 | Apache-2.0 OR MIT |
| Time | `jiff` or `time` | any | Choose one, workspace-wide |
| Door sandbox | `wasmtime` | M7 (post-MVP) | |

## Stays in-house by design

- The shared sanitizer (`russh-bbs-core::sanitize`).
- The SSRF wrapper.
- The single access-resolution function (D33).
