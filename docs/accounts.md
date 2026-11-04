# Accounts, keys, device linking and staff recovery

Status: **Implemented and Verified** for FIX-M3-11 (D97), within the limits under "Not built" and "Verified" below. An account is a handle plus a password (D94); an SSH key is the optional extra. This page covers the extra: managing keys, linking a new machine without the password, and getting someone back in. How an account is made is [registration.md](registration.md); the sign-in flow is `core::signin` (D96); the permissions are in [access.md](access.md); the audit rows are in [audit.md](audit.md).

## My keys

**Members, My keys** (`u`, `k`; permission `keys.manage`). It lists up to four keys by fingerprint, each with when it was added and when it last signed in ("never" if it has not since this was recorded); a line says when more exist. The screen's keys:

| Key | Does |
|---|---|
| `a` | **Add a key.** Paste the one `ssh-ed25519 AAAA...` line of a `.pub` file. Only Ed25519 is accepted for a pasted key. A private key, another type or a bad line is refused with a sentence that never echoes the paste. A key that already belongs to an account, this one or another, gets one message: "That key cannot be added." |
| `r` | **Remove a key.** Pick it by number, confirm with `y`. **The last way in is never removed**: a key can go only while the account has a password or another key. |
| `l` | **Link a new machine** (below). |

The server stores only each key's public fingerprint. Nothing on these screens says "private".

## Linking a new machine without the password

1. On a signed-in session choose **Link a new machine**. The screen shows a code once: 10 characters (50 random bits), for example `7QX9M-2KD4P`. It works once, expires in 10 minutes, and you can make at most 5 an hour. A new code voids the last unused one. Only a SHA-256 hash is stored.
2. On the new machine connect as `ssh new@host` (the SSH user name `new` is the waiting room, D96). Choose **Link this device** (`d`) and type the code. The connection's key is bound to the account and the machine is signed in.

A telnet session can make a code, and the page says it can be used from an ssh connection only: telnet has no key to bind, so the telnet waiting room does not offer **Link this device** or **Recovery code**. A connection that offers no key (`ssh new@host` with no key) is in the same position.

`ssh <handle>@host` with a key that is not bound to that handle is a refusal that falls back to the password (D96); it does not reach the waiting room, so a new machine uses `new@`.

## Staff recovery code

For a member who has lost their key and their password.

1. **Confirm who is asking, outside the board.** A call, a message on a channel you already trust, in person. The board has nothing to check them against. The screen says this before it asks for the handle.
2. Under **Sysop tools, Recovery code** (permission `keys.recover`; `cosysop` and `sysop` by default) type the member's handle. The screen shows a code once: 16 characters (80 random bits) in four groups of four. It works once, expires in 24 hours, and a new code voids the last. Only a hash is stored. Issuing it is audited (`recovery_code.issued`: the staff member, the member, no code).
3. Give the member the code. They connect as `ssh new@host`, choose **Recovery code** (`y`), type the code and **a new password twice** (12 to 128 characters, D81). In one step the connection's key is bound, the password is replaced and the code is used up.

**Rank rule (D69):** the member must be active and strictly below the staff member's rank. A cosysop cannot help a sysop or another cosysop, and nobody can issue a code for themselves. There is no code for a sysop: a second sysop, or the setup token on a fresh board, is the way.

Recovery does **not** remove the member's older keys: the server cannot know which one was lost and which stolen. The member is told at the next sign-in and removes any key they do not recognise under My keys.

## What the member is told

Every key added (pasted, linked with the password in the waiting room, linked with a code), every key removed and every code redeemed leaves a notice in the same transaction. The member's **next sign-in** (ssh or telnet) lists them on the login screen:

```
Changes to your keys since you were last here:
A key was added on 2026-11-04 12:00 UTC (SHA256:...).
A key was removed on 2026-11-04 12:05 UTC (SHA256:...).
If this was not you, remove the key under My keys, change your password, and tell the sysop.
```

Each is shown once. Nothing marks a notice told when the session that caused it ends, so a change made by someone else is still shown to the owner.

## No leak

A wrong, used, replaced, expired or other-kind code, a key that already has an account, an account that is no longer active, a weak password, a missing permission and a store failure all give one sentence: "That code did not work. Check it, or ask for a new one." A refusal writes nothing, and a code refused only because the key was taken is still usable. Redeeming is rate limited per network and per presenting key with the password limiter (D81, D96). The recovery password is hashed before the code is looked up, so a wrong code costs what a right one does.

## What is stored

| Where | What | Kept until |
|---|---|---|
| `account_keys.last_used_at` | when the key last signed in | the key is removed |
| `login_code` | kind, the SHA-256 of the code, the account, who made it, three times (made, expires, used or replaced). Never the code | the account is deleted (rows are small; a retention sweep is future work) |
| `key_notice` | kind, time, the key's public fingerprint, and whether it was told | the account is deleted |

## Implemented

- `core::keys`: the code type and its hash, the strict Ed25519 public-key parser, the service (list, add, remove, make a link code, issue a recovery code, redeem both), the My keys and staff screens as pure state machines, the notice wording.
- Store, SQLite and Postgres, migration `0022`: the three permissions, the two tables, the one `DELETE` that keeps the last way in, the single-statement code claim, the hourly limit under a lock.
- Waiting room: **Link this device** (`d`) and **Recovery code** (`y`) for a visitor with a key; the Members menu entry; the Sysop tools entry; the login notices.
- Audit: `key.added`, `key.removed`, `device_code.issued`, `device_code.redeemed`, `recovery_code.issued`, `recovery_code.redeemed` (and `key.linked` now leaves a notice too).

## Not built

- A retention sweep for used and expired codes.
- Naming a key ("work laptop"). A fingerprint and its dates are what the list has.
- Pasting an ECDSA key (D21 lets one sign in; a machine that has one links by code).
- Email as a second recovery route (D94 (9)).
- A recovery code for a sysop.
- A reminder that a key was last used long ago.

## Verified

- Core unit tests (`keys::tests`, `keys::pubkey`, `keys::code`, waiting room): the rank rule (a cosysop for a sysop, an equal, themselves, an unknown, a suspended member), the permission in front of every action, the generic refusals, the code kinds not crossing, the screens, the fingerprint of the all-zero Ed25519 key matching `ssh-keygen -l`.
- The shared store suite (`suite/keys.rs`, 17 checks) on **SQLite, Postgres 16 and TLS-required Postgres 16**: the last way in is never removed, including two removals racing; a code works once, also with four redeemers at once; it expires to the second with an injected clock; the hourly limit and the voiding of the last code; the recovery code replacing the password in one step; the kinds not crossing; a failed audit write rolling the whole change back; only the hash stored and nothing secret in the audit log; the roles holding the permissions the docs say.
- A real russh client over SSH (`tests/keys.rs`, `tests/access_paths.rs`, `tests/telnet.rs`): link a second machine with a code, remove the old key, the notices told once, recover with a staff code, a cosysop refused for a sysop, an equal, themselves and an unknown handle, the five-an-hour limit, one sentence for every bad code, each permission taken away mid-session.
- **A real OpenSSH 9.6 client** (`ssh -tt`, rendered with `pyte` at 80x24) against the built server on SQLite, 32 checks, all passing: the whole of the above from a claimed board, including `ssh-keygen -lf` fingerprints in the list, a removed key refused at the next `ssh`, and the old password refused after a recovery.

**Not verified:** the Postgres backend under a real OpenSSH walk (the same code runs through the shared suite); a terminal emulator other than `pyte`; a screen reader; two server processes on one database.
