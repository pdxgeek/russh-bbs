# Architecture

Status: **Planned.** No code exists yet.

## Shape

One Cargo workspace. Business rules in a core crate that depends on nothing concrete; everything else implements its traits or calls it.

```
Cargo.toml                 workspace
crates/
  core/    russh-bbs-core    domain types, traits, rules, sanitizer, icon registry, invites, ledger, scheduler
  store/   russh-bbs-store   SQLite and Postgres implementations of core's storage traits, one migration set each (`sqlite/`, `postgres/` modules; D44)
  server/  russh-bbs-server  the binary: SSH + SFTP front end, TUI, config
  proto/   russh-bbs-proto   reserved: wire types for the future client (empty until M9)
  api/     russh-bbs-api     reserved: GraphQL front end (M5)
docs/
```

Dependency direction: `server`, `api`, `store` → `core`. `core` depends on none of them.

## Layers

```
 ssh / sftp client
        │
  russh-bbs-server  (SSH, PTY, TUI, SFTP)      russh-bbs-api (GraphQL, later)
        └──────────────┬───────────────────────────────┘
                 russh-bbs-core   (rules, sanitizer, traits)
                       │ traits
                 russh-bbs-store  (SQLite or Postgres, blobs on disk)
```

Rules never live in a front end. A rule that exists in both the TUI and GraphQL is a bug.

## Core responsibilities

- **Identity and access:** accounts, SSH key fingerprints, a named permission per action, roles as sets of permissions (globally or scoped to an area), per-area policies, one `access` module that answers every "may this account do this here" question (D33). Front ends call it; they never decide.
- **Invites:** states (pending, approved, active, suspended), codes, quotas, invite tree.
- **Content:** boards, threads, posts, read pointers, search.
- **Chat:** rooms, presence, whispers (public tier; M3-F, [chat.md](chat.md)). Room definitions are stored; lines and whispers live only in a bounded in-memory hub.
- **Mail:** internal mailboxes; shaped for external mail later.
- **Files:** areas, blobs, metadata, quarantine, scan hooks.
- **Feeds:** feed definitions, fetcher service, item dedupe.
- **Credits:** double-entry-style ledger, non-transferable.
- **Scheduler:** general event scheduler (feed polls, maintenance, door resets).
- **Sanitizer:** the single place that cleans untrusted text.
- **Icon registry and theme model:** logical names to glyphs per tier.

## Front ends

- **SSH TUI:** `russh` server, public-key auth, restricted session for unknown keys. `ratatui` rendering on the channel PTY with resize handling.
  - **Implemented (M1-A, `russh-bbs-server::ssh`):** persistent Ed25519 host key, public-key-only auth, our own accept loop (connection cap, auth deadline), one PTY shell per connection, `ratatui` over an in-memory backend flushed to the channel (D19), resize, and the lockdown in the threat model. The access rule lives in `russh-bbs-core::access` (`decide_access`: unknown key => `Restricted`); the server only calls it. The server is started with a `MemberDirectory`; the binary passes the SQLite store (M1-B), so a key bound to an active account is `Member`. Both access levels still see the same placeholder screen until M1-C.
  - **Implemented (FIX-M1-1, D49, D50):** the handler only receives; a per-session shell task (`ssh/shell.rs`) owns the screen, parses keys with a stateful parser that survives packet boundaries (`anes` plus bracketed paste, `ssh/terminal.rs`), folds window changes and keys into one redraw per interval, and writes through a bounded queue with a stall cut-off. Per-session budgets are in `ssh/budget.rs`.
  - **Planned:** SFTP (M4), screens that update on a timer rather than on input.
- **Telnet (optional, off by default, M3):** separate listener in `crates/server/src/telnet`, option negotiation via a crate, drives the same `Screen` abstraction as SSH. Adds the `UNENCRYPTED` marker to chrome. See D27.
- **SFTP:** virtual filesystem mapped to file areas by ID, never raw paths.
- **GraphQL (M5):** `async-graphql` on `axum`, dataloaders for nested fields.

## Storage

- Datasource chosen in config (D6, D24): SQLite in WAL mode (default) or Postgres. Managed Postgres is this backend with TLS. PGlite was tried and does not work with the pooled client (D44). No MySQL. Each backend has its own forward-only migrations and passes the same behavior test suite.
  - **Implemented (M1-B, `russh-bbs-store`):** `SqliteStore` over `sqlx` (D34, D35, D36): `accounts`, `account_keys`, `settings`; core traits in `russh-bbs-core::store` (`AccountStore`, `SettingsStore`); `MemberDirectory` implemented for the store. Opened at start-up from `datasource.path` (`--db` / `RUSSH_BBS_DB`, see [config.md](config.md)). Planned: boards, posts, mail, files and invites in their milestones.
  - **Implemented (M1-G, D44):** `crates/store/src/{sqlite,postgres}` side by side, each with its own migrations (`migrations/{sqlite,postgres}/`) and `.sqlx` data in one directory; shared rules (membership, role, founder flag) once in `directory.rs`, shared account row mapping in `rows.rs`. `main.rs` opens the store the config selects and runs the same server code over it (generic over core traits). One behavior suite in `crates/store/src/suite` runs against both backends; Postgres adds TLS (always on, D31) and refuses URL/socket settings that would weaken it.
- File blobs behind one `BlobStore` trait with object-store semantics (D30), addressed by BLAKE3 hash; metadata in the database. Backends: local directory (default) and S3-compatible, both through `object_store`, both behaving identically, including quarantine as a prefix plus a database flag. Managed Postgres such as RDS is just the Postgres backend with TLS (D31).
- Backups with a tested restore procedure (M5).

## Cross-cutting

- **Config:** TOML, with environment-variable overrides for container use. Hot-reload for themes and layouts, with validation and fallback.
- **Deployment:** a container image is a first-class deliverable (D25): static binary, non-root, one `/data` volume, logs to stdout. With Postgres and an object store configured, the only state left on the volume is the SSH host key, which can instead be supplied from a secret or environment variable, making the container replaceable.
- **Logging:** `tracing`, structured, never logging content, keys, or invite codes.
- **Time:** injected `Clock` trait.
- **Doors:** `Door` trait in core; Rust-native runtime in M4; WASM runtime later.

## Mail stages

1. Internal-only (MVP). Address shown as `username@domain`, shaped for external mail (raw message, headers, `Message-ID` threading).
2. Inbound SMTP, with SPF/DKIM/DMARC checks and encrypt-on-arrival to the member's PGP key if present.
3. Outbound through a relay provider, with per-member limits and probation.
4. IMAP/SMTP submission (optional).

## UI model

See [ui.md](ui.md).

## Verified

SSH transport (M1-A): integration tests drive a real `russh` client against the server on loopback, and the OpenSSH `ssh` and `sftp` clients were run by hand. See [features.md](features.md). Everything else is unverified.
