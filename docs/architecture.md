# Architecture

Status: **Planned.** No code exists yet.

## Shape

One Cargo workspace. Business rules in a core crate that depends on nothing concrete; everything else implements its traits or calls it.

```
Cargo.toml                 workspace
crates/
  core/    russh-bbs-core    domain types, traits, rules, sanitizer, icon registry, invites, ledger, scheduler
  store/   russh-bbs-store   SQLite implementation of core's storage traits, migrations
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
                 russh-bbs-store  (SQLite, blobs on disk)
```

Rules never live in a front end. A rule that exists in both the TUI and GraphQL is a bug.

## Core responsibilities

- **Identity:** accounts, SSH key fingerprints, roles.
- **Invites:** states (pending, approved, active, suspended), codes, quotas, invite tree.
- **Content:** boards, threads, posts, read pointers, search.
- **Chat:** rooms, presence, whispers (public tier).
- **Mail:** internal mailboxes; shaped for external mail later.
- **Files:** areas, blobs, metadata, quarantine, scan hooks.
- **Feeds:** feed definitions, fetcher service, item dedupe.
- **Credits:** double-entry-style ledger, non-transferable.
- **Scheduler:** general event scheduler (feed polls, maintenance, door resets).
- **Sanitizer:** the single place that cleans untrusted text.
- **Icon registry and theme model:** logical names to glyphs per tier.

## Front ends

- **SSH TUI:** `russh` server, public-key auth, restricted session for unknown keys. `ratatui` rendering on the channel PTY with resize handling.
- **SFTP:** virtual filesystem mapped to file areas by ID, never raw paths.
- **GraphQL (M5):** `async-graphql` on `axum`, dataloaders for nested fields.

## Storage

- SQLite in WAL mode behind traits. Migrations forward-only.
- File blobs on disk, addressed by BLAKE3 hash; metadata in SQLite.
- Backups with a tested restore procedure (M5).

## Cross-cutting

- **Config:** TOML. Hot-reload for themes and layouts, with validation and fallback.
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

Nothing yet.
