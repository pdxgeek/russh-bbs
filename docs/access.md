# Permissions and roles

Status: **Implemented** (M2-B, not yet Verified: the QA gate has not run). The model is decision D33 in [decisions.md](decisions.md); how it was built is D55 and the "Implementation plan" at the end of this page. This page grows as milestones add actions, and every new action adds a row here in the same PR.

## Permissions

One named permission per action, a closed enum in `core::access`. Names are `area.verb`. A front end cannot perform an action without the check passing in core.

| Group | Permissions |
|---|---|
| Waiting room | `waiting.view`, `invite.redeem`, `membership.apply` |
| Account | `account.edit_profile`, `account.edit_settings`, `account.set_password` |
| Members | `members.view`, `members.approve`, `members.deny`, `members.vouch`, `members.suspend`, `members.edit`, `members.delete_data` |
| Invites | `invites.create`, `invites.revoke`, `invites.view_tree` |
| Forums | `forum.list`, `forum.read`, `forum.post`, `forum.reply`, `forum.edit_own`, `forum.moderate`, `forum.manage` |
| Chat | `chat.join`, `chat.speak`, `chat.whisper`, `chat.moderate`, `chat.manage_rooms` |
| Mail | `mail.read`, `mail.send` |
| Files | `files.list`, `files.download`, `files.upload`, `files.tag`, `files.approve`, `files.manage_areas` |
| Feeds | `feeds.read`, `feeds.manage` |
| Doors | `doors.run`, `doors.manage` |
| Boardlets | `oneliners.post`, `graffiti.post`, `polls.vote`, `polls.create`, `events.view`, `events.manage` |
| Sysop | `sysop.page`, `sysop.chat`, `sessions.view`, `sessions.disconnect`, `audit.view`, `settings.edit`, `roles.edit`, `groups.edit`, `themes.manage`, `backups.run` |

A permission is held either globally (from the role) or scoped to one area (one board, file area, room, or door). Scoped grants are how a trusted member moderates one board without becoming a moderator everywhere.

## Default roles

Shipped so a sysop never has to build a role from scratch. All of them except `sysop` and `waiting` can be edited; any can be copied as the starting point of a custom role.

| Role | Rank | Holds | Meant for |
|---|---|---|---|
| `waiting` | 0 | Waiting room group only | Unknown and unapproved keys. Not editable. |
| `member` | 10 | Account, `members.view`, `invites.create`, all read and post permissions in Forums, Chat, Mail, Files (`files.upload` goes to quarantine), Feeds, Doors, Boardlets except `polls.create` and `events.manage` | Everyone who was approved |
| `trusted` | 20 | `member` plus `polls.create`, `members.vouch`, `invites.view_tree` | Long-standing members; the inviter tier |
| `moderator` | 30 | `trusted` plus every `*.moderate`, `files.approve`, `members.deny`, `sessions.view` | Keeps the board clean; no settings or roles |
| `cosysop` | 40 | `moderator` plus every `*.manage`, `members.approve`, `members.suspend`, `members.edit`, `groups.edit`, `audit.view`, `events.manage`, `sysop.chat` | Runs the board day to day; cannot edit roles, settings, or backups |
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

- **Per-resource policy.** Each resource carries: an owner; for each role, which of the resource's permissions that role holds there (default: the role's global set); and an explicit **allow list** of members or groups who get a permission there regardless of role (how a private board or an invite-only room works in the plain-ssh MVP, labeled "visible to the sysop" as always). There is **no deny list**: access is purely additive. To take access away, remove the grant, remove the member from the group, change their role, or suspend the account.
- **Visibility follows policy.** A resource a member cannot `list` or `read` is absent from menus, search, new-scan, feeds out, and the SFTP tree, and is not reachable by name or ID. Tests cover every one of those paths, since a leak through search or SFTP is the same leak as one through the menu.
- **Ownership.** `*.edit_own` and delete-own apply to the member's own posts, files, and profile. Moderators act on others' through `*.moderate`. Ownership transfers are audited.
- **Resource limits per role**, overridable per member: minutes per day, calls per day, maximum concurrent sessions, posts per hour, upload bytes per day and per file, mail per day, invites per month, credits earn rate. Exceeding a limit is a normal, honest message, never a silent drop. Limits are checked in core next to permissions, so a front end cannot forget them.
- **Sysop sees everything.** The `sysop` role holds every permission everywhere; the UI says so wherever a member sets an allow list, in keeping with the honesty rule.

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
- `sysop` and `cosysop` are refused over telnet (D27).
- Adding an action without adding its permission here and in the enum does not pass review.

