# russh-bbs documentation index

Status of the project: M0, M1 and M2 are merged. M1's QA gate passed except the v0.1.0 image, which is withdrawn (D76; see [qa/m1.md](qa/m1.md)). M2's QA gate is closed: its fixes were re-checked on `0caab35` (see the Re-check section of [qa/m2.md](qa/m2.md)); the remaining FIX-M2 rows in [coordination.md](coordination.md) are low or parked. The next release is v0.2.0, tagged by the owner (D76). M3 slices are proposed in [coordination.md](coordination.md) and claimable once the owner approves them. M3-E (forums with search, D83) is implemented on `m3/forums`. [features.md](features.md) holds the status of every feature.

## Read first (everyone)

| Document | Purpose |
|---|---|
| [vision.md](vision.md) | What this is, who it is for, and what it refuses to be |
| [decisions.md](decisions.md) | Decisions made, and open questions that need the owner |
| [coordination.md](coordination.md) | Open work, claims, and handoffs between sessions |

## By area

| Area | Documents |
|---|---|
| Whole system | [architecture.md](architecture.md), [threat-model.md](threat-model.md), [access.md](access.md), [audit.md](audit.md) |
| Profiles and the members list | [profiles.md](profiles.md) |
| The sysop queue (request an invite, approve, deny, vouch) | [queue.md](queue.md) |
| Registration modes `invite`, `apply`, `open`; the daily cap | [registration.md](registration.md) |
| Configuration keys, env vars, flags | [config.md](config.md) |
| Scope and order of work | [features.md](features.md), [mvp-scope.md](mvp-scope.md), [milestones.md](milestones.md) |
| Terminal UI, menus, layouts, themes, icons | [ui.md](ui.md) |
| Running cloud sessions | [cloud-sessions.md](cloud-sessions.md), [orchestration.md](orchestration.md) |
| Third-party crates (default choices, library-first D47) | [third-party.md](third-party.md) |
| Coding standards (Rust) | [../.claude/skills/rust-coding/SKILL.md](../.claude/skills/rust-coding/SKILL.md), copied from [pdxgeek/ai-skills](https://github.com/pdxgeek/ai-skills) (upstream is the source of truth) |

## Conventions

- Each feature document has an **Implemented / Planned / Verified** section. Keep them honest.
- Planned work that is deliberately out of scope is listed in [mvp-scope.md](mvp-scope.md) under "Not in the MVP". Do not build it early.
- Estimates in the milestone plan are guesses, not commitments.
