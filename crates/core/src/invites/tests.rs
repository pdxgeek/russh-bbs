//! The invite rules over an in-memory store: permission and quota checks, what the screens
//! show, and that nothing prints the code. The SQL behaviour (atomic redemption, expiry,
//! quota under concurrency) is in the store crate's shared suite, on both backends.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unused_async_trait_impl
)]

use std::sync::Mutex;

use super::*;
use crate::access::{Fingerprint, Permission, Slug, Snapshot, Via};
use crate::limits::{EffectiveLimits, Limit, LimitKind, RoleLimit, effective_limits};
use crate::sanitize::{Username, sanitize_username};
use crate::store::{Account, AccountId, AccountState, StoreError, UnixSeconds};

#[derive(Default)]
struct Fake {
    created: Mutex<Vec<NewInvite>>,
    used: Mutex<u32>,
    redeem_with: Mutex<Option<RedeemError>>,
    password_hashes: Mutex<Vec<String>>,
    broken: bool,
}

impl InviteStore for Fake {
    async fn create_invite(&self, new: &NewInvite) -> Result<InviteRecord, CreateError> {
        if self.broken {
            return Err(CreateError::Store(StoreError::AccountNotFound));
        }
        let mut used = self.used.lock().unwrap();
        if let Quota::Limited(limit) = new.quota
            && *used >= limit
        {
            return Err(CreateError::QuotaReached {
                limit,
                resets_at: UnixSeconds::from_raw(2_000),
            });
        }
        *used += 1;
        let mut created = self.created.lock().unwrap();
        created.push(new.clone());
        Ok(InviteRecord {
            id: InviteId::from_raw(i64::try_from(created.len()).unwrap()),
            expires_at: UnixSeconds::from_raw(1_000 + new.ttl_secs),
            used_in_month: *used,
        })
    }

    async fn redeem_invite(
        &self,
        _hash: &CodeHash,
        username: Username,
        _fingerprint: &Fingerprint,
    ) -> Result<Redeemed, RedeemError> {
        if let Some(error) = self.redeem_with.lock().unwrap().take() {
            return Err(error);
        }
        Ok(Redeemed {
            account: Account {
                id: AccountId::from_raw(9),
                username,
                state: REDEEMED_STATE,
                created_at: UnixSeconds::from_raw(0),
            },
            invite: InviteId::from_raw(1),
            inviter: AccountId::from_raw(2),
        })
    }

    async fn redeem_invite_password(
        &self,
        hash: &CodeHash,
        username: Username,
        password_hash: &str,
        _fingerprint: Option<&Fingerprint>,
    ) -> Result<Redeemed, RedeemError> {
        self.password_hashes
            .lock()
            .unwrap()
            .push(password_hash.to_owned());
        self.redeem_invite(hash, username, &key()).await
    }

    async fn invites_by(
        &self,
        _inviter: AccountId,
        _limit: u32,
    ) -> Result<Vec<InviteSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn invite_tree(&self, _limit: u32) -> Result<Vec<TreeNode>, StoreError> {
        Ok(Vec::new())
    }
}

fn member(role: &str, permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(2)),
        &Slug::parse(role).unwrap(),
        10,
        &Via::Account,
        permissions.iter().copied(),
    )
}

/// The limits a role of that name with `invites_per_month = n` gets (the sysop is unlimited by
/// rule, whatever is stored).
fn limits_of(role: &str, n: u64) -> EffectiveLimits {
    let role = Slug::parse(role).unwrap();
    effective_limits(
        std::slice::from_ref(&role),
        &[RoleLimit {
            role: role.clone(),
            kind: LimitKind::InvitesPerMonth,
            limit: Limit::AtMost(n),
        }],
        &[],
    )
}

fn key() -> Fingerprint {
    Fingerprint::parse(&format!("SHA256:{:A>43}", 1)).unwrap()
}

fn waiting() -> Snapshot {
    Snapshot::from_role(
        None,
        &Slug::WAITING,
        0,
        &Via::Waiting,
        [Permission::InviteRedeem],
    )
}

#[test]
fn the_redeemed_account_starts_active() {
    // D67: the inviter vouched, so the account is a member at once.
    assert_eq!(REDEEMED_STATE, AccountState::Active);
    assert!(REDEEMED_STATE.is_member_access());
}

