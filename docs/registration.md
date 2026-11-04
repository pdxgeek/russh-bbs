# Registration modes: `invite`, `apply`, `open`

Status: **Implemented** (M2-E for `invite`, M2-G for `apply` and `open`; D2, D23, D73, D74); not yet Verified (the M2 QA gate has not run). Code: `core::registration`, `core::waiting_room`, `core::queue` (shared with M2-E), the store's `RegistrationStore` on both backends, migration `0010_registration`.

The sysop picks one mode with the stored setting `registration_mode` (default `invite`, D2). The waiting room reads it **for every connection**, so a change needs no restart (D38). Whatever the mode, a stranger sees only the waiting room: no members, no boards, no statistics. A mode adds a registration path and nothing else.

| Mode | Menu on the waiting room's home page | What a visitor gets |
|---|---|---|
| `invite` | rules, request an invite, redeem an invite code | A queued request (M2-E, [queue.md](queue.md)) or an account from a code (M2-D) |
| `apply` | rules, apply for membership | A queued **application**. The account does not exist until a sysop approves |
| `open` | rules, create an account | An **active** account at once, within limits and a daily cap |

Switching the mode never changes existing accounts, and a request already in the queue stays there with its kind.

## `apply`

The application form asks for a handle and a line about yourself, like the request form. After the visitor submits, **one fixed text** appears, before the store is asked:

> Thank you. If your application is kept, the staff and trusted members here can read your handle, your message and this key's fingerprint, and the staff decide when they can. This key stays outside the board until they say yes. Nobody is notified: connect again later with this same key to find out.

Core then checks `membership.apply` and that the stored mode is `apply` (or `open`, where a full day shows this form), and hands the application to M2-E's queue. Everything the queue does applies unchanged ([queue.md](queue.md), D71, D72): one pending request per key, the cap (`queue_max_pending`, default 200), the per-key (3, then 1 per 10 minutes) and per-network (10, then 1 per 2 minutes) limits, 14-day expiry, the 24-hour cooldown after a denial, vouches, and the sysop's approve/deny screen (*Sysop tools, Invite requests*; an application shows as "Application N: handle"). A kept, a repeated, a rate-limited, a refused-by-a-full-queue, a cooling, a key-already-has-an-account, a permission-less, a wrong-mode and a failed submission read **identically**.

The applicant's key has no account, so it keeps reaching the waiting room. Approval creates an `active` account bound to that key with `invited_by` = the approver (the approver vouched; D71, D73). Nobody is told: the applicant finds out by connecting again.

Audit: `application.submitted` (actor `system`, target the request's id, no text, no key) when an application is kept, in the insert's transaction.

## `open`

The create-account form asks for a handle only. A valid handle becomes an attempt that the server makes in core:

1. `membership.open` must be held by the waiting snapshot.
2. The **stored** mode must be `open` right now (a form left open across a mode change is refused).
3. The key and the network must be inside the `open` gate: per key a burst of 2, then 1 an hour; per network a burst of 5, then 1 every 10 minutes (IPv6 per /64). In memory; reset on restart.
4. One store transaction: count the account against the UTC day's cap, create the account (`active`, `invited_by` NULL), bind the key, write `account.created` (actor `system`, `via open state active`). A taken handle, a lookalike, a bound key, or a full day rolls the whole transaction back.

Success turns the visitor's session into the new member's, at the login screen, exactly as redeeming an invite does. **Every failure is one sentence**: "That account could not be created. Try a different handle, or disconnect and connect again later." A taken handle, a key that already has an account, a rate limit, a full day, a mode change, a missing permission and a store failure cannot be told apart, so the form is not an oracle for who is on the board. The reason goes to the log only.

An `open` account is a plain `member` with no inviter. It starts at the lowest tier, so quotas that depend on inviter standing are the `member` role's values until the sysop raises it; the invite tree shows it as a root (D67).

### The daily cap and the degrade

`open_daily_cap` is a stored setting ([config.md](config.md)): a whole number from 1 to 100,000, or `0` for no cap; **default 25 per UTC day**; anything else reads as 25, never as "no cap". The day is the UTC day by the store's clock; the count is a one-row-per-day table incremented by a capped upsert in the account-creating transaction, so concurrent visitors cannot take the last place twice.

When the day's places are used:

- Every **new connection** gets the `apply` room in place of the `open` one: the same screens as an `apply` board, with this notice on the home and application pages until 00:00 UTC: *"Today's limit on new accounts has been reached. Until the day ends (UTC), people apply instead and the staff decide."* If the count cannot be read the room takes the `apply` screens **without** the notice, never the looser `open`.
- A visitor whose create-account form was already open is refused with the one sentence above (identical to every other refusal, so the form is no oracle). When they leave the form (Esc) the room becomes the `apply` room with its notice, on the same connection: `Screen::open_cap_reached` marks the screen, and `apply_keys` calls `WaitingRoom::degrade_open(true)` once the room is no longer on the create-account page. Until they leave, the form shows nothing about the cap. Only a full day does this; any other refusal leaves the `open` room as it was.
- The `open` account that takes the day's last place writes one `registration.capped` audit row (`day <n> cap <n>`) in the same transaction, **after** its `account.created` row (FIX-M2-4, QA M2 L2), so the log reads in the order things happened; and no more that day (if the sysop lowers the cap below the day's count, the first refusal writes it instead), so the log records the cap being reached and strangers cannot grow the append-only log.
- At 00:00 UTC the next connection is `open` again.

