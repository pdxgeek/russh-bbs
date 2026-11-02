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
| Malicious member | Spam, impersonate, inject terminal escapes, abuse invites, cheat in doors, exfiltrate others' data |
| Malicious external content (feeds, mail, files) | Terminal injection, SSRF, malware, resource exhaustion |
| Curious or compromised operator or server | Read private content |
| Network attacker | Impersonate the server, downgrade, read or alter a telnet session on the wire, replay a telnet password |

## What the MVP guarantees

| Area | Guarantee |
|---|---|
| Transport | SSH by default. Host key is persistent and its fingerprint is published by the sysop. Telnet is off unless the sysop enables it (D27); a telnet session is plaintext and every screen says so with a fixed `UNENCRYPTED` marker. |
| Authentication | Public-key only over SSH. No passwords over SSH. Over telnet, login is off unless the sysop also enables it; then a per-member argon2id password that is set over SSH. SFTP, invite redemption, and sysop functions never work over telnet. |
| Waiting room | An unknown key sees only the banner, rules, request-invite, and redeem-invite, plus apply or create-account when the registration mode (D2) allows it. No member list, board list, or stats in any mode. |
| Registration | Default mode is `invite`. `apply` creates an inactive account visible only to the sysop queue. `open` creates an active account only within per-IP and per-key creation limits and an optional cap on new accounts per day; when the cap is hit the stranger sees the `apply` path instead. |
| Output safety | Untrusted text cannot inject terminal escapes or spoof UI icons or security badges. |
| Outbound fetching | User-supplied URLs cannot reach private networks. |
| Uploads | Files are quarantined and scanned before becoming visible. |
| Honesty | Nothing is labeled private unless the server genuinely cannot read it. |
| Container | The image runs as a non-root user on an unprivileged port with no added capabilities. The founder setup token is printed to stdout, so anyone who can read the container logs during first run can claim sysop; the docs say so. The container boundary is a second layer, not the door sandbox. |

## What the MVP does not guarantee

- Privacy from the operator (see above).
- Metadata privacy: the server knows who talks to whom and when.
- Anonymity: accounts are tied to keys and an invite tree.

## Rules for implementers

### Sanitization (one shared implementation in core)
Applied to every string that is not UI chrome: usernames, post bodies, subjects, chat lines, feed items, mail headers and bodies, file names and descriptions, door output, LLM or NPC text.
- Remove C0 and C1 control characters (except explicit newline and tab where allowed), ANSI/CSI/OSC/DCS sequences, and bidi override characters.
- Neutralize the Unicode **private-use ranges** used by Nerd Fonts, so content cannot draw lock icons or badges.
- Normalize Unicode for usernames and block confusables for names.
- Enforce length limits.
- Ship fuzz and property tests.

### Security chrome
Lock, signature, key-changed, and server-readable markers are drawn only by chrome code. Layouts and themes can position them but cannot hide, recolor to mislead, or replace them. Every icon tier has a text fallback ("E2EE", "SERVER-READABLE").

### SSRF guard
For every server-side fetch of a user-supplied URL: only `http` and `https`, resolve DNS and re-check the IP at connect time, block loopback, link-local, private, and metadata ranges, cap redirects, set connect and total timeouts, cap response size, and disallow credentials in URLs.

### Uploads
Quarantine until scanned (antivirus hook), detect type by content not extension, cap size and archive expansion depth and ratio, map the SFTP view to IDs (no path traversal), never render or execute uploaded content on the server.

### Invites, registration, and abuse
Rate-limit the waiting room per key and per IP, with a small cost or queue cap against floods. Expire denied requests quickly and keep minimal personal data. Invite codes are single-use, expiring, and bound to an inviter. Bans act on keys and affect the inviter's standing.

Registration mode (D2) adds: account creation in `apply` and `open` modes is rate-limited per IP and per key separately from waiting-room requests; `open` mode has a configurable daily cap on new accounts and degrades to `apply` when the cap is reached; usernames are reserved for a short window during registration and confusables are blocked so a stranger cannot squat or impersonate a member; accounts created in `open` mode have no inviter, so quotas that depend on inviter standing default to the most restrictive tier until the sysop vouches. Switching the mode never changes existing accounts.

### Telnet (when enabled)
Bind to a separate port and listener; never share the SSH accept loop. Negotiate only the options the TUI needs (NAWS, terminal type, echo, suppress go-ahead) with a well-tested crate, and treat everything else as untrusted input through the sanitizer. Rate-limit per IP like the waiting room. Passwords are argon2id with per-user salt, compared in constant time, never logged, set or changed only over SSH, and lockout after repeated failures. A member logged in over telnet sees the `UNENCRYPTED` marker on every screen and the composer shows "readable on the wire" next to "visible to the sysop". Sysop role is refused over telnet.

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

## Verified

Nothing yet. Each guarantee above needs a test; track them in [features.md](features.md).
