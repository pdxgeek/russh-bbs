# Vision

## One sentence

An invite-only bulletin board you reach with plain `ssh`: forums, chat, mail, files, feeds, and door games, with the feel of a classic BBS and the security habits of a modern service.

## Principles

1. **No install for the MVP.** If a user has `ssh` and `sftp`, they can do everything the MVP offers.
2. **Invite-only and quiet.** Strangers see a waiting room and nothing else. Growth is by vouching, and the invite tree is a feature.
3. **Honest privacy.** The MVP server can read nearly everything, and the product says so. Real end-to-end encryption comes later, with a native client, and is never faked before that.
4. **Open edges.** RSS in and out, a GraphQL API, and data export, so the board is not a walled garden.
5. **Alive, not nostalgic museum.** Door games, shared worlds, scheduled events, and a sysop with a voice.
6. **Themeable everywhere.** Menu layouts, colors, icons, and every user-facing string can be changed by the sysop and chosen by the member.
7. **Accessible.** A plain text mode with no icons or art must always be available.

## Who it is for

- A sysop who wants to run a small community board on their own server.
- Members who are comfortable in a terminal, invited by someone they know.
- Developers who want to build another interface on the API.

## What it refuses to be

- A public, open-registration service.
- A chain, token, or financial product. Credits are an internal, non-transferable game mechanic.
- A system that pretends to be private when it is not.
- A system that runs code from strangers without a sandbox.

## Names

`russh-bbs` is the software. The first board's name is undecided (candidates: Chatsubo, Lowtek, both from William Gibson's work). See [decisions.md](decisions.md). Do not copy text from Gibson's books into screens or docs.

## Audience for the docs

Humans and coding agents. Agents start at [../AGENTS.md](../AGENTS.md).
