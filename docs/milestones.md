# Milestones

Each milestone ends with something runnable. Estimates are guesses for one developer and are not commitments. A cloud session takes one slice (see [cloud-sessions.md](cloud-sessions.md)).

## M0: Foundations
- Cargo workspace (`core`, `store`, `server`; `proto` and `api` as empty reserved crates), `rust-toolchain.toml`, `deny.toml`, CI.
- Shared **sanitizer** in core with fuzz or property tests.
- **Icon registry** (logical names to glyph per tier) and a width table.
- Threat model and decisions already in `docs/`.
- **Done when:** CI is green on fmt, clippy, tests, and cargo-deny; sanitizer and icon registry have tests.

## M1: SSH server and waiting room
- `russh` server, persistent host key, public-key auth, restricted session for unknown keys.
- TUI over the PTY with resize handling (resolve Q5, record it in decisions).
- Store traits in core, SQLite backend and migrations (resolve Q4), with the backend selected by config (D6, D24).
- Postgres backend with its own migrations, behavior tests shared with SQLite, run in CI against PGlite or a Postgres service.
- Container image: Dockerfile, compose example, GHCR publish in CI, one `/data` volume (D25).
- Waiting room screens, per-IP and per-key rate limits.
- Founder flow: one-time setup token printed to the console claims sysop.
- Per-user terminal settings and the glyph test screen. Login and logoff screens.
- **Done when:** `ssh localhost -p <port>` shows the waiting room, the founder can claim sysop, tests show an unknown key learns nothing beyond the waiting room, the same tests pass on SQLite and Postgres, and the published container image does all of the above from a fresh volume.

## M2: Invites and members
- States (pending, approved, active, suspended), codes (single-use, expiring, bound to inviter), quotas, invite tree.
- Sysop queue UI with approve, deny, and vouch. Flood defenses and quick expiry of denied requests.
- Profiles and members list. Optional time and call limits.
- **Done when:** a member invites a second user and a stranger is approved from the queue, both over real `ssh`.

## M3: Public core
- Menu tree plus layouts, themes, and icon tiers ([ui.md](ui.md)): at least classic hotkey, lightbar, and compact.
- Forums with FTS5 search; full-screen message editor with quoting and drafts.
- Public chat rooms with presence and whispers; one-liners, graffiti wall, last callers; polls.
- RSS in (feeds as read-only boards, SSRF guard, sanitization) and RSS out for public boards.
- Event scheduler, credits ledger, internal mail (stage 1).
- Member passwords (argon2id, set over SSH), password login over SSH as an opt-in (D28), and the telnet transport, off by default (D27): separate listener, same screens, fixed `UNENCRYPTED` marker, low-security mode marker for the sysop.
- **Done when:** three users post and chat over plain `ssh`, a malicious feed (escape codes, SSRF attempt) is neutralized by tests, and the layout can be switched live.

## M4: Files and doors
- File areas over SFTP (virtual filesystem by ID), quarantine and scanning hook, tagging and batch download, metadata and search.
- Global new-scan across posts, files, and feeds.
- Door framework and one or two Rust-native games (a small daily-turn RPG as the flagship).
- **Done when:** upload, scan, approve, and download work over `sftp`; a door runs in a session with no filesystem or network access.

## M5: Sysop tooling and API
- Admin TUI (user editor, live sessions, logs), page the sysop and split chat, moderation workflow and audit log.
- Notifications and digests, backups with a tested restore, data export and deletion, accessibility mode.
- Read-only GraphQL for the public tier, outbound feeds polish.
- **MVP launch:** closed beta.

## Post-MVP
- **M6:** external mail (inbound, then outbound through a relay) and PGP mail via local `gpg`.
- **M7:** WASM plugin runtime for menu scripting and community doors.
- **M8:** opt-in federation, NNTP gateway, finger, directory listing.
- **M9:** native client and E2EE (DMs, MLS groups), multi-device, hardening, external crypto review.
