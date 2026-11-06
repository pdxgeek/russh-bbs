# Privacy: who can read what

This is the one place the guides say what the board can and cannot see. It follows [threat-model.md](../threat-model.md), [mail.md](../mail.md) and decisions D78, D90 and D93; if this page and those disagree, those win. The screens do not repeat it (D93): there is no "private" badge, no lock and no warning on a mail or chat screen, because nothing the server can read is called private.

## The short version

**The server, and so the person who runs it, can read nearly everything.** The exceptions:

1. **Mail you PGP-encrypt yourself before you send it.** The board stores the ciphertext it was given.
2. **Files you encrypt yourself** (for example with `age` or `gpg`) before uploading. File areas are **not built yet** (M4), so there is nothing to upload today.

Everything else (forum posts, chat lines, whispers, one-liners, the wall, your profile, who you invited, when you were last on) is readable by the server.

The board does no PGP for you. Encrypting a message is something you do on your own computer with your own tools (for example `gpg --armor --encrypt`) and then paste into the mail editor as text. Built-in PGP is planned (M6), and end-to-end encrypted messaging needs a native client that does not exist yet (D3 to D5). The board does not check that what you paste is encrypted.

## What each thing is, plainly

| Thing | What happens to it | Can the sysop read it through the board? |
|---|---|---|
| **Transport (ssh)** | Encrypted between you and the server. Host-key fingerprint is the thing to check on first connect | The board decrypts it, of course: ssh protects the wire, not the server |
| **Transport (telnet)** | **Not encrypted.** Everyone on the path can read it, your password included. Every telnet screen carries the fixed `UNENCRYPTED` marker | Same as ssh, and the network can too |
| **Forum posts, one-liners, wall, polls, profile** | Stored as text in the database | Yes. Staff can also remove them. Poll votes are stored with who cast them; members see counts only |
| **Chat lines and whispers** | Held in memory while you talk; **not stored**, not in the audit log, not in logs; gone on restart. A short backlog lives in memory only while the server runs | The running server sees them as they pass. There is no staff screen, export or log of them |
| **Internal mail** | **Encrypted at rest** with a key the server holds in `mail.key`. A copy of the database or a backup **without** that key does not reveal mail | **No staff reader exists** (D78, D90): no screen, permission, export or audit field shows a message. A recipient can *report* a message to staff, which sends staff a clear copy of that one message (the screen says so first) |
| **Who wrote to whom, and when** | Stored in ordinary columns | Someone with the database can see it, not the text |
| **Your password** | Stored only as an argon2id hash | No |
| **Your keys** | Only the public fingerprint is stored | No private key ever reaches the board |
| **Door saves** | Stored by the framework | Yes, in principle (it is game state); doors cannot read anything else |

## What "encrypted at rest" does and does not mean

Mail is sealed with a key the server holds. That protects a stolen disk or database copy. It does **not** hide mail from someone who controls the running server: the sysop is a person, the server is code, and a person who can change the code, read its memory, or take the key file *and* the database can read everything. The board offers no way for staff to do that from inside; it is just not impossible. (This is why an honest board never calls mail "private".)

## What you can do about it

- Treat posts, chat and whispers as readable by the sysop.
- For mail the sysop must not read, encrypt it yourself first and share keys another way.
- Prefer ssh to telnet. Never reuse a password you typed over telnet.
- Ask your sysop for the board's policy; this page is the software's contract, not the sysop's promise.

## For sysops

You can publish this page (or [threat-model.md](../threat-model.md)) as your board's policy. Whatever you set, the board refuses text that claims the board is private, secure or encrypted: in `[board]` and `[widgets]` text, menu and page labels and board names ([config.md](../config.md#board-ui-2)). Back up `mail.key` separately from the database ([sysop/07-operate.md](sysop/07-operate.md#backups)).
