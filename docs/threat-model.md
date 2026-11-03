# Threat model

Status: **Partly implemented.** Each row below says what is implemented and tested and what is still planned. This document is the contract for what the product may claim. Code must not promise more than this.

## The honest statement

In the MVP, **the server operator can read nearly everything**: posts, chat, internal mail, and files. What the server cannot read:
- Mail that a member has PGP-encrypted to another member before sending (the server stores ciphertext).
- Files a member encrypted locally (for example with `age` or `gpg`) before uploading.

SSH protects data in transit. Encryption at rest protects a stolen disk, not a curious operator.

True end-to-end encrypted private messaging needs code on the user's machine, so it arrives with the native client after the MVP (decisions D3 to D5).

## Assets

Member identities and key fingerprints, the invite tree, message and mail content, uploaded files, the credits ledger, server host keys, and the sysop account.

## Adversaries

| Adversary | Goal |
|---|---|
| Unauthenticated stranger | Learn who is on the board, get in without an invite, flood the waiting room, mass-create accounts when registration is `apply` or `open`, squat usernames |
| Malicious member | Spam, impersonate, inject terminal escapes, abuse invites, cheat in doors, exfiltrate others' data, reach areas or actions above their role |
| Malicious external content (feeds, mail, files) | Terminal injection, SSRF, malware, resource exhaustion |
| Curious or compromised operator or server | Read private content |
| Network attacker | Impersonate the server, downgrade, read or alter a telnet session on the wire, replay a telnet password |

## What the MVP guarantees

| Area | Guarantee |
|---|---|
| Transport | SSH by default. Host key is persistent and its fingerprint is published by the sysop. Telnet is off unless the sysop enables it (D27); a telnet session is plaintext and every screen says so with a fixed `UNENCRYPTED` marker. |
| Authentication | Public-key over SSH by default. Password login over SSH is a sysop opt-in (D28, D81), encrypted on the wire, with per-handle and per-network attempt limits and one refusal for every failure. Over telnet, login is off unless the sysop also enables it; then a per-member argon2id password that is set over SSH. SFTP, invite redemption, and sysop functions never work over telnet. |
| Waiting room | An unknown key sees only the banner, rules, request-invite, and redeem-invite, plus apply or create-account when the registration mode (D2) allows it. No member list, board list, or stats in any mode. |
| Registration | Default mode is `invite`. `apply` creates an inactive account visible only to the sysop queue. `open` creates an active account only within per-IP and per-key creation limits and an optional cap on new accounts per day; when the cap is hit the stranger sees the `apply` path instead. |
| Output safety | Untrusted text cannot inject terminal escapes or emit the exact reserved marker codepoints or styling. It can still imitate a security badge with lookalike characters or, in the ASCII tier, the same text; the reserved style and a chrome-only position are what tell the real one apart (see Security chrome, D39, D45). |
| Outbound fetching | User-supplied URLs cannot reach private networks. |
| Uploads | Files are quarantined and scanned before becoming visible. |
| Honesty | Nothing is labeled private unless the server genuinely cannot read it. |
| Access control | Every read, post, upload, moderation and admin action is a named permission checked in core against the account's roles, the resource's own policy (owner, role grants, allow list) and the role's resource limits (D33); an area a role cannot see is not listed and not reachable by name; role changes are audited. |
| Container | The image runs as a non-root user on an unprivileged port with no added capabilities. The founder setup token is printed to stdout, so anyone who can read the container logs during first run can claim sysop; the docs say so. The container boundary is a second layer, not the door sandbox. |

## What the MVP does not guarantee

- Privacy from the operator (see above).
- Metadata privacy: the server knows who talks to whom and when.
- Anonymity: accounts are tied to keys and an invite tree.

## Rules for implementers

### Sanitization (one shared implementation in core)
Applied to every string that is not UI chrome: usernames, post bodies, subjects, chat lines, feed items, mail headers and bodies, file names and descriptions, door output, LLM or NPC text.
- Remove C0 and C1 control characters (except explicit newline and tab where allowed), ANSI/CSI/OSC/DCS sequences, and bidi override characters.
- Neutralize the Unicode **private-use ranges** used by Nerd Fonts and the **reserved chrome set** (U+27E6/U+27E7 and the four Unicode marker glyphs), so content cannot draw the Nerd Font or delimited Unicode-tier markers.
- Remove invisible filler and format characters (Hangul and Khmer fillers, braille blank, soft hyphen, U+2060-206F, variation selectors except U+FE0F after an emoji base), map Unicode spaces to ASCII space, treat VT, FF, NEL, LS and PS as line separators, and cap combining marks at 4 per base character (D40).
- Normalize Unicode for usernames and block confusables for names.
- Enforce length limits.
- Ship fuzz and property tests.

