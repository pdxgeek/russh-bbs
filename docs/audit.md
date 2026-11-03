# Audit log

An append-only record of who changed what (M2-C, D58, D59). Code: `crates/core/src/audit`,
`crates/store/src/{sqlite,postgres}/audit.rs`, migration `0004_audit.sql` on both backends. The
sysop screen over it is M5; the board has no audit screen yet.

## Implemented

- **Typed event.** `AuditEvent { actor, action, target, resource, detail }`. `Action` is a closed
  enum (stored as `area.verb`). `Actor` is `System` or an account. `Target` is none, an account,
  a group, a role, or a key (public `SHA256:` fingerprint only). `resource` is an optional
  `ResourceRef`. `Detail` is text that went through the shared sanitizer (`SingleLine`) and a
  120-character cap; its only constructor takes raw text and sanitizes it.
- **No secrets by construction.** Every field is an id, a closed enum, a validated slug, a public
  fingerprint, or sanitized capped text. There is no field that takes a token, key material or a
  password, and `Detail::new` / `AuditEvent::detail` take `&str`, so a secret type does not
  convert by accident (a `compile_fail` doctest passes a `SetupToken`). Callers still must not
  pass a secret as text; review for that.
- **Timestamp from an injected clock.** The store holds a `WallClock` (`SystemWallClock` by
  default, `with_clock` for tests). An event carries no time, so a caller cannot back-date one.
