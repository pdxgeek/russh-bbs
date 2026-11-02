# Permissions and roles

Status: **Planned** (M2). The model is decision D33 in [decisions.md](decisions.md). This page is the working list the M2 slice implements; it grows as milestones add actions, and every new action adds a row here in the same PR.

## Permissions

One named permission per action, a closed enum in `core::access`. Names are `area.verb`. A front end cannot perform an action without the check passing in core.

| Group | Permissions |
|---|---|
| Waiting room | `waiting.view`, `invite.redeem`, `membership.apply` |
| Account | `account.edit_profile`, `account.edit_settings`, `account.set_password` |
| Members | `members.view`, `members.approve`, `members.deny`, `members.vouch`, `members.suspend`, `members.edit`, `members.delete_data` |
| Invites | `invites.create`, `invites.revoke`, `invites.view_tree` |
| Forums | `forum.list`, `forum.read`, `forum.post`, `forum.reply`, `forum.edit_own`, `forum.moderate`, `forum.manage` |
| Chat | `chat.join`, `chat.speak`, `chat.whisper`, `chat.moderate`, `chat.manage_rooms` |
| Mail | `mail.read`, `mail.send` |
| Files | `files.list`, `files.download`, `files.upload`, `files.tag`, `files.approve`, `files.manage_areas` |
| Feeds | `feeds.read`, `feeds.manage` |
| Doors | `doors.run`, `doors.manage` |
| Boardlets | `oneliners.post`, `graffiti.post`, `polls.vote`, `polls.create`, `events.view`, `events.manage` |
| Sysop | `sysop.page`, `sysop.chat`, `sessions.view`, `sessions.disconnect`, `audit.view`, `settings.edit`, `roles.edit`, `themes.manage`, `backups.run` |

A permission is held either globally (from the role) or scoped to one area (one board, file area, room, or door). Scoped grants are how a trusted member moderates one board without becoming a moderator everywhere.

## Default roles

Shipped so a sysop never has to build a role from scratch. All of them except `sysop` and `waiting` can be edited; any can be copied as the starting point of a custom role.

| Role | Rank | Holds | Meant for |
|---|---|---|---|
| `waiting` | 0 | Waiting room group only | Unknown and unapproved keys. Not editable. |
| `member` | 10 | Account, `members.view`, `invites.create`, all read and post permissions in Forums, Chat, Mail, Files (`files.upload` goes to quarantine), Feeds, Doors, Boardlets except `polls.create` and `events.manage` | Everyone who was approved |
| `trusted` | 20 | `member` plus `polls.create`, `members.vouch`, `invites.view_tree` | Long-standing members; the inviter tier |
| `moderator` | 30 | `trusted` plus every `*.moderate`, `files.approve`, `members.deny`, `sessions.view` | Keeps the board clean; no settings or roles |
| `cosysop` | 40 | `moderator` plus every `*.manage`, `members.approve`, `members.suspend`, `members.edit`, `audit.view`, `events.manage`, `sysop.chat` | Runs the board day to day; cannot edit roles, settings, or backups |
| `sysop` | 100 | Everything, always | The founder and whoever they trust completely. Not editable. |

Rank only orders roles and stops an account from granting or editing a role at or above its own. It never unlocks an action by itself.

## Custom roles

The sysop (and any role holding `roles.edit`) can create a role with a name, a rank below their own, and any set of permissions, globally or scoped. Examples the docs will show: a `door-admin` that holds `doors.manage` and nothing else above `member`; a `feed-curator` with `feeds.manage`; a per-board moderator made by scoping `forum.moderate` to one board.

## Rules

- Every check goes through `core::access`. A front end never decides.
- An area where a role holds no `*.list` or `*.read` is not listed to that role and is not reachable by name.
- Role and permission changes are audited and apply on the next action.
- `sysop` and `cosysop` are refused over telnet (D27).
- Adding an action without adding its permission here and in the enum does not pass review.

## Implemented / Verified

Nothing yet.
