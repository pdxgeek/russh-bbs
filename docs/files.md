# File areas

File areas, quarantine, approval, tags, search and download lists, and the SFTP subsystem that moves the bytes (M4-F2, D141). The bytes live in the blob store ([D30, D106](decisions.md)); this document is about the rules above it. Code: `russh_bbs_core::files` (rules and the Files screens' state machine), the file store in `russh-bbs-store` (migration `0028`), `russh_bbs_server::sftp` (the subsystem and the ClamAV adapter).

**Privacy, honestly.** The board can read every file. The only files it cannot read are ones a member encrypted themselves (for example with `age` or `gpg`) before uploading. Nothing here is labelled private, and the Files screens say so in one line (`files.note.readable`). See [threat-model.md](threat-model.md).

## Model

- **Areas** are access resources (`ResourceKind::FileArea`). `member` holds `files.list`, `files.download`, `files.upload` and `files.tag` globally, so a global holding reaches every area; an area marked **restricted** is not reached that way. It needs the permission **scoped to that area** (an allow entry or a scoped role; `files::allow_member` makes the three entries a member needs) or `files.manage_areas` held globally. One rule, in `core::files::rules`, used by every screen, search and SFTP path.
- **Files** have a state: `quarantine` (just uploaded), `approved` (live) or `rejected` (refused; the bytes are deleted and the row is kept). A file also carries the scanner's verdict: `unscanned` (no scanner is configured), `clean`, `failed` (the scanner could not answer) or `infected`.
- **Names.** The name a client sends is kept only as data (`original_name`, never drawn, never a path). What members see is the display name, which went through the shared sanitizer, lost `/` and `\`, and is 1 to 120 characters. A name that sanitizes to nothing or is too long is refused, not cut.
- **Tags** are 1 to 24 characters of `a-z 0-9 _ -`, at most 8 per file. A bad tag is refused, not rewritten. **Descriptions** are one line, up to 480 characters, sanitized.
- **Search** takes plain word terms (the same parser as the forums, so a typed operator is just a word); every term must appear in the name, description or a tag, ignoring case. The areas the member may list are part of the query, so a file in another area is never read. (SQLite lower-cases ASCII only; Postgres lower-cases by locale.)

## Quarantine and approval

1. An upload is stored under the `quarantine` blob namespace, recorded as a `quarantine` file, and **scanned** (see below).
2. In quarantine a file is visible to its uploader and to anyone holding `files.approve` on the area, and to nobody else: not in a listing, by id, in a search, in a download list, or to download. Everything else answers "not found", the same as a file that does not exist.
3. `files.approve` (moderators and up) approves or rejects. **Approving** copies the bytes into the `files` namespace through the blob store (verifying the SHA-256 and size), marks the row `approved` in one transaction with its audit row, and deletes the quarantine copy. **Rejecting** marks the row and deletes the bytes. Two deciders at once cannot both win (the update is conditional on `quarantine`).
4. An area can be created **auto-approving**: a clean (or, with no scanner, unscanned) upload goes live at once. A scan that **failed** never auto-approves, and an **infected** file is rejected whatever the area says. Auto-approvals and scanner rejections are audited with the system as actor.
5. Staff may remove any visible file; an uploader may remove their own while it is in quarantine.

Audit actions (see [audit.md](audit.md)): `file_area.created`, `file.approved`, `file.rejected`, `file.removed`. Rows hold ids only, never a name.

## Scanning

`core::files::ScanHook` is an interface; the default (`NoScan`) scans nothing and says so (files are labelled "not scanned" on their screen). `[files] scan = "clamd"` turns on the ClamAV adapter (`server::sftp::ClamdScanner`): the stored bytes are streamed to `clamd` with the `INSTREAM` protocol over TCP (`clamd_address`, not encrypted: keep it on a private network) or a unix socket (`clamd_socket`). A reply that is not `stream: OK` or `... FOUND` (an error, a size-limit message, a dropped connection, a timeout of 60 s) is a failed scan. The adapter is tested against a fake `clamd` that speaks the protocol; a real ClamAV was not run.

## Limits

`upload_bytes_per_file` is checked on every write packet, so a transfer stops at the first byte past it and nothing is stored. `upload_bytes_per_day` is added to the day's usage as bytes arrive, in blocks of 1 MiB (smaller near the cap), and a day already used up is refused before the first byte. The count is **not refunded**: bytes sent in a transfer that failed, was refused or was rejected still count, and each transfer is rounded up to a block. Both numbers come from the role and the per-member override ([access.md](access.md#resource-limits)); `sysop` is unlimited. The store also caps an object at `blobs.max_object_bytes`.

## SFTP (ssh only)

`[files] sftp = true` (the default) serves the `sftp` subsystem to a **signed-in member** over ssh; a visitor, a key that is not a member's, a suspended account and a board with `sftp = false` get a refusal. It is the only subsystem served. There is one SFTP session per connection and at most 16 open handles. SFTP does not count as a "call" or as minutes against the session limits (only the upload limits apply).

The client sees a **virtual tree addressed by id**; no real path exists behind it:

```text
/                        one directory per area you may list, and downloads
/<id>-<slug>/            the area's approved files:  <id>-<display name>
/<id>-<slug>/pending/    your own uploads still in quarantine (approvers: all of them)
/downloads/              your download list
```

Only the leading id of a component is read (`3`, `3-docs` and `3-anything` are area 3), so a slug cannot select anything. The parser (`core::files::path`) refuses `..` (except in `realpath`, where it is resolved inside the tree and never above `/`), backslashes, NUL, any component the shared sanitizer would change (control characters, escapes, invisible format characters, private use), components over 255 bytes, paths over 1,024 bytes and trees deeper than three levels. It is fuzz-tested. There are no symlinks, no directories to make and nothing to rename.

- **Download:** open for reading. An approved file needs `files.download` on its area; a file in quarantine can be read by its uploader and by approvers (to review it). The stream verifies the stored hash at its end, so a damaged object fails the read. A resume starts the server-side read over and skips; it still works.
- **Upload:** open `/<area>/<name>` for writing (create). Writes must arrive in order (the OpenSSH client's do) and are streamed into `BlobStore::put` through a queue of four packets, so memory stays small. Closing the handle commits; a handle dropped without closing (a vanished client) stores nothing. The close reply carries the outcome: a rejection by the scanner fails it. An upload never replaces a file (writing to an existing file's listed name makes a new file with that name), and `setstat` is refused (attributes are never stored; a client that sets them on the handle it is writing is told OK).
- **Remove:** only an upload still in quarantine, from `pending/`.
- **Close waits for the scan.** The reply to closing an upload comes after the file is stored, recorded and scanned (up to 60 s with `clamd`); the session's other requests wait behind it.
- **Names that look like ids.** Reading, `stat` and removing take `<id>` or `<id>-<name>`; a file called `2026-notes.txt` is uploaded as that name (it becomes `<newid>-2026-notes.txt`) but fetched by its listed name.
- Errors never say whether a hidden thing exists: a missing file, a file in an area you cannot list, and a file in quarantine that is not yours all answer "no such file".
- Access is loaded again for every open, listing and stat, so a role or group change applies on the next action.

## The Files screens

`Files` on the main menu opens the areas (`files.list`). Members browse an area (nine files a page), open a file for its details (size, uploader, SHA-256, scan verdict, tags, description) and the **sftp path** to fetch it by, tag it (`files.tag`), search (`/`), and keep a **download list** (`b` on a file; `l` for the list; `sftp> get /downloads/*` fetches the lot). Uploaders and approvers change a description; approvers (`files.approve`) get **Waiting for approval** (`q`) with approve, reject and remove; `files.manage_areas` holders make areas (`n`: name, then `a` auto-approve, `r` restricted). All text is sanitized; the screens are drawn by the same code as the members and community screens.

Over **telnet** the screens browse and say "Downloads and uploads need ssh"; with SFTP turned off they say that instead.

## Implemented / Verified

- **Implemented:** everything above.
- **Verified:** the store suite on SQLite, Postgres 16 and TLS-required Postgres 16; core unit and property tests (path parser fuzz, text rules); OpenSSH `sftp` end to end on the local backend **and** MinIO with the same scenario (upload, quarantine invisible to others, staff approval, download as another member with a hash check, resume, the download list, auto-approve, per-file and per-day limits, traversal attempts, refused operations); the scan hook against a fake `clamd`; the Files screens through a real `ssh` client.
- **Not built / not verified:** a real ClamAV; overwriting or replacing a file; renaming; per-area quotas; archive inspection and type-by-content detection (the threat model asks for them; this slice stores and scans, it does not look inside); an orphan sweeper for quarantine blobs left by a crash between a put and its row (the blob namespace can be listed and compared with the `file` table); SFTP over telnet (never); the global new-scan (M4-F3).
