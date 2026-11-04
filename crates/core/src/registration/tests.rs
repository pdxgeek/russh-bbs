//! The registration rules over an in-memory store: the cap setting, the permission, mode and
//! rate-limit checks in front of the store, and that a refusal never says why. The SQL
//! behaviour (the cap under concurrency, the day boundary with an injected clock, the audit
//! rows, atomicity) is in the store crate's shared suite, on both backends.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl
)]

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::time::Duration;

use super::*;
use crate::access::{Fingerprint, NoMembers, Permission, Slug, Snapshot, Via};
use crate::queue::{DropReason, InviteAsk, RequestGate, RequestKind, Submission};
use crate::sanitize::{Username, sanitize_username};
use crate::store::{Account, AccountId, AccountState, RegistrationMode, StoreError, UnixSeconds};
use crate::waiting_room::{ManualClock, RatePolicy};

fn key(n: u32) -> Fingerprint {
    Fingerprint::parse(&format!("SHA256:{n:A>43}")).unwrap()
}

fn handle(name: &str) -> Username {
    sanitize_username(name).unwrap()
}

fn waiting(permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        None,
        &Slug::WAITING,
        0,
        &Via::Waiting,
        permissions.iter().copied(),
    )
}

fn everything() -> Snapshot {
    waiting(&[Permission::MembershipOpen, Permission::MembershipApply])
}

#[allow(clippy::unnecessary_wraps)]
fn peer(n: u8) -> Option<IpAddr> {
    Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, n)))
}

#[derive(Default)]
struct Fake {
    mode: Mutex<Option<RegistrationMode>>,
    answer: Mutex<Option<Result<(), OpenError>>>,
    calls: Mutex<Vec<(String, DailyCap)>>,
    setting: Mutex<Option<Result<Option<String>, ()>>>,
    password_hashes: Mutex<Vec<String>>,
}

impl Fake {
    fn in_mode(mode: RegistrationMode) -> Self {
        let fake = Self::default();
        *fake.mode.lock().unwrap() = Some(mode);
        fake
    }

