# 7. Running it: backups, upgrades, troubleshooting

## Backups

> **Not built (M5): there is no backup command, no scheduled backup and no tested restore tool.** "Backups and tested restore, every backend" is Planned ([features.md](../../features.md)). What follows is the manual procedure that works today. It was run (SQLite) and it is only as good as your habit of testing a restore.

**What a board is made of:**

| Piece | Where | If you lose it |
|---|---|---|
| Database | `russh-bbs.db` (SQLite) or your Postgres database | Everything: members, posts, invites, door saves |
| `mail.key` | The data directory (`mail.key_file`) | **Every stored message is unreadable for good**, and the server **refuses to start** while mail exists. It never makes a replacement |
| `host_ed25519_key` | The data directory (`ssh.host_key`) | Members see the changed-host-key warning on their next connect |
| `blobs/` | The data directory, or your S3 bucket | Uploaded files (none exist yet) |
| `config.toml`, `menus.toml`, `pages/`, `themes/`, `art/`, `packs/` | The data directory / config path | Your look and settings |

**SQLite, stopped (the safe way).** Stop the server (`SIGTERM`; it checkpoints the log and exits 0), copy the directory, start again:

```bash
# systemd:   systemctl stop bbs
# container: docker stop bbs
tar czf bbs-backup.tgz -C ~/.local/share/russh-bbs .
# start it again
```

Container, with a throwaway helper: `docker run --rm -v bbs-data:/data -v "$PWD:/backup" alpine tar czf /backup/bbs-data.tgz -C /data .` ([deploy.md](../../deploy.md#backups), not walked here). Never copy `russh-bbs.db` from a **running** server without its `-wal` file.

**Restore, walked.** The archive was unpacked into an empty directory and a new server started from it with `XDG_DATA_HOME` pointing there. It logged the **same host key fingerprint**, `mail key ready`, and `founder already claimed; no setup token issued`; signing in as the founder showed the mail that had been in the old board, readable. That is the test to run after every backup: restore somewhere else, start it on another port, sign in, open a mail.

**Keep `mail.key` separately.** A copy of the database without the key reveals no mail; a copy of the database **with** the key reveals all of it. So: back the key up somewhere different from the database copies, store the archive like a secret (it holds the host key, the mail key and your member list), and a restored database needs the key that was current when its mail was written.

**Postgres.** Use Postgres's own tools (`pg_dump`, base backups) for the database and still copy the data directory for the host key, `mail.key` and `blobs/`. The server holds no state about the database that is not in it.

## Upgrades and migrations

- **Back up first.** Migrations run automatically at start and are **forward-only and versioned**; a landed migration is never edited. The Postgres first start applied all 26 and the SQLite database had its schema built the same way.
- **Do not run an older binary on a newer database.** There is no downgrade path. Roll back by restoring the backup you took.
- To upgrade: build or pull the new version, stop the old one (`docker stop`, `systemctl stop`), start the new one against the same data directory or volume, read the log. In a container: `docker compose pull && docker compose up -d`, then confirm `docker inspect` shows the new image. Pin a version tag in production; the image's `latest` follows the newest stable release.
- The host key and `mail.key` survive an upgrade because they are in the data directory. If you changed `XDG_DATA_HOME` or the user the service runs as, the server will not find them: it **makes a new host key** (new fingerprint), but it **refuses to start** if the database already holds mail and the key is missing. That refusal is the safety net, not a bug.
- **Not exercised:** an actual version-to-version upgrade (there was one binary). The migration runner and the append-only checks are covered by the repository's store tests on SQLite and Postgres.

## Troubleshooting

Read the log first (`journalctl`, `docker logs`). `RUST_LOG=debug` raises the level. Real messages, each produced by running the binary:

| You see | Cause and fix |
|---|---|
| `unknown key … in config file …`, `unknown environment variable RUSSH_BBS_…: no setting reads it` | A typo. Fix the name; the board will not start with it ([03-config.md](03-config.md#unknown-or-wrong-settings-stop-the-boot)) |
| `cannot listen on …: Address already in use` | Another copy, or another program, holds the port. Change `ssh.listen` or stop the other |
| `host key file … has mode 644; it must not be accessible to group or others` | `chmod 600` the file. The server never regenerates a key it cannot use |
| `loading the mail key … the database holds mail but the mail key file … is missing. A new key is never made for an existing board` | `mail.key` is not where the server looks. It follows `XDG_DATA_HOME` and `mail.key_file`, **not** `--db` or `--host-key`. Put the file back from your backup, or point `mail.key_file` at it |
| `Error: opening the postgres datasource (TLS is required)` … `UnknownIssuer` | The server's certificate is not signed by a CA the board trusts. Set `datasource.tls_root_cert` to the CA file ([install](01-install.md#postgres-with-tls-required)) |
| `needs telnet.enabled = true` | `telnet.allow_login` without `telnet.enabled` |
| A member says "Connection reset by peer" right after a burst | The per-address connection limit (`waiting_room.ip_burst`, 10, then one every 2 s). Wait a few seconds; do not retry in a tight loop. Behind NAT many members share one address: raise it |
| A member keeps getting "That code did not work" or the password prompt again | Wrong, expired, used or replaced codes and wrong passwords all give one identical refusal on purpose. Make a new code. Wrong passwords back off per handle (4 free, then 1 s doubling to 30 s) and per network (3 free, then 5 s up to 5 min) |
| A member lost both key and password | Sysop tools, Recovery code ([04-people.md](04-people.md#moderation-as-it-stands)) |
| Members' box lines show gaps | Their terminal draws line-drawing characters from the font. It is a terminal setting ([member guide](../member/02-terminal.md#box-lines-with-gaps-terminalapp)), or have them choose the Blocks border |
| The first connection after a restart asks about a changed host key | The host key was lost with the data directory. Restore it, or tell members the new fingerprint |
| A staff screen shows `⟦⊖⟧ telnet is on` | Low-security mode: `telnet.enabled` is on. Intended; turn telnet off to clear it |
| `menus.toml refused; the built-in menu stays reason=…` (in the log) | The file or a page it links is bad; the reason names it. The board runs on the built-in menu until you fix and reload |
| `RUSSH_BBS_PORT` and friends stop boot under Kubernetes | Service links; the documented names are ignored with a warning. Set `enableServiceLinks: false` ([config.md](../../config.md#kubernetes-service-links)) |

**Reading the log.** Start-up lines worth knowing: `host key ready … fingerprint=`, `database ready backend=…`, `mail key ready … key_id=1`, `ssh server listening … password_login=true render_cap=500x200 render_memory_ceiling_mib=585`, `scheduler started jobs=6`, and when relevant `board is unclaimed` or `founder already claimed`. The log never contains a password or the database URL's password (the Postgres run in this guide was grepped for it and it was absent). The one secret on stdout is the setup token before the claim: it is printed to the console, not through the logger, but a container runtime keeps stdout, so `docker logs` still shows it after the claim (it is void by then).

Still stuck: [deploy.md](../../deploy.md), [config.md](../../config.md), and [coordination.md](../../coordination.md) for known gaps.
