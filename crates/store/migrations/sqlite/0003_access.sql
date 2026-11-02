-- M2-B: roles, groups and grants (D33, docs/access.md "Implementation plan").
-- Six tables, one grant shape. Forward-only: once merged this file is never edited
-- (see src/sqlite/tests.rs). No secrets; names and ids only.
--
-- A global grant stores resource_kind = '' and resource_id = 0 (no NULLs, so the uniqueness
-- index works the same on both backends).

CREATE TABLE role (
    name TEXT    NOT NULL PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 32),
    rank INTEGER NOT NULL CHECK (rank BETWEEN 0 AND 100),
    kind TEXT    NOT NULL CHECK (kind IN ('locked', 'builtin', 'custom'))
) STRICT;

-- The complete permission set of a role. Names are the `area.verb` strings of core's
-- `Permission`; an unknown name is skipped when read (fail closed).
CREATE TABLE role_permission (
    role       TEXT NOT NULL REFERENCES role (name) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (role, permission)
) STRICT;

CREATE TABLE member_group (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT    NOT NULL UNIQUE CHECK (length(name) BETWEEN 1 AND 32),
    kind TEXT    NOT NULL CHECK (kind IN ('locked', 'builtin', 'custom'))
) STRICT;

-- Direct edges only: an account or another group inside a group.
CREATE TABLE group_member (
    group_id       INTEGER NOT NULL REFERENCES member_group (id) ON DELETE CASCADE,
    account_id     INTEGER REFERENCES accounts (id)     ON DELETE CASCADE,
    child_group_id INTEGER REFERENCES member_group (id) ON DELETE CASCADE,
    CHECK ((account_id IS NULL) <> (child_group_id IS NULL))
) STRICT;
CREATE UNIQUE INDEX group_member_unique
    ON group_member (group_id, COALESCE(account_id, 0), COALESCE(child_group_id, 0));
CREATE INDEX group_member_by_account ON group_member (account_id) WHERE account_id IS NOT NULL;
CREATE INDEX group_member_by_child   ON group_member (child_group_id) WHERE child_group_id IS NOT NULL;

-- The cached transitive closure over accounts, rewritten in the same transaction as every
-- change to `group_member`. The per-check lookup reads this and never recurses.
CREATE TABLE group_closure (
    group_id   INTEGER NOT NULL REFERENCES member_group (id) ON DELETE CASCADE,
    account_id INTEGER NOT NULL REFERENCES accounts (id)     ON DELETE CASCADE,
    PRIMARY KEY (group_id, account_id)
) STRICT;
CREATE INDEX group_closure_by_account ON group_closure (account_id);

-- The one grant shape: an account or a group gets a role or one permission, globally or on one
-- resource. A permission grant is an allow-list entry. There is no deny.
CREATE TABLE access_grant (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id    INTEGER REFERENCES accounts (id)     ON DELETE CASCADE,
    group_id      INTEGER REFERENCES member_group (id) ON DELETE CASCADE,
    role          TEXT    REFERENCES role (name)       ON DELETE CASCADE,
    permission    TEXT,
    resource_kind TEXT    NOT NULL DEFAULT '',
    resource_id   INTEGER NOT NULL DEFAULT 0,
    CHECK ((account_id IS NULL) <> (group_id IS NULL)),
    CHECK ((role IS NULL) <> (permission IS NULL)),
    CHECK ((resource_kind = '' AND resource_id = 0) OR (resource_kind <> '' AND resource_id > 0))
) STRICT;
CREATE UNIQUE INDEX access_grant_unique
    ON access_grant (COALESCE(account_id, 0), COALESCE(group_id, 0),
                     COALESCE(role, ''), COALESCE(permission, ''), resource_kind, resource_id);
CREATE INDEX access_grant_by_account  ON access_grant (account_id) WHERE account_id IS NOT NULL;
CREATE INDEX access_grant_by_group    ON access_grant (group_id)   WHERE group_id   IS NOT NULL;
CREATE INDEX access_grant_by_resource ON access_grant (resource_kind, resource_id) WHERE resource_id > 0;

-- ---- Built-in roles (D33). Complete sets, no inheritance: the layers are copied once, here. ----
-- `sysop` and `waiting` are locked: the editing services refuse them. Rank is ordering only.
INSERT INTO role (name, rank, kind) VALUES
    ('waiting',   0, 'locked'),
    ('member',   10, 'builtin'),
    ('trusted',  20, 'builtin'),
    ('moderator', 30, 'builtin'),
    ('cosysop',  40, 'builtin'),
    ('sysop',   100, 'locked');

-- waiting: the waiting room and nothing else (the room reveals nothing, threat model).
INSERT INTO role_permission (role, permission) VALUES
    ('waiting', 'waiting.view'),
    ('waiting', 'invite.redeem'),
    ('waiting', 'membership.apply');

-- member: account, members.view, invites.create, and every read and post permission in
-- Forums, Chat, Mail, Files (uploads are quarantined by the files slice), Feeds, Doors and
-- Boardlets except polls.create and events.manage.
INSERT INTO role_permission (role, permission) VALUES
    ('member', 'account.edit_profile'),
    ('member', 'account.edit_settings'),
    ('member', 'account.set_password'),
    ('member', 'members.view'),
    ('member', 'invites.create'),
    ('member', 'forum.list'),
    ('member', 'forum.read'),
    ('member', 'forum.post'),
    ('member', 'forum.reply'),
    ('member', 'forum.edit_own'),
    ('member', 'chat.join'),
    ('member', 'chat.speak'),
    ('member', 'chat.whisper'),
    ('member', 'mail.read'),
    ('member', 'mail.send'),
    ('member', 'files.list'),
    ('member', 'files.download'),
    ('member', 'files.upload'),
    ('member', 'files.tag'),
    ('member', 'feeds.read'),
    ('member', 'doors.run'),
    ('member', 'oneliners.post'),
    ('member', 'graffiti.post'),
    ('member', 'polls.vote'),
    ('member', 'events.view');

