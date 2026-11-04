//! Accounts bound to a password and no SSH key (D94, D95) against any backend: an invite
//! redeemed, an `open` account and an approved request each make an active account with an
//! `account_password` row and no `account_keys` row, in the same single transaction and with the
//! same audit rows as the key path; a refusal leaves no password row and consumes nothing; a
//! queued request holds only the hash and it leaves with the request.
#![allow(clippy::unwrap_used, clippy::panic)]

use russh_bbs_core::access::Slug;
use russh_bbs_core::invites::{
    CODE_BYTES, INVITE_TTL_SECS, InviteCode, InviteStore, NewInvite, Quota, RedeemError,
    RejectReason,
};
use russh_bbs_core::passwords::{PasswordStore, hash::hash};
use russh_bbs_core::queue::{
    ApproveError, Message, NewRequest, PENDING_TTL_SECS, QueueStore, Refusal, RequestKind,
    SubmitTerms, Submitted, password_identity,
};
use russh_bbs_core::registration::{DailyCap, OpenError, OpenRefusal, RegistrationStore};
use russh_bbs_core::sanitize::{Username, sanitize_username};
use russh_bbs_core::store::{Account, AccountState, AccountStore, NewAccount};

use super::{Harness, fingerprint};

fn name(text: &str) -> Username {
    sanitize_username(text).unwrap()
}

/// A real argon2id hash of a throwaway password (the store only needs the PHC shape).
fn a_hash(password: &str) -> String {
    hash(password).unwrap()
}

async fn member<S: AccountStore>(store: &S, text: &str) -> Account {
    store
        .create_account(NewAccount {
            username: name(text),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

async fn code_for<S: InviteStore>(store: &S, inviter: &Account, seed: u8) -> InviteCode {
    let code = InviteCode::from_bytes([seed; CODE_BYTES]);
    store
        .create_invite(&NewInvite {
            inviter: inviter.id,
            code_hash: code.hash(),
            ttl_secs: INVITE_TTL_SECS,
            quota: Quota::Unlimited,
        })
        .await
        .unwrap();
    code
}

async fn count<H: Harness>(h: &H, from_where: &str) -> String {
    h.texts(&format!("SELECT CAST(count(*) AS TEXT) FROM {from_where}"))
        .await
        .remove(0)
}

/// The account's password row is the hash given, and it has no key.
async fn assert_password_account<H: Harness>(h: &H, account: &Account, hash: &str) {
    let store = h.store();
    assert_eq!(
        store.stored_hash(account.id).await.unwrap().as_deref(),
        Some(hash)
    );
    assert!(!store.has_key(account.id).await.unwrap());
    assert_eq!(
        count(
            h,
            &format!("account_keys WHERE account_id = {}", account.id)
        )
        .await,
        "0"
    );
    let found = store
        .login_record(&account.username.as_str().to_ascii_lowercase())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.account.id, account.id);
    assert_eq!(found.hash.as_deref(), Some(hash));
}

pub(crate) async fn redeeming_with_a_password_makes_a_keyless_member_with_the_key_paths_audit_rows<
    H: Harness,
>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let code = code_for(store, &alice, 1).await;
    let phc = a_hash("a long password nobody guesses");
    let got = store
        .redeem_invite_password(&code.hash(), name("newbie"), &phc, None)
        .await
        .unwrap();
    assert_eq!(got.account.state, AccountState::Active);
    assert_eq!(got.inviter, alice.id);
    assert_password_account(&h, &got.account, &phc).await;
    // The same two audit rows as a key redemption, and no hash in either.
    let log = h
        .texts("SELECT action || ' ' || detail FROM audit_log ORDER BY id")
        .await
        .join("\n");
    assert!(log.contains("account.created"), "{log}");
    assert!(log.contains("invite.redeemed"), "{log}");
    assert!(!log.contains("$argon2id$"), "{log}");
    // The tree shows the new member under their inviter, as for a key member.
    let tree = store.invite_tree(100).await.unwrap();
    let node = tree.iter().find(|n| n.account == got.account.id).unwrap();
    assert_eq!(node.invited_by, Some(alice.id));
    // The code works once.
    let again = store
        .redeem_invite_password(&code.hash(), name("second"), &phc, None)
        .await;
    assert!(matches!(
        again,
        Err(RedeemError::Rejected(RejectReason::Code))
    ));
}

pub(crate) async fn a_refused_password_redemption_leaves_no_password_and_consumes_nothing<
    H: Harness,
>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let taken = member(store, "taken").await;
    let code = code_for(store, &alice, 2).await;
    let phc = a_hash("another long password here");
    // A taken handle, and a lookalike of one.
    for handle in [taken.username.as_str(), "TAKEN"] {
        let Ok(username) = sanitize_username(handle) else {
            continue;
        };
        let refused = store
            .redeem_invite_password(&code.hash(), username, &phc, None)
            .await;
        assert!(
            matches!(refused, Err(RedeemError::Rejected(RejectReason::Handle))),
            "{handle}: {refused:?}"
        );
    }
    assert_eq!(count(&h, "account_password").await, "0");
    // The code was not consumed by the refusals.
    let made = store
        .redeem_invite_password(&code.hash(), name("fresh"), &phc, None)
        .await
        .unwrap();
    assert_password_account(&h, &made.account, &phc).await;
}

