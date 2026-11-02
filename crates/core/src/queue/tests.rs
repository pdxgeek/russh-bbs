//! The queue rules over an in-memory store: permissions, the person-facing no-oracle contract,
//! the rate limits, the settings, and the screens. The SQL behaviour (caps and uniqueness
//! under concurrency, atomic decisions, expiry with an injected clock, the audit rows) is in
//! the store crate's shared suite, on both backends.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl
)]

use std::sync::Mutex;
use std::time::Duration;

use sha2::{Digest as _, Sha256};

use super::*;
use crate::access::{Fingerprint, Permission, Slug, Snapshot, Via};
use crate::audit::{Action, Actor, Target, events};
use crate::sanitize::{Username, sanitize_username};
use crate::store::{Account, AccountId, AccountState, StoreError, UnixSeconds};
use crate::waiting_room::{ManualClock, RatePolicy};

fn key(n: u32) -> Fingerprint {
    Fingerprint::parse(&format!("SHA256:{n:A>43}")).unwrap()
}

fn handle(name: &str) -> Username {
    sanitize_username(name).unwrap()
}

fn ask(name: &str, message: &str) -> InviteAsk {
    InviteAsk {
        handle: handle(name),
        message: message.to_owned(),
    }
}

fn waiting() -> Snapshot {
    Snapshot::from_role(
        None,
        &Slug::WAITING,
        0,
        &Via::Waiting,
        [Permission::InviteRequest],
    )
}

fn staff(permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(2)),
        &Slug::parse("cosysop").unwrap(),
        40,
        &Via::Account,
        permissions.iter().copied(),
    )
}

#[derive(Default)]
struct Fake {
    submitted: Mutex<Vec<(NewRequest, SubmitTerms)>>,
    answer: Mutex<Option<Result<Submitted, ()>>>,
    setting: Mutex<Option<Result<Option<String>, ()>>>,
    queue: Mutex<Vec<PendingRequest>>,
    approve: Mutex<Option<ApproveError>>,
    decide: Mutex<Option<DecideError>>,
    vouch: Mutex<Option<VouchError>>,
    decisions: Mutex<Vec<String>>,
}

impl QueueStore for Fake {
    async fn submit_request(
        &self,
        new: &NewRequest,
        terms: SubmitTerms,
    ) -> Result<Submitted, StoreError> {
        let mut submitted = self.submitted.lock().unwrap();
        submitted.push((new.clone(), terms));
        let id = RequestId::from_raw(i64::try_from(submitted.len()).unwrap());
        match *self.answer.lock().unwrap() {
            None => Ok(Submitted::Stored(id)),
            Some(Ok(answer)) => Ok(answer),
            Some(Err(())) => Err(StoreError::AccountNotFound),
        }
    }

    async fn pending_requests(&self, _limit: u32) -> Result<Queue, StoreError> {
        Ok(Queue {
            now: UnixSeconds::from_raw(10_000),
            requests: self.queue.lock().unwrap().clone(),
        })
    }

    async fn approve_request(
        &self,
        id: RequestId,
        actor: AccountId,
    ) -> Result<Account, ApproveError> {
        if let Some(error) = self.approve.lock().unwrap().take() {
            return Err(error);
        }
        self.decisions
            .lock()
            .unwrap()
            .push(format!("approve {id} by {actor}"));
        Ok(Account {
            id: AccountId::from_raw(9),
            username: handle("newcomer"),
            state: AccountState::Active,
            created_at: UnixSeconds::from_raw(0),
        })
    }

    async fn deny_request(
        &self,
        id: RequestId,
        actor: AccountId,
        cooldown_secs: i64,
    ) -> Result<(), DecideError> {
        if let Some(error) = self.decide.lock().unwrap().take() {
            return Err(error);
        }
        self.decisions
            .lock()
            .unwrap()
            .push(format!("deny {id} by {actor} for {cooldown_secs}"));
        Ok(())
    }

    async fn vouch_request(&self, id: RequestId, voucher: AccountId) -> Result<(), VouchError> {
        if let Some(error) = self.vouch.lock().unwrap().take() {
            return Err(error);
        }
        self.decisions
            .lock()
            .unwrap()
            .push(format!("vouch {id} by {voucher}"));
        Ok(())
    }

