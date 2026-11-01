# Running cloud sessions

How to hand this repo to remote coding sessions safely.

## Prerequisites

1. **Everything a session needs must be pushed to GitHub.** A cloud session starts from the pushed repository, not from a local checkout. Docs, `AGENTS.md`, and `.claude/skills/` that are only local are invisible to it.
2. A cloud environment with a Rust stable toolchain, `cargo-deny`, and a C compiler. If it lacks any, the session must say so and stop rather than skip the checks.
3. The owner has chosen the next slice from [coordination.md](coordination.md).

### Billing note
It is not confirmed whether cloud sessions draw on plan usage limits or on usage credits. Run one small session first and watch the billing page before and after. See the owner's notes; this repo does not store billing details.

## Sizing

One session = one slice from [coordination.md](coordination.md). A slice should fit in one pull request a human can review in under an hour. If a slice is bigger, split it in the coordination table before starting.

## Parallel work rules

- Sessions run in parallel only when their **Owns** paths do not overlap and neither depends on the other.
- Anything that touches the workspace root, `Cargo.toml` dependency lists, or shared traits in `core` is serialized: M0-A first, then others.
- A new dependency needs a one-line justification in the PR and a clean `cargo deny check`.

### Crate ownership map

| Path | Purpose | Notes |
|---|---|---|
| `crates/core` | Rules, traits, sanitizer, icons | Everything depends on it; change traits carefully |
| `crates/store` | SQLite implementation | Implements core traits only |
| `crates/server` | SSH, SFTP, TUI, binary | Thin; calls core |
| `crates/proto`, `crates/api` | Reserved | Empty until their milestones |
| `docs/` | Documentation | Each slice updates only its own docs and the shared registers |

## Brief template

Paste this, filled in, as the session's first message:

```
Slice: <id and title from docs/coordination.md>
Branch: <milestone>/<slice-name>
Goal: <one or two sentences>
Read first: AGENTS.md, docs/README.md, docs/vision.md, docs/decisions.md,
            <area docs>, .claude/skills/rust-coding/SKILL.md
Owns: <paths>
Do not touch: everything else (note needs in docs/coordination.md instead)
Acceptance: <copied from docs/milestones.md plus any slice-specific tests>
Report: what you built, what you tested (and how), what you did not verify,
        and any decision you made that belongs in docs/decisions.md
```

## Definition of done for a slice

- Code and tests merged-ready on the branch; `cargo fmt`, `clippy -D warnings`, `cargo test`, and `cargo deny check` are clean.
- User-visible behavior exercised with a real `ssh` or `sftp` client, and the report says what was and was not exercised.
- `docs/features.md` status updated honestly (Implemented vs. Verified), the area doc updated, and `docs/coordination.md` set to `In review` with the PR link.
- Any new decision recorded in `docs/decisions.md`.
- No credentials, host keys, or generated secrets committed.

## What a session must not do

- Start another slice or build anything listed under "Not in the MVP" in [mvp-scope.md](mvp-scope.md).
- Merge its own pull request, push to `main`, add a license, publish a crate, or change repo visibility.
- Weaken a security rule in [threat-model.md](threat-model.md) to make a test pass.
- Claim "private" for anything the server can read.

## First batch

1. **M0-A** alone. It creates the workspace the others need.
2. After M0-A merges, run **M0-B**, **M0-C**, **M1-A**, and **M1-B** in parallel. Their owned paths do not overlap.
3. Then **M1-C**, **M1-D**, and **M1-E**.
4. Review each PR before starting the next batch. After M1, decide whether to adjust the plan from what was learned.

## Reviewing a session's PR

Check the report against the diff. Run the validation commands yourself. Exercise the feature with `ssh`. Confirm the docs match what the code does. Treat "implemented" claims without tests as unverified.