pub(crate) async fn open_with_a_password_makes_a_keyless_account_and_counts_against_the_cap<
    H: Harness,
>() {
    let h = H::fresh().await;
    let store = h.store();
    let phc = a_hash("open registration password");
    let made = store
        .open_account_password(name("walkin"), &phc, None, DailyCap::Limited(1))
        .await
        .unwrap();
    assert_eq!(made.account.state, AccountState::Active);
    assert_password_account(&h, &made.account, &phc).await;
    assert_eq!(
        h.texts(&format!(
            "SELECT CASE WHEN invited_by IS NULL THEN 'null' ELSE 'set' END \
             FROM accounts WHERE id = {}",
            made.account.id
        ))
        .await,
        ["null"]
    );
    // The day was full after one: the next is the cap, and writes nothing.
    let capped = store
        .open_account_password(name("second"), &phc, None, DailyCap::Limited(1))
        .await;
    assert!(matches!(capped, Err(OpenError::CapReached)));
    assert_eq!(count(&h, "account_password").await, "1");
}

pub(crate) async fn open_with_a_taken_handle_rolls_back_the_count_and_the_password<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    member(store, "taken").await;
    let phc = a_hash("open registration password");
    let refused = store
        .open_account_password(name("taken"), &phc, None, DailyCap::Limited(5))
        .await;
    assert!(matches!(
        refused,
        Err(OpenError::Rejected(OpenRefusal::Handle))
    ));
    assert_eq!(count(&h, "account_password").await, "0");
    assert_eq!(count(&h, "registration_day WHERE opened > 0").await, "0");
}

fn password_request(handle: &str, kind: RequestKind, phc: &str) -> NewRequest {
    let handle = name(handle);
    NewRequest {
        fingerprint: password_identity(&handle),
        handle,
        message: Message::new("hello from a telnet client"),
        kind,
        password_hash: Some(phc.to_owned()),
        key_bound: false,
    }
}

fn terms() -> SubmitTerms {
    SubmitTerms {
        max_pending: 50,
        ttl_secs: PENDING_TTL_SECS,
    }
}

pub(crate) async fn an_approved_password_request_makes_a_keyless_member_and_the_hash_moves<
    H: Harness,
>() {
    let h = H::fresh().await;
    let store = h.store();
    let sysop = member(store, "the-sysop").await;
    let phc = a_hash("a request password, twelve+");
    let Submitted::Stored(id) = store
        .submit_request(
            &password_request("hopeful", RequestKind::Application, &phc),
            terms(),
        )
        .await
        .unwrap()
    else {
        panic!("expected the request to be stored");
    };
    // Waiting: the hash is held by the request and no account exists.
    assert_eq!(
        count(&h, "invite_request WHERE password_hash IS NOT NULL").await,
        "1"
    );
    assert_eq!(count(&h, "account_password").await, "0");
    let account = store.approve_request(id, sysop.id).await.unwrap();
    assert_eq!(account.username.as_str(), "hopeful");
    assert_eq!(account.state, AccountState::Active);
    assert_password_account(&h, &account, &phc).await;
    // The request, and with it the hash it carried, is gone.
    assert_eq!(count(&h, "invite_request").await, "0");
    // Approval is audited without the hash.
    let log = h
        .texts("SELECT action || ' ' || detail FROM audit_log ORDER BY id")
        .await
        .join("\n");
    assert!(log.contains("member.approved"), "{log}");
    assert!(!log.contains("$argon2id$"), "{log}");
}

