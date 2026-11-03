# Deploying with a container

Status: **Implemented** in M1-F (D25, D43) and FIX-M1-2 (D46). **The published image does not exist until the owner cuts the first release tag** (see [Owner action: release](#owner-action-release)); until then build locally. What is verified is in `docs/features.md`.

The server ships as one static (musl) binary in a `scratch` image: no shell, no package manager, no libc. It runs as a non-root user, listens on port 2222, and keeps **all state in one volume mounted at `/data`**.

> **The container is not the door sandbox.** It limits what the server process can touch on the host. It does not isolate one member's door game from another's or from the board's data; that isolation is built inside the process (D25) and arrives with doors.

> This project uses [`russh`](https://crates.io/crates/russh) and is not affiliated with or endorsed by that project.

## Run

### Published image (after the first release)

```bash
docker run -d --name bbs --restart unless-stopped \
  -p 2222:2222 -v bbs-data:/data \
  ghcr.io/pdxgeek/russh-bbs:latest
```

Images are published to GHCR on version tags (`v*`) only. `latest` follows the newest **stable** tag (a pre-release tag such as `v0.2.0-rc1` does not move it); `0.1.0` and `0.1` tags are published too. Pin a version tag in production. Both `linux/amd64` and `linux/arm64` are published. If `docker pull` answers `denied`, the package has not been published or made public yet.

### Local build (works today)

```bash
docker build -t russh-bbs .
docker run -d --name bbs -p 2222:2222 -v bbs-data:/data russh-bbs
```

With compose, use [`compose.yaml`](../compose.yaml): one service, one named volume (`bbs-data`), port 2222. It points at the published image; for a local build, uncomment `build: .` and comment out `image:`.

### Building behind a TLS-intercepting proxy

The builder stage downloads the Rust toolchain (rust-lang.org) and crates (crates.io) over HTTPS. A proxy that re-signs TLS makes those downloads fail with `invalid peer certificate: UnknownIssuer`. Two ways out:

1. **Give the builder the proxy's CA.** Mount or `COPY` the CA into the builder image and point `SSL_CERT_FILE` / `CARGO_HTTP_CAINFO` at it (this needs a Dockerfile change on your side; the project's Dockerfile does not ship one).
2. **Build the static musl binary on the host and package it** with the same final stage:

```bash
rustup target add x86_64-unknown-linux-musl     # aarch64-unknown-linux-musl on arm64
sudo apt-get install -y musl-tools
SQLX_OFFLINE=true cargo build --release --locked -p russh-bbs-server --target x86_64-unknown-linux-musl
docker build --build-arg BIN_STAGE=prebuilt \
  --build-context hostbin=target/x86_64-unknown-linux-musl/release -t russh-bbs .
```

The `prebuilt` stage copies `russh-bbs-server` into the same `scratch` final stage (user, `/data`, entrypoint) and needs to pull `alpine:3` once. Or skip building: use the published image.

## What is in `/data`

| Path | What | Created |
|---|---|---|
| `/data/russh-bbs.db` (+ `-wal`, `-shm`) | SQLite database: members, keys, settings | first start, mode 0600 |
| `/data/host_ed25519_key` | SSH host key (D20), mode 0600 | first start |
| `/data/config.toml` | Optional config (default location; see [config.md](config.md)) | you |
| `/data/` blobs directory | Member-uploaded bytes (D30), later | when files land |

The image sets only `RUSSH_BBS_LISTEN=0.0.0.0:2222`. It has no `HOME`, so the server's default data directory is `/data` itself (D46): the host key and the SQLite database land there with no further setting. The image deliberately does **not** set `RUSSH_BBS_DB`: that variable is a boot error with `datasource.kind = "postgres"`, and a variable baked into an image cannot be unset by `docker run`. A `/data/config.toml` is picked up automatically; the environment overrides the file and flags override both (D38). Only set `RUSSH_BBS_*` names listed in [config.md](config.md): an unknown one stops the server at start, except the Kubernetes service-link names listed in [config.md](config.md#kubernetes-service-links), which are ignored with a warning. If the service is named `russh-bbs`, you can also set `enableServiceLinks: false` on the pod so they are never injected. Logs go to stdout (`docker logs`); set `RUST_LOG` (e.g. `debug`) to change the level.

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
- The token goes to the console only; the server does not write it through its logger. A container runtime still keeps everything on stdout, so `docker logs` retains the token after the claim (it is void by then, so the only cost is confusion). After the claim nothing more is printed. Anyone who can read `docker logs` before the claim can claim the board, so do not use a log driver that ships stdout to a third party before you claim it, and claim promptly.
- `ssh -p 2222 localhost` (any other user name) reaches the waiting room.

### If the claim connection is dropped (connection flood)

The per-address connection limit (`waiting_room.ip_burst` connections, then one per `waiting_room.ip_interval_secs`; defaults 10 and 2 s) runs at accept, before the SSH handshake, and the claim is only recognized from its user name after the handshake. So the founder claim shares the allowance of the address it comes from: after a flood from that address (a scanner, or a loop of retries on a shared NAT), `ssh claim@host` is reset (`Connection reset by peer`) until the allowance refills (D65). Nothing is wrong with the server and nothing is lost; the token stays valid. Any of these gets you through:

- Wait. Each `ip_interval_secs` (2 s by default) puts one connection back; stop retrying in a tight loop, which keeps the allowance empty.
- Claim from another address, or before the port is reachable from the internet (the token is printed on first start, and the claim works on localhost).
- Raise `waiting_room.ip_burst` for the first start (for example `RUSSH_BBS_IP_BURST=100`) and lower it again afterwards.

The limit is per address (an IPv6 address counts as its /64), so one noisy address never blocks others.

## Memory after a connection flood

The image uses the system allocator of the static musl build. Measured on a release musl binary (D57): about 8 MB idle, and after a local flood of 200 sessions with constant resizes and input about 28 MB ten seconds later, i.e. roughly 20 MB stays with the process until restart. Peak memory is bounded by `ssh.max_connections` and the render cap (`ssh.render_cols` x `ssh.render_rows`, D53), not by the allocator. Swapping in `mimalloc` used more memory and was not adopted.

## Telnet (optional, off by default)

Telnet is **off** in the image: no port is bound and nothing in the container listens on it. Turn it on only if you accept that telnet is not encrypted (D27, D85).

```bash
docker run -d --name bbs -p 2222:2222 -p 2323:2323 -v bbs-data:/data \
  -e RUSSH_BBS_TELNET_ENABLED=true -e RUSSH_BBS_TELNET_ALLOW_LOGIN=true \
  ghcr.io/pdxgeek/russh-bbs
```

- `telnet.enabled` starts the listener on `telnet.listen` (default `0.0.0.0:2323`, its own port; publish it with `-p`). The log says `TELNET IS ON ... LOW-SECURITY MODE` at start. Staff screens show a `LOW-SECURITY` marker while it is on.
- `telnet.allow_login` is the second opt-in. Without it a connection is shown "Telnet login is not enabled on this board." and closed. With it, members who set a password over SSH (Terminal settings, Password) can sign in with it; a password typed over telnet crosses the network in the clear, so treat it as already known to anyone on the path and do not reuse it.
- A telnet session is an ordinary member: roles of rank 40 and up (`cosysop`, `sysop`) are dropped for the session, and there is no waiting room, founder claim, registration or SFTP over telnet. Every screen shows `UNENCRYPTED` on its last row.
- Telnet shares the SSH server's session counter (the per-member session limit spans both) but has its own connection-rate throttle and its own `ssh.max_connections` count, so a worst-case memory bill (D53) is up to twice what the log line `render_memory_ceiling_mib` reports when both transports are full. Lower `ssh.max_connections` if that matters.
- Bind it to a private interface (`telnet.listen = "10.0.0.5:2323"`) or firewall it to a trusted network if you can; there is no TLS.

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

## Postgres

SQLite on the volume is the default. To use Postgres instead, select it with environment variables (or `/data/config.toml`); do **not** also set `RUSSH_BBS_DB` or `datasource.path`, which are SQLite-only and refused:

```bash
docker run -d --name bbs -p 2222:2222 -v bbs-data:/data \
  -e RUSSH_BBS_DATASOURCE_KIND=postgres \
  -e RUSSH_BBS_DATASOURCE_URL='postgres://bbs:PASSWORD@db.example.com:5432/bbs' \
  -e RUSSH_BBS_DATASOURCE_TLS_MODE=verify-full \
  -e RUSSH_BBS_DATASOURCE_TLS_ROOT_CERT=/data/ca.crt \
  ghcr.io/pdxgeek/russh-bbs:latest
```

- **TLS is always on** (D31). `tls_mode` is `require` (encrypts, checks no certificate unless `tls_root_cert` is set), `verify-ca` or `verify-full`. A server that does not offer TLS is an error, never a plaintext fallback.
- **The image has no CA store** (`scratch`). `require` works as is. For `verify-ca` and `verify-full`, put the CA that signed the server certificate (a private CA, or the cloud provider's bundle such as the RDS one) in a PEM file **on the `/data` volume** and name it with `tls_root_cert`. A self-signed server certificate that is not itself a CA is rejected; give clients the CA. Without `tls_root_cert`, `verify-*` fails at start rather than weakening.
- Put the file on the volume with a throwaway container, e.g. `docker run --rm -v bbs-data:/data -v "$PWD:/in:ro" alpine sh -c 'chown 65532:65532 /data && cp /in/ca.crt /data/ca.crt && chown 65532:65532 /data/ca.crt'`. The `chown` of `/data` itself matters: a fresh volume takes its contents from the first image that mounts it, `alpine` has no `/data`, so Docker would create it `root:root` and the server (UID 65532) could not write its host key. Alternatively mount the volume once with the server image first (`docker run --rm -v bbs-data:/data ghcr.io/pdxgeek/russh-bbs --help`, which copies the image's correctly owned `/data`) and then copy the CA in.
- The URL carries the password: set it through an environment file or your orchestrator's secret store, not on a shared shell history. The server never logs it, and boot errors scrub it.
- The host key still lives on the volume (`/data/host_ed25519_key`), so the volume must persist even with Postgres, or members see a changed host key. Back up the database with your Postgres tooling.
- See [config.md](config.md) for every `datasource.*` key.

## Owner action: release

Cutting a tag and publishing the package are the repository owner's actions; nothing in CI or the agent sessions does them. Until they are done, `ghcr.io/pdxgeek/russh-bbs` does not exist.

Publishing is manual: pushing a tag no longer starts any workflow (all workflows are `workflow_dispatch` only; see docs/orchestration.md). GitHub Actions minutes are billed for private repositories, and the arm64 half of the image build takes hours under QEMU until FIX-REL-1 lands.

1. **Prove the workflow first (optional, pushes nothing).** GitHub: Actions, "Publish image", "Run workflow" on `main` (a dry run builds amd64 and arm64 and never logs in or pushes).
2. **Cut the tag** from the commit on `main` you want to ship:
   ```bash
   git checkout main && git pull
   git tag -a v0.1.0 -m "russh-bbs 0.1.0"
   git push origin v0.1.0
   ```
   (or create a release for the tag in the GitHub UI). Then start it by hand: Actions, "Publish image", "Run workflow", and pick the tag as the ref. Run on a tag, it pushes `ghcr.io/pdxgeek/russh-bbs:0.1.0`, `:0.1` and `:latest` for both architectures.
3. **Make the package public.** A new GHCR package starts private. GitHub: your profile or organization, Packages, `russh-bbs`, Package settings, "Danger Zone", Change visibility, Public. Until then `docker pull` answers `denied` for everyone but you. If the package is not linked to the repository, connect it there too.
4. **Check it from a clean machine, not logged in:** `docker pull ghcr.io/pdxgeek/russh-bbs:latest` and `docker buildx imagetools inspect ghcr.io/pdxgeek/russh-bbs:latest` (both `linux/amd64` and `linux/arm64` listed), then repeat the first-start walk above (empty volume, token in logs, waiting room, `claim@`, restart keeps the fingerprint, clean `docker stop`) and the Postgres run.
5. Record the result in `docs/features.md` (container row) and mark QA M1 H2 closed.

## Not covered here

TLS termination or reverse proxies (SSH is not HTTP; publish port 2222 or map another host port), and door sandboxing.
