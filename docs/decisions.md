# Decisions

Decided items are not reopened without the owner. Open items need the owner before work depends on them.

## Decided

| ID | Decision | Why |
|---|---|---|
| D1 | The MVP works over plain `ssh` and `sftp` with **no custom client**. | "No install required" is the MVP goal. |
| D2 | The board is **invite-only**. Unknown keys reach only a waiting room (request or redeem an invite). | Quiet community; the invite tree is also the trust graph. |
| D3 | In the MVP **the server can read everything** except PGP-encrypted mail and files users encrypt themselves. The product states this. | Real E2EE needs code on the user's machine; faking it would be dishonest. |
| D4 | True E2EE (DMs via double ratchet, groups via MLS) comes **after the MVP** with a native client. | A server-rendered session sees keystrokes. |
| D5 | Public spaces are server-side and plaintext; member-to-member private spaces are E2EE once the client exists. No mixed or silent-fallback rooms. | Locks must mean what they say. |
| D6 | Storage is **SQLite** (WAL) behind a trait. | Single-file, easy backups, swappable via dependency inversion. |
| D7 | Stack: `russh`, `tokio`, `ratatui`/`crossterm`, `sqlx`, `serde`/`toml`, `tracing`. | Library over bespoke. |
| D8 | Credits are an **internal, non-transferable ledger**. No token, no chain, not redeemable for money. | Avoids financial-service risk. Lightning tips are a possible later optional module. |
| D9 | Mail is staged: internal first (MVP), then inbound SMTP, then outbound through a relay, then IMAP. Never run a hand-written MTA. | Deliverability and abuse cost. |
| D10 | Menu tree, layout, theme, and icon set are **separate layers**. Security cues are fixed chrome. | Themeability without spoofing risk. |
| D11 | Offline readers (QWK) are **out of scope**. | Owner decision. |
| D12 | Door games: Rust-native first-party doors first; WASM sandbox for community doors later. Remakes are original ("clean-room"), not ports of copyrighted code, unless a license permits it. | Safety and copyright. |
| D13 | The software is named **russh-bbs**. A running board has its own name. Not affiliated with the `russh` project. | Software name vs. board name. |
| D14 | Federation, directory listing, finger, and NNTP are opt-in and post-MVP. | Quiet, invite-only feel. |
| D15 | Workspace uses **edition 2024** and resolver 3, toolchain **pinned to an exact stable release** in `rust-toolchain.toml` (bumped deliberately in its own change), workspace crates `publish = false`, with no license declared. | The rust-coding skill assumes 2024 (AGENTS.md allows 2021 or later); a new repo gains nothing from an older edition. The pin exists because floating `stable` made local and CI clippy disagree (a newer lint failed only in CI); the trade-off is a manual bump. `publish = false` plus `licenses.private.ignore` in `deny.toml` lets cargo-deny pass without inventing a license (Q1 stays open). |
| D16 | Sanitizer limits count Unicode scalar values (`char`), not bytes or grapheme clusters. Escape sequences are parsed by a small hand-written scanner (ESC, CSI, OSC, DCS/SOS/PM/APC, nF and two-byte escapes, and the 8-bit C1 forms); an unterminated string sequence swallows the rest of the input, as a terminal would. Private-use and noncharacter codepoints become U+FFFD (visible, stable, not itself private-use) rather than being deleted; controls and bidi/invisible characters are deleted. Single-line context turns newline, CR and tab into a space; multi-line keeps `\n` and `\t` and normalizes CR and CRLF to `\n`. | Graphemes need a segmentation dependency and Unicode tables for no security gain; a `char` cut can at worst leave a trailing combining mark, which is harmless once controls are gone. Replacement keeps users aware that something was there. Properties (idempotence, no ESC/control/PUA/bidi in output) hold because ESC and C1 never survive. |
| D17 | Usernames are **rejected, not repaired**: NFKC, then only `[A-Za-z0-9_-]`, 2 to 24 characters, starting with a letter or digit. Confusables: non-ASCII is rejected outright, and a minimal ASCII skeleton (`username_skeleton`) is the uniqueness key. Unicode names can be added later with a real confusables table. | Strict and boring is the safe default for names that appear in security-relevant places. Adds only the `unicode-normalization` dependency. |

## Open (owner needed)

| ID | Question | Notes |
|---|---|---|
| Q1 | License for the repo. | Do not add a license file until decided. |
| Q2 | Board name and domain. | Candidates: Chatsubo, Lowtek. Domains checked by the owner. |
| Q3 | GraphQL as the single API for the future client, or a separate SSH-subsystem protocol? | Leaning single GraphQL API. Decide before M5/M9 design work. |
| Q4 | `sqlx` vs `rusqlite` for the store crate. | Leaning `sqlx` (checked queries). Decide in M1. |
| Q5 | How the TUI is driven over the SSH PTY (direct ratatui backend on the channel vs. a terminal abstraction). | Prototype in M1 and record the result. |
| Q6 | Whether the sysop can "spy" on sessions, and whether users are notified. | Default if built: notify the user. |
| Q7 | Plain-ssh private messages: offered (labeled "sysop can read") or hidden until the client exists? | Either must be labeled honestly. |

## Naming reminders

- Candidate board names come from William Gibson's work. Using a name is fine; do not copy passages into screens or docs.
- `ssh.chat` is an existing project. Do not mimic it.