    async fn max_pending_setting(&self) -> Result<Option<String>, StoreError> {
        match self.setting.lock().unwrap().clone() {
            None => Ok(None),
            Some(Ok(value)) => Ok(value),
            Some(Err(())) => Err(StoreError::AccountNotFound),
        }
    }

    async fn purge_expired(&self) -> Result<Purged, StoreError> {
        Ok(Purged::default())
    }
}

fn gate(clock: &ManualClock) -> RequestGate<ManualClock> {
    RequestGate::with_rates(
        RatePolicy::new(3, Duration::from_secs(600)),
        RatePolicy::new(5, Duration::from_secs(120)),
        100,
        clock,
    )
}

fn peer(last: u8) -> std::net::IpAddr {
    std::net::IpAddr::from([192, 0, 2, last])
}

// ---- the data -----------------------------------------------------------------------------

#[test]
fn the_cooldown_key_is_a_domain_separated_hash_that_holds_no_fingerprint() {
    let a = cooldown_key(&key(1));
    assert_eq!(a.len(), 64);
    assert!(
        a.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert_eq!(a, cooldown_key(&key(1)), "deterministic");
    assert_ne!(a, cooldown_key(&key(2)));
    let fingerprint = key(1);
    assert!(!a.contains(fingerprint.as_str().trim_start_matches("SHA256:")));
    // Not the bare SHA-256 of the fingerprint text: the domain prefix is part of it.
    let bare =
        Sha256::digest(fingerprint.as_str().as_bytes())
            .iter()
            .fold(String::new(), |mut out, b| {
                use std::fmt::Write as _;
                let _ = write!(out, "{b:02x}");
                out
            });
    assert_ne!(a, bare);
}

#[test]
fn a_message_is_sanitized_capped_and_one_line() {
    let hostile = format!(
        "hi\u{1b}[2J\u{7}\r\nthere\u{e0b0}\u{301a}{}",
        "x".repeat(1_000)
    );
    let message = Message::new(&hostile);
    let text = message.as_str();
    assert!(text.chars().count() <= MESSAGE_MAX_CHARS);
    assert!(!text.contains('\u{1b}') && !text.contains('\u{7}'));
    assert!(!text.contains('\n') && !text.contains('\r'));
    assert!(!text.contains('\u{e0b0}'), "Nerd Font range is neutralized");
    assert!(text.starts_with("hi"));
}

#[test]
fn nothing_the_requester_typed_is_in_a_debug_line() {
    let new = NewRequest {
        handle: handle("secretname"),
        message: Message::new("secret message"),
        fingerprint: key(7),
    };
    for shown in [
        format!("{new:?}"),
        format!("{:?}", ask("secretname", "secret message")),
        format!("{:?}", new.message),
    ] {
        assert!(!shown.contains("secret"), "{shown}");
    }
}

#[test]
fn the_documented_numbers_are_these() {
    assert_eq!(PENDING_TTL_SECS, 14 * 86_400);
    assert_eq!(DENIAL_COOLDOWN_SECS, 86_400);
    assert_eq!(DEFAULT_MAX_PENDING, 200);
    assert_eq!(MESSAGE_MAX_CHARS, 280);
    const { assert!(DEFAULT_MAX_PENDING <= MAX_MAX_PENDING) };
}

#[test]
fn audit_events_carry_ids_and_no_content() {
    let id = RequestId::from_raw(12);
    let denied = events::member_denied(AccountId::from_raw(2), id);
    assert_eq!(denied.action, Action::MemberDenied);
    assert_eq!(denied.target, Target::Request(id));
    assert_eq!(denied.actor, Actor::Account(AccountId::from_raw(2)));
    assert_eq!(denied.detail.as_str(), "");
    let vouched = events::member_vouched(AccountId::from_raw(3), id);
    assert_eq!(vouched.action, Action::MemberVouched);
    assert_eq!(vouched.detail.as_str(), "");
    let approved = events::member_approved(AccountId::from_raw(2), AccountId::from_raw(9), id);
    assert_eq!(approved.target, Target::Account(AccountId::from_raw(9)));
    assert_eq!(approved.detail.as_str(), "request 12");
    // A request target survives the stored columns.
    let (kind, reference) = Target::Request(id).parts();
    assert_eq!((kind, reference.as_str()), ("request", "12"));
    assert_eq!(
        Target::from_parts(kind, &reference),
        Some(Target::Request(id))
    );
    assert_eq!(Target::from_parts("request", "twelve"), None);
}

// ---- submitting ---------------------------------------------------------------------------

#[tokio::test]
async fn a_request_needs_invite_request_and_is_stored_with_the_documented_terms() {
    let store = Fake::default();
    let clock = ManualClock::new();
    let gate = gate(&clock);

    let none = Snapshot::empty();
    let outcome = submit_request(
        &store,
        &gate,
        &none,
        ask("alice", "hi"),
        &key(1),
        Some(peer(1)),
    )
    .await;
    assert_eq!(outcome, Submission::Dropped(DropReason::NotPermitted));
    assert!(
        store.submitted.lock().unwrap().is_empty(),
        "nothing written"
    );

    let outcome = submit_request(
        &store,
        &gate,
        &waiting(),
        ask("alice", "hi"),
        &key(1),
        Some(peer(1)),
    )
    .await;
    assert!(matches!(outcome, Submission::Queued(_)));
    let submitted = store.submitted.lock().unwrap();
    let (new, terms) = &submitted[0];
    assert_eq!(new.handle.as_str(), "alice");
    assert_eq!(new.message.as_str(), "hi");
    assert_eq!(new.fingerprint, key(1));
    assert_eq!(terms.max_pending, DEFAULT_MAX_PENDING);
    assert_eq!(terms.ttl_secs, PENDING_TTL_SECS);
}

#[tokio::test]
async fn the_per_key_limit_refills_and_one_key_cannot_dodge_it_by_changing_network() {
    let store = Fake::default();
    let clock = ManualClock::new();
    let gate = gate(&clock);
    for n in 0..3 {
        let outcome = submit_request(
            &store,
            &gate,
            &waiting(),
            ask("alice", "hi"),
            &key(1),
            Some(peer(n + 1)),
        )
        .await;
        assert!(matches!(outcome, Submission::Queued(_)), "{n}");
    }
    let outcome = submit_request(
        &store,
        &gate,
        &waiting(),
        ask("alice", "hi"),
        &key(1),
        Some(peer(99)),
    )
    .await;
    assert_eq!(
        outcome,
        Submission::Dropped(DropReason::RateLimited(SubmitLimited::Key))
    );
    assert_eq!(store.submitted.lock().unwrap().len(), 3, "not stored");
    // Another key is unaffected; the first earns a slot back after ten minutes.
    let other = submit_request(
        &store,
        &gate,
        &waiting(),
        ask("bob", "hi"),
        &key(2),
        Some(peer(50)),
    )
    .await;
    assert!(matches!(other, Submission::Queued(_)));
    clock.advance(Duration::from_secs(601));
    let again = submit_request(
        &store,
        &gate,
        &waiting(),
        ask("alice", "hi"),
        &key(1),
        Some(peer(51)),
    )
    .await;
    assert!(matches!(again, Submission::Queued(_)));
}

#[tokio::test]
async fn the_per_network_limit_covers_many_keys_and_ipv6_counts_as_its_64() {
    let store = Fake::default();
    let clock = ManualClock::new();
    let gate = gate(&clock);
    for n in 0..5 {
        let outcome = submit_request(
            &store,
            &gate,
            &waiting(),
            ask("alice", "hi"),
            &key(100 + n),
            Some(peer(7)),
        )
        .await;
        assert!(matches!(outcome, Submission::Queued(_)), "{n}");
    }
    let outcome = submit_request(
        &store,
        &gate,
        &waiting(),
        ask("alice", "hi"),
        &key(200),
        Some(peer(7)),
    )
    .await;
    assert_eq!(
        outcome,
        Submission::Dropped(DropReason::RateLimited(SubmitLimited::Network))
    );
    let a: std::net::IpAddr = "2001:db8:1:2::1".parse().unwrap();
    let b: std::net::IpAddr = "2001:db8:1:2:ffff::9".parse().unwrap();
    let c: std::net::IpAddr = "2001:db8:1:3::1".parse().unwrap();
    let gate = gate_of(&clock);
    for n in 0..5 {
        assert!(gate.admit(&key(300 + n), Some(a)).is_ok());
    }
    assert_eq!(gate.admit(&key(400), Some(b)), Err(SubmitLimited::Network));
    assert!(gate.admit(&key(401), Some(c)).is_ok(), "another /64");
    assert!(
        gate.admit(&key(402), None).is_ok(),
        "no address, no network limit"
    );
}

fn gate_of(clock: &ManualClock) -> RequestGate<ManualClock> {
    gate(clock)
}

#[tokio::test]
async fn every_store_outcome_is_a_drop_the_person_cannot_tell_apart() {
    let clock = ManualClock::new();
    for answer in [
        Submitted::Refused(Refusal::AlreadyPending),
        Submitted::Refused(Refusal::QueueFull),
        Submitted::Refused(Refusal::Cooling),
        Submitted::Refused(Refusal::KeyHasAccount),
    ] {
        let store = Fake::default();
        *store.answer.lock().unwrap() = Some(Ok(answer));
        let outcome = submit_request(
            &store,
            &gate(&clock),
            &waiting(),
            ask("alice", "hi"),
            &key(1),
            None,
        )
        .await;
        let Submission::Dropped(DropReason::Refused(_)) = outcome else {
            panic!("{outcome:?}");
        };
    }
    let store = Fake::default();
    *store.answer.lock().unwrap() = Some(Err(()));
    let outcome = submit_request(
        &store,
        &gate(&clock),
        &waiting(),
        ask("alice", "hi"),
        &key(1),
        None,
    )
    .await;
    assert_eq!(outcome, Submission::Dropped(DropReason::Store));
}

#[tokio::test]
async fn the_cap_is_a_setting_with_a_default_and_a_bad_value_never_removes_it() {
    for (stored, want) in [
        (None, DEFAULT_MAX_PENDING),
        (Some(Ok(Some("5".to_owned()))), 5),
        (Some(Ok(Some(" 40 ".to_owned()))), 40),
        (Some(Ok(Some("10000".to_owned()))), MAX_MAX_PENDING),
        (Some(Ok(Some("0".to_owned()))), DEFAULT_MAX_PENDING),
        (Some(Ok(Some("10001".to_owned()))), DEFAULT_MAX_PENDING),
        (Some(Ok(Some("-3".to_owned()))), DEFAULT_MAX_PENDING),
        (Some(Ok(Some("many".to_owned()))), DEFAULT_MAX_PENDING),
        (Some(Ok(Some(String::new()))), DEFAULT_MAX_PENDING),
        (Some(Err(())), DEFAULT_MAX_PENDING),
    ] {
        let store = Fake::default();
        *store.setting.lock().unwrap() = stored.clone();
        assert_eq!(max_pending(&store).await, want, "{stored:?}");
    }
}

// ---- the staff actions --------------------------------------------------------------------

fn request(id: i64, name: &str, vouchers: &[(i64, &str)]) -> PendingRequest {
    PendingRequest {
        id: RequestId::from_raw(id),
        handle: handle(name),
        message: Message::new("a friend"),
        created_at: UnixSeconds::from_raw(10_000 - 7_300),
        expires_at: UnixSeconds::from_raw(10_000 + 86_400),
        vouchers: vouchers
            .iter()
            .map(|(account, name)| Voucher {
                account: AccountId::from_raw(*account),
                handle: handle(name),
            })
            .collect(),
    }
}

#[tokio::test]
async fn listing_needs_one_of_the_three_staff_permissions() {
    let store = Fake::default();
    for permission in [
        Permission::MembersApprove,
        Permission::MembersDeny,
        Permission::MembersVouch,
    ] {
        assert!(
            list_pending(&store, &staff(&[permission])).await.is_ok(),
            "{}",
            permission.name()
        );
    }
    for access in [
        staff(&[Permission::MembersView, Permission::InvitesCreate]),
        Snapshot::empty(),
        waiting(),
    ] {
        assert!(matches!(
            list_pending(&store, &access).await,
            Err(QueueError::NotPermitted)
        ));
    }
}

#[tokio::test]
async fn each_decision_needs_its_own_proof_and_names_the_actor() {
    let store = Fake::default();
    let id = RequestId::from_raw(4);
    let access = staff(&[
        Permission::MembersApprove,
        Permission::MembersDeny,
        Permission::MembersVouch,
    ]);
    let approve = access.require(Permission::MembersApprove, None).unwrap();
    let deny = access.require(Permission::MembersDeny, None).unwrap();
    let vouch = access.require(Permission::MembersVouch, None).unwrap();

    // A proof for another permission is refused and nothing is written.
    assert!(matches!(
        approve_request(&store, &deny, id).await,
        Err(QueueError::NotPermitted)
    ));
    assert!(matches!(
        deny_request(&store, &vouch, id).await,
        Err(QueueError::NotPermitted)
    ));
    assert!(matches!(
        vouch_request(&store, &approve, id).await,
        Err(QueueError::NotPermitted)
    ));
    assert!(store.decisions.lock().unwrap().is_empty());

    let done = approve_request(&store, &approve, id).await.unwrap();
    assert_eq!(done.account.state, AccountState::Active);
    deny_request(&store, &deny, id).await.unwrap();
    vouch_request(&store, &vouch, id).await.unwrap();
    assert_eq!(
        *store.decisions.lock().unwrap(),
        [
            "approve 4 by 2".to_owned(),
            format!("deny 4 by 2 for {DENIAL_COOLDOWN_SECS}"),
            "vouch 4 by 2".to_owned(),
        ]
    );
}

#[tokio::test]
async fn execute_checks_the_permission_and_maps_every_failure() {
    let id = RequestId::from_raw(4);
    let store = Fake::default();
    let nothing = staff(&[Permission::MembersView]);
    for request in [
        Request::List,
        Request::Approve(id),
        Request::Deny(id),
        Request::Vouch(id),
    ] {
        assert_eq!(
            execute(&store, &nothing, request).await,
            Response::NotPermitted,
            "{request:?}"
        );
    }
    assert!(store.decisions.lock().unwrap().is_empty());

    let all = staff(&[
        Permission::MembersApprove,
        Permission::MembersDeny,
        Permission::MembersVouch,
    ]);
    *store.approve.lock().unwrap() = Some(ApproveError::HandleTaken);
    assert_eq!(
        execute(&store, &all, Request::Approve(id)).await,
        Response::HandleTaken
    );
    *store.approve.lock().unwrap() = Some(ApproveError::KeyHasAccount);
    assert_eq!(
        execute(&store, &all, Request::Approve(id)).await,
        Response::KeyHasAccount
    );
    *store.approve.lock().unwrap() = Some(ApproveError::NotFound);
    assert_eq!(
        execute(&store, &all, Request::Approve(id)).await,
        Response::Gone
    );
    *store.decide.lock().unwrap() = Some(DecideError::NotFound);
    assert_eq!(
        execute(&store, &all, Request::Deny(id)).await,
        Response::Gone
    );
    *store.vouch.lock().unwrap() = Some(VouchError::AlreadyVouched);
    assert_eq!(
        execute(&store, &all, Request::Vouch(id)).await,
        Response::AlreadyVouched
    );
    *store.vouch.lock().unwrap() = Some(VouchError::Store(StoreError::AccountNotFound));
    assert_eq!(
        execute(&store, &all, Request::Vouch(id)).await,
        Response::Failed
    );
    assert_eq!(
        execute(&store, &all, Request::Approve(id)).await,
        Response::Approved(handle("newcomer"))
    );
}

// ---- the screens --------------------------------------------------------------------------

fn opened(mode: QueueMode, requests: Vec<PendingRequest>) -> QueueScreen {
    let mut screen = QueueScreen::open(mode, Some(AccountId::from_raw(2)));
    assert_eq!(screen.take_request(), Some(Request::List));
    assert_eq!(screen.take_request(), None, "a request is taken once");
    let step = screen.deliver(Response::Queue(Queue {
        now: UnixSeconds::from_raw(10_000),
        requests,
    }));
    assert_eq!(step, QueueStep::Stay);
    screen
}

fn press(screen: &mut QueueScreen, access: &Snapshot, c: char) -> QueueStep {
    screen.press(access, QueueKey::Char(c))
}

#[test]
fn keys_are_ignored_until_the_first_answer_except_leaving() {
    let access = staff(&[Permission::MembersApprove, Permission::MembersDeny]);
    let mut screen = QueueScreen::open(QueueMode::Sysop, None);
    assert_eq!(press(&mut screen, &access, '1'), QueueStep::Stay);
    assert_eq!(screen.press(&access, QueueKey::Escape), QueueStep::Leave);
}

#[test]
fn an_unread_queue_is_not_shown_as_empty() {
    let access = staff(&[Permission::MembersApprove]);
    let mut screen = QueueScreen::open(QueueMode::Sysop, None);
    screen.take_request();
    screen.deliver(Response::Failed);
    let view = screen.view(&access);
    assert!(view.lines.contains(&ViewLine::Text(keys::UNREAD)));
    assert!(!view.lines.contains(&ViewLine::Text(keys::EMPTY)));
    assert!(view.lines.contains(&ViewLine::Notice {
        error: true,
        text: keys::NOTICE_FAILED
    }));
    let empty = opened(QueueMode::Sysop, Vec::new());
    assert!(
        empty
            .view(&access)
            .lines
            .contains(&ViewLine::Text(keys::EMPTY))
    );
}

#[test]
fn the_list_numbers_rows_and_pages_four_at_a_time() {
    let access = staff(&[Permission::MembersApprove, Permission::MembersDeny]);
    let rows: Vec<_> = (1..=6)
        .map(|n| request(n, &format!("person{n}"), &[]))
        .collect();
    let mut screen = opened(QueueMode::Sysop, rows);
    let entries = |screen: &QueueScreen| -> Vec<(char, String)> {
        screen
            .view(&access)
            .lines
            .into_iter()
            .filter_map(|line| match line {
                ViewLine::Entry(row) => Some((row.hotkey, row.handle)),
                _ => None,
            })
            .collect()
    };
    assert_eq!(entries(&screen).len(), PAGE_SIZE);
    assert_eq!(entries(&screen)[0], ('1', "person1".to_owned()));
    press(&mut screen, &access, 'n');
    assert_eq!(
        entries(&screen),
        [('1', "person5".to_owned()), ('2', "person6".to_owned())]
    );
    press(&mut screen, &access, 'n');
    assert_eq!(entries(&screen).len(), 2, "no page past the last");
    press(&mut screen, &access, 'p');
    assert_eq!(entries(&screen).len(), PAGE_SIZE);
    // A digit with no row does nothing; '5' is not a row on a four-row page.
    press(&mut screen, &access, '5');
    assert!(matches!(
        screen.view(&access).lines[0],
        ViewLine::Heading(_)
    ));
    assert_eq!(press(&mut screen, &access, 'q'), QueueStep::Leave);
}

#[test]
fn a_row_shows_age_vouches_and_whether_one_is_mine() {
    let access = staff(&[Permission::MembersApprove]);
    let screen = opened(
        QueueMode::Sysop,
        vec![request(1, "alice", &[(2, "me"), (3, "carol")])],
    );
    let ViewLine::Entry(row) = screen
        .view(&access)
        .lines
        .into_iter()
        .find(|l| matches!(l, ViewLine::Entry(_)))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(row.age, "2h");
    assert_eq!(row.vouches, 2);
    assert!(row.mine);
    assert_eq!(row.message, "a friend");
}

#[test]
fn the_sysop_screen_offers_approve_and_deny_and_the_vouch_screen_only_vouch() {
    let all = staff(&[
        Permission::MembersApprove,
        Permission::MembersDeny,
        Permission::MembersVouch,
    ]);
    let offered = |mode, access: &Snapshot| -> Vec<&'static str> {
        let mut screen = opened(mode, vec![request(1, "alice", &[])]);
        press(&mut screen, access, '1');
        screen
            .view(access)
            .lines
            .into_iter()
            .find_map(|line| match line {
                ViewLine::Actions(keys_on_offer) => Some(keys_on_offer),
                _ => None,
            })
            .unwrap_or_default()
    };
    assert_eq!(
        offered(QueueMode::Sysop, &all),
        [keys::ACTION_APPROVE, keys::ACTION_DENY, keys::ACTION_VOUCH]
    );
    assert_eq!(offered(QueueMode::Vouch, &all), [keys::ACTION_VOUCH]);
    // Only what is held is offered, and a key for something not offered does nothing.
    let deny_only = staff(&[Permission::MembersDeny]);
    assert_eq!(offered(QueueMode::Sysop, &deny_only), [keys::ACTION_DENY]);
    let mut screen = opened(QueueMode::Sysop, vec![request(1, "alice", &[])]);
    press(&mut screen, &deny_only, '1');
    press(&mut screen, &deny_only, 'a');
    assert!(screen.take_request().is_none(), "approve is not offered");
    assert!(!screen.view(&deny_only).lines.iter().any(|l| matches!(
        l,
        ViewLine::Filled {
            key: keys::CONFIRM_APPROVE,
            ..
        }
    )));
}

