# Coordination

The source of truth for who is doing what. Read it before starting. Claim a slice by changing its **Status** to `Claimed` with your branch name, in the first commit of your branch. Mark it `In review` when the PR opens and `Done` when it merges.

A slice with an unmet dependency is not claimable.

| Slice | Owns | Depends on | Status | Branch / PR |
|---|---|---|---|---|
| M0-A Workspace, toolchain, CI, deny config | repo root, `.github/`, crate scaffolds | none | In review | `m0/workspace`, [PR #1](https://github.com/pdxgeek/russh-bbs/pull/1) |
| M0-B Shared sanitizer with tests | `crates/core/src/sanitize` | M0-A | In review | `m0/sanitizer` (base `m0/workspace`), [PR #2](https://github.com/pdxgeek/russh-bbs/pull/2) |
| M0-C Icon registry and width table | `crates/core/src/icons` | M0-A | In review | `m0/icons` (base `m0/sanitizer`), [PR #3](https://github.com/pdxgeek/russh-bbs/pull/3) |
| M1-A SSH server, host key, restricted session | `crates/server/src/ssh` | M0-A | In review | `m1/ssh-server` (base `m0/icons`), [PR #4](https://github.com/pdxgeek/russh-bbs/pull/4) |
| M1-B SQLite store and migrations | `crates/store`, `crates/core/src/store` traits | M0-A | In review | `m1/store` (base `m1/ssh-server`), [PR #6](https://github.com/pdxgeek/russh-bbs/pull/6) |
| M1-C Waiting room screens and rate limits | `crates/server/src/waiting_room`, `crates/core/src/waiting_room` | M1-A, M0-B | In review | `m1/waiting-room` (base `m1/store`), [PR #15](https://github.com/pdxgeek/russh-bbs/pull/15) |
| M1-D Founder claim flow | `crates/server/src/founder`, `crates/core/src/founder`, store | M1-A, M1-B | In review | `m1/founder` (PR link below) |
| M1-E Terminal settings and glyph test screen | `crates/server/src/settings` | M1-A, M0-C | Open | |
| M1-F Container image, compose example, GHCR publish | `Dockerfile`, `compose.yaml`, the publish workflow in `.github/`, `docs/deploy.md` | M1-A, M1-B, M1-D | Open | |
| M1-G Postgres backend and migrations | `crates/store/src/postgres`, the Postgres CI job, and the restructure of `crates/store` into per-backend modules | M1-B | Open | Note: move the SQLite code to `crates/store/src/sqlite` with migrations under `crates/store/migrations/sqlite/`; add the `datasource` setting (sqlite path \| postgres URL); turn the store behavior tests into one suite generic over the core Store traits and run it against both backends (PGlite via pglite-socket, or a Postgres service in CI). |

Later milestones are not claimable until the owner adds their slices here.

## Cross-slice requests

Add a line here if you need a change in code another slice owns. Do not make it yourself.

| From | To | Request | Resolved |
|---|---|---|---|
| M1-A | M1-B | `russh-bbs-core::access` defines `Fingerprint` and the `MemberDirectory` trait (`is_member(&Fingerprint) -> Future<bool>`, fail closed). Implement it over the store; the server takes it as `Arc<D>` in `SshServer::start`. Replace if you want a richer return type, but keep "unknown key => Restricted" in `decide_access`. | Yes, M1-B (`m1/store`): `SqliteStore` implements `MemberDirectory`; only `active` accounts are members; errors answer `false`. `main.rs` takes `--db` / `RUSSH_BBS_DB`. |
| M1-A | M1-C | Plug per-IP limits in at `AcceptLoop::admit` and per-key limits at the top of `ClientHandler::auth_publickey` (both marked `M1-C`). The placeholder lives in `ssh/screen.rs` (`render_waiting_room`); replace it with your screens and keep drawing through `Screen` (D19). Any text that is not chrome must go through `core::sanitize`. | Resolved by M1-C (`m1/waiting-room`): both limits are in, the placeholder is replaced, `Screen::new` now takes a `WaitingRoom`, and `Key` gained `Enter`, `Backspace`, `Escape` in `ssh/terminal.rs` (D37). |
| M1-A | M1-D | The handler never reads or writes the store; the founder flow needs a way to mark a key as member/sysop through `MemberDirectory` and a way for `ClientHandler` to learn the role. Not designed here. | Yes, M1-D (`m1/founder`): `core::access::RoleDirectory` (role of an active account's key), `ssh::Backend` supertrait, `SshServer::start_with_founder`; the handler learns the role at auth and shows the signed-in placeholder (D38). |
| M1-B | M1-D | Store API for you: `AccountStore::create_account` (role/state), `attach_fingerprint`, and `SettingsStore` with `SettingKey::FounderClaimed` (value `"true"`). The claim must be a single atomic step, which the current trait does not give you (get then set can race): add a compare-and-set or a transactional store method in `crates/store` as part of M1-D. `MemberDirectory` already sees a new active account's key immediately (no cache). | Yes, M1-D (`m1/founder`): `FounderStore::claim_founder` (core trait) is one SQLite transaction that is a CAS on `founder_claimed`; raced in `crates/store` and over SSH. No migration needed (D38). |
| M1-B | M1-A / owner | CI sets `SQLX_OFFLINE: "true"` in `.github/workflows/ci.yml` (M0-A owned; one env line). Please keep it. | |
| M1-C | M1-B | FYI, no action: M1-C added `SettingKey::WaitingRoomBanner` and `WaitingRoomRules` to `russh-bbs-core::store` (stored in the existing `settings` table, no migration). A sysop-facing way to set them arrives with the sysop console; until then edit the `settings` rows. | |
| M1-C | M1-D | The waiting room is built per shell from `WaitingRoomSource` (implemented for every `SettingsStore`). When the founder flow lands, a member's key still sees the same room; add the founder claim screen as a new page in `russh_bbs_core::waiting_room::screens` or a separate screen, not by branching on membership inside the room. | Yes, M1-D (`m1/founder`): the claim prompt is a separate screen (`core::founder::ClaimForm`), reached by the SSH user name `claim`, not a page in the room. The room is untouched and tests assert it is identical before and after the claim. A member's key now sees the signed-in placeholder instead of the room (members are no longer shown the room). |
| M1-D | M1-H | `main.rs` now calls `founder::start(&store, &mut stdout())` and `SshServer::start_with_founder(..)` (3 lines). If the config struct lands, the claim rate limits (`DEFAULT_CLAIM_KEY_RATE`, `DEFAULT_CLAIM_IP_RATE`) and per-session attempts (`MAX_ATTEMPTS_PER_SESSION`) are constants today and can move into it. | |
| M1-D | M1-F | The setup token is printed to stdout, so it is in `docker logs`; a restart before the claim prints a new one. Document this in `docs/deploy.md` and do not set a log driver that ships stdout to a third party before the claim. | |
| M1-D | M2 | Re-point the claim at D33's `sysop` role (it stores `Role::Sysop` today) and make `RoleDirectory` use real permissions. The "signed in" placeholder and the account name `sysop` are placeholders. | |
