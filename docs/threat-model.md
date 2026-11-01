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
| Network attacker | Impersonate the server, downgrade |

## What the MVP guarantees

| Area | Guarantee |
|---|---|
| Transport | SSH only. No telnet. Host key is persistent and its fingerprint is published by the sysop. |
| Authentication | Public-key only. No passwords. |
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

Implementation status (`russh-bbs-core::sanitize`): **Implemented and unit/property tested:** control and escape-sequence removal, bidi and invisible-character removal, private-use neutralization (replaced with U+FFFD), length limits (in `char`s), NFKC and a strict ASCII allowed set for usernames, and a minimal ASCII lookalike skeleton (`0`/`o`, `1`/`i`/`l`, `-`/`_`). **Still planned:** a `cargo-fuzz` target; multi-letter lookalikes (`rn`/`m`); wiring into front ends, which is each consuming slice's job. Cross-script confusables are blocked by rejection (non-ASCII names are not allowed in the MVP), not by a confusables table.

### Security chrome
Lock, signature, key-changed, and server-readable markers are drawn only by chrome code. Layouts and themes can position them but cannot hide, recolor to mislead, or replace them. Every icon tier has a text fallback ("E2EE", "SERVER-READABLE").

### SSRF guard
For every server-side fetch of a user-supplied URL: only `http` and `https`, resolve DNS and re-check the IP at connect time, block loopback, link-local, private, and metadata ranges, cap redirects, set connect and total timeouts, cap response size, and disallow credentials in URLs.

### Uploads
Quarantine until scanned (antivirus hook), detect type by content not extension, cap size and archive expansion depth and ratio, map the SFTP view to IDs (no path traversal), never render or execute uploaded content on the server.

### Invites, registration, and abuse
Rate-limit the waiting room per key and per IP, with a small cost or queue cap against floods. Expire denied requests quickly and keep minimal personal data. Invite codes are single-use, expiring, and bound to an inviter. Bans act on keys and affect the inviter's standing.

Registration mode (D2) adds: account creation in `apply` and `open` modes is rate-limited per IP and per key separately from waiting-room requests; `open` mode has a configurable daily cap on new accounts and degrades to `apply` when the cap is reached; usernames are reserved for a short window during registration and confusables are blocked so a stranger cannot squat or impersonate a member; accounts created in `open` mode have no inviter, so quotas that depend on inviter standing default to the most restrictive tier until the sysop vouches. Switching the mode never changes existing accounts.

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
| Waiting room reveals nothing | **Partly (M1-A).** Every unknown key gets a restricted session with a fixed placeholder; tests assert it names no members, boards or stats. The real waiting room screens and the founder flow are M1-C and M1-D. |
| Restricted session lockdown | **Implemented and tested (M1-A).** Refused: exec, subsystem (incl. sftp), env, agent forwarding, X11, direct-tcpip, forwarded-tcpip, direct-streamlocal, tcpip-forward, streamlocal-forward, a shell without a PTY, and a second PTY or shell. At most 4 session channels. |
| Flood limits | **Partly (M1-A).** Auth deadline, auth-attempt cap, idle timeout and a global connection cap are implemented and tested. **Per-IP and per-key rate limits are planned (M1-C).** A single IP can still use every connection slot until then. |
| Logging | **Implemented (M1-A).** Logs carry fingerprints, peer address, request kinds, sizes. They never carry key material, commands, environment values or client input. Not audited beyond review of the code. |
| Output safety | **Trivially met in M1-A:** the only text drawn is fixed UI chrome; nothing from the client (not even the SSH username) is rendered. Sanitizer wiring starts when a screen shows user text. |
| Outbound fetching, uploads, honesty labels | Not started. |

## Verified

Only what the table above marks tested. Each remaining guarantee needs a test; track them in [features.md](features.md).