pub(crate) async fn a_password_request_is_one_per_handle_and_a_denial_removes_the_hash<
    H: Harness,
>() {
    let h = H::fresh().await;
    let store = h.store();
    let sysop = member(store, "the-sysop").await;
    let phc = a_hash("a request password, twelve+");
    let Submitted::Stored(id) = store
        .submit_request(
            &password_request("hopeful", RequestKind::Invite, &phc),
            terms(),
        )
        .await
        .unwrap()
    else {
        panic!("expected the request to be stored");
    };
    // A second request for the same handle, or a lookalike, is the one-per-identity rule.
    for again in ["hopeful", "H0peful"] {
        let second = store
            .submit_request(&password_request(again, RequestKind::Invite, &phc), terms())
            .await
            .unwrap();
        assert_eq!(
            second,
            Submitted::Refused(Refusal::AlreadyPending),
            "{again}"
        );
    }
    store
        .deny_request(id, sysop.id, russh_bbs_core::queue::DENIAL_COOLDOWN_SECS)
        .await
        .unwrap();
    assert_eq!(count(&h, "invite_request").await, "0");
    assert_eq!(count(&h, "account_password").await, "0");
    // The denied identity cools down like a denied key.
    let cooling = store
        .submit_request(
            &password_request("hopeful", RequestKind::Invite, &phc),
            terms(),
        )
        .await
        .unwrap();
    assert_eq!(cooling, Submitted::Refused(Refusal::Cooling));
}

pub(crate) async fn approving_a_password_request_for_a_taken_handle_keeps_the_request<
    H: Harness,
>() {
    let h = H::fresh().await;
    let store = h.store();
    let sysop = member(store, "the-sysop").await;
    let phc = a_hash("a request password, twelve+");
    let Submitted::Stored(id) = store
        .submit_request(
            &password_request("wanted", RequestKind::Invite, &phc),
            terms(),
        )
        .await
        .unwrap()
    else {
        panic!("expected the request to be stored");
    };
    member(store, "wanted").await;
    let refused = store.approve_request(id, sysop.id).await;
    assert!(matches!(refused, Err(ApproveError::HandleTaken)));
    assert_eq!(count(&h, "invite_request").await, "1");
    assert_eq!(count(&h, "account_password").await, "0");
}

pub(crate) async fn has_key_is_true_for_a_key_account_and_false_for_a_password_account<
    H: Harness,
>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    assert!(!store.has_key(alice.id).await.unwrap());
    store
        .attach_fingerprint(alice.id, &fingerprint(1))
        .await
        .unwrap();
    assert!(store.has_key(alice.id).await.unwrap());
}

pub(crate) async fn the_database_refuses_a_password_hash_that_is_not_argon2id<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let code = code_for(store, &alice, 3).await;
    // The CHECK on `account_password` stops the whole redemption, so the code is not spent.
    let refused = store
        .redeem_invite_password(&code.hash(), name("newbie"), "plain-text-not-a-hash", None)
        .await;
    assert!(matches!(refused, Err(RedeemError::Store(_))), "{refused:?}");
    assert_eq!(count(&h, "accounts WHERE username = 'newbie'").await, "0");
    let ok = store
        .redeem_invite_password(
            &code.hash(),
            name("newbie"),
            &a_hash("a long enough password"),
            None,
        )
        .await;
    assert!(ok.is_ok());
    // And a queued request cannot hold one either.
    let bad = store
        .submit_request(
            &password_request("sneaky", RequestKind::Invite, "$bcrypt$nope"),
            terms(),
        )
        .await;
    assert!(bad.is_err());
}

