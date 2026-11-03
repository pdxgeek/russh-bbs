# Permissions and roles

Status: **Implemented** (M2-B, not yet Verified: the QA gate has not run). The model is decision D33 in [decisions.md](decisions.md); how it was built is D55 and the "Implementation plan" at the end of this page. This page grows as milestones add actions, and every new action adds a row here in the same PR.

## Permissions

One named permission per action, a closed enum in `core::access`. Names are `area.verb`. A front end cannot perform an action without the check passing in core.

| Group | Permissions |
|---|---|
| Waiting room | `waiting.view`, `invite.request`, `invite.redeem`, `membership.apply`, `membership.open` |
| Account | `account.edit_profile`, `account.edit_settings`, `account.set_password` |
| Members | `members.view`, `members.approve`, `members.deny`, `members.vouch`, `members.suspend`, `members.edit`, `members.delete_data` |
| Invites | `invites.create`, `invites.revoke`, `invites.view_tree` |
| Forums | `forum.list`, `forum.read`, `forum.post`, `forum.reply`, `forum.edit_own`, `forum.moderate`, `forum.manage` |
| Chat | `chat.join`, `chat.speak`, `chat.whisper`, `chat.moderate`, `chat.manage_rooms` |
| Mail | `mail.read`, `mail.send` |
| Files | `files.list`, `files.download`, `files.upload`, `files.tag`, `files.approve`, `files.manage_areas` |
| Feeds | `feeds.read`, `feeds.manage` |
| Doors | `doors.run`, `doors.manage` |
| Boardlets | `oneliners.post`, `graffiti.post`, `polls.vote`, `polls.create`, `community.view`, `community.moderate`, `events.view`, `events.manage` |
| Sysop | `sysop.page`, `sysop.chat`, `sessions.view`, `sessions.disconnect`, `audit.view`, `settings.edit`, `roles.edit`, `groups.edit`, `limits.edit`, `themes.manage`, `backups.run` |

A permission is held either globally (from the role) or scoped to one area (one board, file area, room, or door). Scoped grants are how a trusted member moderates one board without becoming a moderator everywhere.

**`community.view` and `community.moderate` (M3-J, D86)** are the two actions the small community features share. `community.view` opens the Community menu and reads the one-liners, the graffiti wall, last callers and the poll list and results (`member` and above; the waiting role never holds it, so nothing here is visible to an unauthenticated key). `community.moderate` removes a one-liner, a wall line or a poll, and closes a poll someone else made (`moderator` and above, as every `*.moderate`); each use is audited. Posting stays `oneliners.post` and `graffiti.post`, voting `polls.vote`, creating a poll `polls.create` (`trusted` and above). Migration `0017` inserts the role rows (`sysop` included); custom roles get none until a sysop edits them.

**`account.set_password` (M3-D, D81)** gates every part of the Password screen under Terminal settings: seeing whether a password is set, setting or changing it, clearing it. It is checked in `core::passwords` against the snapshot loaded for the action, not by the screen. `member` and above hold it. It does not gate password *login*: an existing password still signs in if the permission is later revoked (see D81 for how a sysop stops that).

## Default roles

Shipped so a sysop never has to build a role from scratch. All of them except `sysop` and `waiting` can be edited; any can be copied as the starting point of a custom role.

| Role | Rank | Holds | Meant for |
|---|---|---|---|
| `waiting` | 0 | Waiting room group only | Unknown and unapproved keys. Not editable. |
| `member` | 10 | Account, `members.view`, `invites.create`, all read and post permissions in Forums, Chat, Mail, Files (`files.upload` goes to quarantine), Feeds, Doors, Boardlets except `polls.create` and `events.manage` | Everyone who was approved |
| `trusted` | 20 | `member` plus `polls.create`, `members.vouch`, `invites.view_tree` | Long-standing members; the inviter tier |
| `moderator` | 30 | `trusted` plus every `*.moderate`, `files.approve`, `members.deny`, `sessions.view` | Keeps the board clean; no settings or roles |
| `cosysop` | 40 | `moderator` plus every `*.manage`, `members.approve`, `members.suspend`, `members.edit`, `groups.edit`, `limits.edit`, `audit.view`, `events.manage`, `sysop.chat` | Runs the board day to day; cannot edit roles, settings, or backups |
| `sysop` | 100 | Everything, always | The founder and whoever they trust completely. Not editable. |

Rank only orders roles and stops an account from granting or editing a role at or above its own. It never unlocks an action by itself.

## Groups

A **group** is a named set of members. Groups exist so a sysop can manage people in bulk instead of one account at a time.

- A member belongs to any number of groups; a group can contain members and other groups (nesting is allowed, cycles are rejected, depth is capped).
- A group can be **assigned roles**: every member of the group holds those roles in addition to their own.
- A group can be **granted resource permissions** directly: a forum, file area, room, or door policy can name a group in its allow list exactly as it can name a member. This is how "the staff group can see the staff board" is expressed without touching each account.
- Built-in groups: `everyone` (all approved members; cannot be edited or deleted) and `staff` (holds the `moderator` role by default; editable). Custom groups are created by anyone holding `groups.edit`.
- Group membership changes are audited and apply on the next action.

### How a check resolves

Access is **additive only**. A member holds a permission on a resource if any one of these is true:

1. one of their roles (own, or assigned to a group they are in) holds it globally;
2. one of their roles holds it scoped to that resource;
3. the resource's allow list names them, or a group they are in, for that permission.

Otherwise denied. Suspended accounts and the `waiting` role never reach step 1. There is no deny list and no precedence to reason about. The resolution is one pure function in `core::access` that returns the **reason** (which role, group, or allow entry granted, or nothing) so the sysop UI can explain any decision, with a table-driven test over the combinations.

Group membership is resolved through a **cached transitive closure** (member to all groups, direct and nested) that is rebuilt when a group changes, never recursively per check.

## Custom roles

The sysop (and any role holding `roles.edit`) can create a role with a name, a rank below their own, and any set of permissions, globally or scoped. Examples the docs will show: a `door-admin` that holds `doors.manage` and nothing else above `member`; a `feed-curator` with `feeds.manage`; a per-board moderator made by scoping `forum.moderate` to one board.

## Resource security

Permissions say what a role may do. Resource security says it on **each thing**: every board, file area, chat room, door, feed, poll, and event is a resource with its own policy, and every post, file, and message has an owner.

- **Per-resource policy.** Each resource carries: an owner; for each role, which of the resource's permissions that role holds there (default: the role's global set); and an explicit **allow list** of members or groups who get a permission there regardless of role (how an allow-list board or an invite-only room works in the plain-ssh MVP; never called private, no per-screen label, D78). There is **no deny list**: access is purely additive. To take access away, remove the grant, remove the member from the group, change their role, or suspend the account.
- **Visibility follows policy.** A resource a member cannot `list` or `read` is absent from menus, search, new-scan, feeds out, and the SFTP tree, and is not reachable by name or ID. Tests cover every one of those paths, since a leak through search or SFTP is the same leak as one through the menu.
- **Ownership.** `*.edit_own` and delete-own apply to the member's own posts, files, and profile. Moderators act on others' through `*.moderate`. Ownership transfers are audited.
- **Resource limits per role**, overridable per member: minutes per day, calls per day, maximum concurrent sessions, posts per hour, upload bytes per day and per file, mail per day, invites per month (the credits earn rate waits for the credit economy). Exceeding a limit is a normal, honest message, never a silent drop. Limits are checked in core next to permissions, so a front end cannot forget them. Details in "Resource limits" below.
- **Sysop sees everything.** The `sysop` role holds every permission everywhere; the UI says so wherever a member sets an allow list, in keeping with the honesty rule.