Implementation status (`russh-bbs-core::sanitize`): **Implemented and unit/property tested:** control and escape-sequence removal, bidi and invisible-character removal by Unicode property (`Cf` and Default_Ignorable, D45), ZWJ/ZWNJ kept only in a joining context, Unicode-space mapping, combining-mark cap (not reset by joiners), private-use and reserved-chrome neutralization (replaced with U+FFFD), length limits (in `char`s), NFKC and a strict ASCII allowed set for usernames, reserved usernames compared on the skeleton, and an ASCII lookalike skeleton with digit and multi-letter folds (best effort, errs toward collisions). **Still planned:** a `cargo-fuzz` target; wiring into front ends, which is each consuming slice's job. Cross-script confusables are blocked by rejection (non-ASCII names are not allowed in the MVP), not by a confusables table.

### Security chrome
Lock, signature, key-changed, and server-readable markers are drawn only by chrome code. Layouts and themes can position them but cannot hide, recolor to mislead, or replace them. Every icon tier has a text fallback ("E2EE", "SERVER-READABLE"). What is guaranteed, per tier (D39):

| Tier | What content cannot produce | Tested |
|---|---|---|
| All | The reserved style (bold + reverse video): the sanitizer removes every escape sequence | Property test: no sanitized string contains ESC or the style bytes |
| Nerd Font | The marker glyph (private-use, replaced by the sanitizer) | Property test |
| Unicode | The **exact codepoints** of the delimiters U+27E6/U+27E7 and the four marker glyphs (replaced by the sanitizer). **Not** lookalikes: `〚☉〛`, `⸨◉⸩`, fullwidth or ASCII brackets and similar-looking glyphs pass through and may render like a marker in some fonts. Like the ASCII tier, the Unicode tier therefore depends on the reserved style and a chrome-only position in the end (QA M0 N1, D45) | Property test over the exact set; the lookalike limit is pinned by a test |
| ASCII | **Nothing textual.** Content can type `E2EE` or `SERVER-READABLE`; only the reserved style and a chrome-only screen position tell the real marker apart | A test pins this limit |

On a terminal with no color or styling, **position is the only cue** for the ASCII tier and, because of lookalikes, in practice for the Unicode tier too, so markers must be drawn in a chrome-only place (status line or fixed gutter) that content is never drawn into. Usernames equal to a marker label, a staff word or a default role or group name are rejected. Not yet exercised end to end: no front end draws markers, so the renderer applying the style and position is an M2 screen-framework task.

### SSRF guard
For every server-side fetch of a user-supplied URL: only `http` and `https`, resolve DNS and re-check the IP at connect time, block loopback, link-local, private, and metadata ranges, cap redirects, set connect and total timeouts, cap response size, and disallow credentials in URLs.

### Uploads
Quarantine until scanned (antivirus hook), detect type by content not extension, cap size and archive expansion depth and ratio, map the SFTP view to IDs (no path traversal), never render or execute uploaded content on the server. Blobs are reached only through the `BlobStore` interface (D30); object-store credentials come from the environment, are never logged, and the bucket policy denies public reads. Quarantined objects are under a separate prefix and are never served.

