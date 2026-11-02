# Deploying with a container

Status: **Implemented** in M1-F (D25, D43). Verified items are listed at the end.

The server ships as one static (musl) binary in a `scratch` image: no shell, no package manager, no libc. It runs as a non-root user, listens on port 2222, and keeps **all state in one volume mounted at `/data`**.

> **The container is not the door sandbox.** It limits what the server process can touch on the host. It does not isolate one member's door game from another's or from the board's data; that isolation is built inside the process (D25) and arrives with doors.

> This project uses [`russh`](https://crates.io/crates/russh) and is not affiliated with or endorsed by that project.

## Run

```bash
docker run -d --name bbs --restart unless-stopped \
  -p 2222:2222 -v bbs-data:/data \
  ghcr.io/pdxgeek/russh-bbs:latest
```

Images are published to GHCR on version tags (`v*`) only; `latest` follows the newest tag. To build from a checkout instead: `docker build -t russh-bbs .`.

With compose, use [`compose.yaml`](../compose.yaml): one service, one named volume (`bbs-data`), port 2222.

## What is in `/data`

| Path | What | Created |
|---|---|---|
| `/data/russh-bbs.db` (+ `-wal`, `-shm`) | SQLite database: members, keys, settings | first start, mode 0600 |
| `/data/host_ed25519_key` | SSH host key (D20), mode 0600 | first start |
| `/data/config.toml` | Optional config (default location; see [config.md](config.md)) | you |
| `/data/` blobs directory | Member-uploaded bytes (D30), later | when files land |

The image sets `RUSSH_BBS_LISTEN=0.0.0.0:2222`, `RUSSH_BBS_HOST_KEY=/data/host_ed25519_key` and `RUSSH_BBS_DB=/data/russh-bbs.db`. A `/data/config.toml` is picked up automatically; the environment overrides the file and flags override both (D38). Only set `RUSSH_BBS_*` names listed in [config.md](config.md): an unknown one stops the server at start. Logs go to stdout (`docker logs`); set `RUST_LOG` (e.g. `debug`) to change the level.

## Volume permissions and the non-root user

The process runs as **UID/GID 65532**. A named volume copies the image's `/data` (owned by 65532, mode 0700) on first use, so it just works. A **bind mount** keeps the host directory's ownership, so make it writable for 65532:

```bash
mkdir -p ./data && sudo chown 65532:65532 ./data && chmod 700 ./data
docker run -d -p 2222:2222 -v "$PWD/data:/data" ghcr.io/pdxgeek/russh-bbs:latest
```

Check the user with `docker inspect --format '{{.Config.User}}' bbs` (`docker exec ... id` does not work: the image has no shell or `id`). The server refuses to start if the host key file is group- or world-accessible, and it never regenerates a key it cannot use (D20).

## First start: the founder token

On first start of an unclaimed board the server prints a one-time **setup token** to stdout, between marked lines:

```bash
docker logs bbs
ssh -p 2222 claim@localhost     # paste the token to become sysop (D41)
```

- The token is held in memory only. **Restarting before the claim prints a new token and voids the old one**; read it again from `docker logs`.
- After the claim nothing is printed. Anyone who can read `docker logs` before the claim can claim the board, so do not use a log driver that ships stdout to a third party before you claim it, and claim promptly.
- `ssh -p 2222 localhost` (any other user name) reaches the waiting room.

## Host key fingerprint

The fingerprint is logged at start (`host key ready ... fingerprint=SHA256:...`): `docker logs bbs | grep fingerprint`. Publish it so members can check their first connect. It is stable across `docker restart`, upgrades and `docker rm` as long as the volume survives. Losing the volume changes it, and clients will see the standard changed-host-key warning.

## Stopping, restarting, upgrading

`docker stop` sends SIGTERM. The server stops accepting connections, disconnects sessions, checkpoints the SQLite WAL, closes the database and exits 0, within 8 seconds (under Docker's 10 s default; the compose example allows 15 s). SIGINT does the same. If a stop is cut off by SIGKILL, SQLite recovers the WAL on the next start.

To upgrade: pull the new tag, then recreate the container with the same volume (`docker compose pull && docker compose up -d`). Database migrations are forward-only and run at start; **back up first** and do not run an older image on a newer database.

## Backups

Real, tested backups are M5. Until then, stop the container (so the WAL is checkpointed) and copy the volume:

```bash
docker stop bbs
docker run --rm -v bbs-data:/data -v "$PWD:/backup" alpine tar czf /backup/bbs-data.tgz -C /data .
docker start bbs
```

The archive holds the host key and the member list: store it like a secret. Do not copy the `.db` file of a running server without its `-wal` file.

## Not covered here

Postgres (M1-G), TLS termination or reverse proxies (SSH is not HTTP; publish port 2222 or map another host port), and door sandboxing.