## Resource limits

Status: **Implemented** (M2-H, `m2/limits`, D62, D63); not yet Verified (the QA gate has not run). Code: `core::limits`; tables `role_limit`, `member_limit`, `limit_usage` (migration `0006_limits` on both backends).

**Order: permission first, limit second, both in core.** An action first needs its permission (`Snapshot::require`, which yields the `Granted` proof); then the action asks core whether a limit allows it. A refusal is a `LimitRefusal` that names the limit, its value and when it resets, shown to the member as it is. Nothing is dropped silently.

### Kinds

A closed enum, `LimitKind`; each kind has a value per role (a number, or unlimited) and an optional per-member override.

| Kind (stored name) | Counts | Window | Enforced by |
|---|---|---|---|
| `minutes_per_day` | signed-in time | UTC day | **M2-H now** (SSH sessions) |
| `calls_per_day` | signed-in sessions started | UTC day | **M2-H now** (SSH sessions) |
| `max_sessions` | signed-in sessions open at once | none (in-process count) | **M2-H now** (SSH sessions) |
| `posts_per_hour` | forum posts and replies | rolling hour | **M3-E now** (counted from the post rows inside the write; a refusal is worded with the rolling hour, not the UTC reset of `LimitRefusal::lines`) |
| `upload_bytes_per_day` | bytes uploaded | UTC day | M4 files |
| `upload_bytes_per_file` | bytes in one file | none | M4 files |
| `mail_per_day` | mail messages sent | UTC day | M3 mail |
| `invites_per_month` | invite codes created | calendar month (UTC) | **M2-D now** (`create_invite`, from the invite rows) |

The **credits earn rate** is left out until the credit economy exists (M3 credits ledger); adding it is a new variant, its row here and a migration. The kinds not enforced yet are stored, resolved by `load_effective` and editable now; the slice that owns the action calls `load_effective` (and `add_usage` for a per-day count) when it builds the action. Hand-off rows are in [coordination.md](coordination.md).

### The effective limit

`effective_limits` (pure, unit tested) gives the limit of each kind for an account:

1. **`sysop` is unlimited**, whatever is stored, so the founder cannot lock themselves out with an edit.
2. Otherwise the **member override**, if one is set for the kind.
3. Otherwise the **most generous value among the roles the account holds globally** (additive, like permissions, D33; unlimited is the most generous). A role held only on one resource (a per-board moderator) does not count. A role with no stored value for the kind is skipped.
4. If no held role has a value, the kind is **unlimited**. (So a custom role never loosens a limit by existing, but an account holding only roles without values is unlimited. Every active account holds `member` through `everyone`, which has all values.)

Zero is a real limit ("none"). `NULL` in the table means unlimited and is an explicit value, different from "no row".

### Defaults

Seeded by migration `0006_limits` and editable with `limits.edit`. Conservative on purpose: a new board starts tight and the sysop loosens it. `waiting` has none (it never signs in). A test keeps this table equal to the seeded rows.

| Role | Minutes/day | Calls/day | Sessions | Posts/hour | Upload/day | Upload/file | Mail/day | Invites/month |
|---|---|---|---|---|---|---|---|---|
| `member` | 60 | 5 | 2 | 20 | 10 MiB | 5 MiB | 20 | 2 |
| `trusted` | 120 | 10 | 3 | 40 | 50 MiB | 10 MiB | 50 | 5 |
| `moderator` | 240 | 20 | 4 | 100 | 100 MiB | 25 MiB | 100 | 10 |
| `cosysop` | unlimited | unlimited | 8 | 200 | 1024 MiB | 100 MiB | 500 | 50 |
| `sysop` | unlimited | unlimited | unlimited | unlimited | unlimited | unlimited | unlimited | unlimited |

### What is enforced for SSH sessions now

- **At sign-in** (a member session start, `begin_member_session`), in this order so a refusal never costs a count: concurrent sessions, then minutes left today, then calls today (the call is counted only when it is allowed, in one atomic step). The refusal screen says which limit and when it resets (midnight UTC, with the time left), is printed again on the main screen so it stays in the scrollback, and the session ends.
- **At each action** (each batch of keys), for minutes: elapsed time is charged to the day's counter at least every 30 seconds and when the session ends; when the day's minutes are used up the member sees the message and is logged off. An idle session is charged too, but is told at its next keypress. A limit edit reaches an open session at its next 30-second checkpoint.
- **Not limited by this:** the waiting room and the founder claim (the M1 per-IP and per-key limits still apply to them). A founder claim's own session is not metered; its next sign-in is, as `sysop`, unlimited.
- **Counters** are persisted per account per UTC day in `limit_usage` (minutes are stored as seconds), so a restart does not reset them. Rows older than 62 days are deleted when the account signs in. Time comes from an injected clock in core, so the tests never sleep.
- **Concurrent sessions are counted in this process.** A deployment with two server processes on one database is **not covered**: each process counts for itself, so an account could hold its limit in each. Sharing the count needs a lease row in the database. The board runs as one process today.
- **If the store fails** at sign-in, the member path fails closed (the member sees the waiting room, as for any member-start failure). If it fails while charging time, the session carries on and the failure is logged: an outage must not log every member off, and the time is charged at the next checkpoint.

### Editing limits

