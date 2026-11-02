# The sysop queue: requests for an invite, approve, deny, vouch

Status: **Implemented** (M2-E, `m2/sysop-queue`, D71, D72); not yet Verified (the M2 QA gate has not run). Code: `core::queue`, `server::sysop::queue`, the store's `QueueStore` on both backends, migration `0009_queue`. The `apply` registration mode (M2-G) uses it too: an application is a request of kind `application` ([registration.md](registration.md)); `open` mode does not use the queue (except on a day its cap is used up, when the board shows the application form).

## What a stranger does

In `invite` mode the waiting room offers *Request an invite*: a handle and a line about themselves. Whatever happens next, **the person reads the same words**:

> Thank you. If your request is kept, the staff and trusted members here can read your handle, your message and this key's fingerprint, and the staff decide when they can. Nobody is notified: connect again later with this same key to find out.

The answer is shown before the store is asked, so no outcome can change it. These all look alike to the person (and are logged with the reason, never the text):

| Outcome | Why |
|---|---|
| Kept | The request is in the queue |
| Dropped | This key already has a request waiting |
| Dropped | This key has asked three times in a few minutes (rate limit per key) |
| Dropped | This network has asked ten times in a few minutes (rate limit per network) |
| Dropped | The queue is full (`queue_max_pending`) |
| Dropped | This key was denied less than 24 hours ago |
| Dropped | This key already has an account (pending or suspended accounts still reach the room) |
| Dropped | The waiting snapshot does not hold `invite.request` |
| Dropped | The store failed or was slow |

The waiting room shows nothing else about the queue, in any mode: no size, no members, no boards. A test compares the room's whole screens with an empty queue, a busy one and a full one and they are identical.

## What is stored

| Table | Columns | Kept until |
|---|---|---|
| `invite_request` | id, key fingerprint (unique), handle, message, created, expires | decided, or 14 days |
| `invite_request_vouch` | request, voucher account, created | with the request, or with the voucher's account |
| `request_denial` | SHA-256 of the key's fingerprint (domain-separated), expires | 24 hours |

That is all. **No IP address, no user agent, nothing the client sent beyond the key's fingerprint.** The network rate limit keeps the address in memory as the limiter's key only. The handle passes `sanitize_username`; the message is sanitized as one line and capped at 280 characters, and both are sanitized again on read and on render.

## Defaults (D72)

| Setting | Default | Where |
|---|---|---|
| Pending requests per key | 1 | unique index |
| Pending requests in total | 200 | stored setting `queue_max_pending` (1 to 10,000); see [config.md](config.md) |
| Requests per key | burst 3, then 1 per 10 minutes | `core::queue::DEFAULT_KEY_RATE` (M1-C's limiter) |
| Requests per network | burst 10, then 1 per 2 minutes (IPv6: per /64) | `core::queue::DEFAULT_NETWORK_RATE` |
| A request waits | 14 days (`PENDING_TTL_SECS`) | constant |
| A denied key is turned away | 24 hours (`DENIAL_COOLDOWN_SECS`) | constant |
| Purge | on every submission and listing, and hourly | store, and a server task |

## What staff do

**Sysop tools, Invite requests** (reached with `members.approve`) lists the waiting requests, oldest first, four to a page: handle, how long ago, how many members vouched, and the message cut to one line. A digit opens one: the whole message, the vouchers' names (two, then a count), when it expires. Then:

- **`a` approve** (`members.approve`) creates an `active` account with that handle, bound to the key that asked, with the approver recorded as the one who admitted them (`accounts.invited_by`). The key signs in as that account the next time it connects. If the handle is taken (or looks too much like one that is, D17) or the key got an account meanwhile, nothing changes and the sysop is told; the request stays so it can be denied.
- **`d` deny** (`members.deny`) deletes the request and its vouches and turns the key away for 24 hours.
- **`v` vouch** (`members.vouch`) adds the viewer's name, once.

Each write asks `y` first. Which keys are offered follows the permissions held *now*; the real check is in core on every request, and a permission taken away closes the screen on the next key.

**Members, Vouch for a request** (`members.vouch`, trusted and up) is the same list with only the vouch key. A vouch is a statement for the sysop to weigh. It decides nothing: there is no threshold and nothing is approved automatically. The sysop sees active members' names only (a suspended member's vouch is not shown).

