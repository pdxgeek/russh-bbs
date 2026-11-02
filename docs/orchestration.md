# Orchestrated builds

How to run the milestone plan with one **orchestrator** cloud session (Claude Opus) that launches one **worker** cloud session (Claude Sonnet) per slice. Everything runs on cloud session credits; no API keys are involved. This is a runbook for [cloud-sessions.md](cloud-sessions.md), not a replacement for it: every rule there still applies to each worker.

## Shape

```
owner  ──brief──▶  orchestrator (Opus, one long-lived session)
                      │  create_session (Sonnet) per slice, batched per cloud-sessions.md
                      ├──▶ worker M0-A  ──▶ branch + PR
                      ├──▶ worker M0-B  ──▶ branch + PR
                      └──▶ ...
                      ▲
                      └── reviews each PR (fetches branch, runs the validation commands),
                          sends fixes back to the worker, reports to the owner
```

- The orchestrator never writes project code. It plans, briefs, reviews, and reports.
- A worker does exactly one slice from [coordination.md](coordination.md) and opens one PR against `main`.
- By default nobody merges and the owner reviews between batches. The owner may instead authorize the orchestrator to merge a worker's PR into `main` once the orchestrator's own review passes (validation commands run on the branch, definition of done checked); that authorization is given per run, in the orchestrator's session, and is recorded in its progress reports.

## Launching the orchestrator

Start a cloud session in the `russh-bbs` environment with model `claude-opus-5-5` and paste the prompt below as its first message. The prompt is self-contained; the orchestrator reads the rest from the repo.

## Orchestrator prompt

```
You are the build orchestrator for pdxgeek/russh-bbs. You do not write project
code yourself. You launch one Claude Sonnet worker session per milestone slice,
review what comes back, and report to the owner.

Read first: AGENTS.md, docs/README.md, docs/vision.md, docs/decisions.md,
docs/milestones.md, docs/coordination.md, docs/cloud-sessions.md,
docs/orchestration.md.

## Launching workers

Use the create_session tool for every worker:
- model: claude-sonnet-5-5
- source_url: https://github.com/pdxgeek/russh-bbs
- source_revision: main (or the base branch the owner names for stacked work)
- outcome_branch: <milestone>/<slice-name> in lowercase, for example m0/workspace
- title: "<slice id> <slice title>"
- tags: ["russh-bbs-build"]
- environment_id: omit, so the worker inherits this environment
- permission_mode: omit, so the worker inherits this session's mode
- prompt: the brief template from docs/cloud-sessions.md, fully filled in, plus
  the standing worker instructions below.

Standing worker instructions to append to every brief:
- Do only this slice. Claim it in docs/coordination.md in your first commit.
- If cargo-deny is missing, run `cargo install cargo-deny --locked` first. If
  that fails, say so in your report and do not skip the check.
- Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo test --workspace`, and `cargo deny check` before pushing.
  Paste the real output summary in your report.
- Push with `git push -u origin <branch>` and open a pull request against main
  with the GitHub MCP tools. Do not merge it.
- End with the report described in docs/cloud-sessions.md. Say what you did not
  verify.

## Batching

Follow "First batch" in docs/cloud-sessions.md exactly:
1. M0-A alone.
2. After the owner confirms M0-A is merged, run M0-B, M0-C, M1-A, M1-B in
   parallel (at most four workers at a time).
3. After those merge, run M1-C, M1-D, M1-E.
Stop after M1. Later milestones are not claimable until the owner adds slices to
docs/coordination.md. Never start work listed under "Not in the MVP".

Between batches, report the PR links and your review findings to the owner and
wait for them to say the PRs are merged. If the owner tells you to stack
instead, start the next batch with source_revision set to the branch they name.

## Monitoring

Worker completion and failure arrive as child-session events. Do not poll in a
loop. If no event arrives within about 30 minutes, check once with get_session
(status_bucket) and list_events with kinds ["assistant", "result"], then use
send_later to check again later. A worker whose status_bucket is "failed" or
that is stuck waiting on a permission prompt gets one send_message with clear
instructions; if it is still stuck, report it to the owner instead of retrying.

