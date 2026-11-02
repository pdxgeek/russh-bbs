# russh-bbs agent entrypoint

Read `docs/README.md`, `docs/vision.md`, and `docs/decisions.md`. Then read only the documents for your area from the index. Check `docs/coordination.md` for open work before starting, and claim your slice there.

## Rules

- **One milestone slice per session.** Slices are defined in `docs/milestones.md`. Do not start the next milestone or a feature that is not in `docs/features.md`.
- **Docs change in the same pass as code.** State what is implemented, planned, and verified. Never mark something implemented that is not tested. Update `docs/features.md` status and `docs/coordination.md`.
- **Do not duplicate or edit another session's work.** Stay inside the crates and docs your slice owns (ownership map in `docs/cloud-sessions.md`). If you need a change elsewhere, note it in `docs/coordination.md` instead.
- **Prefer a focused reliability or security fix over another untested feature.**
- **Record decisions.** Add to `docs/decisions.md` when you choose between real alternatives. Do not reverse a recorded decision without the owner.

## Non-negotiable product rules

- **Be honest about privacy.** In the MVP the server can read everything except PGP-encrypted mail and files users encrypt themselves. Never label anything "private" or show a lock where the server can read the content. No silent fallback from private to server-readable. See `docs/threat-model.md`.
- **Sanitize all text that is not UI chrome**: user posts, usernames, feed items, mail, file names and descriptions, door output. Strip control characters and ANSI escapes, and neutralize the Nerd Font private-use range so content cannot spoof UI icons or security badges. One shared sanitizer in `russh-bbs-core`; do not write ad hoc ones.
- **Security cues are not themeable.** Layouts and themes may place them but never hide or restyle them in a misleading way.
- **Fetching user-supplied URLs requires SSRF protection** (see threat model).
- **Never persist credentials, private keys, or tokens** in the repo, logs, fixtures, or the database in plaintext. Test keys are generated at test time.
- **The waiting room reveals nothing** about members, boards, or stats to an unauthenticated key, in every registration mode. Registration mode is a sysop setting with `invite` as the default (D2); `apply` and `open` add a registration path and nothing else.

## Engineering rules

Follow the `rust-coding` skill (`.claude/skills/rust-coding/`: `SKILL.md` plus `references/`). It is a copy of the skill in [pdxgeek/ai-skills](https://github.com/pdxgeek/ai-skills), which is the source of truth: do not edit the copy here, change it upstream and re-copy. Agents that cannot load skills should read those files directly before writing Rust.

- Rust stable, edition 2021 or later. Cargo workspace. No `unsafe` without a comment explaining why and a test.
- Business logic lives in `russh-bbs-core`. Front ends (SSH TUI, SFTP, GraphQL later) stay thin and call it. Do not put rules in a front end.
- Database migrations are forward-only and versioned. Do not edit a migration that has landed.
- No new dependency without a one-line justification in the PR and a `cargo deny` pass. Keep license and attribution files with anything reused.
- This project uses `russh`; it is not affiliated with that project. Keep the README disclaimer.
- Do not add a license file, publish a crate, or change the repo's visibility. Those are the owner's decisions.

## Validation

Run before every PR, and report the results honestly:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

A passing build does not prove behavior. For anything user-visible, also run the server and exercise it with a real `ssh` client, and say what you did and did not verify. If a check cannot run in your environment, say so.

## Git

Work on a branch named `<milestone>/<slice>` (for example `m1/ssh-waiting-room`). Open a pull request against `main`. Do not push to `main` and do not merge your own PR. End PR descriptions and commits with the attribution lines your environment gives you.