- **One write helper per backend, called inside the change's transaction.** `audit_in(conn,
  event)` (`SqliteStore`, `PostgresStore`) is the only function that inserts into `audit_log`
  for a change. A change and its row commit or roll back together. `AuditStore::append` (its own
  transaction) exists for events with no other write, such as a refusal. The event wording for
  today's writes is built once, in `core::audit::events`.
- **Append-only, enforced by the database.** SQLite: triggers abort `UPDATE` and `DELETE`, and a
  third aborts an insert that reuses an id (`INSERT OR REPLACE` would otherwise delete the old
  row without firing the delete trigger). Postgres: row triggers abort `UPDATE` and `DELETE`
  (so an upsert by id fails too) and a statement trigger aborts `TRUNCATE`. `AuditStore` has no
  update or delete method. There is no foreign key from `audit_log` to accounts or resources, so
  deleting an account cannot cascade into history.
- **Bounded, paged read.** `AuditStore::page(&AuditQuery)`: newest first, filter by actor, action
  and time (`since` inclusive, `until` exclusive), a cursor (`before` an id), page size clamped
  to 1..=100. The page is a list of `LogEntry`:
  `Event(AuditEntry)` for a row this binary can have written, `Unknown(UnknownEntry)` for a row whose
  **action name it does not know** (D77). A structurally invalid row is still `StoreError::Corrupt`.
- **An unknown action does not make the log unreadable (D77, FIX-M2-2).** A renamed action, or
  one left by another version, reads as `LogEntry::Unknown`: id, time, actor, the stored name as
  `UnknownAction` (sanitized as one line, capped at 64 characters), the target if this binary can
  read it (else `None`), and the sanitized `Detail`. `UnknownAction` is not an `Action`; no
  `AuditEvent`, `append` or `audit_in` accepts it, so the code cannot write one (a `compile_fail`
  doctest shows it). Filters: actor and time apply to unknown rows; an action filter is a known
  `Action`, so unknown rows match none. `Corrupt` stays for a bad actor, and for a **known** action
  with a target or resource the code could not have written. An unknown action's target and resource
  are not held to that, because another version's action may bring its own target kinds.
- **Read service gated by `audit.view`.** `core::audit::view_log(store, &Snapshot, &query)`
  checks `audit.view` globally first and does not touch the store when it is missing.

### Wired writes

| Write | Action | Actor | Target | Detail |
|---|---|---|---|---|
| Founder claim | `founder.claimed` | the new account | the new account | `role sysop` |
| `create_account` | `account.created` | `system` | the new account | `role <role> state <state>` |
| `create_group` | `group.created` | caller | the group | `name <name>` |
| `change_groups` add / remove | `group.member_added` / `group.member_removed` | caller | the group | `account <id>` or `group <id>` |
| `change_groups` delete | `group.deleted` | caller | the group | `name <name>` |
| `add_grant` | `role.granted` / `permission.granted` | caller | the subject (account or group) | `role <name>` or `permission <name>`; the grant's resource, if scoped |
| `remove_grant` (M2-B2) | `role.revoked` / `permission.revoked` | caller | the subject | same wording as the grant; the grant's resource, if scoped |
| `put_role` (M2-B2) | `role.created` | caller | the role | `rank <n> permissions <count>` |
| `set_role_permissions` (M2-B2) | `role.permissions_set` | caller | the role | `added <n> removed <n>` (counts; the names are in the role) |
| `delete_role` (M2-B2) | `role.deleted` | caller | the role | none |
| `set_role_limit` / `set_member_limit` (M2-H) | `limit.changed` | caller | the role or the account | `<kind> set to <n>`, `<kind> set to unlimited` or `<kind> override cleared`; a repeat of the same value, or clearing an override that is not set, writes no row |
| `set_terminal_settings` | `setting.changed` | the account | the account | `terminal settings` |
| `create_invite` (M2-D) | `invite.created` | the inviter | the invite (row id, never the code) | `expires at <unix seconds>` |
| `redeem_invite` (M2-D) | `account.created` | the new account | the new account | `via invite <id> by account <id> state active` |
| `redeem_invite` (M2-D) | `invite.redeemed` | the new account | the invite | `account <id> inviter <id>` |
| `change_state` (M2-D) | `account.state_changed` | caller | the account | `<from> to <to>` |
| `approve_request` (M2-E) | `account.created` | the approver | the new account | `via request <id> state active` |
| `approve_request` (M2-E) | `member.approved` | the approver | the new account | `request <id>` |
| `deny_request` (M2-E) | `member.denied` | the denier | the request (`request`, its id) | none: no text, no handle, no key |
| `vouch_request` (M2-E) | `member.vouched` | the voucher | the request (`request`, its id) | none |
| `open_account` (M2-G) | `account.created` | `system` | the new account | `via open state active` |
| `submit_request` of an application (M2-G) | `application.submitted` | `system` | the request (`request`, its id) | none: no handle, no message, no key. Written only when the application is kept, in the insert's transaction. An invite request writes no row (D71) |
| `open_account` that reaches the cap (M2-G) | `registration.capped` | `system` | none | `day <n> cap <n>` (the UTC day number). Written **once per UTC day**: by the `open` account that takes the day's last place, in its transaction and after that account's `account.created` row (or by the first refusal if the cap was lowered below the count), so strangers cannot grow the log |
| `set_password` / `clear_password` (M3-D) | `password.set` / `password.cleared` | the member (their own account) | the same account | `key session` or `password session`: which kind of session did it. Never a password, hash or salt. `password.cleared` is written only when there was a password to clear. |
| `create_board` (M3-E) | `board.created` | the creator (`forum.manage`) | none; the resource is the board | `open` or `restricted`: never the name or description |
| `remove_post` (M3-E) | `post.removed` | the moderator (`forum.moderate` on the board) | the post's author account (none if the account is gone); the resource is the board | `post <id>` or `post <id> with its thread`: ids only, never the text |
| `allow_member` (M3-E) | `permission.granted` | the creator (`forum.manage`) | the member; the resource is the board | `forum.list`, `forum.read`, `forum.post` and `forum.reply`: one row each, none when already held |
| `save_profile` by staff (M2-F) | `profile.edited` | the editor (`members.edit`) | the edited account | `fields <names>`: field names only, never values. The owner's own edits write no row |

A write that changed nothing (adding a member already in the group, removing one who is not, a
grant already held or not there, the same permission set again, a refused change) writes no row.
The M2-B2 services refuse before any write, so a refused edit (rank, locked, escalation, last
sysop) writes no row either; audit.md does not log refusals.

`create_group`, `change_groups`, `add_grant` and the M2-B2 role and revoke writes take an `Actor` argument; the store cannot know
who is calling.

### Tested (unknown actions)

The shared store suite (`suite/audit.rs`, SQLite, Postgres 16, TLS-required Postgres 16) plants
rows by hand: a page of known and unknown rows reads whole and in order; paging with a cursor
neither skips nor repeats; actor and time filters include unknown rows and an action filter never
does; hostile names and notes are sanitized; invalid structure is still `Corrupt`. Core tests cover
`UnknownAction` sanitizing and capping and `known_events`. Nothing reads the log on screen until M5,
which must render `Unknown` rows with a visible label and never as an ordinary event.

## Planned

- The rest of M2-E's list (`member.suspended`,
  `member.banned`, `member.unbanned`, `member.role_changed`; M2-E wrote `member.approved`,
  `member.denied` and `member.vouched`): see the handoff row in [coordination.md](coordination.md).
  A queue **submission** and the **purge** of expired requests write no row, on purpose (the log
  is append-only and an unauthenticated flood must not grow it; D71). Add the `Action` variants there and
  call `audit_in` in the same transaction. **M2-D** wrote `invite.created`, `invite.redeemed`,
  `account.created` (by invite) and `account.state_changed`; `invite.revoked` waits for a revoke
  action (nothing revokes an invite yet) and `quota.changed` for M2-H's editable limits.
- **M5**: the sysop screen over the log; **retention** (trimming old rows: a sysop-set policy and
  a way to prune an append-only table, for example a dated archive table or a partition drop).
  Nothing prunes the log today, so it grows without bound; that is accepted until M5.
- Auditing of board-level `set_setting` (registration mode, banner) arrives with the `settings.edit`
  screen (M5), which knows the actor.

## Verified

- Core unit tests: action names, `Detail` sanitizing and capping, target and resource round trips,
  event wording, query bounds, the `audit.view` gate (the store is not touched when denied).
- Shared store suite on SQLite and Postgres 16 (TLS required): append/page, filters and time
  bounds, paging bound and cursor, raw `UPDATE` and `DELETE` fail and leave the row, each wired
  write writes its row (and no row for a no-op or refusal), **a change does not land when the audit
  insert fails** (every write is tried against a database that refuses audit rows), reopen, and the
  read service against the real role data (sysop and cosysop may read; moderator, member and the
  waiting room may not).
- SQLite only: `INSERT OR REPLACE`, `REPLACE` and upsert by id fail. Postgres only: `TRUNCATE`
  and upsert by id fail.

Invites (M2-D) are in the same shared suite, on both backends: the created invite's row names the invite by id and neither the code nor its hash appears in the log, a redemption writes `account.created` and `invite.redeemed` in its transaction and a refusal writes none, a state change writes its row and a refused one writes none, and creating, redeeming and changing a state each fail whole when the audit insert is made to fail.

Not verified: no screen exists, so there is no `ssh` session that reads the log; the wired writes
were exercised through the store and the existing server tests, not by a new SSH path.
