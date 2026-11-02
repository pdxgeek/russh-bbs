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
- A group can be **granted resource permissions** directly: a forum, file area, room, or door policy can name a group in its allow list exactly as it can name a member. This is how "the staff group can see the staff board" is expressed without touching each account.
- Built-in groups: `everyone` (all approved members; cannot be edited or deleted) and `staff` (holds the `moderator` role by default; editable). Custom groups are created by anyone holding `groups.edit`.
- Group membership changes are audited and apply on the next action.

### How a check resolves

Access is **additive only**. A member holds a permission on a resource if any one of these is true:

1. one of their roles (own, or assigned to a group they are in) holds it globally;
2. one of their roles holds it scoped to that resource;
3. the resource's allow list names them, or a group they are in, for that permission.

Otherwise denied. Suspended accounts and the `waiting` role never reach step 1. There is no deny list and no precedence to reason about. The resolution is one pure function in `core::access` that returns the **reason** (which role, group, or allow entry granted, or nothing) so the sysop UI can explain any decision, with a table-driven test over the combinations.

Group membership is resolved through a **cached transitive closure** (member to all groups, direct and nested) that is rebuilt when a group changes, never recursively per check.

## Custom roles

The sysop (and any role holding `roles.edit`) can create a role with a name, a rank below their own, and any set of permissions, globally or scoped. Examples the docs will show: a `door-admin` that holds `doors.manage` and nothing else above `member`; a `feed-curator` with `feeds.manage`; a per-board moderator made by scoping `forum.moderate` to one board.

## Resource security

Permissions say what a role may do. Resource security says it on **each thing**: every board, file area, chat room, door, feed, poll, and event is a resource with its own policy, and every post, file, and message has an owner.

- **Per-resource policy.** Each resource carries: an owner; for each role, which of the resource's permissions that role holds there (default: the role's global set); and an explicit **allow list** of members or groups who get a permission there regardless of role (how a private board or an invite-only room works in the plain-ssh MVP, labeled "visible to the sysop" as always). There is **no deny list**: access is purely additive. To take access away, remove the grant, remove the member from the group, change their role, or suspend the account.
- **Visibility follows policy.** A resource a member cannot `list` or `read` is absent from menus, search, new-scan, feeds out, and the SFTP tree, and is not reachable by name or ID. Tests cover every one of those paths, since a leak through search or SFTP is the same leak as one through the menu.
- **Ownership.** `*.edit_own` and delete-own apply to the member's own posts, files, and profile. Moderators act on others' through `*.moderate`. Ownership transfers are audited.
- **Resource limits per role**, overridable per member: minutes per day, calls per day, maximum concurrent sessions, posts per hour, upload bytes per day and per file, mail per day, invites per month, credits earn rate. Exceeding a limit is a normal, honest message, never a silent drop. Limits are checked in core next to permissions, so a front end cannot forget them.
- **Sysop sees everything.** The `sysop` role holds every permission everywhere; the UI says so wherever a member sets an allow list, in keeping with the honesty rule.

## Prior art

Borrowed, not adopted wholesale:

- **RFC 9068** (JWT profile for OAuth access tokens) names the three sources exactly as this model has them: `roles`, `groups`, `entitlements` (direct grants). The M5 API uses those claim names.
- **RFC 9396** (Rich Authorization Requests) describes a scoped grant as type, location, action. The `area.verb` permission name plus a resource ID is that triple; keep the split formal in the enum.
- **Zanzibar-style systems** (OpenFGA, SpiceDB) are additive only with groups as subject sets and a precomputed membership closure. That is why there is no deny list here.

No tokens: one process and one database, so the resolution function is the authority.

## Keep the implementation simple

This is the most intricate feature in the MVP, so the M2 access slice starts with a short written plan (a section at the end of this page) that the orchestrator reviews before any code. The plan must show:

- **One grant table** for all three sources: `grant(subject, permission, resource)` where subject is a member, a group, or a role, and resource is a specific resource or `global`. Roles are rows with subject = role; role assignment is `member_role` and `group_role`; group membership is `group_member`. No other access tables.
- **One query** that answers a check, or a cached per-session permission set computed from that table; whichever the worker picks, the pure function in core is what tests exercise.
- **Visibility** derived from the same function (`*.list`), not a second mechanism.
- **Defaults as data**: the built-in roles and groups are migration rows, not code paths.
- What is deliberately left for later (per-member limits overrides, scoped grants in the UI) and why.

## Rules

- Every check goes through `core::access`. A front end never decides.
- An area where a role holds no `*.list` or `*.read` is not listed to that role and is not reachable by name.
- Role and permission changes are audited and apply on the next action.
- `sysop` and `cosysop` are refused over telnet (D27).
- Adding an action without adding its permission here and in the enum does not pass review.

## Implemented / Verified

**Placeholder for the menu framework (M2-A, `core::access::permission`).** M2-B owns the real enum, roles and resolution; this only lets a menu item carry a permission and ask one question.

- `Permission` has the six permissions the menus name: `account.edit_settings`, `members.view`, `members.approve`, `forum.list`, `mail.read`, `files.list`. M2-B extends it to the full list above.
- `PermissionCheck::allows(Permission)` is the single hook. Menus show an item only if it passes, and the same check decides whether its hotkey does anything. `RolePermissions(Role)` is today's stand-in: `member` holds everything except `members.approve`, `sysop` holds everything.
- Menu items and their gates:

| Menu item | Needs | Notes |
|---|---|---|
| Terminal settings | `account.edit_settings` | an action |
| Forums, Mail, Files, Members | `forum.list`, `mail.read`, `files.list`, `members.view` | placeholders, shown only to a session that would hold them; picking one says "not built yet" |
| Sysop tools, Approval queue | `members.approve` | a submenu and a placeholder |
| Log off | none | every signed-in session may end itself; there is nothing to grant |
| Help, Back, Quit keys | none | universal keys, not actions |

- **Tested:** the visible set follows the hook, a hidden item's hotkey does nothing, and every action item in the tree takes its permission from the action. **Not done (M2-B):** real roles, groups, scoped grants, "every front-end action path calls `core::access`" tests.