### Invites, registration, and abuse
Rate-limit the waiting room per key and per IP, with a small cost or queue cap against floods. Expire denied requests quickly and keep minimal personal data. (Implemented, M2-E, D71 and D72, [queue.md](queue.md): a request keeps only the handle, a one-line message, the key's public fingerprint and two times, and **no address**; one request per key; a sysop-set cap, default 200; M1-C's limiter per key and per network; requests expire after 14 days; a denied key leaves only a hash and an expiry for 24 hours; the answer a stranger reads is identical whatever the queue did; drops are logged without the text.) Invite codes are single-use, expiring (7 days), and bound to an inviter (implemented, M2-D, D67: 160-bit codes, only a hash stored, one refusal for every failure, redemption in one transaction, a monthly quota from the `invites_per_month` limit; the redeem form shares the waiting room's per-session submission cap and the per-key and per-network connection limits). Bans act on keys and affect the inviter's standing.

Registration mode (D2) adds (implemented, M2-G, D73, D74, [registration.md](registration.md)): account creation in `open` mode is rate-limited per IP and per key by its own gate (per key 2 then 1 an hour, per network 5 then 1 every 10 minutes), and an `apply` application is a queue request under the queue's limits and caps (nothing new to flood: one per key, a sysop-set cap, 14-day expiry); the form's answer never depends on who is on the board, the queue or the day's count, and the only thing a stranger can read about the cap is the fixed notice that a full day shows; `open` mode has a configurable daily cap on new accounts (stored setting `open_daily_cap`, default 25 per UTC day, `0` = none, a bad value reads as the default) and degrades to `apply` for the rest of the UTC day when the cap is reached (one `registration.capped` audit row per day, so the log cannot be grown by strangers); usernames are reserved for a short window during registration and confusables are blocked so a stranger cannot squat or impersonate a member; accounts created in `open` mode have no inviter, so quotas that depend on inviter standing default to the most restrictive tier until the sysop vouches. Switching the mode never changes existing accounts.

### Telnet (when enabled: low-security mode)
Enabling telnet is called low-security mode everywhere a sysop sees it, and a marker on staff screens says it is on (built in M3-H: `LOW-SECURITY` beside the usual marker; the start of the listener also logs a loud warning). Bind to a separate port and listener; never share the SSH accept loop (built: its own accept loop, throttle and connection count; it shares only the session registry so the session limit spans both). Negotiate only the options the TUI needs: built as echo, suppress go-ahead and NAWS; terminal type is not requested, and every other option is refused once. This is a small hand-written decoder with a fixed memory bound, not a crate (D85 (8)): everything the client sends is untrusted, only data bytes and a window size come out of it, and no protocol byte can reach the key parser. Rate-limit per IP like the waiting room (built: the same per-network connection rate, plus per-handle and per-network password attempt limits and the SSH per-session request and input budgets). Passwords are argon2id with per-user salt, compared in constant time, never logged, set or changed only over SSH, and lockout after repeated failures. A member logged in over telnet sees the `UNENCRYPTED` marker on every screen, on the transport's own last row, which no layout, theme or content can draw into. The composer's "readable on the wire" note beside "visible to the sysop" arrives with the composer (M3-E): it must read `Limits`/the transport, not assume SSH. Roles of rank 40 and up (`cosysop`, `sysop`) are refused over telnet by dropping their grants from every snapshot the session loads. There is no waiting room, founder claim, invite redemption or registration over telnet, because each would put a key, a code or a setup token on the wire.

### Keys and secrets
Never log keys, invite codes, message bodies, or mail. Zero key material in memory when finished. No credentials in the repo, fixtures, or the database in plaintext.

### Doors
First-party Rust doors only until the WASM sandbox exists. No door gets filesystem or network access. Game results are computed server-side; never trust client-reported state.

### Credits
Non-transferable and non-redeemable. Ledger changes are append-only and auditable.

## Planned (post-MVP) E2EE design notes

- Native client opens an SSH subsystem channel. Keys stay on the client.
- DMs: X3DH-style setup and double ratchet. Groups: MLS. Mail: OpenPGP (`sequoia-openpgp`).
- Invite signatures vouch for members' keys; key changes warn loudly.
- Metadata mitigations (padding, sealed sender) are hardening work, not v1.
- Moderation in private spaces uses member reports with franking, not server reading.

## Implementation status of the guarantees

| Guarantee | Status |
|---|---|
| Transport: SSH only, persistent host key, fingerprint published | **Implemented and tested (M1-A).** Ed25519 key, 0600, reused across restarts (test proves the fingerprint is stable and equals what a client sees on the wire); fingerprint logged at startup. Publishing it is the sysop's job. |
| Authentication: public-key only by default | **Implemented and tested (M1-A, M3-D).** `none` and keyboard-interactive are always rejected; by default only `publickey` is advertised and the `password` method is refused without a hash. With the sysop's `ssh.allow_password_login` (off by default, D28) `password` is advertised too: argon2id (19 MiB, 2 iterations, 1 lane, parameters in the stored string, upgraded on login), one verification per attempt on every path including an unknown name, a name with no password and a non-active account, the same refusal for all of them, per-handle (burst 5, then 1 per 30 s) and per-network (burst 10, then 1 per 6 s) limits, nothing the client typed logged. A password is set or cleared only from a signed-in session (a password session gives the current one again), audited without a secret. A handle's allowance can be spent by anyone who knows it to block that member's password login for a while (key login is unaffected). RSA client keys are unsupported (D21). See D81. |
| Waiting room reveals nothing | **Implemented and tested (M1-C, extended by M1-D to the founder claim entry).** The room is built from the registration mode and the sysop's banner and rules only; it never sees the store's members, so it cannot name one, and form replies depend only on the typed text (a member's name and a stranger's get the same reply). Tests drive every page of every mode and assert no seeded member name, board name, setting name or stat word appears, over the rendering layer and over a real SSH client. A member's key sees the same room in M1. The claim entry `ssh claim@host` shows the same prompt and the same failure on a claimed and an unclaimed board (asserted word for word), and the ordinary waiting room is identical before and after the claim (D41). Not verified: scrolling (a very long rules text is clipped on a small window), non-ASCII typing (the key parser ignores it), and timing side channels between member and non-member authentication. |
| Restricted session lockdown | **Implemented and tested (M1-A).** Refused: exec, subsystem (incl. sftp), env, agent forwarding, X11, direct-tcpip, forwarded-tcpip, direct-streamlocal, tcpip-forward, streamlocal-forward, a shell without a PTY, and a second PTY or shell. At most 4 session channels. |
| Flood limits | **Implemented and tested (M1-A, M1-C, FIX-M1-1, FIX-M1-3).** *Before login:* auth deadline, auth-attempt cap, idle timeout and a global connection cap (M1-A); per-network connection limit at accept (burst 10, 1 per 2 s, IPv6 per /64), per-key authentication limit before the membership lookup (burst 5, 1 per 10 s), and 5 form submissions per session (M1-C, D37). *After login, per session cost (FIX-M1-3, D52; FIX-M1-4, D53):* a session draws into at most `ssh.render_cols` x `ssh.render_rows` cells (centered; the rest of a bigger window stays blank) whatever window it asks for, so its render buffers are bounded by the sysop's cap (two buffers of 48-byte cells: 9.6 MB at the default 500x200, which draws the whole clamped window) and the total is bounded by that times `ssh.max_connections` (default 64, about 600 MB worst case; logged at start as `render_memory_ceiling_mib`). A window change that changes the drawn rectangle resizes the buffers in place; one that does not only repaints. The D52 numbers below were measured with the old fixed 100x40 cap and have not been re-measured at the new default; the bound is now arithmetic, not a measurement. Measured on a 4-core host, release build, 240 sessions each asking for 500x200: peak RSS **114 MB idle and 138-150 MB while every session sends 20 window changes a second** (11 MB empty), against 2,223-2,771 MB before; CPU for 30 and 240 sessions at 20 window changes a second 0.4 and 1.7 cores against 3.5 and 2.8, and a concurrent fresh login 0.10 s and 0.48 s (max 0.62 s) against 0.13 s and 3.4 s (max 4.6 s). Not measured: the container under a memory limit (no Docker daemon in the build environment); a window change that does not change the drawn rectangle repaints in place and allocates nothing. *After login, per session rate (FIX-M1-1, D49):* window changes and keystrokes are folded into one redraw per cycle and a session redraws at most 20 times a second (`ssh.redraw_interval_millis`); a session may send 200 requests and 8 KiB of input a second with a two-second burst, excess data is dropped, and 1000 over-budget events disconnect it; output goes through a bounded queue and a peer that stops reading for 10 s is cut off at the socket instead of being queued for. All limits are configurable, and the limiter tables are bounded and evict. Tested with a fake clock (burst, refill, strikes, a typist who never earns a strike), with real russh clients (`tests/ssh_flood.rs`: a coalesced burst draws at most the redraw ceiling and the last size is drawn; a window-change flood and a keystroke flood each end the session; a second login is served while one session floods; a client with a one-byte window is dropped with under 128 KiB queued) and with OpenSSH. Measured on a 4-core test host, release build, before and after: a resize flood from one session cost 1.0 core and from four sessions 3.0 cores; a 10 MB keystroke flood into a form cost 5.2 CPU-seconds; a client that never reads grew the server from 11 to 106 MB. After, those sessions are disconnected within about half a second at a few hundredths of a CPU-second in total. With the strike limit turned off so a flood stays connected, four resize floods cost 0.12 cores, 10 MB of keystrokes 0.06 CPU-seconds, and the never-reading client left memory flat (12 to 14 MB). The numbers are in the FIX-M1-1 pull request. **Not covered:** the receive cost of a request (decrypt and parse in russh) cannot be budgeted away, only cut short by the disconnect (about 3.5 microseconds each in the strike-free run); many sessions at budget add up: the total is bounded by the connection cap times the per-session render memory and request budget, not by anything per process, and CPU for `max_connections` sessions each at budget is not capped (it is measured, not enforced); memory the allocator keeps after a flood was not re-measured (QA L10); limits are per process and in memory (reset on restart, not shared across instances); shared NAT addresses share an allowance, and so does the founder claim from the same address (a flood from it is dropped at accept; the operator's workaround is in `docs/deploy.md`, D65); invite-request and account-creation rate limits arrive with those flows in M2. |
| Logging | **Implemented (M1-A).** Logs carry fingerprints, peer address, request kinds, sizes. They never carry key material, commands, environment values or client input. Not audited beyond review of the code. |
| Audit log | **Implemented (M2-C, [audit.md](audit.md)).** Role-relevant writes (founder claim, account creation, group and grant changes, terminal-setting saves) append a row in the same transaction. The database refuses `UPDATE`, `DELETE` and `TRUNCATE` on it. Rows hold ids, closed action names, public fingerprints and a sanitized 120-character note; no token, key or password has a field. It does not stop the host's owner editing the database file (D59). No retention yet (M5). |
| Chat | **Implemented and tested (M3-F, D84, D78).** Lines and whispers are never stored, audited or logged (a test reads the database file and the log output after a chat with canary text); rooms are access resources and a miss answers the same for a denied, closed or absent room; a whisper to anyone not in the sender's room is "Not here."; the waiting room holds no chat permission. Every line is bounded (length, send rate, scrollback, per-session queue that drops its oldest event, rooms and members), so one slow or noisy client cannot grow memory or block a room. |
| Output safety | **Implemented and tested (M1-C).** Banner, rules and typed form text pass through the core sanitizer before they are stored in the room (a banner with ESC and Nerd Font characters renders neutralized, tested); everything else is fixed chrome in `waiting_room::strings`. The SSH username is never rendered. |
| Invites | **Implemented and tested (M2-D, D67).** Codes are 160 random bits and stored only as a SHA-256 hash; a code works once, for 7 days, while its inviter is active; every failed redemption (unknown, used, expired, taken handle, key already bound) shows the same sentence and writes nothing; redemption is atomic under concurrency on SQLite and Postgres; the audit log names an invite by id only. Not built: revoking an invite, per-IP limits specific to redemption. |
| Outbound fetching, uploads, honesty labels | Not started. |

## Verified

Only what the table above marks tested. Each remaining guarantee needs a test; track them in [features.md](features.md).

## Founder claim (M1-D, D41)

| Risk | Control | Status |
|---|---|---|
| Guessing the setup token | 160 random bits from the OS CSPRNG; 3 attempts per session, per-key (3, then 1 per 30 s) and per-network (10, then 1 per 10 s) limits applied before the token is read | Implemented, tested |
| Token stolen from storage | Only a SHA-256 hash, in memory; nothing token-related in the database, files or logs (tests scan the DB and the log output) | Implemented, tested |
| Token read from the console or `docker logs` | The one deliberate print; accepted and documented: anyone who can read the first-run console can claim the board. A restart before the claim voids the old token | Accepted |
| Two claimants at once | One transaction compare-and-set on `founder_claimed`; raced in the store and over SSH | Implemented, tested |
| Token as an oracle (claimed vs wrong) | Constant-time comparison that takes the same steps when closed; one failure message; identical prompt in both states | Implemented, tested. Not verified: network-level timing |
| An unclaimed board advertising itself | The claim prompt is selected by the SSH user name, is always available and identical; the waiting room has no claim wording | Implemented, tested |
