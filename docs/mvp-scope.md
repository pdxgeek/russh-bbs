# MVP scope

The MVP is a closed-beta board that works with plain `ssh` and `sftp`. No install.

## In the MVP

- Waiting room, invite codes, sysop queue, invite tree, founder claim. Registration mode setting (`invite` default, `apply`, `open`).
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
- Sysop admin TUI, page the sysop, moderation workflow, audit log.
- Notifications and digests, backups with a tested restore, data export and deletion.
- Accessibility mode.
- Read-only GraphQL API for the public tier.

## Honesty requirement

The MVP must display plainly that the server can read private-looking content. Wording such as "visible to the sysop" appears wherever messages or mail are composed or read. See [threat-model.md](threat-model.md).

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
