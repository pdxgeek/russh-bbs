# Milestones

Each milestone ends with something runnable. Estimates are guesses for one developer and are not commitments. A cloud session takes one slice (see [cloud-sessions.md](cloud-sessions.md)).

## QA gate at the end of every milestone

A milestone is not done when its last slice merges. It is done when a **QA pass** on `main` passes. The QA pass is its own session (or a human) that did not write the code, with this brief:

1. Build from `main`, run the server (the container image from M1-F onward, from an empty volume), and walk every "Done when" line of the milestone with a real `ssh` and `sftp` client. Record the exact commands and what appeared on screen.
2. Try to break it along the threat model: unknown key probing for member or board information, control characters and ANSI and private-use glyphs in every input, oversized input, rate-limit floods, concurrent sessions, resize storms, disconnect mid-screen, restart with the same volume (host key fingerprint unchanged, data intact).
3. Audit honesty: every `docs/features.md` row marked Implemented has a test, every row marked Verified was exercised in step 1, no screen labels anything private that the server can read, security markers cannot be hidden by the layouts that exist.
4. Run the validation commands and `cargo deny check` on `main`, and check `Cargo.toml` for dependencies without a justification in their PR.
5. Write `docs/qa/<milestone>.md`: what was exercised, what passed, every finding with severity and reproduction, and what could not be tested in the environment. Open one fix slice per finding of severity high or above in `docs/coordination.md`; the next milestone does not start until those are merged and re-checked.

The QA pass never fixes code. It reports. Low findings go into the coordination table as open slices for later.

## M0: Foundations
- Cargo workspace (`core`, `store`, `server`; `proto` and `api` as empty reserved crates), `rust-toolchain.toml`, `deny.toml`, CI.
- Shared **sanitizer** in core with fuzz or property tests.
- **Icon registry** (logical names to glyph per tier) and a width table.
- Threat model and decisions already in `docs/`.
- **Done when:** CI is green on fmt, clippy, tests, and cargo-deny; sanitizer and icon registry have tests; QA pass written to `docs/qa/m0.md`.

## M1: SSH server and waiting room
- `russh` server, persistent host key, public-key auth, restricted session for unknown keys.
- TUI over the PTY with resize handling (resolve Q5, record it in decisions).
- Store traits in core, SQLite backend and migrations (resolve Q4), with the backend selected by config (D6, D24).
- Postgres backend with its own migrations, behavior tests shared with SQLite, run in CI against PGlite or a Postgres service, and against a Postgres that requires TLS (D31).
- Container image: Dockerfile, compose example, GHCR publish in CI, one `/data` volume (D25).
- **Config loader** (`russh-bbs-server::config`): one TOML file plus environment overrides (`RUSSH_BBS_*`), parsed into a typed struct, validated at boot, every key documented in `docs/config.md` with its default. The existing CLI flags become overrides of the same struct. Carries `registration_mode`, `datasource`, `blobs`, `telnet`, `ssh` sections with their defaults even before the features behind them exist, so later slices add behavior, not plumbing. Small, and launched first in the second M1 batch because M1-C, M1-F, and M1-G all read it.
- Waiting room screens, per-IP and per-key rate limits.
- Founder flow: one-time setup token printed to the console claims sysop.
- Per-user terminal settings and the glyph test screen. Login and logoff screens.
- **Done when:** `ssh localhost -p <port>` shows the waiting room, the founder can claim sysop, tests show an unknown key learns nothing beyond the waiting room, the same tests pass on SQLite and Postgres, and the published container image does all of the above from a fresh volume.

## M2: Invites and members
- **Screen framework first** (moved from M3): the menu tree, the **classic hotkey** layout, the universal keys, and the themeable string table. Every M2 screen (queue, profiles, members list) is built on it so nothing is written ad hoc and reworked in M3. Lightbar, compact, and themes stay in M3.
- **Audit log early** (moved from M5): an append-only `audit` table and one write helper in core, used from the first approval, denial, ban, role change, and setting change. The M5 slice becomes the sysop UI over it.
- States (pending, approved, active, suspended), codes (single-use, expiring, bound to inviter), quotas, invite tree.
- Sysop queue UI with approve, deny, and vouch. Flood defenses and quick expiry of denied requests.
- Profiles and members list. Optional time and call limits.
- **Done when:** a member invites a second user and a stranger is approved from the queue, both over real `ssh`.

## M3: Public core
- Remaining layouts, themes, and icon tiers on the M2 screen framework ([ui.md](ui.md)): lightbar and compact, theme packs, live switching.
- Forums with FTS5 search; full-screen message editor with quoting and drafts.
- Public chat rooms with presence and whispers; one-liners, graffiti wall, last callers; polls.
- RSS in (feeds as read-only boards, SSRF guard, sanitization) and RSS out for public boards.
- Event scheduler, credits ledger, internal mail (stage 1).
- Member passwords (argon2id, set over SSH), password login over SSH as an opt-in (D28), and the telnet transport, off by default (D27): separate listener, same screens, fixed `UNENCRYPTED` marker, low-security mode marker for the sysop.
- **Done when:** three users post and chat over plain `ssh`, a malicious feed (escape codes, SSRF attempt) is neutralized by tests, and the layout can be switched live.

## M4: Files and doors
- `BlobStore` trait with object-store semantics and the local backend (D30); S3-compatible backend tested against MinIO in CI, same test suite.
- File areas over SFTP (virtual filesystem by ID), quarantine and scanning hook, tagging and batch download, metadata and search. Everything streams through `BlobStore`; no module touches a blob path.
- Global new-scan across posts, files, and feeds.
- Door framework and one or two Rust-native games (a small daily-turn RPG as the flagship).
- **Done when:** upload, scan, approve, and download work over `sftp` on both the local and the S3-compatible blob backend with the same tests; a door runs in a session with no filesystem or network access.

## M5: Sysop tooling and API
- Admin TUI (user editor, live sessions, logs), page the sysop and split chat, moderation workflow and audit log.
- Notifications and digests, data export and deletion, accessibility mode.
- Backups with a tested restore for **every backend**: SQLite online backup, `pg_dump` for Postgres, and a blob-store sync; the restore test runs in CI against the default install and is documented for the others.
- Sysop UI over the audit log written since M2.
- Read-only GraphQL for the public tier, outbound feeds polish.
- **MVP launch:** closed beta.

## Post-MVP
- **M6:** external mail (inbound, then outbound through a relay) and PGP mail via local `gpg`.
- **M7:** WASM plugin runtime for menu scripting and community doors.
- **M8:** opt-in federation, NNTP gateway, finger, directory listing.
- **M9:** native client and E2EE (DMs, MLS groups), multi-device, hardening, external crypto review.