The notice is the one thing a stranger can read about the cap (D2 asks for it). The waiting room never shows how many places remain, how many accounts were made, or how full the queue is.

## Passwords on every join, and the user name (FIX-M3-12, D94 (6) to (10), D96)

Every join form (redeem, request, apply, create an account) asks for a password and its repeat, over ssh as over telnet, and the founder claim asks for one too. A visitor with an SSH key is bound to the key and the password; one with no key (`ssh new@host` with no key offered, or telnet) to the password alone. The SSH user name picks the path: `new` is the waiting room (any key or none), `claim` the founder claim, any other name a sign-in. A visitor with a key also sees "I already have an account" (`e`): a handle and password that bind this key to that account. `docs/decisions.md` D96 has the rules and what each refusal says.

## Keys, linking and recovery (FIX-M3-11, D97)

A visitor with an SSH key who already has an account has three ways to add that key, all in the waiting room: **I already have an account** (handle and password, D96), **Link this device** (a one-time code made on a signed-in session) and **Recovery code** (a one-time code from staff, with a new password). They are in [accounts.md](accounts.md), which also tells staff to confirm the person's identity outside the board before issuing a recovery code. A visitor with no key (telnet, or ssh with no key offered) is shown none of the three that bind a key.

## Over telnet: password accounts (FIX-M3-10, D94, D95)

A telnet client has no key, so the same waiting room has one difference: a visitor types `new` at the `login:` prompt, and every form that makes or asks for an account (redeem an invite, request an invite, apply, create an account) also asks for **a password and the same password again** (12 to 128 printable ASCII characters, D81). Nothing else about the room changes: the modes, the menu, the words, the permissions, the limits and the no-leak rule are the ssh room's, built by the same function.

- **Redeem and `open`** make an `active` account at once, bound to the password and to no key (`account_password`, no `account_keys` row), in the same transaction and with the same audit rows as the key path. The new member carries on in the same session as a plain member and can log in again with handle and password.
- **Request and apply** queue a request exactly as for a key, but it holds the **argon2id hash** of the chosen password (`invite_request.password_hash`, migration 0020) instead of relying on a key. The queue's one-request-per-identity rule, its rate limits and the denial cooldown use a stand-in identity derived from the handle (`password_identity`), so two requests for one handle (or a lookalike) are one. Approval makes an `active` account with `invited_by` = the approver and moves the hash to `account_password`; denial and expiry delete it. The fixed answer says "your password is kept only as a salted hash" and how to find out: log in again later with the handle and password. Nobody is notified.
- The password and the invite code cross the wire in clear (the `UNENCRYPTED` marker is on every screen). A password account cannot clear its password (it has no key to fall back on). A member may later add an SSH key over ssh: paste it under Members, My keys, or link a machine (see [accounts.md](accounts.md), FIX-M3-11).
- Tests: `tests/telnet.rs` over a raw TCP telnet client, the shared store suite (`password_accounts`) on SQLite and Postgres 16 (plain and TLS-required), and a real `telnet` client.

## What is stored

| Where | What | Kept until |
|---|---|---|
| `invite_request` (M2-E) | now with a `kind` column: `invite` or `application`; otherwise exactly the queue's fields (handle, one-line message, key fingerprint, two times). No address | decided, or 14 days |
| `registration_day` | UTC day number, how many `open` accounts that day, when the cap first refused someone | 7 days (pruned by the next creation) |
| `accounts` / `account_keys` | the new account, as for any account | until deleted |
| `invite_request.password_hash` (0020) | for a request made over telnet: the argon2id hash of the chosen password, never the password | decided, or 14 days |

