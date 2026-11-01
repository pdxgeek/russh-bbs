# Coordination

The source of truth for who is doing what. Read it before starting. Claim a slice by changing its **Status** to `Claimed` with your branch name, in the first commit of your branch. Mark it `In review` when the PR opens and `Done` when it merges.

A slice with an unmet dependency is not claimable.

| Slice | Owns | Depends on | Status | Branch / PR |
|---|---|---|---|---|
| M0-A Workspace, toolchain, CI, deny config | repo root, `.github/`, crate scaffolds | none | Claimed | `m0/workspace` |
| M0-B Shared sanitizer with tests | `crates/core/src/sanitize` | M0-A | Open | |
| M0-C Icon registry and width table | `crates/core/src/icons` | M0-A | Open | |
| M1-A SSH server, host key, restricted session | `crates/server/src/ssh` | M0-A | Open | |
| M1-B SQLite store and migrations | `crates/store`, `crates/core/src/store` traits | M0-A | Open | |
| M1-C Waiting room screens and rate limits | `crates/server/src/waiting_room` | M1-A, M0-B | Open | |
| M1-D Founder claim flow | `crates/server/src/founder`, store | M1-A, M1-B | Open | |
| M1-E Terminal settings and glyph test screen | `crates/server/src/settings` | M1-A, M0-C | Open | |

Later milestones are not claimable until the owner adds their slices here.

## Cross-slice requests

Add a line here if you need a change in code another slice owns. Do not make it yourself.

| From | To | Request | Resolved |
|---|---|---|---|
| | | | |