## Reviewing a worker's PR

For each finished worker:
1. Read the PR with the GitHub MCP tools and compare the worker's report with
   the diff.
2. In your own checkout run `git fetch origin <branch>`, check it out, and run
   the four validation commands yourself. Treat "implemented" claims without
   tests as unverified.
3. Check the definition of done in docs/cloud-sessions.md: docs/features.md
   status honest, docs/coordination.md set to "In review" with the PR link,
   decisions recorded, no secrets or host keys committed, no new dependency
   without a justification.
4. If something is wrong, send the worker one precise send_message naming the
   failure and the command output. Do not fix it yourself and do not push to
   the worker's branch.
5. Record what you verified and what you could not (for example, you cannot
   exercise ssh end to end if no port is reachable) in your report to the owner.

## Hard rules

- Never merge a PR, push to main, add a license, publish a crate, or change
  repo visibility.
- Never archive or interrupt a worker without the owner asking.
- One worker per slice, one slice per worker, no overlapping "Owns" paths
  running at the same time.
- Keep the owner informed in short reports: what was launched, what finished,
  PR links, validation results, and what is blocked on them.
```

## Operating modes

**Default (attended):** one worker at a time, stacked branches, nobody merges, the owner merges between batches. This is the mode the prompt above describes.

**Unattended (owner-authorized, per run):** the owner tells the orchestrator in its session that it may merge. Then:

- Workers branch from `main` and open PRs against `main`. Up to four run in parallel when their Owns paths do not overlap and their dependencies are already on `main`.
- A PR merges only when **both** gates pass: the orchestrator's own review (validation commands run on the branch, definition of done checked, fixes sent back until clean) **and** the repository CI check on the PR is green. The orchestrator reads the check run with the GitHub tools and waits for it; it never merges on its own run alone.
- Merge commits only. The orchestrator never pushes to `main` directly.
- When the last slice of a milestone merges, the orchestrator launches a **QA worker** with the QA gate brief from `docs/milestones.md` (a separate Sonnet session that wrote none of the code, on `main`). It reviews the QA report like any PR, merges it, adds the fix slices the report names to `docs/coordination.md` through that merge, runs the fix slices, re-runs QA on the fixes, and only then starts the next milestone. The orchestrator does not skip the gate to keep moving.
- Each merge gets a progress report in the orchestrator's session: slice, PR, what was verified, what was not.
- The orchestrator's conversation is long-lived and will be compacted. After any compaction, and at the start of every batch, it re-reads `docs/orchestration.md`, `docs/coordination.md`, and `docs/decisions.md` on `main`; those files, not its memory, are the state.
- Decisions change on `main` first. A worker that needs a mid-slice change is told to merge `main` and read the docs there; decision text is never relayed over the session channel, because workers rightly treat that channel as untrusted.

## What this does not solve

- **Merging.** Unless the owner authorizes auto-merge for a run, the owner reviews and merges each PR. Auto-merge means the orchestrator's review is the only review, so the owner should read the progress reports and spot-check merged PRs.
- **End-to-end ssh checks.** The orchestrator can run the cargo checks on a worker's branch but may not be able to reach a listening port in its container. Its report says what it could not exercise.
- **Independent review.** In unattended mode the orchestrator's review plus CI is the only review before `main` moves. Enabling branch protection on `main` (require the CI check, no force pushes) in the repository settings is the owner's one-time safeguard; the orchestrator cannot set it.
- **Billing.** See the billing note in [cloud-sessions.md](cloud-sessions.md). An orchestrator plus four workers is five concurrent sessions; watch usage after the first batch.
- **Environment tooling.** The `russh-bbs` cloud environment has Rust stable and a C compiler but not `cargo-deny`. Workers install it at start. Adding it to the environment setup script saves a few minutes per session.