#[test]
fn a_member_cannot_vouch_twice_and_is_not_offered_the_key() {
    let access = staff(&[Permission::MembersVouch]);
    let mut screen = opened(QueueMode::Vouch, vec![request(1, "alice", &[(2, "me")])]);
    press(&mut screen, &access, '1');
    press(&mut screen, &access, 'v');
    assert!(screen.take_request().is_none());
    assert!(
        !screen
            .view(&access)
            .lines
            .iter()
            .any(|line| matches!(line, ViewLine::Actions(_))),
        "nothing is on offer"
    );
}

#[test]
fn a_decision_is_confirmed_and_only_y_sends_it() {
    let access = staff(&[Permission::MembersApprove, Permission::MembersDeny]);
    let mut screen = opened(QueueMode::Sysop, vec![request(7, "alice", &[])]);
    press(&mut screen, &access, '1');
    press(&mut screen, &access, 'd');
    assert!(screen.take_request().is_none(), "asked first");
    let confirm = screen.view(&access);
    assert_eq!(confirm.footer, keys::FOOTER_CONFIRM);
    assert!(confirm.lines.iter().any(|l| matches!(
        l,
        ViewLine::Filled { key: keys::CONFIRM_DENY, values }
            if values.contains(&("handle", "alice".to_owned()))
                && values.contains(&("hours", "24".to_owned()))
    )));
    // Anything but y cancels and sends nothing.
    press(&mut screen, &access, 'n');
    assert!(screen.take_request().is_none());
    press(&mut screen, &access, 'a');
    press(&mut screen, &access, 'y');
    assert_eq!(
        screen.take_request(),
        Some(Request::Approve(RequestId::from_raw(7)))
    );
    // Keys do nothing while the answer is outstanding.
    assert_eq!(press(&mut screen, &access, 'y'), QueueStep::Stay);
    assert_eq!(screen.take_request(), None);
}