pub(crate) async fn a_visitor_with_a_key_gets_the_key_and_the_password_on_every_path<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let phc = a_hash("both credentials here");

    // Redeem.
    let code = code_for(store, &alice, 9).await;
    let made = store
        .redeem_invite_password(&code.hash(), name("keyed"), &phc, Some(&fingerprint(1)))
        .await
        .unwrap();
    assert_eq!(
        store.stored_hash(made.account.id).await.unwrap().as_deref(),
        Some(phc.as_str())
    );
    assert!(store.has_key(made.account.id).await.unwrap());
    let bound = store
        .account_by_fingerprint(&fingerprint(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bound.id, made.account.id);
    // The same key cannot make a second account, and nothing is left behind.
    let code = code_for(store, &alice, 10).await;
    let again = store
        .redeem_invite_password(&code.hash(), name("second"), &phc, Some(&fingerprint(1)))
        .await;
    assert!(matches!(
        again,
        Err(RedeemError::Rejected(RejectReason::Key))
    ));
    assert_eq!(count(&h, "accounts WHERE username = 'second'").await, "0");

    // Open.
    let opened = store
        .open_account_password(
            name("walkin"),
            &phc,
            Some(&fingerprint(2)),
            DailyCap::Limited(5),
        )
        .await
        .unwrap();
    assert!(store.has_key(opened.account.id).await.unwrap());
    assert!(
        store
            .stored_hash(opened.account.id)
            .await
            .unwrap()
            .is_some()
    );

    // A request with a real key and a password binds both on approval.
    let sysop = member(store, "the-sysop").await;
    let handle = name("hopeful");
    let request = NewRequest {
        fingerprint: fingerprint(3),
        handle,
        message: Message::new("hi"),
        kind: RequestKind::Application,
        password_hash: Some(phc.clone()),
        key_bound: true,
    };
    let Submitted::Stored(id) = store.submit_request(&request, terms()).await.unwrap() else {
        panic!("expected the request to be stored");
    };
    let account = store.approve_request(id, sysop.id).await.unwrap();
    assert!(store.has_key(account.id).await.unwrap());
    assert_eq!(
        store.stored_hash(account.id).await.unwrap().as_deref(),
        Some(phc.as_str())
    );
}

pub(crate) async fn the_founder_can_be_claimed_with_a_password_with_or_without_a_key<H: Harness>() {
    use russh_bbs_core::founder::FounderStore;
    let phc = a_hash("a founder's long password");
    // Password only.
    let h = H::fresh().await;
    let store = h.store();
    let account = store
        .claim_founder_password(name("founder"), &phc, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.state, AccountState::Active);
    assert_password_account(&h, &account, &phc).await;
    // The claim is spent: a second one changes nothing.
    let again = store
        .claim_founder_password(name("other"), &phc, Some(&fingerprint(1)))
        .await
        .unwrap();
    assert!(again.is_none());
    assert_eq!(count(&h, "account_password").await, "1");
    // With a key: both.
    let h = H::fresh().await;
    let store = h.store();
    let account = store
        .claim_founder_password(name("founder"), &phc, Some(&fingerprint(2)))
        .await
        .unwrap()
        .unwrap();
    assert!(store.has_key(account.id).await.unwrap());
    assert_eq!(
        store.stored_hash(account.id).await.unwrap().as_deref(),
        Some(phc.as_str())
    );
}

pub(crate) async fn a_key_is_linked_once_audited_and_a_taken_key_changes_nothing<H: Harness>() {
    use russh_bbs_core::audit::events;
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let bob = member(store, "bob").await;
    let key = fingerprint(5);
    store
        .link_key(alice.id, &key, &events::key_linked(alice.id, &key))
        .await
        .unwrap();
    assert!(store.has_key(alice.id).await.unwrap());
    // The key belongs to alice now: bob cannot take it, and no audit row is left for the try.
    let taken = store
        .link_key(bob.id, &key, &events::key_linked(bob.id, &key))
        .await;
    assert!(matches!(
        taken,
        Err(russh_bbs_core::store::StoreError::FingerprintTaken)
    ));
    assert!(!store.has_key(bob.id).await.unwrap());
    let log = h
        .texts("SELECT action FROM audit_log WHERE action = 'key.linked'")
        .await;
    assert_eq!(log.len(), 1);
    let missing = store
        .link_key(
            russh_bbs_core::store::AccountId::from_raw(9_999),
            &fingerprint(6),
            &events::key_linked(alice.id, &fingerprint(6)),
        )
        .await;
    assert!(missing.is_err());
    assert_eq!(count(&h, "account_keys").await, "1");
}
