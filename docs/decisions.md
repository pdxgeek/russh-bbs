# Decisions

Decided items are not reopened without the owner. Open items need the owner before work depends on them.

## Decided

| ID | Decision | Why |
|---|---|---|
| D1 | The MVP works over plain `ssh` and `sftp` with **no custom client**. | "No install required" is the MVP goal. |
| D2 | **Registration mode is a sysop setting** with three values, default `invite`. `invite`: unknown keys reach only a waiting room (request or redeem an invite). `apply`: anyone can submit a membership application that lands in the sysop queue; the account is inactive until approved. `open`: an unknown key can create an active account at once, subject to rate limits and an optional cap on new accounts. Amended by the owner on 2026-11-01; the original decision was invite-only with no option. | Quiet by default; the invite tree is still the trust graph in `invite` mode. Sysops who want a classic apply-and-approve board or an open board get it by configuration, not by a fork. |
| D3 | In the MVP **the server can read everything** except PGP-encrypted mail and files users encrypt themselves. The product states this. | Real E2EE needs code on the user's machine; faking it would be dishonest. |
| D4 | True E2EE (DMs via double ratchet, groups via MLS) comes **after the MVP** with a native client. | A server-rendered session sees keystrokes. |
| D5 | Public spaces are server-side and plaintext; member-to-member private spaces are E2EE once the client exists. No mixed or silent-fallback rooms. | Locks must mean what they say. |
| D6 | Storage sits behind traits in core. **The datasource is a sysop setting**: `sqlite` (default, WAL, single file) or `postgres` (a server URL). The Postgres backend must also work against **PGlite**, which is used as the lightweight Postgres for development and CI through its wire-protocol socket server; there is no in-process PGlite embedding in Rust, so PGlite is a deployment of the Postgres backend, not a third backend. **MySQL and MariaDB are not supported** and will not be. Amended by the owner on 2026-11-01; the original decision was SQLite only. | SQLite keeps the single-binary, single-file story; Postgres is for sysops who already run it. Two dialects is the limit the project can test honestly. |
| D7 | Stack: `russh`, `tokio`, `ratatui`/`crossterm`, `sqlx`, `serde`/`toml`, `tracing`. | Library over bespoke. |
| D8 | Credits are an **internal, non-transferable ledger**. No token, no chain, not redeemable for money. | Avoids financial-service risk. Lightning tips are a possible later optional module. |
| D9 | Mail is staged: internal first (MVP), then inbound SMTP, then outbound through a relay, then IMAP. Never run a hand-written MTA. | Deliverability and abuse cost. |
| D10 | Menu tree, layout, theme, and icon set are **separate layers**. Security cues are fixed chrome. | Themeability without spoofing risk. |
| D11 | Offline readers (QWK) are **out of scope**. | Owner decision. |
| D12 | Door games: Rust-native first-party doors first; WASM sandbox for community doors later. Remakes are original ("clean-room"), not ports of copyrighted code, unless a license permits it. | Safety and copyright. |
| D13 | The software is named **russh-bbs**. A running board has its own name. Not affiliated with the `russh` project. | Software name vs. board name. |
| D14 | Federation, directory listing, finger, and NNTP are opt-in and post-MVP. | Quiet, invite-only feel. |
| D24 | Datasource design: one `Store` trait family in core, one backend module per database in `crates/store` (`sqlite`, `postgres`), each with its own forward-only migration set, since the dialects differ (FTS5 vs `tsvector`, `INTEGER PRIMARY KEY` vs `BIGSERIAL`). Shared behavior tests run against every backend. A feature is not marked Implemented until it passes on both. No `AnyPool` or dialect-neutral SQL that is only tested on one. | Dialect-neutral SQL hides bugs until the other backend is used in production. |
| D25 | The server ships as a **container image** as a first-class deliverable from M1: static musl build, non-root, unprivileged listen port (default 2222), all state under one volume (`/data`: SQLite file, host key, blobs), config from TOML plus environment overrides, logs to stdout, SIGTERM shutdown that checkpoints SQLite. The container is not the door sandbox (that rule holds inside the process). Decided by the owner on 2026-11-01. | Reproducible runs for members, sysops, and reviewers; the single binary makes this cheap. |
| D28 | **Password login over SSH is an opt-in, off by default.** `ssh.allow_password_login = false` ships. When on, a member may authenticate over SSH with keyboard-interactive auth and the same per-member argon2id password used by telnet (D27); the password is encrypted on the wire. Public-key auth stays the default and the only method in a fresh install. Lockout after repeated failures; the waiting room reveals nothing either way. Passwords are set or changed only over SSH and never required for key users. A second factor on top of a key is not in the MVP. Decided by the owner on 2026-11-02. | Lets members without a key at hand log in from a phone or a borrowed machine without opening the plaintext path. |
| D27 | **Telnet is an optional transport, disabled by default, and enabling it puts the whole board in a named low-security mode.** The config key is `telnet.enabled = false`; the setting, the sysop admin screens, and the docs call the enabled state **low-security mode**, and the sysop sees a persistent marker for it on every admin screen while it is on. The mode changes nothing about SSH connections except that marker; it exists so a sysop knows the board as a whole is no longer encrypted-only. when a sysop turns it on, unknown connections get the same waiting room as SSH, and member login over telnet requires a second opt-in `telnet.allow_login` plus a per-member password (argon2id hash, set only over SSH, never required for SSH). Every telnet screen carries a fixed `UNENCRYPTED` security marker that layouts and themes cannot hide (D10), and the banner says the session can be read on the wire. SFTP, invite redemption, and sysop functions are SSH-only. Telnet is an M3 slice; it must not change the SSH server. Decided by the owner on 2026-11-01. | Classic BBS reach for sysops who want it, without weakening the default. The marker and the separate password keep the honesty rule and keep SSH keys out of the clear. |
| D23 | The waiting room is built around the registration mode from the start (M1-C), and the `apply` and `open` flows ship in M2 with the sysop queue. The waiting room reveals nothing about members, boards, or stats in any mode; `apply` and `open` only add a registration path. | Avoids retrofitting the waiting room; the honesty and no-leak rules do not depend on mode. |

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