#[test]
fn after_a_decision_the_list_is_read_again_with_a_notice_that_the_next_key_clears() {
    let access = staff(&[Permission::MembersApprove, Permission::MembersDeny]);
    let mut screen = opened(QueueMode::Sysop, vec![request(7, "alice", &[])]);
    press(&mut screen, &access, '1');
    press(&mut screen, &access, 'a');
    press(&mut screen, &access, 'y');
    screen.take_request();
    screen.deliver(Response::Approved(handle("alice")));
    assert_eq!(screen.take_request(), Some(Request::List));
    assert!(screen.view(&access).lines.contains(&ViewLine::Notice {
        error: false,
        text: keys::NOTICE_APPROVED
    }));
    screen.deliver(Response::Queue(Queue {
        now: UnixSeconds::from_raw(10_000),
        requests: Vec::new(),
    }));
    assert!(
        screen
            .view(&access)
            .lines
            .contains(&ViewLine::Text(keys::EMPTY))
    );
    press(&mut screen, &access, 'x');
    assert!(
        !screen
            .view(&access)
            .lines
            .iter()
            .any(|l| matches!(l, ViewLine::Notice { .. }))
    );
}

#[test]
fn refusals_keep_the_detail_open_and_a_gone_request_returns_to_the_list() {
    let access = staff(&[Permission::MembersApprove, Permission::MembersDeny]);
    for (response, notice) in [
        (Response::HandleTaken, keys::NOTICE_HANDLE_TAKEN),
        (Response::KeyHasAccount, keys::NOTICE_KEY_HAS_ACCOUNT),
        (Response::AlreadyVouched, keys::NOTICE_ALREADY_VOUCHED),
    ] {
        let mut screen = opened(QueueMode::Sysop, vec![request(7, "alice", &[])]);
        press(&mut screen, &access, '1');
        press(&mut screen, &access, 'a');
        press(&mut screen, &access, 'y');
        screen.take_request();
        assert_eq!(screen.deliver(response), QueueStep::Stay);
        let view = screen.view(&access);
        assert!(view.lines.contains(&ViewLine::Notice {
            error: true,
            text: notice
        }));
        assert_eq!(view.footer, keys::FOOTER_DETAIL, "still on the request");
    }
    let mut screen = opened(QueueMode::Sysop, vec![request(7, "alice", &[])]);
    press(&mut screen, &access, '1');
    press(&mut screen, &access, 'd');
    press(&mut screen, &access, 'y');
    screen.take_request();
    screen.deliver(Response::Gone);
    assert_eq!(screen.take_request(), Some(Request::List));
    assert!(screen.view(&access).lines.contains(&ViewLine::Notice {
        error: true,
        text: keys::NOTICE_GONE
    }));
}