## Implemented / Verified

### Implemented (M2-B, `m2/access`)

- **`core::access`:** the closed `Permission` enum (all 56 rows of the table above, names via `strum`), `Slug`, `Snapshot`, the pure `resolve` and `reach`, the `Granted` proof returned by `Snapshot::require`, `GroupGraph` (nesting rules and the closure) and the `AccessStore` trait. The menu hook `PermissionCheck::allows` (M2-A) is implemented by `Snapshot`, so menus now ask the real resolution; the placeholder `RolePermissions` and the `Role` enum are gone.
- **Storage, both backends:** six tables (`role`, `role_permission`, `member_group`, `group_member`, `group_closure`, `access_grant`) in migration `0003_access` for SQLite and for Postgres, with the built-in roles, groups and grants as migration rows, and a backfill of a `sysop` grant for every M1 account whose legacy `accounts.role` is `sysop`. `accounts.role` is still written (a label) and is read by nothing.
- **Per action, not per session:** the SSH shell loads the account's snapshot (one query) before each batch of keys, so a role or group change applies to the next action. A store failure or timeout leaves the session holding nothing for that action. Nothing is cached between actions.
- **Wired paths:** the menu items (`forum.list`, `mail.read`, `files.list`, `members.view`, `members.approve`), the terminal settings page and the save of settings (`account.edit_settings`; the save needs the `Granted` proof and names the account itself), and the waiting room (`waiting.view`; without it the shell is refused). The founder claim is the one documented exception: it has no account yet, and the setup token is its gate. Founder sessions are `sysop` through a grant row written in the claim's transaction.
- **Group changes:** `change_groups` (add, remove, delete) validates cycles and depth (at most 8 groups deep) in core and rewrites `group_closure` in the same transaction; SQLite takes the write lock first (`BEGIN IMMEDIATE`), Postgres locks `group_member`.

### Implemented (M2-F, `m2/profiles`)

