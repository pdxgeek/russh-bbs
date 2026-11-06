# 1. Install

You need one thing: the `russh-bbs-server` program, and somewhere for it to keep its data. There are two ways to get it. Neither needs a database server (SQLite is built in).

> **Two things to know first.** There are **no published binaries**, and the **container image is not published yet** (the owner has not cut the first release tag, [deploy.md](../../deploy.md#owner-action-release)). Today you build from source, or build the image yourself. Pin whatever you run.

## The binary (build from source)

```bash
git clone https://github.com/pdxgeek/russh-bbs && cd russh-bbs
cargo build --release -p russh-bbs-server
# the program is target/release/russh-bbs-server (about 50 MB; it embeds all six doors)
target/release/russh-bbs-server --help
```

`rust-toolchain.toml` pins the toolchain; `rustup` fetches it on the first build. A first release build took about four minutes on a small virtual machine. Run it:

```bash
target/release/russh-bbs-server --listen 0.0.0.0:2222
```

With no config at all it listens on `0.0.0.0:2222`, uses SQLite, and keeps everything in **one data directory**: `$XDG_DATA_HOME/russh-bbs/`, or `~/.local/share/russh-bbs/` when `XDG_DATA_HOME` is not set. On a clean start it creates there, with mode 0600 or 0700:

| File | What |
|---|---|
| `host_ed25519_key` | The server's SSH host key. Its fingerprint is logged at start and is what members check on first connect |
| `russh-bbs.db` (+ `-wal`, `-shm` while running) | The SQLite database |
| `mail.key` | The key that seals stored mail. **Back it up** ([07-operate.md](07-operate.md#backups)) |
| `blobs/` | Uploaded bytes (empty; nothing writes to it until file areas land) |

You add `config.toml`, `menus.toml`, `pages/`, `themes/`, `art/` and `packs/` yourself, in the same place ([03-config.md](03-config.md)).

> **Gotcha, checked by running it.** `--db` and `--host-key` move *their* file only. `mail.key` stays in the default data directory unless you set `mail.key_file` (or `XDG_DATA_HOME`). Starting with `--host-key /srv/bbs/host_key --db /srv/bbs/bbs.db` left `mail.key` in `~/.local/share/russh-bbs/`. Point the whole data directory somewhere with `XDG_DATA_HOME=/srv/bbs` (the files then land in `/srv/bbs/russh-bbs/`), or set all three paths. See [FIX-DOC1-1](../../coordination.md).

Run it under a service manager (systemd or similar) as an unprivileged user. It stops cleanly on `SIGTERM` or `SIGINT`: it disconnects sessions, checkpoints the SQLite log and exits 0.

## The container image

The image is one static binary in a `scratch` image: no shell, runs as UID/GID 65532, listens on 2222, keeps **everything in one volume at `/data`**.

```bash
# build it yourself until the first release is published
docker build -t russh-bbs .
docker run -d --name bbs --restart unless-stopped -p 2222:2222 -v bbs-data:/data russh-bbs
docker logs bbs        # the one-time setup token is here on first start
```

Or `docker compose up -d` with the example [compose.yaml](../../../compose.yaml). A **bind mount** must be writable by UID 65532 (`chown 65532:65532 ./data && chmod 700 ./data`). Behind a TLS-intercepting proxy the build needs extra steps. **Not walked here:** this guide was written where no Docker daemon was available, so the commands above are [deploy.md](../../deploy.md)'s, unchanged. The container is **not** a sandbox for door games.

## SQLite or Postgres

| | SQLite (default) | Postgres |
|---|---|---|
| Setup | None | A Postgres 16 server that **requires TLS**, a database and a login |
| Good for | One server process, one board | When you already run Postgres and back it up |
| Where the data is | `russh-bbs.db` in the data directory | Your database. **The host key, `mail.key` and blobs still live on the server's disk** |
| Switching | There is **no tool to move data between them** | |

Pick one when you start the board. Both are covered by the same test suite; there is no automatic fall back from one to the other, and a Postgres failure stops the boot rather than quietly using SQLite.

### Postgres, with TLS required

TLS is not optional (D31): `datasource.tls_mode` is `require` (encrypts, but checks no certificate unless you also give `tls_root_cert`), `verify-ca` or `verify-full`. **Use `verify-full` with a CA file.** (`verify-ca` currently checks the host name too; do not rely on the difference, [config.md](../../config.md).) A server that does not offer TLS is a boot error.

**1. Make a Postgres that requires TLS.** In `postgresql.conf`: `ssl = on`, `ssl_cert_file`, `ssl_key_file` (the certificate must name the host the board connects to). In `pg_hba.conf` accept only `hostssl` for the board's login, so a client without TLS is refused:

```
hostssl  bbs_db  bbs  10.0.0.0/24  scram-sha-256
host     all     all  0.0.0.0/0    reject
```

Then create the login and an empty database owned by it (`CREATE ROLE bbs LOGIN PASSWORD '…'; CREATE DATABASE bbs_db OWNER bbs;`).

> To *try* this without writing certificates, the repository's CI helper starts a throwaway TLS-only server: `bash .github/scripts/postgres-tls-server.sh /var/tmp/pg-try 55432` (needs `apt install postgresql` and root; prints a URL with a random password). It is a test fixture, not a production setup. It is what this guide's Postgres run used.

**2. Point the board at it.** Put the CA that signed the server certificate in a file on the board's host, and set (the password is in the URL, so keep this in an environment file or secret store, not shell history):

```bash
export RUSSH_BBS_DATASOURCE_KIND=postgres
export RUSSH_BBS_DATASOURCE_URL='postgres://bbs:PASSWORD@db.example.com:5432/bbs_db'   # host name, not a socket path; no sslmode= in the URL
export RUSSH_BBS_DATASOURCE_TLS_MODE=verify-full
export RUSSH_BBS_DATASOURCE_TLS_ROOT_CERT=/srv/bbs/postgres-ca.crt
target/release/russh-bbs-server
```

In a config file these are `datasource.kind`, `datasource.url`, `datasource.tls_mode` and `datasource.tls_root_cert`. Do **not** also set `RUSSH_BBS_DB` or `datasource.path`: they belong to SQLite and are refused.

**What you should see.** Verified against Postgres 16 with TLS required:

| Case | Result |
|---|---|
| Right CA file | `database ready backend="postgres" tls=true`, the schema is created (26 migration rows on the first start), the setup token prints |
| `verify-full` with no `tls_root_cert` and a private CA | Boot stops: `Error: opening the postgres datasource (TLS is required)` / `invalid peer certificate: UnknownIssuer`. The password is not in the output |
| A plain `psql sslmode=disable` to the same server | Refused by Postgres: `pg_hba.conf rejects connection … no encryption` |

In the container the platform CA store is the bundled Mozilla list, so `verify-full` works against a publicly trusted certificate; for a private CA put the PEM on the `/data` volume and name it in `tls_root_cert` ([deploy.md](../../deploy.md#postgres) has the commands). `datasource.iam_auth = true` is **not built** and stops the boot.

Next: [first start and claiming the board](02-first-start.md).
