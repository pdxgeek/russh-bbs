# russh-bbs

A modern, invite-only BBS you reach with plain `ssh`, written in Rust.

> Status: **planning**. Nothing is implemented yet. See [docs/README.md](docs/README.md).

Built on the [`russh`](https://crates.io/crates/russh) crate. This project is not affiliated with or endorsed by the russh project.

## What it is

- **Invite-only.** An unknown SSH key lands in a waiting room where it can only request or redeem an invite.
- **No install.** The whole MVP works over `ssh` and `sftp`. No custom client.
- **Forums, chat rooms, mail, files, RSS, and door games**, rendered as a themeable terminal UI.
- **A GraphQL API** (planned) so others can build their own UI.
- **End-to-end encrypted private messaging** is planned after the MVP and needs a native client. Until then the server can read everything, and the product says so plainly. See [docs/threat-model.md](docs/threat-model.md).

## Software name vs. board name

`russh-bbs` is the software. A running board has its own name, chosen by its sysop (candidates for the first board: Chatsubo, Lowtek). See [docs/decisions.md](docs/decisions.md).

## Working on it

Start at [AGENTS.md](AGENTS.md). Cloud sessions: [docs/cloud-sessions.md](docs/cloud-sessions.md).

## License

Not chosen yet. Do not add a license file without the owner's decision.
