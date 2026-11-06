# M5 slice plan (proposal)

Status: **Proposed. Nothing here is approved or built.** This is the PLAN-M5 draft for the owner to review. It does not change [milestones.md](milestones.md) or [decisions.md](decisions.md); where it disagrees with a recorded decision it says so as a question. The matching table, with every row marked "Proposed (owner review)", is in [coordination.md](coordination.md#m5-sysop-tooling-and-api-proposed). Nothing is claimable until the owner approves the plan and moves rows to `Open`.

Sizes are guesses, not commitments. **S** is under a day for one worker, **M** is one to two days, **L** is three days or more and is the first candidate to split.

## Open owner questions

Ordered by how much work waits on the answer.

| # | Question | Blocks | Default if you do not answer |
|---|---|---|---|
| OQ1 | **Q3: GraphQL as the one API, or an SSH-subsystem protocol?** ([decisions.md](decisions.md), Q3.) M5 only needs a read-only public tier. Also: which port, plain HTTP behind your reverse proxy or TLS in the server, and is it off by default? | M5-Q | Slice stays blocked. Proposal: GraphQL, `[api] enabled = false`, own listener, you terminate TLS in front. |
| OQ2 | **Does the closed beta wait for M4 to finish?** M4-F2 (files over SFTP, [PR #139](https://github.com/pdxgeek/russh-bbs/pull/139)) and M4-F3 (new-scan) are unmerged, and there is no M4 QA gate row yet. Export, deletion and blob backup need the file tables. | M5-H, M5-I, M5-J (digest) | Those three start only after M4-F2 (and F3 for the digest) merge. Others start now. |
| OQ3 | **What does deleting an account do to its posts, mail, credits and audit rows?** Options: (a) posts stay and show "deleted member"; (b) posts are removed; (c) member chooses. The audit log is append-only by trigger (D59) and has no foreign key to accounts, so its rows stay and show "deleted account" either way. Is there a grace period before deletion is final? Who can start it: the member, staff (`members.delete_data`), or both? | M5-I | (a) with a 7-day grace period the member can cancel in; both member and staff can start it. |
| OQ4 | **How does a member receive an export?** Options: an SFTP read-only virtual path that lists their export for 24 hours (needs M4-F2's SFTP tree), or a paged screen only. | M5-I | SFTP read-only path; no screen-only fallback, because a real export is too large to page. |
| OQ5 | **Notifications and digests without an email address.** D71 stores none by design, and external mail is M6. So a notification can only be shown inside the board (login screen, dashboard widget, a Notifications screen). Is that acceptable for the beta, or do you want an optional, member-supplied contact address now (a privacy cost the screens would have to state)? | M5-J | In-board only. No address stored. |
| OQ6 | **Q6: may the sysop watch a live session?** M5-A1 builds a live-sessions list showing handle, address, since, and idle time, and a disconnect action. It does not show another member's screen. Is "spy" wanted at all? If so, Q6's default (notify the member) applies. | M5-A1, M5-F | No screen mirroring is built. |
| OQ7 | **`menus.manage` permission** (UI-6 left it open). Today reload and preview use `themes.manage`. A new permission needs a migration and a decision on which roles get it. | M5-C | Not built; `themes.manage` stays. Recorded as a decision so it stops being open. |
| OQ8 | **Backups: where do they go, and are they encrypted?** The container image is `scratch`: it has no `pg_dump`. The proposal is that the server's `backup` subcommand does SQLite and blobs itself, while Postgres uses the operator's own `pg_dump` and the repo ships the recipe and a CI restore test. Encryption of the archive is the operator's job (an `age` or `gpg` wrapper in the recipe), because a built-in one adds a dependency and a key to lose. Agree? | M5-H | As proposed. |
| OQ9 | **Does a report-a-post feature belong in the MVP?** There is none today. The moderation slice (M5-G) can add one (a report row on a post, a queue, an action), or only give staff tools over what exists (mail reports, community removals, board admin). | M5-G scope | Staff tools over what exists, plus report-a-post as a separable last item you can cut. |
| OQ10 | **Accessibility mode: what does it switch?** Proposal: one per-member setting that forces the ASCII tier, no colour dependence, no motion, no mouse, no panes or dock, one-column linear layout, and messages written as lines instead of toasts. Is that the right set? I cannot test with a real screen reader from this environment (see M5-K). | M5-K | As proposed, and the docs say screen readers were not tested. |
| OQ11 | **FIX-M2-5 (sessions counted per process): do you plan more than one server process on one database for the beta?** | M5-O | No: parked, not scheduled. |
| OQ12 | **D21-REVISIT: optional RSA client keys.** You had no preference and asked to revisit before the beta. Do it (verify-only, rsa-sha2-256/512, a narrow `deny.toml` ignore for RUSTSEC-2023-0071, a new decision amending D21) or leave D21 as is? | M5-N | Left as is; slice not scheduled. |
| OQ13 | **Audit retention.** The table cannot be pruned with `DELETE` (D59). Proposal: monthly dated archive files in the data directory, then a marker row, with a default of 13 months online. What retention do you want? | M5-D1 | 13 months online, archive kept until the sysop removes it. |
| OQ14 | **Logs in the admin TUI.** Server logs contain network addresses. Show them to `sessions.view` holders, to sysop only, or not at all? Proposal: an in-memory ring of the last 500 warn-and-above lines, sysop only, sanitized, never persisted by the TUI. | M5-A1 | Sysop only, as proposed. |
| OQ15 | **Hardening milestone** (second half of this file). Do you want it, before the closed beta or between beta and public launch? Is there a person or firm for the external review, and a date? | all HARD slices | Proposed between the beta and any public launch. Reviewer is yours to find. |
| OQ16 | **PR #41 (FIX-REL-1, zig cross-compile).** Open since 2026-11-02 on a base far behind `main`. Rebase it as it is, or supersede it with HARD-5? | HARD-5 | HARD-5 starts from `main`, reads #41 for the approach, and #41 is closed in favour of it. |
| OQ17 | **D51 says GitHub Actions do not gate merges until production.** The plan assumes they start gating at the closed beta (HARD-5 turns that on if the owner wants it earlier). | HARD-5 | CI gates from the first release tag. |

## Where the loose ends went

Everything earlier slices handed to "M5" or "later", and the slice that picks it up. Sources: the cross-slice requests in [coordination.md](coordination.md), the "Planned" rows in [features.md](features.md), and the "Not done" lists in [access.md](access.md), [audit.md](audit.md), [accounts.md](accounts.md) and [decisions.md](decisions.md).

| Loose end | Source | Slice |
|---|---|---|
| Door staff tools have functions and no screen: `bar_player`, `unbar_player`, `reset_player`, `end_season` (`doors.bar_player`, `doors.reset_player`, `doors.end_season`) | M4-D0 note, D98 | M5-E |
| Per-door on/off and per-door grants (`ResourceKind::Door` needs a door table; `doors.manage`) | M4-D0 note | M5-E |
| `menus.manage` permission | UI-6 owner note | M5-C (OQ7) |
| Login-code and `key_notice` retention sweep | FIX-M3-11, [accounts.md](accounts.md) | M5-L |
| Sessions counted per process | FIX-M2-5 | M5-O (parked, OQ11) |
| `LOW-SECURITY` marker on every admin screen; show telnet mode and `telnet.listen`, `telnet.allow_login` | M3-H note | M5-A0, M5-C |
| Emoji-capable U+26A0 and U+2709 vs D18 | FIX-M0-F5 | M5-M (needs your terminal check) |
| Optional RSA client keys | D21-REVISIT | M5-N (OQ12) |
| Audit viewer (`audit.view`), audit retention, request-id targets shown as ids | M2-C, M2-E, M2-G notes, [audit.md](audit.md) | M5-D1 |
| Editing `registration_mode`, `open_daily_cap`, `queue_max_pending`; today's open count and cap hit | M2-E, M2-G notes | M5-C |
| Invite revoke (`invites.revoke`, needs `revoked_at` and an audit action), sysop view of every invite | M2-D note | M5-B1 |
| Suspend and reinstate (`members.suspend`); an open session of a suspended account is not disconnected | access.md "Not done" | M5-B1 |
| Roles, groups, grants and limits screens; group services need a rank rule first (D69 (5)) | M2-B2, M2-H notes | M5-B1, M5-B2 |
| Mail reports screen (`mail.moderate`) | M3-I note | M5-G |
| Board administration (rename, restrict, delete, remove a member from a board, lock or move a thread, scoped moderators) | M3-E note | M5-G |
| Community removals list and the list-size constants | M3-J note | M5-G |
| Chat room console, `chat.moderate`, page and split with the sysop | M3-F note, [chat.md](chat.md) | M5-F |
| Feed console and a public RSS address view | M3-G note | M5-P |
| Staff reset of a member's password | M3-D note (D81 (9): suspend stops it) | Not scheduled; see M5-B1 note |
| Widget defaults editing; accessibility shows widgets as a plain screen; `fx::allowed` real flag | UI-10, UI-8 notes | M5-K, M5-C |
| Backups carry `mail.key` separately; `draft` rows are in the backup and in an export | M3-I, M3-B notes | M5-H, M5-I |
| Events (`events.view`, `events.manage`) and feeds permissions with no screen | `access_paths.rs` | M5-C (scheduler jobs view), M5-P |
| Retention for purges that use a hand-rolled timer (`purge_every`) | M3-C note | M5-L |

Permissions that already exist and are listed in `NOT_YET_WIRED` in `crates/server/tests/access_paths.rs` are the M5 checklist: each slice that builds a screen for one moves it to `WIRED` with a path test, and the milestone gate (M5-QA) checks the list is empty or each remaining entry has a stated reason. Currently: `account.set_password`, `members.suspend`, `members.delete_data`, `invites.revoke`, `forum.edit_own`, `chat.moderate`, `mail.moderate`, the `files.*` set (M4-F2 wires these), `feeds.read`, `feeds.manage`, `doors.manage`, `doors.reset_player`, `doors.end_season`, `doors.bar_player`, `events.view`, `events.manage`, `sysop.page`, `sysop.chat`, `sessions.view`, `sessions.disconnect`, `audit.view`, `settings.edit`, `roles.edit`, `groups.edit`, `backups.run`. (`account.set_password` and `forum.edit_own` are not in any M5 slice; the QA pass should record why.)

## Rules every slice follows

These are the house-style acceptance lines that every slice repeats implicitly. Each slice below lists only what is specific to it.

1. **Docs in the same pass** (AGENTS.md): `docs/features.md`, `docs/coordination.md`, the area doc, `docs/access.md` and `docs/audit.md` rows for new permissions and audit actions, `docs/config.md` and `config.example.toml` for new keys, `docs/decisions.md` for a real choice.
2. **Core owns the rules.** The new screen is thin; the rule is a `core` function that takes a `&Granted` and a test calls it directly.
3. **Both backends.** Any store trait method has a SQLite and a Postgres implementation and a check in the shared suite, run on SQLite, a Postgres 16 and the TLS-required Postgres (D51).
4. **Access.** The action's permission is checked in core; its `NOT_YET_WIRED` entry moves to `WIRED` with a path test; a missing permission is "not found" for by-id access.
5. **Sanitized.** Every string that is not chrome goes through the shared sanitizer on the way out, tested with a hostile value (control characters, ANSI, private use, the reserved marker codepoints).
6. **Audited.** Every staff action writes its audit row in the store transaction (M2-C), by id, with no secret in any field.
7. **Markers.** Every new staff screen draws `LOW-SECURITY` when telnet is on and the marker cell is never covered; a render test at 80x24, 100x30 and 132x43 in each tier (as the UI slices did).
8. **Honest wording.** Nothing is labelled private that the server can read. Admin screens say what they show and to whom.
9. **A real client.** A walk with OpenSSH, recorded in `docs/qa/<slice>-walk.md` with what was and was not exercised.
10. **Checks** from AGENTS.md: fmt, clippy, tests (also `SQLX_OFFLINE=true`), `cargo deny check`, and the Postgres suites for anything touching the store.

## Hot spots and how the plan avoids them

Past parallel batches conflicted in the same places. The plan names them so the owner or orchestrator can order merges.

| Shared file or area | What collides | Mitigation |
|---|---|---|
| `crates/server/src/ssh/screen.rs`, `ssh/shell.rs`, `main.rs` | Every new screen adds a match arm, a key effect and wiring | M5-A0 builds one registry; later slices add a module file and one registry line. Screen-arm edits are additive and rebased at merge. |
| `core::menu` (`Action` enum, menu tree) | One variant per screen; lightbar row counts in tests | M5-A0 adds a single `Sysop console` entry with sections; slices add section entries, not new top-level items. Member screens (M5-I, M5-J, M5-K) add at most one variant each. |
| Migrations (`0029` onward; `0028` is M4-F2's), `LANDED_CHECKSUMS` in both `tests.rs`, `.sqlx` | Numbers, checksums and the offline cache | Numbers are taken at merge time. Migration-owning slices are marked **[migration]** below and merge one at a time. |
| `Permission` enum, `docs/access.md`, `access_paths.rs` | Same lines edited | Few new permissions: nearly all M5 permissions exist already. Only OQ7 adds one. |
| `core::audit` `Action` enum and `docs/audit.md` | One variant and row per audited action | Additive; rebase at merge. |
| `docs/features.md`, `docs/coordination.md`, `docs/decisions.md` | Every slice edits them | Slices add rows only; decision numbers are `D-TBD` until the orchestrator numbers them at merge, as in the open M4 PRs. |
| `crates/server/src/chat`, `crates/core/src/chat` | Page, sysop chat, moderation | One owner for all chat work: M5-F. M5-G must not touch chat. |
| `feeds::out::public_snapshot` | Feeds polish and GraphQL both read the public tier | M5-P owns it. M5-Q reads it and does not edit it. |

## Order and parallelism

**Batch 0 (one slice, small, serial):** M5-A0, the console skeleton. It is the only gate. Everything else that is a console screen waits for it, because it creates the registry that removes most of the file collisions above.

**Batch 1 (start immediately, eight workers).** None of these edit the console registry and their `Owns` paths are disjoint: **M5-A0, M5-H, M5-I, M5-J, M5-K, M5-L, M5-P**, plus an eighth that needs an owner answer: M5-M (once you have checked the terminals) or M5-N (if OQ12 is yes). If neither is ready, the eighth worker starts M5-R's early draft or waits for batch 2. M5-H, M5-I and M5-J carry a dependency on M4-F2 (OQ2); if it has not merged they start their non-file parts. Their console or menu pieces (the backups action, the delete action, the feed console) are added after M5-A0 merges.

**Batch 2 (after M5-A0 merges, eight workers):** M5-A1, M5-B1, M5-B2, M5-C, M5-D1, M5-E, M5-F, M5-G. Each owns its own console section module, so they do not collide except in the additive places above. M5-B1, M5-E and M5-C each carry a migration, and so do M5-J, M5-K and possibly M5-I from batch 1: those migration slices merge one at a time, in the order below.

**Batch 3:** M5-Q (GraphQL, after OQ1 and M5-P), then M5-R (sysop handbook), then **M5-QA**. M5-O is parked.

Merge order for migration-owning slices (earliest first): M5-K, M5-J, M5-B1, M5-E, M5-C, M5-I. The order only reflects expected finish times; whoever is ready first takes the next number.

## Slices

### M5-A0 Console skeleton and extension seam

- **Owns:** `crates/server/src/sysop/console/` (new: `mod.rs`, `registry.rs`), one `Action::SysopConsole` and its menu entry in `core::menu`, one arm in `ssh/screen.rs`, `docs/ui.md` section.
- **Depends on:** none (M4 not required).
- **Size:** S.
- **Parallel with:** M5-H, M5-I, M5-J, M5-K, M5-L, M5-P (nothing in common). Blocks batch 2.
- **What:** the **Sysop console** menu entry for staff, a `ConsoleSection` trait (title, required permission, `draw`, `key`) and a registry where each later slice adds one line. Sections the viewer cannot reach are not listed (no-leak). The console shows the board's mode (telnet on or off, `telnet.listen`, `telnet.allow_login`) in its header.
- **Acceptance:**
  - A member sees no console entry and gets "not found" if they send its key; a `moderator` sees only the sections their permissions reach; tested through `core::access`.
  - A test registers a fake section and proves hidden-when-unreachable and key routing.
  - `LOW-SECURITY` is drawn on the console by the status bar when telnet is on, at 80x24, 100x30 and 132x43 in every tier; the marker cell is never covered.
  - Walked over OpenSSH: open the console as sysop and as a member.
- **Not in it:** any section content.

### M5-A1 Live sessions and server logs

- **Owns:** `crates/server/src/sysop/console/sessions.rs` and `logs.rs`, a read-only snapshot API on `crates/server/src/ssh/registry` (read only: the registry's counting stays M5-O's), a bounded in-memory log ring in `crates/server/src/` (a `tracing` layer).
- **Depends on:** M5-A0.
- **Size:** M.
- **Parallel with:** all of batch 2 except M5-O.
- **Acceptance:**
  - The list shows handle, transport (ssh or telnet), network address, connected-since and idle time for this process, with a header that says "this server process only".
  - `sessions.view` shows the list; `sessions.disconnect` is needed for the disconnect action, which ends the session within a second, writes an audit row (`session.disconnected`, target the account) and shows the disconnected member the reason line, tested with a real russh client.
  - A member whose account is suspended is disconnected by M5-B1 calling this action's core function; the function exists here and is tested directly.
  - The log viewer shows the last 500 warn-and-above lines, sysop only (OQ14), every line passed through the sanitizer; a log line containing escape sequences and a marker glyph is shown neutralized (hostile test). Nothing is persisted by the viewer.
  - Per OQ6, no screen mirror and no key capture exist; a test asserts the session API exposes no output stream.
- **Not in it:** cross-process sessions (M5-O).

### M5-B1 Users editor: states, roles, member limits, invite revoke **[migration]**

- **Owns:** `crates/server/src/sysop/console/users.rs`, `crates/core/src/members` (suspend and reinstate rules, new if absent), `crates/core/src/invites` (revoke), migration for `invites.revoked_at`, the matching store methods on both backends.
- **Depends on:** M5-A0; calls M5-A1's disconnect function if merged (otherwise lands the function call behind a stub and says so).
- **Size:** L (split revoke off if it runs long).
- **Parallel with:** M5-B2 (they share only `core::access`'s public API, no files), M5-C, M5-D1, M5-E, M5-F, M5-G.
- **Acceptance:**
  - A searchable, paged member list for staff (state, roles, last call, invited by), each field sanitized.
  - Suspend and reinstate go through the existing state transition function (D67), need `members.suspend` and a rank strictly above the target (D60 (6)), write `account.state_changed`, and disconnect an open session within a second (access.md "Not done" closed).
  - Role change uses `grant_role`/`revoke_role` (D69, D70), so the rank and no-escalation rules hold; the screen maps `EditError` to words in one place.
  - Member limits screen over `core::limits::load_effective` and the three edit functions behind `limits.edit`; it says "sysop is unlimited whatever is stored".
  - `invites.revoke` works: a new `invite.revoked` audit action and `revoked_at` column, a revoked code fails redemption with the single refusal, and the sysop can see every invite (no plaintext codes: only the hash prefix and metadata).
  - The shared store suite covers revoke on both backends; a rank test shows a `cosysop` cannot suspend or demote a `sysop`.
  - The last-sysop rule still holds from this screen (tested).
- **Not in it:** staff reset of a password. D81 (9) says suspension is how a sysop stops password login; this plan keeps it so unless the owner asks.

### M5-B2 Roles, groups and limits editor

- **Owns:** `crates/server/src/sysop/console/roles.rs`, `groups.rs`, `crates/core/src/access` (group services with the rank rule, closing D69 (5)).
- **Depends on:** M5-A0.
- **Size:** M.
- **Parallel with:** everything in batch 2.
- **Acceptance:**
  - `create_group`, `delete_group` and membership services exist with the rank rule over the group's grants: a `groups.edit` holder cannot add an account to a group that holds a role above their rank (test reproduces the D69 (5) hole first, then shows it closed).
  - Role editor screens use the M2-B2 services; a custom role made on the screen has no limit rows until the sysop sets them, and the screen says so (D70 (2)).
  - Every change writes `role.*`, `group.*` and `permission.*` audit rows that already exist; the screen text for each `EditError` is in one place.
  - `roles.edit` and `groups.edit` move to `WIRED` with path tests; the shared suite runs on both backends.
- **Not in it:** new permissions.

### M5-C Board settings console and `menus.manage`

- **Owns:** `crates/server/src/sysop/console/settings.rs`, a `core::settings` facade for the stored settings if one is not there, `[widgets]` defaults editing through `WidgetSettings::from_config`, the scheduler jobs view (`events.view`, `events.manage`). If OQ7 is yes: a `Permission::MenusManage` variant, its role rows and a migration **[migration, only if OQ7 yes]**.
- **Depends on:** M5-A0.
- **Size:** M.
- **Parallel with:** the rest of batch 2.
- **Acceptance:**
  - Editing `registration_mode` (invite, apply, open), `open_daily_cap`, `queue_max_pending`, banner and rules, each validated by the same function the boot path uses, each change audited (a setting-change audit row; the M2 handoff notes that board-level `set_setting` is not audited yet because `settings.edit` is M5 and knows the actor), needs `settings.edit`.
  - Switching the mode never reveals anything to the waiting room (the existing no-leak tests run in all three modes after a live switch).
  - Shows today's open registrations, the cap and whether it was hit (`registration_day`, `registration.capped`).
  - Scheduler jobs listed with last run and next run; a job can be run now behind `events.manage`.
  - If OQ7 is yes: `menus.manage` guards menu reload and preview, the sysop role holds it, a `cosysop` does not unless the owner says; a decision is recorded. If no: a decision records that `themes.manage` stays.
  - `settings.edit`, `events.view`, `events.manage` move to `WIRED`.
- **Not in it:** config-file-only keys (those stay file and environment settings; the screen says "set in the config file").

### M5-D1 Audit viewer and audit retention

- **Owns:** `crates/server/src/sysop/console/audit.rs`, `crates/core/src/audit` (name resolution at read time, archive and retention), audit store archive methods on both backends.
- **Depends on:** M5-A0.
- **Size:** M.
- **Parallel with:** the rest of batch 2; M5-L (it registers its job with L's purge registry if L has merged).
- **Acceptance:**
  - Paged, newest first, filter by actor, action and target (using `core::audit::view_log`, 100 maximum), needs `audit.view`, moves it to `WIRED`.
  - Actor and target resolve to names at read time and show "deleted account" when gone; request-id targets show as ids (the row is gone after a decision); every field is drawn through the sanitizer path.
  - An unknown action name shows as a labelled unknown entry (FIX-M2-2) and the screen test includes one.
  - Retention: rows older than the sysop's policy (OQ13) are moved to a dated archive file in the data directory by a scheduler job, never by `DELETE`; a marker row records what was archived (count, first and last id, a SHA-256); the append-only triggers are untouched (test: raw `UPDATE` and `DELETE` still fail). A decision records the archive design (D59 left it open).
  - The viewer says plainly that a host owner can still edit the database file (D59).
- **Not in it:** exporting the log, search by free text.

### M5-E Door staff tools and per-door grants **[migration]**

- **Owns:** `crates/server/src/sysop/console/doors.rs`, `crates/core/src/doors` (staff wrappers, door table, per-door grants), door table migration, store methods on both backends. Not `crates/doors/*` (each game stays its owner's).
- **Depends on:** M5-A0; M4-D0 (merged). Door content PRs (#140 to #142) do not collide: they touch only game crates and docs.
- **Size:** M.
- **Parallel with:** the rest of batch 2.
- **Acceptance:**
  - Screens for `bar_player`, `unbar_player`, `reset_player` and `end_season` over the existing audited core functions, each gated by its permission (`doors.bar_player`, `doors.reset_player`, `doors.end_season`), each asking for a confirmation that names the player and the door; the rank rule from D98 holds and is tested from the screen path.
  - A door table so `ResourceKind::Door` is real; per-door on and off (`doors.manage`); per-door grants through the usual allow entries and role grants, resolved by `rules::can`; a door turned off disappears from the doors menu of members and its open sessions end at the next key.
  - Existing door saves survive the migration (fixture test on both backends); doors that exist before the migration are all on by default.
  - `doors.manage`, `doors.reset_player`, `doors.end_season`, `doors.bar_player` move to `WIRED` with path tests.
  - Door names and player handles shown to staff go through the sanitizer.
- **Not in it:** game balance, door content, the framework's `Door::highlights` request.

### M5-F Page the sysop, sysop chat, split chat, chat moderation

- **Owns:** `crates/core/src/chat`, `crates/server/src/chat`, `crates/server/src/sysop/console/chat.rs`, `docs/chat.md`. No other slice touches chat.
- **Depends on:** M5-A0 (for the room console); the page itself does not.
- **Size:** L.
- **Parallel with:** the rest of batch 2.
- **Acceptance:**
  - **Page:** a member with `sysop.page` pages the sysop with one line (length-capped, sanitized); the page reaches every staff session holding `sysop.chat` as a toast and a console badge, with a per-member rate limit and a "sysop not here" answer when none is online; nothing is stored (like whispers, D78 style) unless the owner says so. Tested with two russh clients.
  - **Sysop chat:** a staff-only room, joined only with `sysop.chat`; a member in the room list never sees it exists (no-leak test).
  - **Split chat:** sysop and a paged member can open a one-to-one split pane (the dock from UI-W) that either side can end; the member sees plainly who is on the other side, uses the reserved marker cell rules, and nothing is logged beyond the hub's in-memory scrollback (state it on screen).
  - **Moderation:** `chat.moderate` kick and mute in a room, with a duration, audited (`chat.kicked`, `chat.muted`); the room console lists closed rooms and per-room policy. No tool shows whispers (D78).
  - `sysop.page`, `sysop.chat` and `chat.moderate` move to `WIRED`.
  - Over telnet the page is received but the split pane is not offered (stated on screen).
- **Not in it:** spying on a session (OQ6), an IRC gateway (M8).

### M5-G Moderation workflow

- **Owns:** `crates/server/src/sysop/console/moderation.rs`, `crates/core/src/forums` (admin services, scoped moderators, lock and move), `crates/core/src/mail` reports (screen only; the existing `reports`/`close_report` functions), `crates/core/src/community` (removals view). Not chat (M5-F) and not feeds (M5-P).
- **Depends on:** M5-A0; M5-D1 for the audit view of removals (soft: it can link to it without it).
- **Size:** L.
- **Parallel with:** the rest of batch 2. If OQ9 is "yes, add report-a-post", split the report feature into **M5-G2** (a member-facing report key on a post, a queue row, `forum.report` permission, migration) after M5-G.
- **Acceptance:**
  - A **moderation inbox** lists open mail reports (`mail.moderate`), with reports opened and closed by id only; staff never see a message body (D90: no reader), and a test proves the screen has no path to one.
  - Board administration (M3-E list): rename, restrict or open a board, delete a board, take a member out of a board using the M2-B2 services, lock and move a thread, and scoped moderators (`forum.moderate` granted on one board). A scoped moderator cannot act outside their board (test).
  - A community removals list from the audit viewer data, and the list-size constants (`MAX_POLLS`, `POLLS_PER_DAY`) shown; making them settings is cut unless the owner asks.
  - Every action is audited with its existing or a new action name documented in `docs/audit.md`; the removed text is never logged.
  - `mail.moderate` moves to `WIRED`.
- **Not in it:** chat moderation (M5-F), feed management (M5-P), automated content filters.

### M5-H Backups and tested restore on every backend

- **Owns:** `crates/core/src/backup` (manifest, verification), `crates/server/src/backup` and a `backup`/`restore` subcommand in `main.rs` (a small additive edit), `crates/store/src/{sqlite,postgres}/backup.rs`, `.github/workflows/backup.yml` (the CI restore job), the Backups section of `docs/deploy.md`, a console section file `sysop/console/backups.rs` once M5-A0 has merged (`backups.run`).
- **Depends on:** M1-B, M1-G, M4-F1 (merged); M4-F2 for the file tables' content (OQ2). M5-A0 only for the console section.
- **Size:** L.
- **Parallel with:** batch 1 and the rest of batch 2.
- **Acceptance:**
  - **SQLite:** an online, consistent snapshot of a running database (SQLite `VACUUM INTO` or the backup API; the worker records the choice as a decision), plus the `mail.key` file **copied to a separate destination**, never into the same archive by default. The backup never touches the host key unless asked, and says so.
  - **Blob store:** a sync of the local backend to a directory, and of an S3-compatible backend to another bucket or prefix, using `BlobStore` only (no module touches a blob path, M4 rule), verifying the SHA-256 of each object on restore.
  - **Postgres:** the repo ships a `pg_dump`/`pg_restore` recipe and the CI restore test uses it against the Postgres service. The server image has no `pg_dump` (OQ8), and the docs say that.
  - **Restore tests in CI:** for each backend, seed through the real store (accounts, a post, a sealed mail, a door save, a file), back up, destroy, restore into an empty target, and compare: row counts per table, the mail decrypts with the restored key and **refuses to boot with a missing or wrong key** (the existing boot check), and the host key fingerprint is unchanged where the host key was restored. Default install (SQLite, local blobs) runs in the normal test job; Postgres in its job; MinIO sync is documented and runs locally as the M4 S3 tests do (D51).
  - A backup manifest records the schema version, the migration count and the key id (never the key); a restore into a database with a **newer** schema is refused, and one with an older schema is migrated forward by the normal start-up path (test).
  - The `backups.run` console action starts a backup, shows the last result and age, and is audited (`backup.started`, `backup.finished`); it refuses to run if the destination is inside the live data directory.
  - `docs/deploy.md` replaces the "stop the container and tar the volume" recipe with the tested procedure and a restore drill; nothing says "encrypted" about an archive the server did not encrypt.
- **Not in it:** scheduled backups on a timer in the server (a documented `cron` or compose sidecar recipe instead, unless the owner wants it), incremental backups, point-in-time recovery for Postgres.

### M5-I Data export and account deletion **[migration if a deletion queue is needed]**

- **Owns:** `crates/core/src/account_data` (export builder, deletion plan), per-area read methods added as new files under `crates/store/src/{sqlite,postgres}/export/` (one file per area; no edits to existing store files), the export blob namespace, a read-only SFTP node (after M4-F2 merges; a one-line registration in `crates/server/src/sftp`, noted in coordination), a member screen under Terminal settings and a console action for `members.delete_data`.
- **Depends on:** M4-F1, M4-F2 (OQ2, OQ4). Answers to OQ3 and OQ4.
- **Size:** L.
- **Parallel with:** batch 1 and batch 2. It reads every area, so it adds read methods rather than editing other slices' files.
- **Acceptance:**
  - **Export:** a member's own data as a ZIP of JSON and plain text: profile, settings, posts, drafts (M3-B), chat is **not** included (nothing is stored), community lines, credits ledger, door saves, file records and their files, key fingerprints (public), audit rows **about** the member only if the owner agrees (default: not included, stated in the export's README), and their own mail only as the member can read it. It is sanitized on the way in and out. A test per area proves it contains the member's rows and **no other member's**, with a second account as the control.
  - Delivery per OQ4: written once to the blob store under an `exports` namespace with a 24-hour expiry swept by M5-L's purge, listed only to its owner on the read-only SFTP path; a second member's session cannot list or read it (no-leak test).
  - Rate limit: one export per member per day.
  - **Deletion:** a member asks for it on a screen that says exactly what is removed and what stays (per OQ3), requires typing the handle, starts the grace period, and can cancel until it ends; `members.delete_data` lets staff do it for a member, audited. After it runs: the account, keys, password hash, drafts, settings, mail rows, door saves and files are gone; posts follow OQ3; the audit log keeps its rows with the actor shown as "deleted account"; the handle is reserved for a stated period. Verified on both backends in the shared suite with a row-count check per table that no row names the account.
  - An open session of a deleted account ends at the next key.
  - The last sysop cannot delete their account (the last-sysop rule).
  - `members.delete_data` moves to `WIRED`.
  - The export and the deletion screens state that the server could read this data; nothing is labelled private.
- **Not in it:** a legal-compliance claim of any kind. The docs say what the feature does, not that it satisfies a regulation.

### M5-J Notifications and digests **[migration]**

- **Owns:** `crates/core/src/notify` (event kinds, per-member preferences, digest builder), notification tables and migration, a Notifications screen and a dashboard widget hook (a `Widget` registration for UI-10, no change to the widget framework), the login-screen "since your last call" summary.
- **Depends on:** M4-F3 for the digest to include files and feed items (OQ2); the notification store itself does not.
- **Size:** M.
- **Parallel with:** batch 1 and batch 2.
- **Acceptance:**
  - Closed event kinds: new mail, a reply to my thread, a file approved or rejected, my account state changed, a staff decision on something I submitted while a member (invite request decisions for a non-member are not notified, because no address exists, D71), a door event the door chooses to post. Each kind is a `core` constructor; producing one is a single call from the owning code path, added by this slice only as hooks, not by editing other slices' business logic (it lists the call sites it wants in `docs/coordination.md` for their owners).
  - Per-member preferences per kind (on, digest only, off), stored on both backends.
  - A digest is built at login from the notification table and from M4-F3's new-scan, bounded in size (at most 20 lines, then "and N more"), every line sanitized, and respects `core::access` (never lists what the member cannot reach; no-leak test with a restricted board).
  - A notification text never includes mail subjects or bodies (sealed, D90), only "you have 2 new messages".
  - Notifications expire (default 30 days) through M5-L's purge registry.
  - Delivered in-board only (OQ5); the docs say that nothing is sent anywhere.
- **Not in it:** email, push, external delivery (M6).

### M5-K Accessibility mode **[migration]**

- **Owns:** `crates/core/src/terminal_settings` (a new per-member `accessible` flag and its migration), `crates/server/src/ui` (a linear layout, the plain path), `ssh/screen.rs` (the real flag passed to `fx::allowed`), the settings screen toggle, `docs/ui.md` section.
- **Depends on:** none.
- **Size:** M.
- **Parallel with:** batch 1. Shares `ui` with M5-A0 only through the registry (read-only use of the frame).
- **Acceptance:**
  - A setting on the Terminal settings screen and a way to choose it before sign-in is not offered (unknown keys see the waiting room only); the first-login glyph test offers it.
  - When on: ASCII tier forced, colour never the only signal (every state has a text label), no motion (`fx::allowed(look, true)` gives one static frame; test), no mouse reporting, no blinking cursor, no panes or dock or widgets (they appear as their own plain screen using `Widget::text`, UI-10 note), a one-column linear layout, toasts written as lines at the bottom, lists as numbered rows, no line wider than 72 columns, each screen's first line is its title.
  - All screens a member can reach render in this mode at 80x24 and at 40x10 without overflow (matrix test over `Content` variants like the UI-4 matrix).
  - Security markers keep their reserved cell and style (D39); the ASCII text marker is still drawn.
  - The docs state what was tested: the output is plain linear text checked for stability and width; **no real screen reader (NVDA, VoiceOver, Speakup, BRLTTY) was used**, and what the owner or a volunteer should test.
- **Not in it:** a braille display driver, a speech output feature, a separate non-TUI interface.

### M5-L Retention sweeps and the purge registry

- **Owns:** `crates/core/src/retention` (a registry of named purge jobs with a policy), the store purge methods for `login_code`, `key_notice` and other expired rows (new files per backend), one scheduler job; moves `sysop::queue::purge_every` onto the scheduler (M3-C note) only if its owner agrees in `docs/coordination.md`.
- **Depends on:** none (M3-C scheduler merged).
- **Size:** S.
- **Parallel with:** batch 1.
- **Acceptance:**
  - Used and expired login codes and notices older than their window are deleted; unexpired and unused ones are untouched (clock-controlled tests on both backends).
  - The registry lists every purge (name, what, window, last run, rows removed) for the console to show; other slices (M5-I exports, M5-J notifications, M5-D1 archive) register theirs.
  - A purge writes no audit row for routine deletes of expired secrets, and a test asserts no code or token reaches a log line.
  - A job that fails is reported once per hour, not every run.
- **Not in it:** audit log trimming (M5-D1), account deletion (M5-I).

### M5-M Emoji-capable glyphs (FIX-M0-F5)

- **Owns:** `crates/core/src/icons` (the `Mail` and `KeyChanged` Unicode-tier entries and the width table).
- **Depends on:** **you**: a check of U+26A0 and U+2709 in real terminals (at least macOS Terminal.app, iTerm2, a Linux terminal, a Windows terminal and a mobile ssh app) with the font you recommend. The environment cannot do it.
- **Size:** S.
- **Parallel with:** anything.
- **Acceptance:**
  - Your result is recorded in the PR (terminal, font, what drew).
  - If they draw as 2-cell colour emoji, the two entries switch to non-emoji BMP East Asian Width Neutral codepoints that are not in the reserved set, or to text with the emoji-capable ones removed from the Unicode tier; the width table and the property tests (D18, D39) pass; the sanitizer round-trip test still passes.
  - If they draw as one cell everywhere you tested, the finding is closed with the evidence and the wording in `docs/ui.md` is corrected.
- **Not in it:** the Nerd Font tier.

### M5-N Optional RSA client keys (D21-REVISIT) — only if OQ12 is yes

- **Owns:** `crates/server/src/ssh` auth, `deny.toml`, a decision amending D21.
- **Depends on:** OQ12.
- **Size:** M.
- **Parallel with:** anything except slices editing `ssh/handler.rs`.
- **Acceptance (from the existing row):** `rsa-sha2-256` and `rsa-sha2-512` only, never SHA-1 `ssh-rsa`; a minimum key size (3072 recommended, the worker confirms); the host key stays Ed25519; verify-only, so RUSTSEC-2023-0071 (private-key timing) does not apply, with a narrow, commented `deny.toml` ignore and `cargo deny check` clean; an end-to-end login with a real RSA key from OpenSSH; a SHA-1-only client is refused; the waiting room still reveals nothing for an RSA key.
- **Not in it:** DSA, SHA-1 signatures.

### M5-O Sessions counted per process (FIX-M2-5) — parked

- **Owns:** `crates/server/src/ssh/registry`, `crates/core/src/limits`, a lease table **[migration]**, `docs/access.md`.
- **Depends on:** OQ11. Do not schedule unless the beta runs more than one process.
- **Size:** M.
- **Acceptance if scheduled:** a lease row per session with a heartbeat and expiry; a crashed process's leases expire; the `sessions` limit holds across two server processes on one Postgres (tested with two processes in CI); M5-A1's list can show sessions from all processes; the docs name what else is per process (in-memory rate limiters reset on restart and are per process, D37; the founder token; the scheduler) and what this slice does and does not fix.
- **Not in it:** distributed rate limits.

### M5-P Outbound feeds polish and feed console

- **Owns:** `crates/core/src/feeds` (out side), `crates/server/src/feeds`, `crates/server/src/sysop/console/feeds.rs`, the RSS-out HTTP surface and its docs.
- **Depends on:** M3-G (merged); M5-A0 for the console file only.
- **Size:** M.
- **Parallel with:** batch 1 and batch 2.
- **Acceptance:**
  - The public RSS output also serves Atom; conditional requests (`ETag`, `If-Modified-Since`) answer 304 correctly; items are in a stable order and capped; a feed is cached for a short period so a polling reader cannot cost a query per request.
  - Content is sanitized and escaped for XML (hostile titles and bodies tested for injection, including a `]]>` in a body and invalid XML characters); a private board never appears (the D89 public-tier rule); unknown or private board returns the same response as no board.
  - Per-IP request limit on the feed endpoint with bounded memory.
  - Feed console (M3-G list): each inbound feed's last outcome, last success and failure count, edit interval and the http flag, disable without removing, and the public RSS address view shown to staff only and never logged; `feeds.manage` and `feeds.read` move to `WIRED`.
  - The SSRF guard tests are re-run for every console edit path (a feed edited to a private address is refused).
- **Not in it:** podcast enclosures, WebSub, authentication on public feeds.

### M5-Q Read-only GraphQL for the public tier

- **Owns:** `crates/api` (currently empty), the `[api]` config section, a listener module in `crates/server/src/api`, `docs/api.md` (new), `docs/config.md`.
- **Depends on:** **OQ1 (Q3)**; M5-P merged (shared public snapshot, read-only use).
- **Size:** L.
- **Parallel with:** M5-R; it should not start before the owner answers Q3.
- **Acceptance:**
  - Off by default (`[api] enabled = false`); its own listener; no mutation and no subscription in the schema (schema snapshot test).
  - Serves only the public tier as D89 defines it: boards listed in `board_public` and their posts, and the board's identity and rules. It serves no member list, no profile and no statistics, and a test shows an unknown or private board is indistinguishable from none.
  - Query depth, complexity, page size and body size are capped; a depth bomb and a huge alias list are refused with a bounded response; a per-IP rate limit with bounded memory; a request timeout.
  - Every string field goes through the sanitizer; hostile post text yields no control character in the JSON.
  - Introspection is a setting, default on for public data only.
  - The new dependency (the worker proposes `async-graphql` or alternative, with a one-line justification) passes `cargo deny check`; `crates/api` depends on `core` only, with no store type leaking out.
  - Walked with `curl` against a running server on both backends; the doc says what was not exercised (a TLS terminator in front, large load).
- **Not in it:** any authenticated or member-tier access, mutations, a native client (M9).

### M5-R Sysop handbook and beta checklist

- **Owns:** `docs/sysop.md` (new), the README index row.
- **Depends on:** the slices it describes (late); can draft early.
- **Size:** S.
- **Parallel with:** M5-Q.
- **Acceptance:** first run to first backup and restore drill, each console section described with the permission it needs, the honesty rules in plain words ("the server can read everything except PGP mail and files you encrypt yourself"), the beta checklist that M5-QA walks; every command in it was run.

### M5-QA Milestone QA gate

- **Owns:** `docs/qa/m5.md` and the fix rows it adds.
- **Depends on:** all M5 slices that the owner scheduled.
- **Size:** M.
- **Acceptance:** the brief in [milestones.md](milestones.md) "QA gate at the end of every milestone", plus these M5 lines. The pass never fixes code.
  - Every `NOT_YET_WIRED` permission is wired or has a stated reason.
  - A staff member of each rank tries every console section: a role below the permission gets nothing and learns nothing.
  - **A restore drill from an empty volume** on SQLite with local blobs (the default install), and the Postgres recipe, with the host key fingerprint and mail readability checked.
  - Hostile text in every new admin screen; markers visible on every console screen at 80x24 and 132x43 in every tier, with telnet on.
  - Export and deletion walked end to end with two accounts; the second account's data is absent from the first's export.
  - Accessibility mode walked over `ssh` at 80x24 and 40x10; the report lists what a screen reader test would still need.
  - The waiting room reveals nothing in all three registration modes after a live mode switch.
  - GraphQL probed for private boards, members and statistics.
  - The pass records the closed-beta go or no-go for the owner. It does not declare a launch.

## Dependency summary

| Slice | Needs before it can finish |
|---|---|
| M5-A0 | nothing |
| M5-A1, B1, B2, C, D1, E, F, G | M5-A0 |
| M5-H | nothing for SQLite and local blobs; M4-F2 for file content |
| M5-I | M4-F2, OQ3, OQ4 |
| M5-J | nothing for the store; M4-F3 for the digest |
| M5-K, M5-L | nothing |
| M5-M | owner's terminal check |
| M5-N | OQ12 |
| M5-O | OQ11 |
| M5-P | nothing (console file after M5-A0) |
| M5-Q | OQ1, M5-P |
| M5-R | late drafts of the others |
| M5-QA | all scheduled M5 slices |

## Proposed hardening milestone (before any public launch)

Status: **Proposed for your decision (OQ15). Not in [milestones.md](milestones.md).** M5 ends in a closed beta among people the owner knows. The things below are what I would want done before strangers can reach the board. I could not find definitions of the "H-1 to H-4" slices in the repository, on any branch, or among the open PRs, so I cannot say what they cover. HARD-1 begins with an inventory so it does not repeat them; if they are defined somewhere I did not see, the owner should send them.

Proposed name: **M5H Hardening.** It could also be folded in as the last part of M5 or as M9's "hardening" pulled forward; that is your call.

| Slice | Name | Owns | Depends on | Size |
|---|---|---|---|---|
| HARD-1 | Fuzz targets beyond H-1 to H-4 | `fuzz/` (new `cargo-fuzz` workspace member), `.github/workflows/fuzz.yml` | none | M |
| HARD-2 | Load and abuse test | `tests/load/` (new harness crate or scripts), `docs/qa/load.md` | M5-A1 (to watch sessions), M5-K not needed | L |
| HARD-3 | External review packet for mail-at-rest and the login model | `docs/review/` (new), no code | M5-H (key backup story), M5-B1 | M |
| HARD-4 | Upgrade path with live data | `crates/store`, `crates/server/src/main.rs` (version stamp), `.github/workflows/upgrade.yml`, `docs/deploy.md` | M5-H | L |
| HARD-5 | Release pipeline | `.github/workflows/`, `Dockerfile`, `docs/deploy.md`, PR #41's content | OQ16, OQ17 | M |
| HARD-6 | Operator runbook and incident drills (my addition; cut if unwanted) | `docs/runbook.md` | M5-H, M5-R | S |

All HARD slices can run in parallel (their `Owns` paths do not overlap), except that HARD-4 should not start until M5-H has fixed the backup manifest format, and HARD-6 waits for both. Eight workers are not needed; five can run at once.

### HARD-1 Fuzz targets beyond H-1 to H-4

- **Scope.** Start with an inventory in `docs/qa/fuzz.md`: every parser or decoder that reads bytes or text from outside, what already has a property or fuzz test (the sanitizer, the `.ans` art parser, the SFTP path parser have some), and what has none. Candidates I can see from the tree: the SSH and telnet key parsers (including mouse reports and IAC), the config loader and `menus.toml`/theme/pack loaders, the feed parsers (RSS and Atom in) and XML output, the sealed-mail decoder and key file format, door save and world migrations (frozen old bytes, truncation, bit flips), the GraphQL request path once M5-Q exists, the SFTP packet layer, and the sanitizer's second pass.
- **Acceptance.** A `cargo-fuzz` target per untested parser with a seed corpus checked in (small, generated, no real data); a documented 10-minute-per-target run in the QA brief and a nightly job (or the owner's local run, D51); every crash found becomes a regression test and a fix slice; the doc lists what is still not fuzzed and why. **Honest limit:** fuzzing finds crashes and hangs, not logic errors, and a clean run is not a proof.
- **Needs a decision.** `cargo-fuzz` needs nightly Rust; the repo pins stable (`rust-toolchain.toml`). Proposal: a separate `fuzz/` workspace with its own toolchain file, not part of the main build.

### HARD-2 Load and abuse test

- **Scope.** A repeatable harness (built on `russh` clients and a telnet client) that runs against a real server on a fresh volume with SQLite and again with Postgres. Scenarios: N hundred idle sessions; connection and auth floods from one and from many addresses; waiting-room form flooding; a slow reader and a stalled writer; resize storms and input bursts (the D49 bounds); many concurrent posters and a chat room with many members; an upload and download mix over SFTP; a reconnect storm after a restart; GraphQL query bombs.
- **Acceptance.** Documented target numbers chosen by the owner (for a closed beta I would suggest 200 sessions, which the harness does not assume is right); memory and CPU recorded before, during and after (the FIX-M1-L10 allocator finding showed the musl allocator keeps memory after a flood); no panic in the log; every limit that is meant to hold (connection cap, per-key and per-network rates, render cap) is shown to hold with numbers; the report says plainly which scenarios ran against a loopback network and what a real network would change. Results go in `docs/qa/load.md`; each failure becomes a fix row.
- **Cost note.** The cloud environment cannot give a realistic network. The numbers are a floor, not a capacity statement.

### HARD-3 External review packet (at-rest mail encryption and the login model)

- **What a worker can do.** A worker cannot be the external reviewer. This slice prepares what a reviewer needs and keeps it honest: a scope document, a code map with file and line references, the design decisions as written (D90 for mail, D94, D96, D28, D81 and the founder/recovery flows for login), the threat-model claims to be tested ("a stolen database copy or backup without the key reveals no mail"), a list of known weaknesses already found (the key sits beside the host key; the metadata is unsealed; the sysop with both can read mail; D90 (5) key rotation is designed and not built), and specific questions (AEAD associated data; nonce handling; key file format; argon2 parameters; SSH user-name routing; the password-login rate limits; telnet plaintext risk; recovery codes; the founder token on stdout).
- **Acceptance.** `docs/review/` contains the packet; every claim in it was checked against the code on the commit named in it; the reviewer's findings get fix rows when they arrive; **the slice is done when the packet is delivered, not when the review passes**. The review itself and its budget are the owner's (OQ15). Until it is done the docs must keep saying that the mail sealing and the login model have had no external review.
- **Candidate follow-up.** Build the key rotation of D90 (5) if the reviewer asks for it.

### HARD-4 Upgrade path between versions with live data

- **Scope.** D76 says the next release is v0.2.0 and deploy.md says "back up first and do not run an older image on a newer database". Make that checkable.
- **Acceptance.** The server records the schema version and refuses to start an older binary on a newer database with a clear message and exit code (test); a start-up flag or subcommand `migrate --dry-run` lists pending migrations without applying them; an upgrade test in CI seeds a database with the **previous tagged release** (built from the tag, with real accounts, posts, sealed mail, door saves and files), starts the new build on a copy, and checks the data and a login still work, on SQLite and Postgres; door save and world formats have frozen old-bytes tests across every released version (they exist per game; the slice adds a release-level check that none is dropped); the upgrade procedure in deploy.md includes the M5-H backup and a rollback that restores it; a migration that cannot run on a large table is flagged in the PR template.
- **Honest limit.** Until two tagged releases exist there is nothing to upgrade from; the first run uses v0.2.0 as the "old" side once it is tagged.

### HARD-5 Release pipeline

- **Scope.** Today a release is the owner pushing a tag and running a workflow by hand (deploy.md "Owner action: release"); PR #41 changes the image build to cross-compile with zig and drop QEMU, but it has been open since 2026-11-02 against an old base.
- **Acceptance.** Starting from `main` (reading #41 for its approach, OQ16): a tag builds multi-arch images, runs the full test and `cargo deny` set, runs the container smoke test on the built image (empty volume, token, waiting room, restart, clean stop), generates an SBOM and provenance attestation, scans the image, publishes the image and release notes from the coordination log, and refuses to publish if any step fails; the build is reproducible enough that two runs give the same binary hash or the doc says why not; Actions become the merge gate (OQ17, amending D51 with the owner's approval); actions stay SHA-pinned. Nothing is published, tagged or made public by the worker: the owner does that (AGENTS.md).
- **Not verified here.** The environment has no Docker daemon in some sessions and a TLS proxy that breaks the in-Docker build (PR #41 hit this); the slice's report must say what ran where.

### HARD-6 Operator runbook and incident drills (optional, my addition)

- **Scope.** One page per incident: lost host key (members see a changed fingerprint), lost mail key, a stolen database copy, a compromised sysop key (recovery codes, accounts.md), a flooded board, a bad migration (rollback by restore), a full disk, a leaked token in a log.
- **Acceptance.** Each page was exercised once on a scratch board and says how; the restore drill is the M5-H one.

### Order and what runs in parallel

HARD-1, HARD-2, HARD-3 and HARD-5 start together once M5's slices that they read have merged (HARD-2 needs the console's sessions view only to watch, so it can start sooner). HARD-4 follows M5-H. HARD-6 is last. The hardening milestone should end with its own QA pass like every other milestone, and the owner's go or no-go for public launch should wait for HARD-3's external review result, not just its packet.

## What this plan does not claim

- It does not know what H-1 to H-4 cover (see above).
- Sizes and the "eight workers at once" shape are estimates; M5-B1, M5-F, M5-G, M5-H, M5-I and M5-Q are large enough that a worker may need to split them mid-way, and the plan says where.
- It assumes the M4 slices in review merge as they stand. If M4-F2 changes the file tables or the SFTP tree, M5-H and M5-I change with it.
- Nothing in it has been built, run or tested. The only check made was reading the repository and the open PRs on 2026-11-06.
