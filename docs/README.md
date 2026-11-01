# russh-bbs documentation index

Status of the project: **planning only. No code exists yet.** Every feature below is "Planned" until its document says otherwise.

## Read first (everyone)

| Document | Purpose |
|---|---|
| [vision.md](vision.md) | What this is, who it is for, and what it refuses to be |
| [decisions.md](decisions.md) | Decisions made, and open questions that need the owner |
| [coordination.md](coordination.md) | Open work, claims, and handoffs between sessions |

## By area

| Area | Documents |
|---|---|
| Whole system | [architecture.md](architecture.md), [threat-model.md](threat-model.md) |
| Scope and order of work | [features.md](features.md), [mvp-scope.md](mvp-scope.md), [milestones.md](milestones.md) |
| Terminal UI, menus, layouts, themes, icons | [ui.md](ui.md) |
| Running cloud sessions | [cloud-sessions.md](cloud-sessions.md), [orchestration.md](orchestration.md) |
| Coding standards (Rust) | [../.claude/skills/rust-coding/SKILL.md](../.claude/skills/rust-coding/SKILL.md), copied from [pdxgeek/ai-skills](https://github.com/pdxgeek/ai-skills) (upstream is the source of truth) |

## Conventions

- Each feature document has an **Implemented / Planned / Verified** section. Keep them honest.
- Planned work that is deliberately out of scope is listed in [mvp-scope.md](mvp-scope.md) under "Not in the MVP". Do not build it early.
- Estimates in the milestone plan are guesses, not commitments.