`core::limits::{set_role_limit, set_member_limit, clear_member_limit}` take the `Granted` proof of **`limits.edit`** (held by `sysop` and `cosysop`; migration rows in `0006_limits`). There is **no sysop screen yet** (M5): these functions are what that screen and the tests call. A proof for any other permission is refused (`NotPermitted`), a number the database cannot hold is refused (`TooLarge`), and an unknown role or account is named. Every edit is **audited** (`limit.changed`, [audit.md](audit.md)) by the store in the same transaction as the write, with the account in the proof as the actor; a repeat that changes nothing writes no row. There are no rank rules on limits (a `cosysop` can edit any role's limits, including their own).

## Prior art

Borrowed, not adopted wholesale:

- **RFC 9068** (JWT profile for OAuth access tokens) names the three sources exactly as this model has them: `roles`, `groups`, `entitlements` (direct grants). The M5 API uses those claim names.
- **RFC 9396** (Rich Authorization Requests) describes a scoped grant as type, location, action. The `area.verb` permission name plus a resource ID is that triple; keep the split formal in the enum.
- **Zanzibar-style systems** (OpenFGA, SpiceDB) are additive only with groups as subject sets and a precomputed membership closure. That is why there is no deny list here.

No tokens: one process and one database, so the resolution function is the authority.

## Keep the implementation simple

This is the most intricate feature in the MVP, so the M2 access slice starts with a short written plan (a section at the end of this page) that the orchestrator reviews before any code. The plan must show:

- **One grant table** for all three sources: `grant(subject, permission, resource)` where subject is a member, a group, or a role, and resource is a specific resource or `global`. Roles are rows with subject = role; role assignment is `member_role` and `group_role`; group membership is `group_member`. No other access tables.
- **One query** that answers a check, or a cached per-session permission set computed from that table; whichever the worker picks, the pure function in core is what tests exercise.
- **Visibility** derived from the same function (`*.list`), not a second mechanism.
- **Defaults as data**: the built-in roles and groups are migration rows, not code paths.
- What is deliberately left for later (per-member limits overrides, scoped grants in the UI) and why.

## Rules

- Every check goes through `core::access`. A front end never decides.
- An area where a role holds no `*.list` or `*.read` is not listed to that role and is not reachable by name.
- Role and permission changes are audited and apply on the next action.
- `sysop` and `cosysop` are refused over telnet (D27): built in M3-H (D85) by `telnet::without_staff`, which drops every `Held` from a role of rank 40 or more, and which is applied to the session's start snapshot and to each snapshot it loads afterward.
- Adding an action without adding its permission here and in the enum does not pass review.

## Implemented / Verified

### Implemented (M2-B, `m2/access`)

- **`core::access`:** the closed `Permission` enum (all 56 rows of the table above, names via `strum`), `Slug`, `Snapshot`, the pure `resolve` and `reach`, the `Granted` proof returned by `Snapshot::require`, `GroupGraph` (nesting rules and the closure) and the `AccessStore` trait. The menu hook `PermissionCheck::allows` (M2-A) is implemented by `Snapshot`, so menus now ask the real resolution; the placeholder `RolePermissions` and the `Role` enum are gone.
- **Storage, both backends:** six tables (`role`, `role_permission`, `member_group`, `group_member`, `group_closure`, `access_grant`) in migration `0003_access` for SQLite and for Postgres, with the built-in roles, groups and grants as migration rows, and a backfill of a `sysop` grant for every M1 account whose legacy `accounts.role` is `sysop`. `accounts.role` is still written (a label) and is read by nothing.
- **Per action, not per session:** the SSH shell loads the account's snapshot (one query) before each batch of keys, so a role or group change applies to the next action. A store failure or timeout leaves the session holding nothing for that action. Nothing is cached between actions.
- **Wired paths:** the menu items (`forum.list`, `mail.read`, `files.list`, `members.view`, `members.approve`), the forum screens (M3-E, below), the terminal settings page and the save of settings, which since M3-A include the layout and theme keys (`account.edit_settings`; the save needs the `Granted` proof and names the account itself), and the waiting room (`waiting.view`; without it the shell is refused). The founder claim is the one documented exception: it has no account yet, and the setup token is its gate. Founder sessions are `sysop` through a grant row written in the claim's transaction.
- **Group changes:** `change_groups` (add, remove, delete) validates cycles and depth (at most 8 groups deep) in core and rewrites `group_closure` in the same transaction; SQLite takes the write lock first (`BEGIN IMMEDIATE`), Postgres locks `group_member`.

### Implemented (M2-F, `m2/profiles`)

- **Three permissions wired** ([profiles.md](profiles.md), D60): `members.view` (the Members menu, the list and a profile, so the Members item now opens a submenu instead of a placeholder), `account.edit_profile` (your own profile) and `members.edit` (anyone else's). They do not stand in for each other. Their `role_permission` rows already existed (migration `0003`), so migration `0005_profiles` adds only the `profiles` table. Core actions take a `Granted`; a proof for another permission is refused. Only `active` accounts are visible through any of them.
- `crates/server/tests/access_paths.rs`: both edit permissions moved to `WIRED`, each with a path test (item and key vanish on the next action, an open form is closed and writes nothing, put back it works again).

### Implemented (M2-H, `m2/limits`)

- See "Resource limits" above: `core::limits`, migration `0006_limits` on both backends, the `limits.edit` permission (one new row in the table), session hooks in `crates/server/src/ssh` (`handler.rs` starts the meter, `shell.rs` ticks it at each action and settles it at the end, `screen.rs` has the limit screen, `registry.rs` holds the in-process session count).

### Implemented (M2-B2, `m2/access-editing`)

- **Editing services** in `core::access::edit` (D69, D70): `create_role`, `set_role_permissions`, `delete_role`, `grant_role`, `revoke_role`, `grant_permission`, `revoke_permission`. Each takes the actor's `Snapshot`, checks `roles.edit` through `Snapshot::require` (and `groups.edit` as well when the subject is a group) and returns a typed `EditError`. No UI (M5).
- **Rules:** rank strictly below the actor's top global rank (with the one `sysop` exception in D69); locked roles refused; no escalation (a permission goes into a role, is granted, or a role is granted, only if the actor holds it where it applies); the last global `sysop` grant and `everyone`'s `member` grant cannot be revoked; `waiting` is never granted; built-in roles can be changed but not deleted; a role in use cannot be deleted.
- **Store, both backends:** `role`, `put_role`, `set_role_permissions`, `delete_role`, `remove_grant`, each in one transaction with its audit row (`role.created`, `role.permissions_set`, `role.deleted`, `role.revoked`, `permission.revoked`; `add_grant` now writes `role.granted` or `permission.granted`). A no-op (same permission set, a grant that is not there) writes no row. No migration.
- `Snapshot::top_rank()` (highest global role rank), for these rules and for other slices' rank checks.

### Tested

- `resolve` table test (16 cases) and two property tests (an independent oracle; `reach` agrees with `resolve`); `GroupGraph` tests (self loop, two and three cycles, diamond, depth 8 and 9, remove, delete, locked and built-in groups); a test that the enum equals the table in this page.
- The shared store suite (`suite/access.rs`, 18 checks) on SQLite, on Postgres 16 and on a TLS-required Postgres 16: seeded ranks and kinds, `waiting` has exactly three permissions, each role contains the one below, `sysop` holds every `Permission`, the founder and a plain member, unknown and inactive accounts get only the waiting snapshot (even a suspended sysop), staff, nested groups through the closure, a rejected cycle changes nothing, the depth cap, two concurrent opposite nestings make exactly one edge, scoped grants, allow entries for accounts and groups, idempotent grants, bad grants refused by name, reopen, and a closed store fails the snapshot.
- A migration test per backend that an M1-era database (migrations 1 and 2, with a sysop) gets `0003` applied and the sysop grant.
- `crates/server/tests/access_paths.rs` over a real russh client and the real SQLite store: each wired permission is taken away from the roles that give it and the path refuses (menu items vanish and their hotkeys do nothing, settings are not saved, the waiting room is refused a shell), then put back and works again; a removal applies on the very next key of an open session; and a guard test that every `Permission` is in exactly one of `WIRED` and `NOT_YET_WIRED`.

### Tested (M2-B2)

- `core::access::edit_tests` (an in-memory store that keeps state and mirrors the two store-side rules): every service refused without `roles.edit` and writing nothing; a session with no account; rank for create (0, equal, above refused), change, delete, grant and revoke; locked and built-in roles; no escalation for create, change (only additions), role grant and allow entries (global and on one resource, and a holding on one resource does not cover a wider grant); the `sysop` exception and nobody else's; the last `sysop` grant (a scoped one does not count); `everyone`'s `member` grant; a group grant needs `groups.edit`; repeated grants and revokes change nothing; `top_rank` counts global roles only.
- The shared store suite (`suite/access_edit.rs`, 9 checks) on SQLite, Postgres 16 and a TLS-required Postgres 16: a role put, changed and deleted with one audit row each and none for a repeat; deleting a granted role refused with nothing cascaded; removing a grant (role and permission, scoped) writes a row and removing nothing writes none; the last sysop grant cannot be removed even by two concurrent revokes (exactly one wins); the rules on the real seeded roles end to end; a revoked role and a role edit are seen by the next snapshot; refused edits write no audit row; **no edit lands when the audit insert fails**; the role survives a reopen.
- `crates/server/tests/access_paths.rs`: a role granted and revoked through the services over a real russh client: the next key offers (then stops offering) the `Sysop tools` item and its hotkey stops working.

### Tested (M2-H)

- `core::limits` unit tests (no database, a manual clock): the `effective_limits` table (11 cases: most generous of several roles, unlimited beats numbers, zero is real, override lowers and raises, a role without a value is skipped, rows of roles not held are ignored, sysop unlimited whatever is stored, kinds do not leak), roles held only on one resource do not count, the session counter (per account, released on drop, zero means none), session start (counts one call; refuses calls, sessions and minutes with the reset time; a refusal costs no call and no slot), the meter (minutes run out, a restart does not reset usage, a limit edit reaches an open session, midnight rollover, old rows pruned), and the editing proof.
- The shared store suite (`suite/limits.rs`, 13 checks) on SQLite, Postgres 16 and a TLS-required Postgres 16: the seeded defaults equal the table above, every role has every kind and no layer is tighter than the one below it, `limits.edit` is held by `sysop` and `cosysop` only, replace and clear, usage per account/kind/day, the capped add (including 12 concurrent adds against a cap of 5: exactly 5 succeed), pruning, survival of a reopen, and a broken store errors instead of reading as unlimited.
- `crates/server/tests/limits.rs` over a real russh client and the real SQLite store: a second sign-in over `calls_per_day` is refused with the message and the reset time; a second concurrent session is refused and the slot comes back when the first ends; a day with no minutes left refuses the sign-in without counting a call; minutes run out during a session and the member is logged off with the message, and the used time is written down; the waiting room is not limited by a member's limits.
- `crates/server/tests/access_paths.rs`: `limits.edit` is in `WIRED` with a path test (no screen yet): the edit works with the proof, a removal of the permission leaves no proof and a wrong proof is refused with nothing written, and putting it back works on the next action.

### Not done / not verified

- Services for group create, delete and membership gated by `groups.edit` alone are **not built** (D69 (5)): a `groups.edit` holder (a `cosysop`) could add an account to a group that holds a role above their rank. They need a rank rule over the group's grants first. `create_group` and `change_groups` remain plain store writes with no check. A group with no members still counts as a global `sysop` grant for the last-sysop rule. Not exercised: the editing services through a real `ssh` session beyond the grant-and-revoke path above (there is no sysop screen until M5).
- Resource limits: see M2-H above. Not verified: a two-process deployment (concurrent sessions are counted in-process), a session that stays open across several UTC days, real `ssh` for the limit screens (driven with russh clients only; see the PR report), and the posts, mail, upload and invite limits (stored and resolved, not yet enforced; the owning slices enforce them). `sysop` and `cosysop` over telnet (D27) is the M3 telnet slice. Resource-owner-managed allow lists wait for the resources (M3, M4).
- Not exercised: a deployment with two server processes on one database (nothing is cached, so it should hold, but it was not run), a database with thousands of groups, and the Postgres `LOCK TABLE` under load beyond the two-task race. A suspended account's open session is not disconnected; it simply holds the waiting snapshot from its next action.
- The first-run icon check stores its answer without `account.edit_settings` being checked as a page (it runs before the menu); the save itself is refused if the permission is not held, and the choice then lasts for the session only.

## Implementation plan

Status: **Approved and implemented** (slice M2-B), with the changes recorded in "As built" at the end of this section. The goal was the fewest tables and code paths that satisfy this page: **six tables, one resolution function, one loader query, one write path for group changes.**

### 0. Where this plan simplifies this page

These are choices the reviewer should confirm or reject; each removes a code path without removing a stated capability.

| Page says | Plan does | Why |
|---|---|---|
| "One grant table `grant(subject, permission, resource)` where subject is a member, a group, or a role" | One `access_grant` table for **account or group** subjects, granting **either a role or a permission**, global or on one resource. A role's own permission set is `role_permission` (a role definition, not a grant). | The owner asked for one shape for "a role given to a member or group" and "an allow-list entry". Putting role definitions in the same table would add a third subject kind and a third reading of every row. |
| A role "holds a permission scoped to one resource" | Done by **granting the role scoped** to a resource: the holder gets that role's permissions on that resource only. A per-board moderator is a custom role (`forum.moderate`) granted on that board, or a single allow entry for `forum.moderate`. | Same capability; no scope column on `role_permission`, no second scoped path. |
| Resource policy: "for each role, which permissions that role holds there" | **Dropped.** A resource's policy is its allow list (accounts and groups). "All members" is the `everyone` group; "staff only" is the `staff` group. | A per-resource, per-role matrix is a third grant shape. Groups already express it. |
| Role inheritance ("`trusted` = `member` plus ...") | **No inheritance.** Each role's permission set is stored in full; the migration builds them by layered `INSERT ... SELECT` so the SQL stays short. | Inheritance would need a parent column and recursion in the loader. |
| `everyone` = "all approved members" | **Implicit group**: no membership rows. The loader always adds it for an active account. It carries one locked grant, role `member`, so no per-account `member` grant row exists. | No rows to keep in sync on approve, suspend or delete. |
| Per-member limit overrides, limits in general | **Not in this slice.** See section 7. | M2-H. |

### 1. Schema: six tables, one grant shape

Tables: `role`, `role_permission`, `member_group`, `group_member`, `group_closure`, `access_grant`. Names avoid the reserved words `group` and `grant`. Roles and groups are keyed by a validated ASCII slug (`[a-z0-9-]{1,32}`, a `Slug` type in core; not content, so it does not need the sanitizer) so a decision reason reads `moderator via staff` without a join. Account ids reference `accounts(id)` and cascade on delete. A global grant stores `resource_kind = ''` and `resource_id = 0` (no NULLs, so the uniqueness index works on both backends; SQLite has no `NULLS NOT DISTINCT`).

```text
role(name PK, rank 0..100, kind: locked | builtin | custom)
role_permission(role -> role.name, permission, PK(role, permission))
member_group(id PK, name UNIQUE, kind: locked | builtin | custom)
group_member(group_id -> member_group, account_id? , child_group_id? , exactly one of the two)   -- direct edges only
group_closure(group_id -> member_group, account_id -> accounts, PK(group_id, account_id))        -- cached transitive closure
access_grant(id PK,
             account_id? | group_id?          -- exactly one: who
             role? | permission?              -- exactly one: what (a role, or one permission)
             resource_kind, resource_id)      -- where: ('' , 0) = global
```

**SQLite** (`crates/store/migrations/sqlite/0003_access.sql`; the number is whatever is free at merge time; STRICT tables like 0001):

```sql
CREATE TABLE role (
    name TEXT    NOT NULL PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 32),
    rank INTEGER NOT NULL CHECK (rank BETWEEN 0 AND 100),
    kind TEXT    NOT NULL CHECK (kind IN ('locked', 'builtin', 'custom'))
) STRICT;

CREATE TABLE role_permission (
    role       TEXT NOT NULL REFERENCES role (name) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (role, permission)
) STRICT;

CREATE TABLE member_group (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT    NOT NULL UNIQUE CHECK (length(name) BETWEEN 1 AND 32),
    kind TEXT    NOT NULL CHECK (kind IN ('locked', 'builtin', 'custom'))
) STRICT;

CREATE TABLE group_member (
    group_id       INTEGER NOT NULL REFERENCES member_group (id) ON DELETE CASCADE,
    account_id     INTEGER REFERENCES accounts (id)     ON DELETE CASCADE,
    child_group_id INTEGER REFERENCES member_group (id) ON DELETE CASCADE,
    CHECK ((account_id IS NULL) <> (child_group_id IS NULL))
) STRICT;
CREATE UNIQUE INDEX group_member_unique
    ON group_member (group_id, COALESCE(account_id, 0), COALESCE(child_group_id, 0));
CREATE INDEX group_member_by_account ON group_member (account_id) WHERE account_id IS NOT NULL;
CREATE INDEX group_member_by_child   ON group_member (child_group_id) WHERE child_group_id IS NOT NULL;

CREATE TABLE group_closure (
    group_id   INTEGER NOT NULL REFERENCES member_group (id) ON DELETE CASCADE,
    account_id INTEGER NOT NULL REFERENCES accounts (id)     ON DELETE CASCADE,
    PRIMARY KEY (group_id, account_id)
) STRICT;
CREATE INDEX group_closure_by_account ON group_closure (account_id);

CREATE TABLE access_grant (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id    INTEGER REFERENCES accounts (id)     ON DELETE CASCADE,
    group_id      INTEGER REFERENCES member_group (id) ON DELETE CASCADE,
    role          TEXT    REFERENCES role (name)       ON DELETE CASCADE,
    permission    TEXT,
    resource_kind TEXT    NOT NULL DEFAULT '',
    resource_id   INTEGER NOT NULL DEFAULT 0,
    CHECK ((account_id IS NULL) <> (group_id IS NULL)),
    CHECK ((role IS NULL) <> (permission IS NULL)),
    CHECK ((resource_kind = '' AND resource_id = 0) OR (resource_kind <> '' AND resource_id > 0))
) STRICT;
CREATE UNIQUE INDEX access_grant_unique
    ON access_grant (COALESCE(account_id, 0), COALESCE(group_id, 0),
                     COALESCE(role, ''), COALESCE(permission, ''), resource_kind, resource_id);
CREATE INDEX access_grant_by_account  ON access_grant (account_id) WHERE account_id IS NOT NULL;
CREATE INDEX access_grant_by_group    ON access_grant (group_id)   WHERE group_id   IS NOT NULL;
CREATE INDEX access_grant_by_resource ON access_grant (resource_kind, resource_id) WHERE resource_id > 0;
```

**Postgres** (`migrations/postgres/0003_access.sql`, same tables and same index names, Postgres-native types as in 0001, D24):

```sql
CREATE TABLE role (
    name TEXT     NOT NULL PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 32),
    rank SMALLINT NOT NULL CHECK (rank BETWEEN 0 AND 100),
    kind TEXT     NOT NULL CHECK (kind IN ('locked', 'builtin', 'custom'))
);
CREATE TABLE role_permission (
    role       TEXT NOT NULL REFERENCES role (name) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (role, permission)
);
CREATE TABLE member_group (
    id   BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name TEXT   NOT NULL UNIQUE CHECK (length(name) BETWEEN 1 AND 32),
    kind TEXT   NOT NULL CHECK (kind IN ('locked', 'builtin', 'custom'))
);
CREATE TABLE group_member (
    group_id       BIGINT NOT NULL REFERENCES member_group (id) ON DELETE CASCADE,
    account_id     BIGINT REFERENCES accounts (id)     ON DELETE CASCADE,
    child_group_id BIGINT REFERENCES member_group (id) ON DELETE CASCADE,
    CHECK ((account_id IS NULL) <> (child_group_id IS NULL))
);
-- indexes: identical statements to SQLite (partial and expression indexes are supported)
CREATE TABLE group_closure (
    group_id   BIGINT NOT NULL REFERENCES member_group (id) ON DELETE CASCADE,
    account_id BIGINT NOT NULL REFERENCES accounts (id)     ON DELETE CASCADE,
    PRIMARY KEY (group_id, account_id)
);
CREATE TABLE access_grant (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id    BIGINT REFERENCES accounts (id)     ON DELETE CASCADE,
    group_id      BIGINT REFERENCES member_group (id) ON DELETE CASCADE,
    role          TEXT   REFERENCES role (name)       ON DELETE CASCADE,
    permission    TEXT,
    resource_kind TEXT   NOT NULL DEFAULT '',
    resource_id   BIGINT NOT NULL DEFAULT 0,
    CHECK ((account_id IS NULL) <> (group_id IS NULL)),
    CHECK ((role IS NULL) <> (permission IS NULL)),
    CHECK ((resource_kind = '' AND resource_id = 0) OR (resource_kind <> '' AND resource_id > 0))
);
```

Indexes (both backends): `group_member_unique`, `group_member_by_account`, `group_member_by_child`, `group_closure_by_account` (the per-check lookup), `access_grant_unique` (idempotent grants, no duplicate rows), `access_grant_by_account` and `access_grant_by_group` (the loader), `access_grant_by_resource` (list or clear a resource's allow list when it is deleted). The primary keys of `role_permission` and `group_closure` cover their joins.

`accounts.role` (the M1 `CHECK (role IN ('member','sysop'))` column) is **left in place and no longer read for any decision**: SQLite cannot cheaply change a CHECK and migrations are forward-only. It stays as a legacy label until M2-D, which rebuilds `accounts` for the new states and drops it. The Rust `Role` enum in `core::store` goes away; `NewAccount` takes a `Slug` role name instead (section 6).

### 2. The resolution function

One pure function, no I/O, no async, no clock. The loader (section 3) flattens everything a member holds into a list of `Held` facts; the function only matches them.

```rust
// core::access
pub fn resolve(snapshot: &Snapshot, permission: Permission, resource: Option<ResourceRef>) -> Decision;

pub struct Snapshot { account: Option<AccountId>, held: Vec<Held> }   // built only by the store loader
pub struct Held { permission: Permission, scope: Scope, source: Source }
pub enum Scope  { Global, Resource(ResourceRef) }
pub struct ResourceRef { kind: ResourceKind, id: ResourceId }          // ResourceKind: closed enum (forum, file_area, room, door, feed, poll, event)
pub enum Source { Role { role: Slug, via: Via }, Allow { via: Via } }
pub enum Via    { Account, Group(Slug), Waiting }                      // Waiting = the role every unknown or inactive key gets

pub enum Decision { Granted(Reason), Denied }
pub struct Reason(Source);                                             // enough for "moderator, via staff" or "allow list, via group lounge"
```

Rules, in this order, first match wins so the reason is deterministic: (1) a `Held` with the permission and `Scope::Global` from a role; (2) one with `Scope::Resource(r)` from a role, when `resource == Some(r)`; (3) an `Allow` entry for `r`. A check with `resource: None` only matches global role holdings (rule 1). There is no deny arm, no precedence and no rank in the function. Rank is used only by the write path (section 6.5). A scoped grant of a permission to a resource of the wrong kind simply never matches; the write path does not police that in this slice (deferred).

The only way to obtain the `Granted` capability token is `snapshot.require(permission, resource) -> Result<Granted, Denied>`, which calls `resolve`. Core action functions (every later slice) take `&Granted` as a parameter, so an action cannot be performed without passing the check, which is the compile-time half of "adding an action without its permission does not compile".

### 3. Where a check gets its data

**Decision: load a snapshot at the start of each user action, one query, no cache across actions.** An action is one dispatched command (a menu choice, a form submit, an SFTP request), not a redraw. The snapshot is used for every check inside that action (a menu with 30 entries, a search with 200 candidate boards) and then dropped. The session stores only its `AccountId`.

- Invalidation rule: **none is needed**, because nothing is cached between actions. A role or group change is visible to the next action, which is the page's "apply on the next action", and it is correct across processes and over both backends.
- Per-session permission sets were rejected: they add a version counter, an invalidation call on every write path (suspend, role change, group change, account delete) and a staleness bug class, to save one indexed query per keypress.
- The loader for an **active** account is one statement (the same text on both backends apart from parameter syntax), returning one row per (grant x role permission):

```sql
SELECT g.role, r.rank, COALESCE(g.permission, rp.permission) AS permission,
       g.resource_kind, g.resource_id, mg.name AS group_name
FROM access_grant g
LEFT JOIN member_group    mg ON mg.id   = g.group_id
LEFT JOIN role            r  ON r.name  = g.role
LEFT JOIN role_permission rp ON rp.role = g.role
WHERE g.account_id = $1
   OR g.group_id IN (SELECT group_id FROM group_closure WHERE account_id = $1)
   OR g.group_id = (SELECT id FROM member_group WHERE name = 'everyone');
```

  An unknown stored permission name (for example a row from a newer build) is skipped and logged, so the loader fails closed.
- For an **unknown key or an account that is not `active`** (pending, approved, suspended) the loader returns the `waiting` role's permissions with `Via::Waiting` (one query on `role_permission`). So suspension and the waiting room need no special case in `resolve`, and the waiting room still reveals nothing: that role holds only `waiting.view`, `invite.request`, `invite.redeem`, `membership.apply`, `membership.open`.
- **Group membership is a cached transitive closure**, kept in `group_closure` and rebuilt inside the same transaction as every change to `group_member`. The checking path never recurses. The rebuild is done in Rust by `GroupGraph` (pure, in core): load all direct edges (a board has tens of groups), apply the change, **reject a cycle or a depth over 8**, compute the closure, then delete and re-insert `group_closure` in the transaction. Rebuilding the whole table is chosen over incremental delete logic because it cannot drift. SQLite serializes writers; on Postgres the transaction first takes `LOCK TABLE group_member IN EXCLUSIVE MODE` so two concurrent changes cannot each pass the cycle check and together create a cycle. Deleting a group, or an account, cascades through the foreign keys and the closure is rebuilt in the same transaction.
- `everyone` has no rows in either group table: the loader always adds it. It may not be a child of another group and has no members to edit.

### 4. Visibility from the same function

A resource a member cannot `list` or `read` is absent everywhere, and there is no second mechanism. `Snapshot` offers exactly two readers over the same `held` list:

```rust
pub fn resolve(&self, permission, resource) -> Decision;                 // the function above
pub fn reach(&self, permission, kind: ResourceKind) -> Reach;            // Reach::All | Reach::Only(Vec<ResourceId>) | Reach::None
```

`reach` is built from the same matching rules (`resolve(p, Some(r))` is `Granted` exactly when `reach(p, r.kind)` contains `r.id`; a property test pins that). Menus, search, new-scan, feeds out and the SFTP tree use it like this: list-style screens call `reach(*.list, kind)` and put the ids in their query (`WHERE id IN (...)` or no filter for `All`); by-name or by-id access calls `resolve(*.read, Some(r))` and answers "not found" identically for "absent" and "denied". This slice delivers the two functions, the tests, and the rule; forums, files and search are later slices, and each adds its own visibility test to the QA matrix. M2-A's menu hook takes `&Snapshot` and an optional `(Permission, ResourceRef)` per entry and calls `resolve`.

### 5. The permission enum

`core::access::Permission` is a closed enum with one variant per row of the "Permissions" table above (all of them, so roles are complete data from the start), plus `ALL`, `name() -> &'static str` (`"forum.read"`), and `parse`. Matches over it have no wildcard arm. Recommended: derive `ALL`/`name`/`parse` with the `strum` crate (`EnumIter`, `EnumString`, `IntoStaticStr`, `#[strum(serialize = "forum.read")]`), already in `Cargo.lock` through a transitive dependency and covered by `cargo deny`, per D47. Fallback if the owner prefers no new direct dependency: one declarative `permissions! { ForumRead => "forum.read", ... }` macro that generates the same four items.

Later slices add a permission in **the same PR as the action**: (1) a variant, (2) its row in the table above, (3) a forward migration that inserts its `role_permission` rows for the roles that hold it by default (the sysop row is mandatory). Two tests enforce this and fail the slice that forgets: an enum-versus-docs test (every variant appears in the table above and every name in the table parses) and a store-suite test that the `sysop` role holds every `Permission::ALL` on both backends.

### 6. Built-in roles, groups, the founder, and the write path

**6.1 Seed data (migration rows).** Roles and ranks are those in "Default roles" above (`waiting` 0, `member` 10, `trusted` 20, `moderator` 30, `cosysop` 40, `sysop` 100). `sysop` and `waiting` are `locked` (not editable); the others are `builtin` (editable, not deletable). Permission sets are inserted in layers, so the SQL is short and identical on both backends:

```sql
-- member: explicit list (account, members.view, invites.create, read/post set per this page)
-- trusted:   INSERT INTO role_permission SELECT 'trusted', permission FROM role_permission WHERE role = 'member';  + polls.create, members.vouch, invites.view_tree
-- moderator: copy of trusted + every *.moderate, files.approve, members.deny, sessions.view
-- cosysop:   copy of moderator + every *.manage (the `*.manage` and `manage_*` names), members.approve, members.suspend, members.edit, groups.edit, audit.view, events.manage, sysop.chat
-- sysop:     explicit full list; waiting: waiting.view, invite.redeem, membership.apply (invite.request is added by 0009, membership.open by 0010)
```

Groups: `everyone` (`locked`), `staff` (`builtin`, editable members). Grants: (`everyone`, role `member`, global, protected: cannot be revoked) and (`staff`, role `moderator`, global). The first migration step lists the exact `member` permission set in the PR description so the reviewer can check it against this page; `files.upload` sits in `member` and the quarantine is the files slice's rule (M4).

**6.2 Existing accounts.** The migration backfills one `access_grant` (account, role `sysop`, global) for every account whose legacy `accounts.role` is `'sysop'`. Members need no row because `everyone` carries `member`.

**6.3 Founder re-pointing (M1-D).** `claim_founder` keeps its single compare-and-set transaction and adds one statement: insert the `sysop` grant for the new account. `NewAccount` and `AccountStore::create_account` take a role name (`sysop` only from the founder path and tests; `member` writes no grant). `RoleDirectory::role_of` and the login screen's "signed in as" read the **highest-rank role in the snapshot** (`Snapshot::top_role()`), not `accounts.role`. Tests: existing founder suite plus an assertion that the founder's snapshot grants every `Permission::ALL`.

**6.4 Telnet (D27).** `sysop` and `cosysop` over telnet are refused by the M3 telnet slice by dropping any `Held` whose role rank is 40 or more before `resolve`. This slice does nothing for it except keep the rank on `Source::Role` so the telnet slice can; it is noted in coordination.

**6.5 Write path (core services over `AccessStore`) - implemented in M2-B2** (`core::access::edit`, D69, D70; see "Implemented (M2-B2)" and "As built"). The group-change path (`change_groups`, `GroupGraph`) and the plain store writes came with M2-B. The text below is the plan; "As built" lists where the build differs.

**6.5 (the plan for M2-B2, now built).** No UI in this slice (the sysop admin screens are M5); these are the functions that screens, tests and later slices call. Each takes the actor's `Snapshot` and refuses without the matching permission:

- `create_role`, `set_role_permissions`, `delete_role` (`roles.edit`). Rules: target rank strictly below the actor's top rank; `locked` roles refused; the actor may only put a permission in a role if the actor holds it globally (no escalation by editing a role).
- `grant_role` / `revoke_role` (account or group, global or on a resource) and `grant_permission` / `revoke_permission` (allow-list entries): `roles.edit` in this slice, with the same rank rule for roles. Letting a resource's owner manage its own allow list is added by the resource's slice (M3+) because resource owners do not exist yet. `waiting` cannot be granted; the (`everyone`, `member`) grant cannot be revoked.
- `create_group`, `delete_group`, `add_group_member`, `remove_group_member` (`groups.edit`): cycles and depth rejected by `GroupGraph`; `everyone` immutable; `staff` not deletable.

Storage traits (`core::access`): `AccessStore` with a few methods only: `snapshot(Option<AccountId>)`, `role(name)`, `put_role` / `set_role_permissions` / `delete_role`, `add_grant` / `remove_grant`, and `change_groups(GroupChange)` (which runs the transaction, lock, `GroupGraph` and closure rewrite described above). The rules live in core; the two backends are persistence only. Role, group and grant changes are audited by M2-C, which depends on this slice and adds the audit call inside these functions; this slice writes no audit rows and says so in the docs.

### 7. Per-role resource limits: what stays and what goes

| Stays in M2-B | Goes to M2-H |
|---|---|
| The ordering rule: permission is checked first, limits second, both in core. Documented here. | Everything else: the `limits` module, `role_limit` and `member_limit` tables for both backends, the limit kinds (minutes, calls, sessions, posts per hour, upload bytes, mail, invites, credits), per-member override, and the SSH session enforcement hooks. |
| Nothing in the schema. No limit columns on `role`. | M2-H adds its own tables keyed by `role.name` and `accounts.id`, so it needs no change to anything in this plan. |

### 8. Deferred, and why

- **Limits** (above): their own slice, no speculative columns.
- **Per-resource role matrix**: dropped in favor of the allow list and groups (section 0).
- **Role inheritance**: not needed; layered inserts give the same data.
- **Resource-owner-managed allow lists** and an `owner` column on resources: the resources do not exist until M3/M4; each resource slice adds its owner and uses `grant_permission`.
- **Checking that a scoped permission matches the resource's kind** at write time: harmless without it (a mismatched grant never matches); add when the sysop UI exists.
- **Audit rows** for role, group and grant changes: done in M2-C and M2-B2 ([audit.md](audit.md)). **Retention** and the sysop screen: M5.
- **Sysop/cosysop refused over telnet** (D27): built in M3-H (D85); tested over a real TCP client (`tests/telnet.rs`) and in unit tests (`telnet::tests`).
- **Sysop UI** to edit roles, groups and allow lists, and the "visible to the sysop" wording on allow-list screens: M5 and the resource slices.
- **RFC 9068 claim names** (`roles`, `groups`, `entitlements`) on API tokens: M5.
- **Cross-process cache or push invalidation**: not needed, nothing is cached.

### 9. Test plan

1. **Table-driven test of `resolve`** (`core::access::tests`, no database): one row per case, with the expected decision and reason. Cases: global role; role granted on another resource (denied); role granted on this resource; allow entry for the account; allow entry via a group; role via a group; allow entry for another permission (denied); `resource: None` with only a scoped holding (denied); a global holding answers a resource check; the `waiting` snapshot denies `members.view` and grants `waiting.view`; an empty snapshot denies everything; rule order (global role beats scoped beats allow, so the reason is stable). A property test with an independent oracle: `resolve` is `Granted` iff some held fact covers it, and `resolve` and `reach` agree for every resource.
2. **`GroupGraph` table tests** (pure): self loop, two-cycle, three-cycle, diamond (allowed), depth 8 allowed and depth 9 rejected, remove edge shrinks the closure, `everyone` refused as a child or parent, unknown group.
3. **Shared store suite on both backends** (`crates/store/src/suite/access.rs`, run by the SQLite tests and by the Postgres `--ignored` tests, plain and TLS-required, per D24 and D51): seeded roles, ranks and kinds; `waiting` has exactly three permissions; `member` lacks `polls.create` and `events.manage`; each layer contains the one below it; **`sysop` holds every `Permission::ALL`**; every stored permission name parses; backfill of an M1-era database (create at the old schema, migrate, check the sysop grant); founder snapshot grants everything and a restart keeps it; unknown and suspended accounts get exactly the `waiting` snapshot; closure after add, remove, nest and delete group; a rejected cycle leaves rows unchanged; two concurrent opposite-edge adds produce exactly one success and no cycle; scoped grant reaches only its resource; duplicate grant is idempotent; deleting a role, a group or an account cascades.
4. **Every front-end action path calls `core::access`.** Compile-time: core action functions take `&Granted`, so a path that skips the check does not build. Behavioral (`crates/server/tests/access_paths.rs`, real russh client): a table `ACTION_PATHS` lists each existing action with its permission and a driver (waiting-room views and forms, the settings screen, the signed-in home, the founder prompt). Each driver runs twice against a store where that permission has been removed from the role (refused, nothing written) and where it is present (works). A guard test requires every `Permission::ALL` variant to be either in `ACTION_PATHS` or in an explicit `NOT_YET_WIRED` list, so a later slice cannot add an action without moving its permission across. The founder claim is the one documented exception (it has no account yet; the setup token is its gate) and is pinned by its own test.
5. **Docs tests**: enum versus the "Permissions" table above (section 5).
6. **No-leak regression**: the existing waiting-room no-leak tests still pass unchanged in all three registration modes.

### 10. Size, order and risk for Phase 2

Phase 2 is one PR in this order: migrations for both backends and the sqlx offline cache; `Permission`, `Snapshot`, `resolve`, `reach`, `GroupGraph` with their tests; `AccessStore` for SQLite then Postgres with the shared suite; founder and `create_account` re-pointing; the write-path services; front-end call sites and `access_paths`; docs, `features.md`, coordination, one decision (next free ID at push time). Rough size: about 700 lines of core plus tests, about 350 lines per backend plus about 250 lines of shared suite, and two migrations. If review finds it too large, the write-path services in 6.5 are the clean cut: they can land as a follow-up PR without changing the schema.

Risks to flag: (a) front-end call sites sit in `crates/server/src/{ssh,settings,waiting_room}` while M2-A works in `ui`; Phase 2 merges main first and, if M2-A landed, wires only its menu access hook instead of adding call sites; (b) the legacy `accounts.role` column lives until M2-D; (c) `LOCK TABLE` on Postgres is the one piece of backend-specific locking and has its own concurrent test.

### As built (differences from the plan above)

- **Editing services:** built in M2-B2 (D69, D70) in `core::access::edit`. Differences from the plan: (1) a `sysop` may grant and revoke `sysop` despite the strict rank rule; (2) granting a role also needs the actor to hold every permission of that role (the plan checked only role edits); (3) a grant to a group needs `groups.edit` too, and the `create_group`/`delete_group`/membership services are not added (see "Not done"); (4) delete refuses while the role is granted; (5) the rank of a role never changes, so there is no "change rank" service; (6) no `grant.revoked` audit action, `role.revoked` and `permission.revoked` cover it. `AccessStore` gained `role`, `put_role`, `set_role_permissions`, `delete_role` and `remove_grant`; `add_grant` stays a plain write (no rank rules, no permission check) that the services call.
- **`strum`** derives the name, parsing and the variant list of `Permission` (`Permission::ALL`), as recommended in section 5. It was already in `Cargo.lock` (transitive); it is now a direct dependency of `russh-bbs-core`.
- **One statement per active account** also reads the account's state (it joins `accounts`), so "inactive gets the waiting snapshot" costs no second query. The waiting snapshot is a second, trivial query on `role_permission`.
- **Waiting room:** the SSH shell refuses the session (a failed shell request) if the unauthenticated snapshot does not hold `waiting.view`. The redeem form (`invite.redeem`) is wired (M2-D): `redeem_invite` requires the permission from the waiting snapshot on every attempt, and a test takes it from the `waiting` role and shows the same refusal and nothing written. `membership.apply` is wired by M2-G (below).
- **`MemberSessions`** gained `access_for(Option<AccountId>)` and `save_settings(&Granted, ..)`; `MemberStart` carries a `Snapshot` instead of a `Role`; `RoleDirectory`, the `Role` enum and `Account.role` are removed. The login and menu code read `Snapshot::top_role()`.
- **Test constructor:** `Snapshot::from_role` builds "an account holding one role globally"; the store uses it for the waiting snapshot and tests use it to say "a session that holds exactly these".
- **Query text:** the offline query cache is keyed by query text, so a query that is word for word the same on both backends would collide; the Postgres copies of parameterless queries end in `-- postgres` for that reason.
- **Invites (M2-D, D67).** `invites.create` (members and up) makes a code and lists the member's own codes; `invites.view_tree` (trusted and up) shows the invite tree; `invite.redeem` (the waiting room) redeems. `core::invites::{create_invite, my_invites, invite_tree, redeem_invite}` take the session's `Snapshot` and call `Snapshot::require` themselves, so no front end can skip the check; the menu items use the same permissions through `Action::permission()`. The three are in `WIRED` in `crates/server/tests/access_paths.rs`: `invites.create` in the menu-path test (removed from the roles, the Invites item disappears and its hotkey does nothing on the very next key), `invites.view_tree` and `invite.redeem` in `tests/invites.rs`. `invites.revoke` has no action yet and stays in `NOT_YET_WIRED`. No migration rows were needed: 0003 already gave `invites.create` to `member` and up, `invites.view_tree` to `trusted` and up, and every permission to `sysop`.
- **Account states (M2-D).** `AccountState::{Pending, Approved, Active, Suspended}` with `can_become` (pure, unit-tested exhaustively): pending to approved, active or suspended; approved to active or suspended; active to suspended; suspended to active; nothing returns to pending and no state moves to itself. `AccountStateStore::change_state` reads the current state and writes in one transaction and audits (`account.state_changed`). Only `active` is member access (`is_member_access`, the single rule the directory, the session start and the snapshot loader share); every other state gets the `waiting` snapshot, tested through the store for each state.
- **Chat (M3-F, D84).** No new permission: `chat.join`, `chat.speak`, `chat.whisper` and `chat.manage_rooms` (held by `member`, `member`, `member`, and `cosysop` and `sysop` since 0003) now have actions; `chat.moderate` does not. Rooms are `ResourceKind::Room` resources (id = the `chat_room` row id): the room list is `reach(chat.join, room)`, joining by id is `require(chat.join, room)` and a miss is "No such room." whether the room is denied, closed or never existed, speaking and whispering are `require(chat.speak / chat.whisper, room)` on every line, and every in-room action re-checks `chat.join` on the room. `chat.manage_rooms` is global and gates opening and closing a room (audited). The four are in `WIRED` in `tests/access_paths.rs`, with their path tests in `tests/chat.rs`. See [chat.md](chat.md).
- **The sysop queue (M2-E, D71, D72).** One new permission, **`invite.request`** (Waiting room group): the request-an-invite form's action, held by `waiting` and `sysop` through migration `0009_queue` (a store test checks the waiting set and that `sysop` holds every permission). `members.approve`, `members.deny` and `members.vouch` now have actions: `core::queue::{approve_request, deny_request, vouch_request}` take the `Granted` proof of exactly that permission (another proof is refused), `list_pending` needs any of the three, and `execute` calls `Snapshot::require` itself. All four are in `WIRED` in `crates/server/tests/access_paths.rs`; their path tests are in `tests/queue.rs` (taken away from the role, the key is no longer offered, pressing it writes nothing, on the very next action; the open screen closes with a notice when no permission for it remains; `invite.request` taken from `waiting` gives the same answer and stores nothing). `moderator` holds `members.deny` but the Sysop tools menu needs `members.approve`, so moderators cannot reach the screen yet (a gap listed in [queue.md](queue.md)).
- **Registration modes (M2-G, D73, D74).** `membership.apply` (since `0003`) now gates the application form and one new permission, **`membership.open`** (Waiting room group, held by `waiting` and `sysop` through migration `0010_registration`; a store test checks the waiting set and that `sysop` holds every permission), gates the create-account form. `core::registration::{submit_application, open_account}` check the permission from the waiting snapshot on every attempt (an application also needs the stored mode to be `apply` or `open`, an `open` account the stored mode `open`, both read at submission), so no front end can skip it. Both are in `WIRED` in `crates/server/tests/access_paths.rs` with path tests in `tests/registration.rs`: with the permission taken from `waiting` the form gives the same words as for any other failure and stores or creates nothing, on the very next attempt. An invite request still needs `invite.request`; the queue picks the permission by the request's kind.

### Implemented (M3-E, `m3/forums`)

Forums are the first resource kind with real resources (D83; Status: **Implemented and tested**, QA gate not run). Code: `core::forums`, tables `board`, `thread`, `post` (migration `0015_forums`, both backends); screens in `server::forums`.

- **Permissions, all existing rows, now wired:** `forum.list` (the board list; the menu item opens it), `forum.read` (open a board, a thread, a post; search), `forum.post` (start a thread), `forum.reply`, `forum.moderate` (remove a post, on the board where it is held, globally or scoped), `forum.manage` (create a board; let a member into a restricted one). `forum.edit_own` is still not wired: there is no edit screen. No new variant and no new `role_permission` row were needed; a path test (`forum_paths_follow_their_permissions_on_the_next_action` in `crates/server/tests/access_paths.rs`) takes each away while a forum screen loaded with it is open and checks that the next key is refused in words and writes nothing.
- **Lists use `reach`; by-id access answers "not found" the same for absent and denied** (store suite: the two messages are equal, for a board, a thread, a reply, a post and a removal).
- **Restricted boards.** `member` holds the forum permissions globally and access is additive, so a board that only some may use is marked `restricted` when it is created. A restricted board is reached only through a grant **scoped to that board** (an allow entry or a scoped role, to the account or a group it is in) or by a session holding `forum.manage` globally. An open board needs only the permission. `core::forums::rules::can` is the one function; lists and search use `rules::reachable`, built on `Snapshot::reach`. Staff let a member in with the `a` key on the board (`allow_member`): four allow entries (list, read, post, reply) scoped to the board, each audited as `permission.granted`. Taking access back, or restricting an existing board, is for the M5 console.
- **Search never leaks.** The boards the reader may read are part of the query, so a title, a snippet or even a count from another board is never fetched; the screen for a hidden word is the screen for a word nobody wrote. A member's query is cut into plain word terms in core before any database sees it (D83 (5)).
- **Limits:** `posts_per_hour` is enforced from the post rows inside the write (see [Resource limits](#resource-limits), the "Enforced by" column is now M3-E for this kind).

### Implemented (M3-G, `m3/feeds`)

Feeds are boards (D89; Status: **Implemented and tested**, QA gate not run). Code: `core::feeds`, tables `feed`, `feed_item`, `board_public` and the `board.feed` column (migration `0018_feeds`, both backends); the fetcher, the job and the RSS listener in `server::feeds`.

- **No new permission and no new `role_permission` row.** `feeds.manage` (global; held by `cosysop` and `sysop`) adds and removes feeds and marks boards public; it is checked in core on every call and by no screen alone. `feeds.read` stays unwired: reading a feed board is `forum.list` and `forum.read` on the board like any board (the board is a `forum` resource, not a `feed` resource).
- **A feed board is read-only by rule.** `forums::rules::can` returns `false` for `forum.post` and `forum.reply` on a board with `feed` set, for every session (a scoped grant and `forum.manage` change nothing), and the post and reply services return `ForumError::ReadOnly` before the limit check. `allow_member` on such a board grants nothing that works.
- **The public tier is the anonymous snapshot.** RSS out resolves with the snapshot an unknown key gets (`snapshot(None)`, source `Via::Waiting`) plus scoped `forum.list` and `forum.read` entries for each board in `board_public`. `rules::can` then decides readability as for any board, and a restricted board is never public (refused when marked and again when rendered). An absent, unmarked, restricted or malformed board id gets one identical answer; an id outside the snapshot never reaches the database. The waiting room does not use this snapshot.
- **Audit:** `feed.added` (resource: the new board; detail `https` or `http allowed`; never the address), `feed.removed` (detail `board <id>`), `board.published` and `board.unpublished` (resource: the board). See [audit.md](audit.md).
