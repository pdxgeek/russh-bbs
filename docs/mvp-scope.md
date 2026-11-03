# MVP scope

The MVP is a closed-beta board that works with plain `ssh` and `sftp`. No install.

## In the MVP

- Waiting room, invite codes, sysop queue, invite tree, founder claim. Registration mode setting (`invite` default, `apply`, `open`).
- A named permission for every action; roles as sysop-editable sets of permissions, globally or per area (D33).
- User profiles, members list, per-user terminal settings.
- Forums with search, full-screen message editor, public chat, one-liners, last callers, polls.
- Menu layouts, themes, Nerd Font icon tiers with ASCII fallback, login and logoff screens.
- RSS in and out.
- Internal mail (address shown as `username@domain`).
- File areas over SFTP with scanning, tagging, and batch download.
- Global new-scan.
- Credits ledger (non-transferable).
- Event scheduler.
- Door framework and one or two Rust-native games.
- Datasource selection: SQLite (default) or Postgres, the latter also against PGlite. No MySQL.
- Container image with a compose example.
- Blob storage behind one object-store interface: local directory (default) or S3-compatible (D30). Managed Postgres works as the Postgres backend with TLS (D31).
- Password login over SSH, disabled by default (D28). Telnet transport, disabled by default and labeled low-security mode when on, with telnet login as a further opt-in (D27).
- Sysop admin TUI, page the sysop, moderation workflow, audit log.
- Notifications and digests, backups with a tested restore, data export and deletion.
- Accessibility mode.
- Read-only GraphQL API for the public tier.

## Honesty requirement

The MVP never calls anything private, secure or encrypted, or shows a lock, when the server can read it. Mail is encrypted at rest and no staff screen shows mail or whispers, but the running server can still read them, so the board rules and the help screen say once what that protects and what it does not; individual compose and read screens do not repeat it (D78). See [threat-model.md](threat-model.md).

## Not in the MVP (do not build early)

- Native client, E2EE DMs and groups.
- External email (inbound or outbound), IMAP.
- PGP mail helpers (planned as the first post-MVP item).
- WASM plugins, menu scripting, community doors.
- Federation, NNTP, finger, directory listing.
- Lightning tips or any payment rail.
- ZMODEM or XMODEM.
- Offline readers.

## Definition of MVP done

Closed beta of invited users runs for a month with no data loss, a tested restore, every guarantee in the threat model backed by a test, and every feature in [features.md](features.md) marked Verified or explicitly deferred by the owner.
