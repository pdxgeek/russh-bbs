# Chat rooms, presence and whispers (M3-F)

Live chat over plain `ssh`: rooms, who is here, whispers. Decisions: [D84](decisions.md), D78
(nothing said is stored), D33/D55 (access), D49 (per-session bounds).

## What it is, and what it is not

- The server can read everything said. The product does not call chat or whispers private,
  secure or encrypted, shows no lock, and puts **no privacy notice on the chat screens** (D78).
  A whisper is a "whisper".
- **Nothing said is stored.** There is no table for lines or whispers; they are in memory only,
  are not written to the audit log or to the logs, and are gone on restart. No staff screen,
  permission or export shows them. The only things stored are room *definitions* (a name) and an
  audit row when staff open or close a room.

## Using it

Main menu `[c] Chat` (held with `chat.join`). The room list shows the rooms the member may join.
Up/Down choose, Enter joins, Esc goes back. In a room: type and press Enter to speak; `/w name
text` whispers; `/who` lists who is here; `/leave` or Esc returns to the list; `/help`.
Staff (`chat.manage_rooms`) also get `n` (new room, a name of a-z, 0-9, `-`) and `x` then `y`
(close the selected room; everyone in it is put out). A board starts with one room, `lobby`.

## Access

Rooms are access resources (`ResourceKind::Room`, the room's row id). No new permission was
needed: the five `chat.*` permissions have existed since migration 0003 with their role rows.

| Action | Permission | On |
|---|---|---|
| See the room list | `chat.join` | `reach(chat.join, room)`: only reachable rooms are listed |
| Join a room | `chat.join` | the room; a room the member cannot reach, a closed one and one that never existed all answer "No such room." |
| Speak | `chat.speak` | the room, checked on every line |
| Whisper | `chat.whisper` | the room, checked on every whisper |
| Open / close a room | `chat.manage_rooms` | global; audited (`chat.room_created`, `chat.room_closed`) |
| Moderate (kick, mute) | `chat.moderate` | **not built** |

Every in-room action re-checks `chat.join` on the room against the permissions loaded for that
action (D55). A member who loses it is put back at the list on their next key; one who presses
no key keeps receiving the room for at most 10 seconds (the push path re-checks when events
arrive and the last load is older than that).

## The hub and its bounds

`core::chat::Chat` is the pure state (tested without a runtime or a sleep); `server::chat::Hub`
wraps it in a `std::sync::Mutex` that is never held across an `.await`, and wakes sessions with
a `tokio::sync::Notify` each.

| Bound | Value | When exceeded |
|---|---|---|
| Line or whisper length | 300 characters | cut by the shared sanitizer (SingleLine); a blank line is not sent |
| Send rate | burst 5, one more per second, per session, lines and whispers together | the line is refused with "Slow down. Nothing was sent." and reaches nobody |
| Scrollback per room | 100 lines, memory only, while the process runs | oldest dropped |
| Queue per session | 128 events | **oldest dropped**, counted, shown as "N lines were skipped because this connection was slow" |
| Members per room | 64 | "That room is full." |
| Sessions in chat | 512 | "Chat is full. Try again soon." |
| Open rooms | 32 (`MAX_ROOMS`) | "The board has the most rooms it allows." (enforced atomically in the store) |
| Screen transcript | 200 rows | oldest scroll away |

**A slow or stalled client never blocks a room.** Delivering to a session is a push onto its
bounded queue, which drops the oldest entry when full. A client that stops reading entirely is
disconnected by the shell's write timeout (`ssh.write_stall_secs`, D49); ending the shell drops
its `ChatSession`, which leaves the room and tells the others.

**Disconnect mid-line.** The line being typed lives only in the member's screen state; a
connection that ends sends nothing of it. The others see "* name left".

**Whispers.** A whisper goes to one session in the sender's room, chosen by handle (case and
look-alike letters ignored, as handles are). A target who is not in the room, whether the handle
exists elsewhere or not, is **"Not here."** The sender could already read the room's list, so
nothing is revealed. The sender sees an echo; the room and the scrollback never do.

**Presence** (who is in a room, join and leave notices) is sent only to members of that room,
all of whom hold `chat.join` on it. The waiting room holds none of the chat permissions and its
screens have no chat entry, so it learns nothing, in any registration mode.

## The push path (new lines without a key press)

The shell task (`ssh/shell.rs`) used to wake only for input. It now also waits on its
`ChatSession`'s `Notify`. A line, join, leave, whisper or room closing queues an event and wakes
the sessions it concerns; the shell takes them with `drain`, hands them to the screen and draws
once, inside the same `redraw_interval` cycle that rate-limits keys (D49), so a busy room costs a
session at most 20 redraws a second. A stale wake (a stored permit with nothing queued) is
harmless: the drain is empty and nothing is drawn.

## Implemented / Planned / Verified

- **Implemented:** the above, on SQLite and Postgres (migration `0016`, `chat_room`). Drawn in the
  member's layout, theme and icon tier through the classic content box and its reserved status
  row; other people's text is cleaned again when drawn and is never given the marker style.
- **Planned (not built):** `chat.moderate` (kick, mute, report), per-room topics and settings,
  a room console (M5), a sysop "page" or split chat (M5), scrolling back through a room's
  transcript.
- **Telnet (M3-H):** chat works over telnet with the same screens, hub and rooms (the hub lives
  in the `SessionRegistry`, which SSH and telnet share). Telnet is the low-security mode (D27):
  its sessions carry the `UNENCRYPTED` marker beside the server-readable one, and its input is
  untrusted like SSH input. `chat.manage_rooms` is a staff permission, which telnet withholds.
- **Verified:** core unit tests (the hub's rules with a manual clock: fan-out, presence, the
  scrollback and queue caps, flood refusal, whisper isolation, the proof checks, the room
  service against a fake store, the screen); hub tests in the server crate on a multi-thread
  runtime (eight concurrent sessions see one order, a stalled reader, disconnect mid-line, a
  wake without a key, the permission checks, staff actions, nothing logged, nothing in the
  database file); the store suite on SQLite, Postgres 16 and TLS-required Postgres 16;
  `server/tests/chat.rs` over a real russh client; and a run with real OpenSSH (see the PR).