    fn calls(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl RegistrationStore for Fake {
    async fn registration_mode(&self) -> Result<RegistrationMode, StoreError> {
        self.mode
            .lock()
            .unwrap()
            .ok_or_else(|| StoreError::Corrupt("no mode".into()))
    }

    async fn open_cap_setting(&self) -> Result<Option<String>, StoreError> {
        match self.setting.lock().unwrap().clone() {
            None => Ok(None),
            Some(Ok(value)) => Ok(value),
            Some(Err(())) => Err(StoreError::Corrupt("unreadable".into())),
        }
    }

    async fn open_cap_reached(&self, _cap: DailyCap) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn open_account(
        &self,
        username: Username,
        _fingerprint: &Fingerprint,
        cap: DailyCap,
    ) -> Result<Opened, OpenError> {
        self.calls
            .lock()
            .unwrap()
            .push((username.as_str().to_owned(), cap));
        match self.answer.lock().unwrap().take() {
            Some(Err(error)) => Err(error),
            Some(Ok(())) | None => Ok(Opened {
                account: Account {
                    id: AccountId::from_raw(1),
                    username,
                    state: AccountState::Active,
                    created_at: UnixSeconds::from_raw(0),
                },
            }),
        }
    }

    async fn open_account_password(
        &self,
        username: Username,
        password_hash: &str,
        cap: DailyCap,
    ) -> Result<Opened, OpenError> {
        self.password_hashes
            .lock()
            .unwrap()
            .push(password_hash.to_owned());
        self.open_account(username, &key(0), cap).await
    }
}

fn gate(clock: &ManualClock) -> OpenGate<ManualClock> {
    OpenGate::with_rates(
        RatePolicy::new(2, Duration::from_secs(3600)),
        RatePolicy::new(3, Duration::from_secs(600)),
        100,
        clock,
    )
}

// ---- the cap setting -----------------------------------------------------------------------

#[test]
fn the_cap_defaults_to_25_and_zero_is_the_only_way_to_turn_it_off() {
    assert_eq!(DEFAULT_OPEN_DAILY_CAP, 25);
    assert_eq!(DailyCap::from_setting(None), DailyCap::Limited(25));
    assert_eq!(DailyCap::from_setting(Some("7")), DailyCap::Limited(7));
    assert_eq!(
        DailyCap::from_setting(Some(" 100000 ")),
        DailyCap::Limited(100_000)
    );
    assert_eq!(DailyCap::from_setting(Some("0")), DailyCap::Unlimited);
    assert_eq!(DailyCap::Unlimited.limit(), None);
    assert_eq!(DailyCap::Limited(7).limit(), Some(7));
}

#[test]
fn a_bad_cap_value_reads_as_the_default_never_as_no_cap() {
    for bad in [
        "",
        "  ",
        "-1",
        "abc",
        "1.5",
        "100001",
        "4294967296",
        "99999999999999999999",
        "٣",
        "unlimited",
        "none",
        "off",
    ] {
        assert_eq!(
            DailyCap::from_setting(Some(bad)),
            DailyCap::Limited(DEFAULT_OPEN_DAILY_CAP),
            "{bad:?}"
        );
        assert_eq!(DailyCap::parse(bad), None, "{bad:?}");
    }
}

#[tokio::test]
async fn an_unreadable_cap_is_the_default() {
    let store = Fake::in_mode(RegistrationMode::Open);
    *store.setting.lock().unwrap() = Some(Err(()));
    assert_eq!(daily_cap(&store).await, DailyCap::Limited(25));
    *store.setting.lock().unwrap() = Some(Ok(Some("3".into())));
    assert_eq!(daily_cap(&store).await, DailyCap::Limited(3));
    *store.setting.lock().unwrap() = Some(Ok(Some("lots".into())));
    assert_eq!(daily_cap(&store).await, DailyCap::Limited(25));
}

// ---- open_account --------------------------------------------------------------------------

#[tokio::test]
async fn open_registration_needs_the_permission_the_mode_and_the_gate() {
    let clock = ManualClock::new();
    let open = gate(&clock);

    // No permission: the store is not asked.
    let store = Fake::in_mode(RegistrationMode::Open);
    let denied = open_account(
        &store,
        &open,
        &waiting(&[]),
        handle("newcomer"),
        &key(1),
        peer(1),
    )
    .await;
    assert!(matches!(
        denied,
        Err(OpenError::Rejected(OpenRefusal::NotPermitted))
    ));
    assert_eq!(store.calls(), 0);

    // Any mode but `open` (it was changed after the room was built): the store is not asked.
    for mode in [RegistrationMode::Invite, RegistrationMode::Apply] {
        let store = Fake::in_mode(mode);
        let refused = open_account(
            &store,
            &open,
            &everything(),
            handle("newcomer"),
            &key(2),
            peer(2),
        )
        .await;
        assert!(
            matches!(refused, Err(OpenError::Rejected(OpenRefusal::NotOpen))),
            "{mode:?}"
        );
        assert_eq!(store.calls(), 0);
    }

    // A mode that cannot be read is an error, not "open".
    let broken = Fake::default();
    let failed = open_account(
        &broken,
        &open,
        &everything(),
        handle("newcomer"),
        &key(3),
        peer(3),
    )
    .await;
    assert!(matches!(failed, Err(OpenError::Store(_))));
    assert_eq!(broken.calls(), 0);

    // Everything in order: the store is asked once, with the stored cap.
    let store = Fake::in_mode(RegistrationMode::Open);
    *store.setting.lock().unwrap() = Some(Ok(Some("4".into())));
    let made = open_account(
        &store,
        &open,
        &everything(),
        handle("newcomer"),
        &key(4),
        peer(4),
    )
    .await
    .unwrap();
    assert_eq!(made.account.state, AccountState::Active);
    assert_eq!(
        store.calls.lock().unwrap().as_slice(),
        &[("newcomer".to_owned(), DailyCap::Limited(4))]
    );
}

#[tokio::test]
async fn the_key_gate_allows_a_burst_of_two_then_one_an_hour() {
    let clock = ManualClock::new();
    let open = gate(&clock);
    let store = Fake::in_mode(RegistrationMode::Open);
    let access = everything();
    let fingerprint = key(1);
    let try_it = |n: u8| {
        open_account(
            &store,
            &open,
            &access,
            handle("newcomer"),
            &fingerprint,
            peer(n),
        )
    };
    assert!(try_it(1).await.is_ok());
    assert!(try_it(2).await.is_ok());
    assert!(matches!(
        try_it(3).await,
        Err(OpenError::Rejected(OpenRefusal::KeyRate))
    ));
    assert_eq!(
        store.calls(),
        2,
        "a limited attempt never reaches the store"
    );
    clock.advance(Duration::from_secs(3600));
    assert!(try_it(4).await.is_ok());
}

#[tokio::test]
async fn the_network_gate_is_separate_from_the_key_gate() {
    let clock = ManualClock::new();
    let open = gate(&clock);
    let store = Fake::in_mode(RegistrationMode::Open);
    // Three different keys from one address use the network burst of three.
    for n in 1_u32..=3 {
        let made = open_account(
            &store,
            &open,
            &everything(),
            handle("newcomer"),
            &key(n),
            peer(9),
        )
        .await;
        assert!(made.is_ok());
    }
    let limited = open_account(
        &store,
        &open,
        &everything(),
        handle("newcomer"),
        &key(10),
        peer(9),
    )
    .await;
    assert!(matches!(
        limited,
        Err(OpenError::Rejected(OpenRefusal::NetworkRate))
    ));
    // Another network is unaffected.
    let other = open_account(
        &store,
        &open,
        &everything(),
        handle("newcomer"),
        &key(11),
        peer(10),
    )
    .await;
    assert!(other.is_ok());
}

#[tokio::test]
async fn the_stores_answers_pass_through_and_a_taken_handle_is_just_a_refusal() {
    let clock = ManualClock::new();
    let open = gate(&clock);
    for (n, (answer, want)) in [
        (OpenError::CapReached, "cap"),
        (OpenError::Rejected(OpenRefusal::Handle), "handle"),
        (OpenError::Rejected(OpenRefusal::Key), "key"),
    ]
    .into_iter()
    .enumerate()
    {
        let store = Fake::in_mode(RegistrationMode::Open);
        *store.answer.lock().unwrap() = Some(Err(answer));
        let got = open_account(
            &store,
            &open,
            &everything(),
            handle("newcomer"),
            &key(50 + u32::try_from(n).unwrap()),
            None,
        )
        .await;
        let name = match got {
            Err(OpenError::CapReached) => "cap",
            Err(OpenError::Rejected(OpenRefusal::Handle)) => "handle",
            Err(OpenError::Rejected(OpenRefusal::Key)) => "key",
            other => panic!("{other:?}"),
        };
        assert_eq!(name, want);
    }
}

// ---- submit_application --------------------------------------------------------------------

fn application() -> InviteAsk {
    InviteAsk {
        handle: handle("applicant"),
        message: "hello".to_owned(),
        // Callers cannot smuggle an invite request through the application path.
        kind: RequestKind::Invite,
    }
}

#[tokio::test]
async fn an_application_is_dropped_unless_the_board_takes_applications() {
    // `NoMembers` reports the default mode, `invite`: no applications.
    let gate = RequestGate::new();
    let got = submit_application(
        &NoMembers,
        &gate,
        &everything(),
        application(),
        &key(1),
        None,
    )
    .await;
    assert_eq!(got, Submission::Dropped(DropReason::WrongMode));
}

#[tokio::test]
async fn an_application_needs_membership_apply_not_invite_request() {
    // Only `invite.request` held: the application path asks for `membership.apply`.
    struct ApplyBoard;
    impl RegistrationStore for ApplyBoard {
        async fn registration_mode(&self) -> Result<RegistrationMode, StoreError> {
            Ok(RegistrationMode::Apply)
        }
        async fn open_cap_setting(&self) -> Result<Option<String>, StoreError> {
            Ok(None)
        }
        async fn open_cap_reached(&self, _: DailyCap) -> Result<bool, StoreError> {
            Ok(false)
        }
        async fn open_account(
            &self,
            _: Username,
            _: &Fingerprint,
            _: DailyCap,
        ) -> Result<Opened, OpenError> {
            Err(OpenError::CapReached)
        }
        async fn open_account_password(
            &self,
            _: Username,
            _: &str,
            _: DailyCap,
        ) -> Result<Opened, OpenError> {
            Err(OpenError::CapReached)
        }
    }
    impl crate::queue::QueueStore for ApplyBoard {
        async fn submit_request(
            &self,
            _: &crate::queue::NewRequest,
            _: crate::queue::SubmitTerms,
        ) -> Result<crate::queue::Submitted, StoreError> {
            panic!("a request without the permission must not reach the store");
        }
        async fn pending_requests(&self, l: u32) -> Result<crate::queue::Queue, StoreError> {
            crate::queue::QueueStore::pending_requests(&NoMembers, l).await
        }
        async fn approve_request(
            &self,
            _: crate::queue::RequestId,
            _: AccountId,
        ) -> Result<Account, crate::queue::ApproveError> {
            Err(crate::queue::ApproveError::NotFound)
        }
        async fn deny_request(
            &self,
            _: crate::queue::RequestId,
            _: AccountId,
            _: i64,
        ) -> Result<(), crate::queue::DecideError> {
            Err(crate::queue::DecideError::NotFound)
        }
        async fn vouch_request(
            &self,
            _: crate::queue::RequestId,
            _: AccountId,
        ) -> Result<(), crate::queue::VouchError> {
            Err(crate::queue::VouchError::NotFound)
        }
        async fn max_pending_setting(&self) -> Result<Option<String>, StoreError> {
            Ok(None)
        }
        async fn purge_expired(&self) -> Result<crate::queue::Purged, StoreError> {
            Ok(crate::queue::Purged::default())
        }
    }
    let gate = RequestGate::new();
    let only_invite_request = waiting(&[Permission::InviteRequest]);
    let got = submit_application(
        &ApplyBoard,
        &gate,
        &only_invite_request,
        application(),
        &key(1),
        None,
    )
    .await;
    assert_eq!(got, Submission::Dropped(DropReason::NotPermitted));
}