-- trusted = member + polls.create, members.vouch, invites.view_tree.
INSERT INTO role_permission (role, permission)
SELECT 'trusted', permission FROM role_permission WHERE role = 'member';
INSERT INTO role_permission (role, permission) VALUES
    ('trusted', 'polls.create'),
    ('trusted', 'members.vouch'),
    ('trusted', 'invites.view_tree');

-- moderator = trusted + every *.moderate, files.approve, members.deny, sessions.view.
INSERT INTO role_permission (role, permission)
SELECT 'moderator', permission FROM role_permission WHERE role = 'trusted';
INSERT INTO role_permission (role, permission) VALUES
    ('moderator', 'forum.moderate'),
    ('moderator', 'chat.moderate'),
    ('moderator', 'files.approve'),
    ('moderator', 'members.deny'),
    ('moderator', 'sessions.view');

-- cosysop = moderator + every *.manage (and manage_*), members.approve, members.suspend,
-- members.edit, groups.edit, audit.view, events.manage, sysop.chat. Not roles, settings, backups.
INSERT INTO role_permission (role, permission)
SELECT 'cosysop', permission FROM role_permission WHERE role = 'moderator';
INSERT INTO role_permission (role, permission) VALUES
    ('cosysop', 'forum.manage'),
    ('cosysop', 'chat.manage_rooms'),
    ('cosysop', 'files.manage_areas'),
    ('cosysop', 'feeds.manage'),
    ('cosysop', 'doors.manage'),
    ('cosysop', 'events.manage'),
    ('cosysop', 'themes.manage'),
    ('cosysop', 'members.approve'),
    ('cosysop', 'members.suspend'),
    ('cosysop', 'members.edit'),
    ('cosysop', 'groups.edit'),
    ('cosysop', 'audit.view'),
    ('cosysop', 'sysop.chat');

-- sysop: everything, always. A later permission adds its sysop row in the same migration that
-- introduces it; a store test fails if one is missing.
INSERT INTO role_permission (role, permission) VALUES
    ('sysop', 'waiting.view'),
    ('sysop', 'invite.redeem'),
    ('sysop', 'membership.apply'),
    ('sysop', 'account.edit_profile'),
    ('sysop', 'account.edit_settings'),
    ('sysop', 'account.set_password'),
    ('sysop', 'members.view'),
    ('sysop', 'members.approve'),
    ('sysop', 'members.deny'),
    ('sysop', 'members.vouch'),
    ('sysop', 'members.suspend'),
    ('sysop', 'members.edit'),
    ('sysop', 'members.delete_data'),
    ('sysop', 'invites.create'),
    ('sysop', 'invites.revoke'),
    ('sysop', 'invites.view_tree'),
    ('sysop', 'forum.list'),
    ('sysop', 'forum.read'),
    ('sysop', 'forum.post'),
    ('sysop', 'forum.reply'),
    ('sysop', 'forum.edit_own'),
    ('sysop', 'forum.moderate'),
    ('sysop', 'forum.manage'),
    ('sysop', 'chat.join'),
    ('sysop', 'chat.speak'),
    ('sysop', 'chat.whisper'),
    ('sysop', 'chat.moderate'),
    ('sysop', 'chat.manage_rooms'),
    ('sysop', 'mail.read'),
    ('sysop', 'mail.send'),
    ('sysop', 'files.list'),
    ('sysop', 'files.download'),
    ('sysop', 'files.upload'),
    ('sysop', 'files.tag'),
    ('sysop', 'files.approve'),
    ('sysop', 'files.manage_areas'),
    ('sysop', 'feeds.read'),
    ('sysop', 'feeds.manage'),
    ('sysop', 'doors.run'),
    ('sysop', 'doors.manage'),
    ('sysop', 'oneliners.post'),
    ('sysop', 'graffiti.post'),
    ('sysop', 'polls.vote'),
    ('sysop', 'polls.create'),
    ('sysop', 'events.view'),
    ('sysop', 'events.manage'),
    ('sysop', 'sysop.page'),
    ('sysop', 'sysop.chat'),
    ('sysop', 'sessions.view'),
    ('sysop', 'sessions.disconnect'),
    ('sysop', 'audit.view'),
    ('sysop', 'settings.edit'),
    ('sysop', 'roles.edit'),
    ('sysop', 'groups.edit'),
    ('sysop', 'themes.manage'),
    ('sysop', 'backups.run');

-- ---- Built-in groups. `everyone` has no member rows: every active account is in it. ----
INSERT INTO member_group (name, kind) VALUES ('everyone', 'locked'), ('staff', 'builtin');

-- Every active account holds `member` through `everyone`; staff hold `moderator`.
INSERT INTO access_grant (group_id, role)
SELECT id, 'member' FROM member_group WHERE name = 'everyone';
INSERT INTO access_grant (group_id, role)
SELECT id, 'moderator' FROM member_group WHERE name = 'staff';

-- ---- Existing accounts (M1). The legacy `accounts.role` column is no longer read for any
-- decision; the founder's authority is now a grant of the `sysop` role. Members need no row.
INSERT INTO access_grant (account_id, role)
SELECT id, 'sysop' FROM accounts WHERE role = 'sysop';
