# Threat model

Status: **Planned.** This document is the contract for what the product may claim. Code must not promise more than this.

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
| Authentication | Public-key over SSH by default. Password login over SSH is a sysop opt-in (D28), encrypted on the wire, with lockout. Over telnet, login is off unless the sysop also enables it; then a per-member argon2id password that is set over SSH. SFTP, invite redemption, and sysop functions never work over telnet. |
| Waiting room | An unknown key sees only the banner, rules, request-invite, and redeem-invite, plus apply or create-account when the registration mode (D2) allows it. No member list, board list, or stats in any mode. |
| Registration | Default mode is `invite`. `apply` creates an inactive account visible only to the sysop queue. `open` creates an active account only within per-IP and per-key creation limits and an optional cap on new accounts per day; when the cap is hit the stranger sees the `apply` path instead. |
| Output safety | Untrusted text cannot inject terminal escapes or emit the exact reserved marker codepoints or styling. It can still imitate a security badge with lookalike characters or, in the ASCII tier, the same text; the reserved style and a chrome-only position are what tell the real one apart (see Security chrome, D39, D44). |
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

Implementation status (`russh-bbs-core::sanitize`): **Implemented and unit/property tested:** control and escape-sequence removal, bidi and invisible-character removal by Unicode property (`Cf` and Default_Ignorable, D44), ZWJ/ZWNJ kept only in a joining context, Unicode-space mapping, combining-mark cap (not reset by joiners), private-use and reserved-chrome neutralization (replaced with U+FFFD), length limits (in `char`s), NFKC and a strict ASCII allowed set for usernames, reserved usernames compared on the skeleton, and an ASCII lookalike skeleton with digit and multi-letter folds (best effort, errs toward collisions). **Still planned:** a `cargo-fuzz` target; wiring into front ends, which is each consuming slice's job. Cross-script confusables are blocked by rejection (non-ASCII names are not allowed in the MVP), not by a confusables table.

### Security chrome
Lock, signature, key-changed, and server-readable markers are drawn only by chrome code. Layouts and themes can position them but cannot hide, recolor to mislead, or replace them. Every icon tier has a text fallback ("E2EE", "SERVER-READABLE"). What is guaranteed, per tier (D39):

| Tier | What content cannot produce | Tested |
|---|---|---|
| All | The reserved style (bold + reverse video): the sanitizer removes every escape sequence | Property test: no sanitized string contains ESC or the style bytes |
| Nerd Font | The marker glyph (private-use, replaced by the sanitizer) | Property test |
| Unicode | The **exact codepoints** of the delimiters U+27E6/U+27E7 and the four marker glyphs (replaced by the sanitizer). **Not** lookalikes: `〚☉〛`, `⸨◉⸩`, fullwidth or ASCII brackets and similar-looking glyphs pass through and may render like a marker in some fonts. Like the ASCII tier, the Unicode tier therefore depends on the reserved style and a chrome-only position in the end (QA M0 N1, D44) | Property test over the exact set; the lookalike limit is pinned by a test |
| ASCII | **Nothing textual.** Content can type `E2EE` or `SERVER-READABLE`; only the reserved style and a chrome-only screen position tell the real marker apart | A test pins this limit |

On a terminal with no color or styling, **position is the only cue** for the ASCII tier and, because of lookalikes, in practice for the Unicode tier too, so markers must be drawn in a chrome-only place (status line or fixed gutter) that content is never drawn into. Usernames equal to a marker label, a staff word or a default role or group name are rejected. Not yet exercised end to end: no front end draws markers, so the renderer applying the style and position is an M2 screen-framework task.

### SSRF guard
For every server-side fetch of a user-supplied URL: only `http` and `https`, resolve DNS and re-check the IP at connect time, block loopback, link-local, private, and metadata ranges, cap redirects, set connect and total timeouts, cap response size, and disallow credentials in URLs.

### Uploads
Quarantine until scanned (antivirus hook), detect type by content not extension, cap size and archive expansion depth and ratio, map the SFTP view to IDs (no path traversal), never render or execute uploaded content on the server. Blobs are reached only through the `BlobStore` interface (D30); object-store credentials come from the environment, are never logged, and the bucket policy denies public reads. Quarantined objects are under a separate prefix and are never served.

