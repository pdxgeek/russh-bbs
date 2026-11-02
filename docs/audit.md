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
  to 1..=100. A stored row the code could not have written is `StoreError::Corrupt`.
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
| `add_grant` | `grant.added` | caller | the subject (account or group) | `role <name>` or `permission <name>`; the grant's resource, if scoped |
| `set_terminal_settings` | `setting.changed` | the account | the account | `terminal settings` |
| `save_profile` by staff (M2-F) | `profile.edited` | the editor (`members.edit`) | the edited account | `fields <names>`: field names only, never values. The owner's own edits write no row |

A write that changed nothing (adding a member already in the group, removing one who is not, a
grant already held, a refused change) writes no row.

`create_group`, `change_groups` and `add_grant` take an `Actor` argument; the store cannot know
who is calling.

## Planned

- **M2-B2** (role, grant, revoke editing), **M2-D/M2-E** (approvals, denials, bans, role
  changes): see the handoff row in [coordination.md](coordination.md). Add the `Action` variants
  there and call `audit_in` in the same transaction.
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

Not verified: no screen exists, so there is no `ssh` session that reads the log; the wired writes
were exercised through the store and the existing server tests, not by a new SSH path.
