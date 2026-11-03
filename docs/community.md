# One-liners, graffiti wall, last callers and polls

Status: **Implemented** (M3-J, D86), tested on SQLite, Postgres 16 and a TLS-required Postgres 16, and walked through with a real OpenSSH client (see "What was verified"). Not **Verified**: the M3 QA gate has not run.

These are the small classic BBS features. Each is a bounded list of short, sanitized text, rate-limited per member, with staff able to remove an entry. All of it is readable by every member who may see the feature and by the sysop. Nothing here is private, secret, anonymous or encrypted, and no screen says so (AGENTS.md, D78).

## Where it is

Main menu `y` (Community, shown with `community.view`) opens `o` One-liners, `g` Graffiti wall, `l` Last callers, `p` Polls. The waiting room never offers it and an unauthenticated key reaches none of it.

## Permissions

| Action | Permission | Held by |
|---|---|---|
| Open the Community menu; read the lists, last callers, polls and results | `community.view` (new) | `member` and above |
| Add a one-liner / a wall line | `oneliners.post` / `graffiti.post` | `member` and above |
| Vote | `polls.vote` | `member` and above |
| Make a poll | `polls.create` | `trusted` and above |
| Remove a one-liner, a wall line or a poll; close a poll someone else made | `community.moderate` (new) | `moderator` and above |

Each request is checked in `core::community::execute` against the snapshot loaded for that key batch, so a permission taken away applies on the very next key and a screen with an out-of-date hint refuses in words and writes nothing.

## The lists

| | One-liners | Graffiti wall |
|---|---|---|
| Length of a line | 70 characters | 60 characters |
| Lines kept (and shown) | newest 15 | newest 12 |
| Per member | 5 an hour | 10 an hour |

The text goes through the shared single-line sanitizer, is trimmed, and is refused (never cut) when blank or too long. Adding a line deletes the lines past the list size in the same transaction, so the oldest roll off. The hourly limit counts the member's lines of that kind from the last hour in the insert's own statement (SQLite) or after locking the member's account row (Postgres); two adds at once cannot both pass it. Because it counts rows still on the list, a line that rolled off or was removed stops counting. Handles are shown on every line.

Staff (`community.moderate`) press `d`, type the number shown beside a line and Enter; the removal writes `community.entry_removed` (kind and row id only, never the text) in the same transaction.

## Last callers

One row per member: the time their latest session began (UTC, to the minute). It is written at the one point member sessions start (`ssh::handler::member_content`, after the limits let the session in) and for the first session of a member who just redeemed an invite. Never the waiting room, never a founder-claim session, never a session a limit refused. The screen shows the 15 newest active accounts; a pending or suspended account never shows. **Staff sessions appear like anyone's**: D60 has no hide-from-the-list setting, so there is no rule to honour. If recording fails the login goes on (the failure is logged).

## Polls

A question (60 characters), 2 to 6 options (40 characters each, no two equal ignoring case) and an optional description (350 characters, 5 lines) typed in the full-screen editor (`d` on the review step). A member with `polls.create` makes at most 3 a rolling day; the board holds at most 90 (at 90, creation is refused until staff remove one).

- **One vote per member**, enforced by the table: `poll_vote` has the primary key `(poll_id, account_id)` and a composite foreign key to `poll_option`, so a second vote and a vote for an option the poll lacks are refused by the database itself on both backends. A vote cannot be changed. The screen answers a second try with "You have already voted on this poll."
- **Votes are stored with the voter's account.** The sysop can read who voted for what; members see counts only.
- **Results visibility is a per-poll setting**, chosen on the review step with `r`: `always`; `after_vote` (default: counts show once the viewer has voted); `after_close`. A closed poll shows counts to everyone. When counts are hidden from a viewer the store is not asked for them.
- **Close**: the creator, or anyone with `community.moderate`; one atomic `UPDATE`. A closed poll takes no votes. Staff closing someone else's poll writes `community.poll_closed`; a creator closing their own writes nothing.
- **Remove** (`x`, staff): deletes the poll, its options and its votes; writes `community.poll_removed`.
- Deleting an account deletes its lines, its last-caller row, its polls and its votes.

## Storage

Migration `0017_community` on both backends: `community_line`, `last_caller`, `poll`, `poll_option`, `poll_vote`, and the `role_permission` rows (`sysop` included). CHECK constraints repeat core's ceilings. The store trait is `core::community::CommunityStore`; `NoMembers` implements it as an empty board.

## What was verified

- `cargo test --workspace` (core unit tests for the sanitizing, bounds and the screen state machine; the server path tests in `tests/access_paths.rs`).
- The shared store suite (`suite/community.rs`, 21 checks) on SQLite, Postgres 16 and a TLS-required Postgres 16: ordering and roll-off, the hourly limit (and two simultaneous adds), audited removal, last callers (active only, newest first, one row each), a poll round trip, one vote per member (and eight simultaneous votes from one member count once), who may close, removal, the daily and board limits (and simultaneous creates), the visibility rule through `execute`, permissions through `execute`, stored text re-sanitized on read, the tables refusing what core would not write, cascades, reopen and a broken store failing closed.
- A real OpenSSH 9.6 client (`ssh -tt`, 80x24, rendered with `pyte`, one fresh ed25519 key per actor) against the debug server binary on SQLite, 28 checks, all passing: the founder claimed the board and invited `alice` and `bob` (both redeemed over ssh); the Community menu showed the four items; alice and bob posted one-liners (bob typed an escape sequence and a bell: neither reached the screen) and the sixth line of the hour was refused in words; alice wrote on the wall; last callers listed alice and bob and **not** the founder-claim session; a stranger key saw only the waiting room; a member saw an empty poll list with no create key; the sysop made a poll through the form, typed its description in the full-screen editor (Ctrl-X returned to the form) and set counts to show after close; alice and bob each voted and each second vote was refused with "You have already voted on this poll."; counts stayed hidden until the creator closed the poll, then showed 2 votes at 50% each; a vote on the closed poll was answered "closed"; the sysop removed a one-liner and `audit_log` held one `community.entry_removed` row, 2 `poll_vote` rows and 2 `last_caller` rows; the server log held no error or panic.

Not verified: Postgres under the real server (the Postgres store is tested through the shared suite only); a second server process sharing one Postgres database (the Postgres locks are tested with simultaneous requests in one process); layouts other than classic and compact were not walked by hand (the screens use the shared content renderer); screen readers; a terminal narrower than 80 columns.