At 80x24 the fullest screens (four rows with the longest handles and messages, a full-width 280-character message in the detail, a refusal notice) fit with their footer and the reserved status row; this is tested in every icon tier. Requester text is drawn only in the content box, never in the status row, never with the reserved marker style, and never as a label or key.

## Audit (see [audit.md](audit.md))

| Write | Action | Actor | Target | Detail |
|---|---|---|---|---|
| approve | `account.created` | the approver | the new account | `via request <id> state active` |
| approve | `member.approved` | the approver | the new account | `request <id>` |
| deny | `member.denied` | the denier | the request (id) | none |
| vouch | `member.vouched` | the voucher | the request (id) | none |

Submissions and purges are **not** audited: the log is append-only (D59) and cannot be pruned, so a flood from strangers must not be able to grow it. No row holds a handle, a message or a key; a denied request has no account to point at, so its target is the request id.

## Implemented

- `core::queue`: `QueueStore`, `submit_request`, `list_pending`, `approve_request`, `deny_request`, `vouch_request`, `purge_expired`, `execute`, `RequestGate`, `QueueScreen`.
- Store: both backends, one transaction per operation, shared suite checks on both.
- Waiting room: the form hands a valid submission to the front end (`RoomEffect::RequestInvite`); the SSH shell offers it to core with the peer address for the network limit.
- Screens: the two screens above; menu items under Sysop tools and Members.
- Permission `invite.request` (waiting, sysop); `members.deny` and `members.vouch` now have actions.
- **Applications (M2-G):** the table has a `kind` column (`invite` or `application`, migration `0010`); an application is submitted through `core::registration::submit_application` (permission `membership.apply`, the stored mode must be `apply` or `open`) and then is exactly an invite request: one per key, the same cap, limits, expiry, cooldown, vouches and approve/deny. The sysop screen's detail title reads "Application N: handle" for one; the list and the intro say "Invite requests and applications". A kept application writes one `application.submitted` audit row (the request id; no text, no key); an invite request still writes none (D71, D73).

## Not built

- **Moderators cannot reach the screen.** They hold `members.deny` but not `members.approve`, and Sysop tools is gated on `members.approve`. The M5 sysop console decides where the screen lives and who sees it.
- **Notification.** Nobody is told of a decision; the person finds out by connecting again with the same key. There is no mail yet.
- **Editing the cap from a screen** (`queue_max_pending` is a stored setting; M5 sets settings).
- **Rank rule** for approvals (M2-B2 brings rank rules; approving creates a plain member, so there is nothing to escalate).

## Verified

- Core unit tests: permissions and proofs, the no-oracle contract for every drop reason, the rate limits (per key, per network, IPv6 /64, refill), the cap setting and every bad value, the screen state machine, text sanitizing, nothing the requester typed in a `Debug` line.
- The shared store suite on **SQLite and Postgres 16** (plain, and TLS-required): exactly the documented fields; one request per key and the cap under 12 concurrent submissions; approve, deny and vouch with their audit rows and no content; a taken handle, a lookalike and a bound key change nothing; five concurrent approvers plus a denier produce one result; expiry and the 24-hour cooldown with an injected clock, and the purge removes every trace; vouches once per member, active vouchers only; every write fails whole when its audit row cannot be written; reopen; a closed store errors instead of answering.
- A real russh client over SSH (`tests/queue.rs`): the same screen for every drop reason; no address in any table; the waiting room unchanged by the queue; the sysop approves from the screen and the requester signs in as a member; a denied key is turned away with the same words; a trusted member vouches once and the sysop sees the name; each permission taken away removes its key on the very next action.
- Drawing tests at 80x24 and other sizes, hostile requester text, every icon tier.
- **A real OpenSSH client** (OpenSSH 9.6, `ssh -tt`, rendered with `pyte` at 80x24) against the built server binary on SQLite: a stranger asks and reads the fixed answer; a trusted member opens *Members, Vouch for a request* and vouches; the founder opens *Sysop tools, Invite requests*, sees the vouch and approves; the requester reconnects with the same key and is signed in as the new member; a second stranger is denied and, asking again, reads the same screen byte for byte; afterwards the database holds no request, one denial hash, no address, the audit log and the server log hold none of the typed text. 19 checks, all passing.

**Not verified:** other terminal emulators (only OpenSSH under `pyte` was used); a screen reader; two server processes on one database (the caps and the one-per-key rule are enforced in the database, so they should hold, but it was not run that way).