#[test]
fn a_request_decided_elsewhere_closes_the_detail_when_the_list_reloads() {
    let access = staff(&[Permission::MembersApprove]);
    let mut screen = opened(
        QueueMode::Sysop,
        vec![request(7, "alice", &[]), request(8, "bob", &[])],
    );
    press(&mut screen, &access, '1');
    // A reload with request 7 gone shows the list, not a detail of nothing.
    screen.deliver(Response::Queue(Queue {
        now: UnixSeconds::from_raw(10_000),
        requests: vec![request(8, "bob", &[])],
    }));
    assert_eq!(screen.view(&access).footer, keys::FOOTER_LIST);
}

#[test]
fn not_permitted_leaves_and_the_screen_follows_the_permissions_held_now() {
    let mut screen = opened(QueueMode::Sysop, Vec::new());
    screen.take_request();
    assert_eq!(screen.deliver(Response::NotPermitted), QueueStep::Denied);
    let both = staff(&[Permission::MembersApprove]);
    assert!(screen.still_permitted(&both));
    assert!(!screen.still_permitted(&staff(&[Permission::MembersVouch])));
    let vouch = QueueScreen::open(QueueMode::Vouch, None);
    assert!(vouch.still_permitted(&staff(&[Permission::MembersVouch])));
    assert!(!vouch.still_permitted(&staff(&[
        Permission::MembersApprove,
        Permission::MembersDeny
    ])));
}

