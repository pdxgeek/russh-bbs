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
| Sysop | `sysop.page`, `sysop.chat`, `sessions.view`, `sessions.disconnect`, `audit.view`, `settings.edit`, `roles.edit`, `groups.edit`, `themes.manage`, `backups.run` |

A permission is held either globally (from the role) or scoped to one area (one board, file area, room, or door). Scoped grants are how a trusted member moderates one board without becoming a moderator everywhere.

## Default roles

Shipped so a sysop never has to build a role from scratch. All of them except `sysop` and `waiting` can be edited; any can be copied as the starting point of a custom role.

| Role | Rank | Holds | Meant for |
|---|---|---|---|
| `waiting` | 0 | Waiting room group only | Unknown and unapproved keys. Not editable. |
| `member` | 10 | Account, `members.view`, `invites.create`, all read and post permissions in Forums, Chat, Mail, Files (`files.upload` goes to quarantine), Feeds, Doors, Boardlets except `polls.create` and `events.manage` | Everyone who was approved |
| `trusted` | 20 | `member` plus `polls.create`, `members.vouch`, `invites.view_tree` | Long-standing members; the inviter tier |
| `moderator` | 30 | `trusted` plus every `*.moderate`, `files.approve`, `members.deny`, `sessions.view` | Keeps the board clean; no settings or roles |
| `cosysop` | 40 | `moderator` plus every `*.manage`, `members.approve`, `members.suspend`, `members.edit`, `groups.edit`, `audit.view`, `events.manage`, `sysop.chat` | Runs the board day to day; cannot edit roles, settings, or backups |
| `sysop` | 100 | Everything, always | The founder and whoever they trust completely. Not editable. |

Rank only orders roles and stops an account from granting or editing a role at or above its own. It never unlocks an action by itself.

## Groups

A **group** is a named set of members. Groups exist so a sysop can manage people in bulk instead of one account at a time.

- A member belongs to any number of groups; a group can contain members and other groups (nesting is allowed, cycles are rejected, depth is capped).
- A group can be **assigned roles**: every member of the group holds those roles in addition to their own.
- A group can be **granted resource permissions** directly: a forum, file area, room, or door policy can name a group in its allow list or deny list, exactly as it can name a member. This is how "the staff group can see the staff board" or "the newcomers group cannot upload yet" is expressed without touching each account.
- Built-in groups: `everyone` (all approved members; cannot be edited or deleted) and `staff` (holds the `moderator` role by default; editable). Custom groups are created by anyone holding `groups.edit`.
- Group membership changes are audited and apply on the next action.

### How a check resolves

For a member performing an action on a resource, core gathers: the member's own roles; the roles of every group they are in (transitively); the member's direct allow and deny entries on the resource; and the allow and deny entries of every group they are in on that resource. Then, for the permission being checked:

1. **Deny** from any source (member or group) wins.
2. Otherwise **allow** from any source (member or group) wins.
3. Otherwise the union of permissions from all roles (own and group-assigned), as granted globally or scoped to this resource, decides.
4. Otherwise denied.

`sysop` is still exempt from allow and deny lists. The resolution is one pure function in `core::access` with a table-driven test over every combination, so that a later front end cannot get a different answer.

## Custom roles

The sysop (and any role holding `roles.edit`) can create a role with a name, a rank below their own, and any set of permissions, globally or scoped. Examples the docs will show: a `door-admin` that holds `doors.manage` and nothing else above `member`; a `feed-curator` with `feeds.manage`; a per-board moderator made by scoping `forum.moderate` to one board.

## Resource security

Permissions say what a role may do. Resource security says it on **each thing**: every board, file area, chat room, door, feed, poll, and event is a resource with its own policy, and every post, file, and message has an owner.

- **Per-resource policy.** Each resource carries: an owner; for each role, which of the resource's permissions that role holds there (default: the role's global set); an explicit **allow list** of members or groups who get a permission regardless of role (how a private board or an invite-only room works in the plain-ssh MVP, labeled "visible to the sysop" as always); and an explicit **deny list** that strips a permission from a member or group regardless of role. Deny wins over allow, allow wins over role, role wins over nothing.
- **Visibility follows policy.** A resource a member cannot `list` or `read` is absent from menus, search, new-scan, feeds out, and the SFTP tree, and is not reachable by name or ID. Tests cover every one of those paths, since a leak through search or SFTP is the same leak as one through the menu.
- **Ownership.** `*.edit_own` and delete-own apply to the member's own posts, files, and profile. Moderators act on others' through `*.moderate`. Ownership transfers are audited.
- **Resource limits per role**, overridable per member: minutes per day, calls per day, maximum concurrent sessions, posts per hour, upload bytes per day and per file, mail per day, invites per month, credits earn rate. Exceeding a limit is a normal, honest message, never a silent drop. Limits are checked in core next to permissions, so a front end cannot forget them.
- **Sysop sees everything.** The `sysop` role is not subject to allow and deny lists; the UI says so wherever a member sets one, in keeping with the honesty rule.

## Rules

- Every check goes through `core::access`. A front end never decides.
- An area where a role holds no `*.list` or `*.read` is not listed to that role and is not reachable by name.
- Role and permission changes are audited and apply on the next action.
- `sysop` and `cosysop` are refused over telnet (D27).
- Adding an action without adding its permission here and in the enum does not pass review.

## Implemented / Verified

Nothing yet.
