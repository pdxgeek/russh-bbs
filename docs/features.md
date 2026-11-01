# Feature register

Status values: **Planned**, **In progress**, **Implemented** (code exists), **Verified** (tests and a real `ssh` run). Everything is Planned today.

Update this table in the same pass as the code.

| Feature | Milestone | Status | Notes |
|---|---|---|---|
| Workspace, CI, threat model | M0 | Planned | |
| Shared sanitizer | M0 | Planned | Owned by core; used everywhere |
| Icon registry and width table | M0 | Planned | |
| SSH server, host key, public-key auth | M1 | Planned | |
| Waiting room (banner, rules, request, redeem) | M1 | Planned | |
| Founder / first-run sysop claim | M1 | Planned | One-time setup token |
| Per-user terminal settings | M1 | Planned | width, height, color, pause, tier |
| Login and logoff screens, themeable strings | M1 | Planned | |
| SQLite store and migrations | M1 | Planned | |
| Invite codes, quotas, invite tree | M2 | Planned | |
| Sysop approval queue and vouching | M2 | Planned | |
| Registration mode setting (`invite`, `apply`, `open`) | M2 | Planned | Default `invite`; D2, D15. Waiting room built mode-aware in M1 |
| User profiles and members list | M2 | Planned | |
| Time and call limits (optional) | M2 | Planned | |
| Menu tree, layouts, themes | M3 | Planned | See [ui.md](ui.md) |
| Forums with search | M3 | Planned | FTS5 |
| Full-screen message editor | M3 | Planned | quoting, drafts |
| Public chat rooms, presence, whispers | M3 | Planned | Plaintext tier |
| One-liners, graffiti wall, last callers | M3 | Planned | |
| Polls and voting booths | M3 | Planned | |
| RSS in (feeds as boards) and RSS out | M3 | Planned | SSRF guard required |
| Internal mail (stage 1) | M3 | Planned | |
| Credits ledger | M3 | Planned | Non-transferable |
| Event scheduler | M3 | Planned | |
| File areas over SFTP, scanning, quarantine | M4 | Planned | |
| File tagging and batch download | M4 | Planned | |
| Global new-scan (posts, files, feeds) | M4 | Planned | |
| Door framework and first games | M4 | Planned | Rust-native |
| Sysop admin TUI | M5 | Planned | |
| Page the sysop and sysop chat | M5 | Planned | |
| Moderation workflow, audit log | M5 | Planned | |
| Notifications and digests | M5 | Planned | |
| Backups and tested restore | M5 | Planned | |
| Data export and account deletion | M5 | Planned | |
| Accessibility mode | M5 | Planned | |
| GraphQL read-only API | M5 | Planned | |
| External mail (inbound, outbound) | M6 | Planned | Post-MVP |
| PGP mail via local `gpg` | M6 | Planned | Post-MVP |
| WASM plugins and menu scripting | M7 | Planned | Post-MVP |
| Federation, NNTP, finger, directory | M8 | Planned | Opt-in, post-MVP |
| Native client and E2EE (DMs, MLS groups) | M9 | Planned | Post-MVP |

Out of scope: offline readers (QWK), any token or blockchain. Open registration is a non-default sysop setting (D2), not a product mode.