- **Three permissions wired** ([profiles.md](profiles.md), D60): `members.view` (the Members menu, the list and a profile, so the Members item now opens a submenu instead of a placeholder), `account.edit_profile` (your own profile) and `members.edit` (anyone else's). They do not stand in for each other. Their `role_permission` rows already existed (migration `0003`), so migration `0005_profiles` adds only the `profiles` table. Core actions take a `Granted`; a proof for another permission is refused. Only `active` accounts are visible through any of them.
- `crates/server/tests/access_paths.rs`: both edit permissions moved to `WIRED`, each with a path test (item and key vanish on the next action, an open form is closed and writes nothing, put back it works again).

### Tested

- `resolve` table test (16 cases) and two property tests (an independent oracle; `reach` agrees with `resolve`); `GroupGraph` tests (self loop, two and three cycles, diamond, depth 8 and 9, remove, delete, locked and built-in groups); a test that the enum equals the table in this page.
- The shared store suite (`suite/access.rs`, 18 checks) on SQLite, on Postgres 16 and on a TLS-required Postgres 16: seeded ranks and kinds, `waiting` has exactly three permissions, each role contains the one below, `sysop` holds every `Permission`, the founder and a plain member, unknown and inactive accounts get only the waiting snapshot (even a suspended sysop), staff, nested groups through the closure, a rejected cycle changes nothing, the depth cap, two concurrent opposite nestings make exactly one edge, scoped grants, allow entries for accounts and groups, idempotent grants, bad grants refused by name, reopen, and a closed store fails the snapshot.
- A migration test per backend that an M1-era database (migrations 1 and 2, with a sysop) gets `0003` applied and the sysop grant.
- `crates/server/tests/access_paths.rs` over a real russh client and the real SQLite store: each wired permission is taken away from the roles that give it and the path refuses (menu items vanish and their hotkeys do nothing, settings are not saved, the waiting room is refused a shell), then put back and works again; a removal applies on the very next key of an open session; and a guard test that every `Permission` is in exactly one of `WIRED` and `NOT_YET_WIRED`.

### Not done / not verified

- The editing services (create or change a role, grant or revoke, with the rank rules) are **M2-B2**; nothing writes roles or grants except the founder claim, account creation, group changes and the tests. Group changes and grants written by `add_grant` are audited since M2-C ([audit.md](audit.md)); the editing services add their own rows.
- Resource limits are **M2-H**. `sysop` and `cosysop` over telnet (D27) is the M3 telnet slice. Resource-owner-managed allow lists wait for the resources (M3, M4).
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
- For an **unknown key or an account that is not `active`** (pending, approved, suspended) the loader returns the `waiting` role's permissions with `Via::Waiting` (one query on `role_permission`). So suspension and the waiting room need no special case in `resolve`, and the waiting room still reveals nothing: that role holds only `waiting.view`, `invite.redeem`, `membership.apply`.
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
-- sysop:     explicit full list; waiting: waiting.view, invite.redeem, membership.apply
```

Groups: `everyone` (`locked`), `staff` (`builtin`, editable members). Grants: (`everyone`, role `member`, global, protected: cannot be revoked) and (`staff`, role `moderator`, global). The first migration step lists the exact `member` permission set in the PR description so the reviewer can check it against this page; `files.upload` sits in `member` and the quarantine is the files slice's rule (M4).

**6.2 Existing accounts.** The migration backfills one `access_grant` (account, role `sysop`, global) for every account whose legacy `accounts.role` is `'sysop'`. Members need no row because `everyone` carries `member`.

**6.3 Founder re-pointing (M1-D).** `claim_founder` keeps its single compare-and-set transaction and adds one statement: insert the `sysop` grant for the new account. `NewAccount` and `AccountStore::create_account` take a role name (`sysop` only from the founder path and tests; `member` writes no grant). `RoleDirectory::role_of` and the login screen's "signed in as" read the **highest-rank role in the snapshot** (`Snapshot::top_role()`), not `accounts.role`. Tests: existing founder suite plus an assertion that the founder's snapshot grants every `Permission::ALL`.

**6.4 Telnet (D27).** `sysop` and `cosysop` over telnet are refused by the M3 telnet slice by dropping any `Held` whose role rank is 40 or more before `resolve`. This slice does nothing for it except keep the rank on `Source::Role` so the telnet slice can; it is noted in coordination.

**6.5 Write path (core services over `AccessStore`) - DEFERRED to M2-B2.** The orchestrator cut the editing services from this PR: no UI calls them until M5. What stayed is the group-change path (`change_groups`, `GroupGraph`) and the plain store writes the founder claim, the backfill and the suite need. The text below is kept as the plan for M2-B2.

**6.5 (plan for M2-B2).** No UI in this slice (the sysop admin screens are M5); these are the functions that screens, tests and later slices call. Each takes the actor's `Snapshot` and refuses without the matching permission:

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
- **Audit rows** for role, group and grant changes: group changes and `add_grant` done in M2-C ([audit.md](audit.md)); the M2-B2 editing services add theirs. **Retention** and the sysop screen: M5.
- **Sysop/cosysop refused over telnet** (D27): M3 telnet slice.
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

- **Editing services:** not built (section 6.5), see M2-B2 in [coordination.md](coordination.md). `AccessStore` has `snapshot`, `roles`, `groups`, `create_group`, `change_groups` and `add_grant` (a plain write that refuses `waiting` and unknown targets; no rank rules, no permission check).
- **`strum`** derives the name, parsing and the variant list of `Permission` (`Permission::ALL`), as recommended in section 5. It was already in `Cargo.lock` (transitive); it is now a direct dependency of `russh-bbs-core`.
- **One statement per active account** also reads the account's state (it joins `accounts`), so "inactive gets the waiting snapshot" costs no second query. The waiting snapshot is a second, trivial query on `role_permission`.
- **Waiting room:** the SSH shell refuses the session (a failed shell request) if the unauthenticated snapshot does not hold `waiting.view`. The forms inside the room (`invite.redeem`, `membership.apply`) store nothing yet, so those two permissions are in `NOT_YET_WIRED` until M2-D, M2-E and M2-G wire them.
- **`MemberSessions`** gained `access_for(Option<AccountId>)` and `save_settings(&Granted, ..)`; `MemberStart` carries a `Snapshot` instead of a `Role`; `RoleDirectory`, the `Role` enum and `Account.role` are removed. The login and menu code read `Snapshot::top_role()`.
- **Test constructor:** `Snapshot::from_role` builds "an account holding one role globally"; the store uses it for the waiting snapshot and tests use it to say "a session that holds exactly these".
- **Query text:** the offline query cache is keyed by query text, so a query that is word for word the same on both backends would collide; the Postgres copies of parameterless queries end in `-- postgres` for that reason.