#[tokio::test]
async fn without_invites_create_nothing_is_written() {
    let store = Fake::default();
    let none = member("member", &[Permission::MembersView]);
    let error = create_invite_within(&store, &none, &limits_of("member", 2), [1; CODE_BYTES])
        .await
        .unwrap_err();
    assert!(matches!(error, InviteError::Denied(_)));
    assert!(store.created.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_created_invite_is_bound_to_the_inviter_hashed_and_expires_in_seven_days() {
    let store = Fake::default();
    let access = member("member", &[Permission::InvitesCreate]);
    let made = create_invite_within(&store, &access, &limits_of("member", 3), [5; CODE_BYTES])
        .await
        .unwrap();
    let recorded = store.created.lock().unwrap();
    let new = &recorded[0];
    assert_eq!(new.inviter, AccountId::from_raw(2));
    assert_eq!(new.ttl_secs, 7 * 24 * 60 * 60);
    assert!(new.code_hash.ct_eq(&made.code.hash()));
    // The record has the hash only: nothing in it prints the code.
    assert!(!format!("{new:?}").contains(made.code.reveal()));
    assert!(!format!("{made:?}").contains(made.code.reveal()));
    assert_eq!(made.remaining, Some(2), "3 per month, 1 made");
}

#[tokio::test]
async fn the_quota_stops_creation_at_the_role_number() {
    let store = Fake::default();
    let access = member("member", &[Permission::InvitesCreate]);
    for n in 0..3_u8 {
        create_invite_within(&store, &access, &limits_of("member", 3), [n; CODE_BYTES])
            .await
            .unwrap();
    }
    let error = create_invite_within(&store, &access, &limits_of("member", 3), [9; CODE_BYTES])
        .await
        .unwrap_err();
    assert!(
        matches!(error, InviteError::QuotaReached { limit: 3, .. }),
        "{error:?}"
    );
    assert_eq!(store.created.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn the_sysop_has_no_quota() {
    let store = Fake::default();
    let access = member("sysop", &[Permission::InvitesCreate]);
    for n in 0..30_u8 {
        let made = create_invite_within(&store, &access, &limits_of("sysop", 3), [n; CODE_BYTES])
            .await
            .unwrap();
        assert_eq!(made.remaining, None);
    }
}

#[tokio::test]
async fn a_store_failure_is_an_error_not_a_code() {
    let store = Fake {
        broken: true,
        ..Fake::default()
    };
    let access = member("member", &[Permission::InvitesCreate]);
    assert!(matches!(
        create_invite_within(&store, &access, &limits_of("member", 3), [1; CODE_BYTES]).await,
        Err(InviteError::Store(_))
    ));
}

#[tokio::test]
async fn redeeming_needs_the_waiting_permission_and_reaches_the_store_only_then() {
    let store = Fake::default();
    let name = sanitize_username("newcomer").unwrap();
    let nothing = member("member", &[]);
    let denied = redeem_invite(&store, &nothing, "ABCD", name.clone(), &key()).await;
    assert!(matches!(
        denied,
        Err(RedeemError::Rejected(RejectReason::Code))
    ));
    let ok = redeem_invite(&store, &waiting(), "ABCD", name, &key())
        .await
        .unwrap();
    assert_eq!(ok.account.username.as_str(), "newcomer");
}

#[tokio::test]
async fn every_refusal_comes_back_as_rejected_with_its_reason_for_the_log_only() {
    let store = Fake::default();
    for reason in [RejectReason::Code, RejectReason::Handle, RejectReason::Key] {
        *store.redeem_with.lock().unwrap() = Some(RedeemError::Rejected(reason));
        let name = sanitize_username("newcomer").unwrap();
        let error = redeem_invite(&store, &waiting(), "ABCD", name, &key())
            .await
            .unwrap_err();
        assert!(matches!(error, RedeemError::Rejected(r) if r == reason));
    }
}

#[tokio::test]
async fn listing_and_the_tree_are_gated_by_their_own_permissions() {
    let store = Fake::default();
    let creator = member("member", &[Permission::InvitesCreate]);
    assert!(my_invites(&store, &creator).await.is_ok());
    assert!(matches!(
        invite_tree(&store, &creator).await,
        Err(InviteError::Denied(_))
    ));
    let viewer = member("trusted", &[Permission::InvitesViewTree]);
    assert!(invite_tree(&store, &viewer).await.is_ok());
    assert!(matches!(
        my_invites(&store, &viewer).await,
        Err(InviteError::Denied(_))
    ));
}

// ---- screens -------------------------------------------------------------------------------

fn text_of(view: &crate::waiting_room::View) -> String {
    let mut out = vec![view.heading.to_owned()];
    out.extend(view.body.iter().cloned());
    out.extend(view.note.map(str::to_owned));
    out.extend(view.notice.map(|n| n.text.to_owned()));
    out.push(view.footer.to_owned());
    out.join("\n")
}

#[tokio::test]
async fn the_create_screen_shows_the_code_and_the_expiry_once() {
    let store = Fake::default();
    let access = member("member", &[Permission::InvitesCreate]);
    let made = create_invite_within(&store, &access, &limits_of("member", 3), [5; CODE_BYTES])
        .await
        .unwrap();
    let code = made.code.reveal().to_owned();
    let screen = InviteScreen::new(InviteOutcome::Created(made));
    let text = text_of(&screen.view());
    assert!(text.contains(&code));
    assert!(text.contains("expires 1970-01-08"), "{text}");
    assert!(text.contains("only time it is shown"));
    assert!(text.contains("Invites you can still make this month: 2"));
    // Honest: it never calls the code or the board private.
    assert!(!text.to_lowercase().contains("private"));
}

#[test]
fn quota_denied_and_failed_screens_say_what_happened() {
    let quota = text_of(
        &InviteScreen::new(InviteOutcome::QuotaReached {
            limit: 3,
            resets_at: UnixSeconds::from_raw(1_793_491_200),
        })
        .view(),
    );
    assert!(
        quota.contains("all 3 of your invites for this month"),
        "{quota}"
    );
    assert!(quota.contains("restarts 2026-11-01 00:00 UTC"), "{quota}");
    assert!(quota.contains("No invite was made."));
    let denied = text_of(&InviteScreen::new(InviteOutcome::Denied).view());
    assert!(denied.contains("not allowed"));
    let failed = text_of(&InviteScreen::new(InviteOutcome::Failed).view());
    assert!(failed.contains("nothing was changed"));
}

#[test]
fn the_list_shows_each_invite_without_any_code() {
    let items = vec![
        InviteSummary {
            id: InviteId::from_raw(3),
            created_at: UnixSeconds::from_raw(0),
            expires_at: UnixSeconds::from_raw(604_800),
            status: InviteStatus::Open,
        },
        InviteSummary {
            id: InviteId::from_raw(2),
            created_at: UnixSeconds::from_raw(0),
            expires_at: UnixSeconds::from_raw(604_800),
            status: InviteStatus::Redeemed(Some(sanitize_username("alice").unwrap())),
        },
        InviteSummary {
            id: InviteId::from_raw(1),
            created_at: UnixSeconds::from_raw(0),
            expires_at: UnixSeconds::from_raw(604_800),
            status: InviteStatus::Expired,
        },
    ];
    let view = InviteScreen::new(InviteOutcome::List(items)).view();
    assert_eq!(view.body.len(), 3);
    assert!(view.body[0].starts_with("#3"));
    assert!(view.body[0].contains("open until 1970-01-08"));
    assert!(view.body[1].contains("used by alice"));
    assert!(view.body[2].contains("expired 1970-01-08"));
    let empty = InviteScreen::new(InviteOutcome::List(Vec::new())).view();
    assert_eq!(empty.body, [strings::LIST_EMPTY]);
}

#[test]
fn the_tree_indents_by_depth_pages_and_stays_inside_the_page_size() {
    let lines: Vec<TreeLine> = (1..=30_i64)
        .map(|id| TreeLine {
            depth: u16::try_from(id % 3).unwrap(),
            account: AccountId::from_raw(id),
            handle: sanitize_username(&format!("member{id}")).unwrap(),
        })
        .collect();
    let mut screen = InviteScreen::new(InviteOutcome::Tree(lines));
    let first = screen.view();
    // count line + 12 people + page line
    assert_eq!(first.body.len(), 1 + TREE_PAGE_LINES + 1);
    assert_eq!(first.body[0], "Who invited whom: 30 people.");
    assert_eq!(first.body[1], "  member1");
    assert_eq!(first.body[2], "    member2");
    assert!(first.body.last().unwrap().contains("Page 1 of 3"));
    assert!(screen.page_key('n'));
    assert!(screen.view().body[1].contains("member13"));
    assert!(screen.page_key('p'));
    assert!(screen.view().body[1].contains("member1"));
    assert!(!screen.page_key('x'), "any other key closes the screen");
    for _ in 0..10 {
        screen.page_key('n');
    }
    assert!(screen.view().body.last().unwrap().contains("Page 3 of 3"));
}

#[test]
fn an_empty_tree_and_a_single_page_tree_have_no_paging() {
    let empty = InviteScreen::new(InviteOutcome::Tree(Vec::new())).view();
    assert_eq!(empty.body, [strings::TREE_EMPTY]);
    let one = InviteScreen::new(InviteOutcome::Tree(vec![TreeLine {
        depth: 0,
        account: AccountId::from_raw(1),
        handle: sanitize_username("founder").unwrap(),
    }]))
    .view();
    assert_eq!(one.footer, strings::FOOTER_TREE);
}

#[test]
fn only_the_create_screen_shows_a_code_and_other_screens_do_not_page() {
    let mut list = InviteScreen::new(InviteOutcome::List(Vec::new()));
    assert!(!list.page_key('n'));
}
