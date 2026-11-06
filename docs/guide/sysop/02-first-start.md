# 2. First start and claiming the board

A new board has no sysop. The first person to present the **setup token** becomes the sysop. Do this before you tell anyone the address.

## 1. Start it and read the token

On first start of an unclaimed board the server prints this to its console (stdout), and **only** there:

```
=== russh-bbs FIRST-RUN SETUP TOKEN (printed to the console only) ===
This board has no sysop yet. The first person to present this token
over SSH becomes the sysop:   ssh -p <port> claim@<host>
Anyone who can read this console or its logs can claim the board.
...
    XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX
=== end of setup token ===
```

- In a container: `docker logs bbs`.
- The token lives in memory. **A restart before the claim prints a new token and voids the old one.** Once the board is claimed, restarts print nothing (`founder already claimed; no setup token issued`).
- Anyone who can read the console or the container logs before you claim can claim the board. Do not send stdout to a third party before you have claimed it, and claim promptly.
- The log line `host key ready … fingerprint=SHA256:…` is the fingerprint to publish to your members. It stays the same across restarts and upgrades as long as the data directory (or volume) survives; losing it changes the fingerprint and every client shows the changed-host-key warning.

## 2. Claim it

From any machine that can reach the port. Use the SSH key you want to sign in with later; the claim binds the key you connect with.

```bash
ssh -p 2222 claim@your.host
```

The screen asks for the token and a password (12 to 128 characters, asked twice), then signs you in as **`founder`** with the `sysop` role. Over a real OpenSSH 9.6 client the whole thing is one form:

```
Setup token
Enter the setup token printed on the server console.
Token:    ***************************************
Choose a password (12 to 128 characters):   ****************
Type the password again:                    ****************
```

Then **Signed in as founder. This is your first login**, and the **icon check**: it shows one set of icons and asks "Do these look like icons?". `y` keeps them, `n` tries the next set (Nerd Font, then Unicode, then plain ASCII), `Esc` picks plain ASCII with no colour. If your box lines look broken, the screen tells you what to do ([Terminal.app tip](../member/02-terminal.md#box-lines-with-gaps-terminalapp)). You can redo it any time from Terminal settings, `i`.

- Handle: the founder is always `founder`. (No prompt to choose another.)
- **A claim from a connection that offers no key** (`ssh -o PubkeyAuthentication=no claim@host`, then Enter at the password prompt; walked) also works; the account then has only the password and you can add a key later under Members, My keys.
- The claim prompt looks the same whether or not the board is already claimed, and it is rate limited. If the connection is *reset* ("Connection reset by peer") right after a flood from your address, wait a few seconds: the claim shares the per-address connection allowance ([deploy.md](../../deploy.md#if-the-claim-connection-is-dropped-connection-flood)).

## 3. Check it from the outside

```bash
ssh -p 2222 new@your.host          # what a stranger sees: the waiting room
ssh -p 2222 founder@your.host      # signs in with your key; with another key it asks for the password
```

The waiting room shows only the banner, the rules and the forms the registration mode allows. It reveals nothing about members, boards or numbers, in any mode. A visitor with no key sees *Read the rules, Request an invite, Redeem an invite code, Disconnect*; a visitor with a key also sees *I already have an account*, *Link this device* and *Recovery code*.

## 4. What there is to do next

There is **no sysop console yet** (it is M5). What a sysop can do from the menus today:

| Where | What |
|---|---|
| Forums, `n` | Make a board: a name, what it is for, and `y` to restrict it to members you let in (`a` in its thread list adds one) or `n` for every member. A board starts with none, so make one first |
| Chat, `n` and `x` | Open a room (a-z, 0-9, `-`); close one (everyone in it is put out). A board starts with `#lobby` |
| Invites | Make invite codes (each shown once, valid seven days, single use). The sysop has no monthly limit |
| Sysop tools | Invite requests, Recovery code, Theme preview, Theme packs, Menus and pages |
| Community | Staff can remove one-liners, wall lines and polls |
| Config file or environment | Everything else, see [03-config.md](03-config.md) |

Everything else the table in [features.md](../../features.md) calls "Sysop admin TUI" (settings, roles, audit, suspensions, backups) is **Not built (M5)**; [04-people.md](04-people.md) says what that means in practice.

Next: [configuration](03-config.md).
