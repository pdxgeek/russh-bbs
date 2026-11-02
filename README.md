# russh-bbs

A modern, invite-only by default BBS you reach with plain `ssh`, written in Rust.

> Status: **planning**. Nothing is implemented yet. See [docs/README.md](docs/README.md).

Built on the [`russh`](https://crates.io/crates/russh) crate. This project is not affiliated with or endorsed by the russh project.

## What it is

- **SSH by default.** Telnet can be switched on by the sysop; it is off out of the box and every telnet screen says the connection is unencrypted.
- **Invite-only by default.** An unknown SSH key lands in a waiting room where it can only request or redeem an invite. The sysop can switch registration to apply-and-approve or open.
- **No install.** The whole MVP works over `ssh` and `sftp`. No custom client.
- **Forums, chat rooms, mail, files, RSS, and door games**, rendered as a themeable terminal UI.
- **A GraphQL API** (planned) so others can build their own UI.
- **End-to-end encrypted private messaging** is planned after the MVP and needs a native client. Until then the server can read everything, and the product says so plainly. See [docs/threat-model.md](docs/threat-model.md).

## Run it in a container

```bash
docker run -d --name bbs -p 2222:2222 -v bbs-data:/data ghcr.io/pdxgeek/russh-bbs:latest
docker logs bbs        # shows the one-time founder token on first start
ssh -p 2222 claim@localhost
```

Or `docker compose up -d` with the example [compose.yaml](compose.yaml). All state lives in the `/data` volume. Images are published on version tags only. Full guide, including backups, upgrades and the host key fingerprint: [docs/deploy.md](docs/deploy.md). The container is not a sandbox for door games.

## Software name vs. board name

`russh-bbs` is the software. A running board has its own name, chosen by its sysop (candidates for the first board: Chatsubo, Lowtek). See [docs/decisions.md](docs/decisions.md).

## Working on it

Start at [AGENTS.md](AGENTS.md). Cloud sessions: [docs/cloud-sessions.md](docs/cloud-sessions.md).

## License

Not chosen yet. Do not add a license file without the owner's decision.