### Invites, registration, and abuse
Rate-limit the waiting room per key and per IP, with a small cost or queue cap against floods. Expire denied requests quickly and keep minimal personal data. Invite codes are single-use, expiring, and bound to an inviter. Bans act on keys and affect the inviter's standing.

Registration mode (D2) adds: account creation in `apply` and `open` modes is rate-limited per IP and per key separately from waiting-room requests; `open` mode has a configurable daily cap on new accounts and degrades to `apply` when the cap is reached; usernames are reserved for a short window during registration and confusables are blocked so a stranger cannot squat or impersonate a member; accounts created in `open` mode have no inviter, so quotas that depend on inviter standing default to the most restrictive tier until the sysop vouches. Switching the mode never changes existing accounts.

### Telnet (when enabled: low-security mode)
Enabling telnet is called low-security mode everywhere a sysop sees it, and a persistent marker on admin screens says it is on. Bind to a separate port and listener; never share the SSH accept loop. Negotiate only the options the TUI needs (NAWS, terminal type, echo, suppress go-ahead) with a well-tested crate, and treat everything else as untrusted input through the sanitizer. Rate-limit per IP like the waiting room. Passwords are argon2id with per-user salt, compared in constant time, never logged, set or changed only over SSH, and lockout after repeated failures. A member logged in over telnet sees the `UNENCRYPTED` marker on every screen and the composer shows "readable on the wire" next to "visible to the sysop". Sysop role is refused over telnet.

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
| Authentication: public-key only | **Implemented and tested (M1-A).** `none`, password and keyboard-interactive are rejected; only `publickey` is advertised. RSA client keys are unsupported (D21). |
| Waiting room reveals nothing | **Implemented and tested (M1-C, extended by M1-D to the founder claim entry).** The room is built from the registration mode and the sysop's banner and rules only; it never sees the store's members, so it cannot name one, and form replies depend only on the typed text (a member's name and a stranger's get the same reply). Tests drive every page of every mode and assert no seeded member name, board name, setting name or stat word appears, over the rendering layer and over a real SSH client. A member's key sees the same room in M1. The claim entry `ssh claim@host` shows the same prompt and the same failure on a claimed and an unclaimed board (asserted word for word), and the ordinary waiting room is identical before and after the claim (D41). Not verified: scrolling (a very long rules text is clipped on a small window), non-ASCII typing (the key parser ignores it), and timing side channels between member and non-member authentication. |
| Restricted session lockdown | **Implemented and tested (M1-A).** Refused: exec, subsystem (incl. sftp), env, agent forwarding, X11, direct-tcpip, forwarded-tcpip, direct-streamlocal, tcpip-forward, streamlocal-forward, a shell without a PTY, and a second PTY or shell. At most 4 session channels. |
| Flood limits | **Implemented and tested (M1-A, M1-C).** Auth deadline, auth-attempt cap, idle timeout and a global connection cap (M1-A). Per-network connection limit at accept (burst 10, 1 per 2 s, IPv6 per /64), per-key authentication limit before the membership lookup (burst 5, 1 per 10 s), and 5 form submissions per session (M1-C, D37), all configurable; the limiter tables are bounded and evict. Tested with a fake clock (burst, refill, flood of distinct sources) and with a real client and OpenSSH. **Not covered:** limits are per process and in memory (reset on restart, not shared across instances); shared NAT addresses share an allowance; invite-request and account-creation rate limits arrive with those flows in M2. |
| Logging | **Implemented (M1-A).** Logs carry fingerprints, peer address, request kinds, sizes. They never carry key material, commands, environment values or client input. Not audited beyond review of the code. |
| Output safety | **Implemented and tested (M1-C).** Banner, rules and typed form text pass through the core sanitizer before they are stored in the room (a banner with ESC and Nerd Font characters renders neutralized, tested); everything else is fixed chrome in `waiting_room::strings`. The SSH username is never rendered. |
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