No address, no user agent, no client banner. The in-memory gates keep the network only as the limiter's key.

## Permissions

| Permission | Held by | Gates |
|---|---|---|
| `membership.apply` | `waiting`, `sysop` (since `0003`) | the application form; wired by M2-G |
| `membership.open` | `waiting`, `sysop` (`0010`) | the create-account form |

Both are in `WIRED` in `crates/server/tests/access_paths.rs`; the path tests are in `crates/server/tests/registration.rs`. See [access.md](access.md).

## Implemented

- `core::registration`: `DailyCap`, `RegistrationStore`, `open_account`, `submit_application`, `open_cap_reached`, `OpenGate`.
- Waiting room (`core::waiting_room`): the application and create-account forms are live; `WaitingRoom::degrade_open`; `JoinKind`; fixed strings.
- Store: both backends, one transaction per operation, shared suite checks on both.
- Queue (additive): `RequestKind`, the `kind` column, the "Application N" detail title.
- Migration `0010_registration` for SQLite and Postgres.

## Not built

- **A screen for the sysop to set the mode or the cap.** Both are stored settings; the M5 console edits them. Until then, set them in the `settings` table.
- **Notification** of an application's outcome (no mail yet; M3). The queue stores no address by design (D71).
- **Persisting the gates.** They reset on restart (D37).
- **Hiding the home menu's mode.** A stranger can see which mode the board is in from the menu; the mode is not a secret, only the board's contents are.

## Verified

- Core unit tests: the cap setting and every bad value, the permission and mode checks in front of the store, the per-key and per-network gates with an injected clock, that a limited attempt never reaches the store, `degrade_open` (and that it never loosens a room), the application and create-account forms in the room.
- The shared store suite on **SQLite and Postgres 16** (plain and TLS-required): an `open` account is active with no inviter and one audit row; the cap stops at the limit, the day boundary (last second capped, first second of the next day fresh) with an injected clock, one `registration.capped` row per day (written by the account that takes the last place, after that account's `account.created` row: an id-order test), a raised cap, `0` = unlimited; a refused creation leaves no trace and spends no place; 14 concurrent creators against a cap of 5 make exactly 5; old counter rows pruned; a creation never lands without its audit row; the mode and the cap setting are read on every call; an application is a row of its kind with one audit row and no content, never lands without it, and is approved by M2-E's path.
- A real russh client over SSH (`tests/registration.rs`): an applicant lands in the queue and stays outside until the sysop approves from the screen, then signs in; the same words for every application outcome; an `open` visitor gets an active account with no inviter; one sentence for every kind of refusal; the network gate; the cap, the degrade with its notice (including a create-account form already open when the last place goes: refused with the one sentence, then the `apply` room and notice on Esc, same connection), the UTC day boundary, `0` and a bad value; the waiting room's screens in `apply` and `open` byte-identical across an empty, a busy and a full board; a mode change seen by the next connection.

- **A real OpenSSH client** (OpenSSH 9.6, `ssh -tt`, rendered with `pyte` at 80x24) against the built server binary on SQLite, 32 checks, all passing: in `apply` mode a stranger applies and reads the fixed answer, the typed handle and message are not left on screen, the queue holds one row of kind `application`, no account exists and the same key reconnects to the waiting room; the founder claims the board, opens *Sysop tools, Invite requests*, sees the application (detail titled "Application 1: hopeful") and approves it; the applicant reconnects as a member (`active`, invited by the approver). The stored mode is then switched to `open` with a cap of 2 **while the server runs** (no restart): two visitors become members at once (`active`, no inviter); the third sees the full-day notice and the apply screens, with no count on screen, and applies (queued as an application); the audit log holds exactly one `registration.capped` row; a taken handle gets the one fixed sentence, identical from two keys. Afterwards no typed handle or message is in the server log or the audit log, and neither `invite_request` nor `registration_day` has an address column.

**Not verified:** a terminal emulator other than OpenSSH under `pyte`; a screen reader; two server processes on one database (the cap and the one-per-key rule are enforced in the database, so they should hold); the degrade of a session whose form was open when the last place went (it gets the generic refusal by design, see above).