#[test]
fn the_detail_shows_requester_text_only_as_content_and_never_a_key() {
    let access = staff(&[Permission::MembersApprove]);
    let mut screen = opened(QueueMode::Sysop, vec![request(7, "alice", &[(3, "carol")])]);
    press(&mut screen, &access, '1');
    let view = screen.view(&access);
    let mut content = Vec::new();
    for line in &view.lines {
        match line {
            ViewLine::Pair {
                value,
                content: true,
                ..
            } => content.push(value.as_str()),
            ViewLine::Filled {
                key: keys::DETAIL_TITLE,
                values,
            } => {
                content.extend(values.iter().map(|(_, value)| value.as_str()));
            }
            _ => {}
        }
    }
    assert!(content.contains(&"alice"));
    assert!(content.contains(&"a friend"));
    assert!(content.contains(&"carol"));
    assert!(!format!("{view:?}").contains("SHA256"));
}

#[test]
fn the_detail_lists_two_vouchers_and_counts_the_rest() {
    let access = staff(&[Permission::MembersApprove]);
    let vouchers: Vec<(i64, String)> = (10..16).map(|n| (n, format!("voucher{n}"))).collect();
    let refs: Vec<(i64, &str)> = vouchers.iter().map(|(n, s)| (*n, s.as_str())).collect();
    let mut screen = opened(QueueMode::Sysop, vec![request(7, "alice", &refs)]);
    press(&mut screen, &access, '1');
    let shown = screen
        .view(&access)
        .lines
        .into_iter()
        .find_map(|line| match line {
            ViewLine::Pair {
                label: keys::DETAIL_VOUCHES,
                value,
                ..
            } => Some(value),
            _ => None,
        });
    assert_eq!(shown.as_deref(), Some("voucher10, voucher11, +4"));
}

#[test]
fn ages_read_as_the_largest_whole_unit() {
    let at = |s| UnixSeconds::from_raw(s);
    assert_eq!(age_text(at(1_000), at(1_000)), "0m");
    assert_eq!(age_text(at(1_059), at(1_000)), "0m");
    assert_eq!(age_text(at(1_060), at(1_000)), "1m");
    assert_eq!(age_text(at(1_000 + 3_599), at(1_000)), "59m");
    assert_eq!(age_text(at(1_000 + 3_600), at(1_000)), "1h");
    assert_eq!(age_text(at(1_000 + 86_399), at(1_000)), "23h");
    assert_eq!(age_text(at(1_000 + 86_400 * 3), at(1_000)), "3d");
    assert_eq!(
        age_text(at(1_000), at(2_000)),
        "0m",
        "a clock that moved back"
    );
}

#[test]
fn every_string_key_is_unique_and_named_queue() {
    let mut seen = std::collections::BTreeSet::new();
    for key in keys::TEXT_KEYS {
        assert!(key.starts_with("queue."), "{key}");
        assert!(seen.insert(*key), "{key} twice");
    }
}
