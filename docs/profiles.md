# Profiles and the members list

Status: **Implemented** (M2-F, `m2/profiles`, decision D60), not yet Verified: the M2 QA gate has not run. Permissions are in [access.md](access.md); the screens use the framework in [ui.md](ui.md) and decision D54.

## What a profile is

Five short text fields and nothing else. No avatar, no custom fields, no links.

| Field | Most characters | Notes |
|---|---|---|
| Display name | 40 | Shown beside the handle in the list |
| Pronouns | 24 | Free text |
| Location | 40 | Free text |
| About | 280 | One line of text (wrapped on screen) |
| Website | 100 | **Plain text.** Never fetched, never linked, never validated as a URL, so it needs no SSRF protection |

Every field is single-line text that has passed the shared sanitizer (`core::sanitize`, `SingleLine`) with its cap and a trim. That happens **on write** (`Profile::from_raw`, the only way to build a `Profile` from typed text), **on read** (`Profile::from_stored`: a row edited by hand is never trusted) and **on render** (the server cleans each value once more on its way to the terminal). The handle is a validated username, not profile text. Content is drawn in plain style: the reserved marker style is never applied to it.

## Who may do what

| Action | Permission | Held by default |
|---|---|---|
| Open the Members menu, list members (paged, sorted by handle, handle-prefix search), open a profile | `members.view` | `member` and above |
| Edit **your own** profile | `account.edit_profile` | `member` and above |
| Edit **someone else's** profile | `members.edit` | `cosysop`, `sysop` |

The two edit permissions do not stand in for each other: `members.edit` is about other people, and `account.edit_profile` about your own. Core actions take a `Granted` from `Snapshot::require` (`list_members`, `view_profile`, `edit_profile`, `load_for_edit`); a proof for another permission is refused. The screens ask for work through `profiles::execute`, which checks against the snapshot loaded for that action, so a role change applies on the next key. Open screens close when the permission they need goes away (same rule as the settings page).

**Only active accounts are members.** Pending, approved and suspended accounts are never listed, never opened and never editable, and an account that is absent and one that is not active get the same answer (`NotFound`), so neither can be probed by id or by handle prefix. An unauthenticated or `waiting` key holds none of these permissions, has no path to the screens (the waiting room shows nothing about members), and a request without the permission never reaches the store (tested).

**Rank rule (D60 (6), FIX-M2-1).** Editing **someone else's** profile also needs the target's top global rank to be strictly below the editor's (`Snapshot::top_rank`; the target's comes from `ProfileStore::standing`, which reads the same snapshot the editing services use). A `cosysop` cannot edit a `sysop`'s profile or another `cosysop`'s; a target holding no role counts as below; an editor with no global rank edits nobody else's. Editing your own profile is unchanged. The permission is checked first, so a session without `members.edit` gets `NotPermitted` and learns nothing about ranks. A refusal is `ProfileError::RankTooLow`, `Response::RankTooLow`, and the staff-edit screen says "You cannot edit the profile of someone whose role ranks at or above yours. Nothing was changed."; nothing is written and no audit row. The rank is read when the request starts, so a promotion between the check and the write is not guarded (text only; the edit is audited).

## Screens

`Main menu > [u] Members` opens a submenu (a normal `core::menu` menu, items gated by permission):

- `[l] Member list`: nine members a page, `1`-`9` opens one, `n`/`p` page, `/` searches by the start of a handle (letters, digits, `_`, `-` only, case-insensitive), `c` clears the search, Esc goes back. Paging is by the last handle seen, so a member who joins while someone reads cannot make a page skip or repeat a row.
- `[p] My profile`: your own profile, read-only, with `e` to edit.
- `[e] Edit my profile`: the form.

On a profile, `e` is offered only if an edit would be allowed (own profile with `account.edit_profile`, someone else's with `members.edit`). The form starts from the current text, edits one field at a time (typing appends, Backspace erases, **Enter moves to the next field, and Enter on the last field saves**), and Esc discards. When staff edit someone else's profile the title names them ("Editing the profile of bob (staff edit)").

All text comes from the string table (`ui/strings.toml`), so a theme can reword it. Content goes in the classic layout's content rectangle; the last row is the reserved status row and shows the server-readable marker, which content cannot reach.

## Honesty

The server can read every profile, and so can every member. The form says so in plain words ("The sysop and every member can read this profile.") and again in the status row ("The server can read what you do here."). Nothing is called private and no field has a lock. Tests assert the words and the absence of "private", "encrypted" and lock glyphs on the form.

## Storage

Table `profiles` (migration `0005_profiles` on SQLite and Postgres): `account_id` primary key and foreign key to `accounts(id)` with `ON DELETE CASCADE`, five `TEXT NOT NULL` columns, `updated_at`. No row means an empty profile. Caps are enforced in core, not in the schema, so widening one needs no migration. The member list orders and pages on `lower(username)` (handles are ASCII, unique ignoring case), with `COLLATE "C"` on Postgres so both backends sort the same bytes. No `role_permission` rows were needed: the three permissions were already held by the right roles (migration `0003`).

## Audit

`edit_profile` returns `Saved { target, changed, by_staff }`. When `by_staff` is true (someone with `members.edit` edited another member) the same call passes an audit event to `ProfileStore::save_profile`, which writes it **in the profile write's own transaction** with the M2-C helper (`audit_in`): `profile.edited`, actor the editor, target the edited account, detail `fields <names>` (field names only, never values). If the audit insert fails the edit does not land. The owner's own edits write no row. See [audit.md](audit.md).

## Implemented / Planned / Verified

**Implemented:** the above, in `core::profiles` (fields, store trait, service, screen state machine), `server::members` (drawing, request running), the Members menu, `ProfileStore` for SQLite and Postgres, and the shared store suite checks for both.

**Tested:** core unit tests (caps, hostile text in every field, re-sanitizing on read, paging and search against a fake store, every proof and permission case, inactive and absent targets alike, the waiting snapshot never reaching the store, the screen state machine); the shared store suite (`suite/profiles.rs`, 10 checks, including the audit row and the rollback when the audit insert fails) on SQLite, on Postgres 16 and on a TLS-required Postgres 16; server drawing tests at 80x24 and four other sizes in all three icon tiers (fullest screens fit with their footer, wide characters stay on one line, the status row is identical to an empty screen and no content cell has the reserved style, ASCII tier draws only ASCII, the form is honest); and over a real russh client against the real SQLite store (`tests/members.rs`, `tests/access_paths.rs`; the staff edit's audit row is checked there too): browse, page, search, open, edit own, staff edit, hand-edited hostile rows, a stranger and a suspended key learning nothing, and each edit permission taken away mid-screen with nothing written.

**Rank rule tested:** core tests (equal, above and below rank, no global rank, own profile, no permission, the screen notice) and a shared store check that `standing` is the top global rank on SQLite and Postgres, and over a real russh client (`tests/members.rs`: a `cosysop` is refused on a `sysop`'s profile with nothing written and no audit row, and still edits and audits a member's).

**Not built / not verified:** no `ssh` OpenSSH client was available in the build environment, so the screens were driven with russh clients, not a real terminal emulator; a sysop-visible "last edited" is stored (`updated_at`) but not shown; search is by handle prefix only (no display-name search); no export or delete of a profile (`members.delete_data` is not built).
