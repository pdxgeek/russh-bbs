# Internal mail (stage 1)

Member-to-member mail inside the board (M3-I, [D90](decisions.md), builds [D78](decisions.md)). Status: implemented and tested; no external delivery (that is M6); **not end-to-end encrypted** (that waits for the native client, D4).

## What a member can do

Main menu, Mail (`mail.read`): the inbox, newest first, 12 to a page, unread marked with `*`. Enter or a digit opens a message in the pager. `c` writes (handle, subject, then the M3-B editor); `r` and `c` in the reader reply, the second with the message quoted; `d` deletes (asks first); `x` reports to staff (says what is copied, asks first). A message that cannot be opened (damaged, moved, sealed under a key the board does not hold) is an entry that says so; it never takes the screen down and `d` deletes it.

Limits: `mail_per_day` per UTC day (a refusal says when it resets); subject 1-120 characters; body up to 8,000 characters and 200 lines (refused, not cut); a mailbox holds at most **200** messages, and a new message to a full mailbox is refused and the sender is told (nothing is dropped, nothing expires). There is no sent folder.

## What is stored

- `mail_message`: `id` (random, chosen by core), `recipient_id`, `sender_id`, `sent_at`, `read_at`, `key_id`, and **sealed** `subject` and `body` (XChaCha20-Poly1305; random nonce per value; the associated data names the field, the message id, the recipient and the key id). Nothing readable is in the row.
- `mail_report`: what a recipient chose to send staff, in clear, as a copy.
- The key is `mail.key` beside the host key (`mail.key_file`). **Back it up separately; see [deploy.md](deploy.md#backups).** Boot refuses a missing, damaged or wrong key while mail exists and never makes a new one.

## What the server and the sysop can and cannot see

Said once, in the welcome (login) message and the help screen, and nowhere else (D78 (7)): "No one, the sysop included, can read your mail through the board. Mail is encrypted at rest. It is not end-to-end encrypted."

What that means in practice:

- No screen, permission, export or audit field shows another member's subject or text. `mail.moderate` lists and closes **reports** only.
- A copy of the database or a backup **without** the key file does not reveal mail. Someone with the key file **and** the database, or the ability to change the server's code or read its memory, can read everything.
- **Not hidden:** who wrote to whom and when, whether it was opened, and each message's size (the text length plus 40 bytes) are plain columns. Unsent **drafts** are stored readable (D80) until sent or discarded. A report is a clear copy by design. Over **telnet** (D27) mail crosses the network unencrypted (the `UNENCRYPTED` marker shows).
- The log records only reports and their closing, by id. Sending, reading and deleting are not audited.

## Addressing

By handle, ignoring case. An unknown handle and a member who is not active get one answer. A reply goes to the sender's **account**, so a handle reused later cannot receive it.

## Where the code is

`russh-bbs-core::mail` (rules, sealing, key format), `russh-bbs-store` (`mail_message` and `mail_report`, migration `0019_mail`, both backends), `russh-bbs-server::mail` (screens, key file, boot check). Tests: core unit tests, the shared store suite (SQLite, Postgres, TLS Postgres), `tests/mail.rs` over a real client, the path test in `tests/access_paths.rs`.

## Not built

Key rotation (how it would work is in D90), sealing drafts, a staff screen for reports (M5), a sent folder, expiry, attachments, notifications of new mail, external mail and PGP (M6), end-to-end encryption with the native client (D4).
