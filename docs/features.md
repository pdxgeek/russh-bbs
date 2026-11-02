# Feature register

Status values: **Planned**, **In progress**, **Implemented** (code exists), **Verified** (tests and a real `ssh` run). Everything is Planned today.

Update this table in the same pass as the code.

| Feature | Milestone | Status | Notes |
|---|---|---|---|
| QA pass per milestone (`docs/qa/`) | all | Planned | Gate before the next milestone starts |
| Workspace, CI, threat model | M0 | Implemented | Workspace, toolchain, deny and CI exist and pass locally (fmt, clippy, test, deny); CI itself not yet seen running on GitHub. Threat model is a doc. Not yet Verified |
| Shared sanitizer | M0 | Implemented | `russh-bbs-core::sanitize`: controls, ANSI/CSI/OSC/DCS/APC/PM/SOS and 8-bit C1 sequences, bidi, invisible filler and format characters, Unicode-space mapping, combining-mark cap, private-use and reserved-chrome neutralization, char-count limits, `Sanitized` newtype, NFKC + strict ASCII usernames, reserved usernames, ASCII lookalike skeleton with digit and pair folds (D40, FIX-M0-1). Unit and property tests; no fuzz target yet (Planned). Not yet used by any front end. Not yet Verified |
| Icon registry and width table | M0 | Implemented | `russh-bbs-core::icons`: closed enums (`ContentIcon`, `SecurityMarker`, `Icon`), three tiers, text labels, explicit width table, padded `render`. Reserved presentation per tier (style, `⟦ ⟧` delimiters, ASCII limit stated honestly, D39); `ContentIcon::Restricted`. Unit and property tested incl. sanitizer round-trip. Not used by any front end yet. Multi-terminal alignment **not verified** (needs real terminals). Not yet Verified |
| SSH server, host key, public-key auth | M1 | Verified | M1-A (`russh-bbs-server::ssh`, `russh-bbs-core::access`). Persistent Ed25519 host key (0600, outside the repo, stable fingerprint logged at start); public-key-only auth; every unknown key gets a restricted session showing a placeholder screen (the real waiting room is M1-C). Lockdown: exec, subsystem/sftp, env, agent, X11, direct-tcpip, tcpip-forward and streamlocal all refused. PTY size honored, clamped, and updated on resize; auth timeout, auth-attempt cap, idle timeout and connection cap. 23 unit + 31 integration tests (real russh client on loopback) and a manual run with OpenSSH `ssh`/`sftp` (see PR). **Not covered:** per-IP and per-key rate limits (M1-C), RSA client keys (unsupported by design, D21), clients other than OpenSSH and russh, CI on GitHub, load or fuzz testing of the transport. Members do not exist yet (M1-B), so `Access::Member` is untested end to end |
| Waiting room (banner, rules, request, redeem), mode-aware | M1 | Implemented | M1-C, `m1/waiting-room`. Screen set follows `registration_mode` (invite: request + redeem; apply: application; open: create account), rules in core, drawn by the SSH front end. Banner and rules are sysop settings, sanitized. Forms take sanitized, length-capped input and then say plainly that the feature is not open yet and nothing was stored or sent: the real invite, apply and create-account flows are M2. Tested in all three modes, including no-leak tests and a real russh client; walked with OpenSSH. D37 |
| Per-IP and per-key rate limits | M1 | Implemented | M1-C. GCRA limiter in core with a fake clock; per network at accept, per key at auth, per-session form cap; bounded memory with eviction. In-memory (resets on restart). D37 |
| Founder / first-run sysop claim | M1 | Planned | One-time setup token |
| Per-user terminal settings | M1 | Planned | width, height, color, pause, tier |
| Login and logoff screens, themeable strings | M1 | Planned | |
| SQLite store and migrations | M1 | Implemented | `m1/store`. `sqlx` (D34), WAL, forward-only checksummed migrations (D36), accounts, key fingerprints, settings; `MemberDirectory` over the store (active accounts only, fails closed). Tested on temp-file DBs and with OpenSSH. Account/state transitions beyond create, and the founder flow, are M1-D/M2 |
| Postgres backend (also used with PGlite) | M1 | Planned | M1-G. Same behavior tests as SQLite, one suite generic over the core store traits; D6, D24 |
| Datasource selection in config | M1 | Partially implemented | `datasource.kind` = `sqlite` (works, with `path`) or `postgres` (url, TLS mode, root cert, IAM flag, D31: all parsed and validated, but **selecting it fails at boot** with "postgres backend is not built yet" until M1-G; no silent fallback); never MySQL. Unit-tested; the postgres error and the sqlite path were exercised with the real binary (M1-H) |
| Container image, compose example, GHCR publish | M1 | Planned | D25 |
| Config file (TOML) with env overrides, documented keys | M1 | Implemented | M1-H. `russh-bbs-server::config`: defaults < file < `RUSSH_BBS_*` env < CLI flags, unknown keys and bad values rejected naming key and source, URL password redacted, `config.example.toml` equals the defaults (tested). `ssh`, `datasource`, `blobs`, `telnet`, `waiting_room` and `registration_mode` are present; `allow_password_login`, `telnet.*`, `blobs.*` and the postgres fields are parsed only. File and env layers verified with OpenSSH. `docs/config.md`, D38 |
| Menu tree, classic hotkey layout, string table | M2 | Planned | Moved from M3; foundation for every later screen |
| Named permissions per action, roles as permission sets, groups, per-resource policies and limits | M2 | Planned | D33, [access.md](access.md); `core::access`; before invites and queue |
| Audit log (append-only table, write helper) | M2 | Planned | Moved from M5; sysop UI over it stays in M5 |
| Invite codes, quotas, invite tree | M2 | Planned | |
| Sysop approval queue and vouching | M2 | Planned | |
| Registration mode setting (`invite`, `apply`, `open`) | M2 | Planned | Default `invite`; D2, D23. Waiting room built mode-aware in M1 |
| User profiles and members list | M2 | Planned | |
| Resource limits per role (time, calls, sessions, rates, bytes) | M2 | Planned | Part of the access slice; per-member override |
| Lightbar and compact layouts, theme packs, live switching | M3 | Planned | See [ui.md](ui.md); menu tree and classic layout are M2 |
| Forums with search | M3 | Planned | FTS5 |
| Full-screen message editor | M3 | Planned | quoting, drafts |
| Public chat rooms, presence, whispers | M3 | Planned | Plaintext tier |
| One-liners, graffiti wall, last callers | M3 | Planned | |
| Polls and voting booths | M3 | Planned | |
| RSS in (feeds as boards) and RSS out | M3 | Planned | SSRF guard required |
| Internal mail (stage 1) | M3 | Planned | |
| Telnet transport, disabled by default (low-security mode) | M3 | Planned | D27; `UNENCRYPTED` marker; password login is a second opt-in |
| Password login over SSH, disabled by default | M3 | Planned | D28; shares the password store with telnet |
| Credits ledger | M3 | Planned | Non-transferable |
| Event scheduler | M3 | Planned | |
| Blob store trait, local and S3-compatible backends | M4 | Planned | D30; `object_store`; one test suite |
| File areas over SFTP, scanning, quarantine | M4 | Planned | Streams through the blob store |
| File tagging and batch download | M4 | Planned | |
| Global new-scan (posts, files, feeds) | M4 | Planned | |
| Door framework and first games | M4 | Planned | Rust-native |
| Sysop admin TUI | M5 | Planned | |
| Page the sysop and sysop chat | M5 | Planned | |
| Moderation workflow, audit log UI | M5 | Planned | Writes exist since M2 |
| Notifications and digests | M5 | Planned | |
| Backups and tested restore, every backend | M5 | Planned | SQLite, Postgres, blob store |
| Data export and account deletion | M5 | Planned | |
| Accessibility mode | M5 | Planned | |
| GraphQL read-only API | M5 | Planned | |
| External mail (inbound, outbound) | M6 | Planned | Post-MVP |
| PGP mail via local `gpg` | M6 | Planned | Post-MVP |
| WASM plugins and menu scripting | M7 | Planned | Post-MVP |
| Federation, NNTP, finger, directory | M8 | Planned | Opt-in, post-MVP |
| Native client and E2EE (DMs, MLS groups) | M9 | Planned | Post-MVP |

Out of scope: offline readers (QWK), any token or blockchain. Open registration is a non-default sysop setting (D2), not a product mode.
