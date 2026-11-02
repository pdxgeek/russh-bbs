#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl
)]

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::*;
use crate::access::Fingerprint;
use crate::sanitize::Username;
use crate::store::{Account, AccountId, AccountState, Role, StoreError, UnixSeconds};

fn fingerprint(n: u32) -> Fingerprint {
    Fingerprint::parse(&format!("SHA256:{n:A>43}")).unwrap()
}

fn token(seed: u8) -> SetupToken {
    SetupToken::from_bytes([seed; TOKEN_BYTES])
}

#[test]
fn a_token_has_at_least_128_bits_and_a_fixed_shape() {
    const { assert!(TOKEN_BYTES * 8 >= 128) };
    let text = token(7).reveal().to_owned();
    // 160 bits = 32 base32 characters in 8 groups of 4.
    assert_eq!(text.len(), 32 + 7);
    assert!(text.split('-').all(|g| g.len() == 4));
    assert!(
        text.chars()
            .all(|c| c == '-' || c.is_ascii_uppercase() || ('2'..='7').contains(&c))
    );
}

#[test]
fn different_bytes_give_different_tokens_and_the_same_bytes_the_same() {
    let mut seen = HashSet::new();
    for seed in 0..=255_u8 {
        assert!(seen.insert(token(seed).reveal().to_owned()));
    }
    assert_eq!(token(9).reveal(), token(9).reveal());
}

#[test]
fn base32_matches_the_rfc_vector() {
    // RFC 4648: "foobar" -> MZXW6YTBOI. Pad to 20 bytes with zeros and compare the prefix.
    let mut bytes = [0_u8; TOKEN_BYTES];
    bytes[..6].copy_from_slice(b"foobar");
    let text = SetupToken::from_bytes(bytes).reveal().replace('-', "");
    assert!(text.starts_with("MZXW6YTBOI"), "{text}");
}

#[test]
fn debug_output_never_contains_the_token() {
    let t = token(3);
    let shown = format!("{t:?} {:?}", t.hash());
    assert!(!shown.contains(&t.reveal().replace('-', "")[..8]));
    assert!(!shown.contains(t.reveal()));
}

#[test]
fn the_gate_keeps_a_hash_not_the_token() {
    let t = token(5);
    let gate = FounderGate::open_with(&t);
    // The Debug form of the whole gate carries neither the token nor any text of it.
    let shown = format!("{gate:?}");
    assert!(!shown.contains(&t.reveal().replace('-', "")[..8]));
    assert!(gate.accepts(t.reveal()));
}

#[test]
fn input_formatting_does_not_matter() {
    let t = token(11);
    let gate = FounderGate::open_with(&t);
    let plain = t.reveal().replace('-', "");
    assert!(gate.accepts(&plain));
    assert!(gate.accepts(&plain.to_lowercase()));
    assert!(gate.accepts(&format!("  {} \n", t.reveal())));
    assert!(!gate.accepts(&plain[..plain.len() - 1]));
    assert!(!gate.accepts(""));
    assert!(!gate.accepts(&"A".repeat(10_000)));
}

#[test]
fn a_closed_gate_accepts_nothing_even_the_old_token_or_the_internal_marker() {
    let t = token(13);
    let gate = FounderGate::open_with(&t);
    gate.close();
    assert!(!gate.accepts(t.reveal()));
    assert!(!gate.accepts("unmatchable"));
    assert!(!gate.accepts(""));
    assert!(!FounderGate::closed().accepts(t.reveal()));
    assert!(!gate.is_open());
}

#[test]
fn the_hash_comparison_is_the_constant_time_one() {
    // `ct_eq` returns subtle::Choice, so a caller cannot branch on bytes. Equal and unequal
    // hashes give the expected Choice.
    let a = token(1).hash();
    let b = token(2).hash();
    assert!(bool::from(a.ct_eq(&a)));
    assert!(!bool::from(a.ct_eq(&b)));
}

/// An in-memory store with the same contract as the real one: a compare-and-set on a flag.
#[derive(Default)]
struct FakeStore {
    claimed: AtomicBool,
    created: AtomicU32,
    fail: AtomicBool,
}

impl FounderStore for FakeStore {
    async fn founder_claimed(&self) -> Result<bool, StoreError> {
        Ok(self.claimed.load(Ordering::SeqCst))
    }

    async fn claim_founder(
        &self,
        username: Username,
        _fingerprint: &Fingerprint,
    ) -> Result<Option<Account>, StoreError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(StoreError::Corrupt("test".into()));
        }
        if self.claimed.swap(true, Ordering::SeqCst) {
            return Ok(None);
        }
        self.created.fetch_add(1, Ordering::SeqCst);
        Ok(Some(Account {
            id: AccountId::from_raw(1),
            username,
            role: Role::Sysop,
            state: AccountState::Active,
            created_at: UnixSeconds::from_raw(0),
        }))
    }
}

#[tokio::test]
async fn the_right_token_claims_once_as_an_active_sysop() {
    let t = token(21);
    let gate = FounderGate::open_with(&t);
    let store = FakeStore::default();
    let outcome = claim_founder(&gate, &store, t.reveal(), &fingerprint(1)).await;
    let ClaimOutcome::Claimed(account) = outcome else {
        panic!("expected a claim");
    };
    assert_eq!(account.role, Role::Sysop);
    assert_eq!(account.state, AccountState::Active);
    assert!(!gate.is_open());
}

#[tokio::test]
async fn a_second_claim_fails_exactly_like_a_wrong_token() {
    let t = token(22);
    let gate = FounderGate::open_with(&t);
    let store = FakeStore::default();
    let wrong = claim_founder(&gate, &store, "NOPE", &fingerprint(1)).await;
    assert!(gate.is_open(), "a wrong token must not close the gate");
    let first = claim_founder(&gate, &store, t.reveal(), &fingerprint(1)).await;
    assert!(matches!(first, ClaimOutcome::Claimed(_)));
    let again = claim_founder(&gate, &store, t.reveal(), &fingerprint(2)).await;
    assert_eq!(again, wrong);
    assert_eq!(again, ClaimOutcome::Rejected);
    assert_eq!(store.created.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_wrong_token_never_reaches_the_store() {
    let gate = FounderGate::open_with(&token(23));
    let store = FakeStore::default();
    assert_eq!(
        claim_founder(&gate, &store, "ABCD", &fingerprint(1)).await,
        ClaimOutcome::Rejected
    );
    assert!(!store.claimed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn a_store_error_is_a_plain_rejection_and_leaves_the_token_usable() {
    let t = token(24);
    let gate = FounderGate::open_with(&t);
    let store = FakeStore::default();
    store.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        claim_founder(&gate, &store, t.reveal(), &fingerprint(1)).await,
        ClaimOutcome::Rejected
    );
    store.fail.store(false, Ordering::SeqCst);
    assert!(matches!(
        claim_founder(&gate, &store, t.reveal(), &fingerprint(1)).await,
        ClaimOutcome::Claimed(_)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_racing_claims_make_exactly_one_sysop() {
    for round in 0..50 {
        let t = token(30);
        let gate = Arc::new(FounderGate::open_with(&t));
        let store = Arc::new(FakeStore::default());
        let text = t.reveal().to_owned();
        let a = {
            let (gate, store, text) = (Arc::clone(&gate), Arc::clone(&store), text.clone());
            tokio::spawn(async move { claim_founder(&gate, &*store, &text, &fingerprint(1)).await })
        };
        let b = {
            let (gate, store) = (Arc::clone(&gate), Arc::clone(&store));
            tokio::spawn(async move { claim_founder(&gate, &*store, &text, &fingerprint(2)).await })
        };
        let wins = [a.await.unwrap(), b.await.unwrap()]
            .iter()
            .filter(|o| matches!(o, ClaimOutcome::Claimed(_)))
            .count();
        assert_eq!(wins, 1, "round {round}");
        assert_eq!(store.created.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn the_form_uses_one_message_for_every_failure_and_hides_the_input() {
    let mut form = ClaimForm::new(MAX_ATTEMPTS_PER_SESSION);
    for c in "abc-123".chars() {
        form.input(ClaimInput::Char(c));
    }
    form.input(ClaimInput::Char('!'));
    form.input(ClaimInput::Char('\u{1b}'));
    form.input(ClaimInput::Char('\u{e0a0}'));
    let view = form.view();
    assert_eq!(
        view.fields[0].value, "*******",
        "typed text is masked, junk dropped"
    );
    let effect = form.input(ClaimInput::Enter);
    assert_eq!(effect, ClaimEffect::Submit("abc-123".into()));
    assert_eq!(form.rejected(), ClaimEffect::Stay);
    assert_eq!(form.view().notice.unwrap().text, strings::REJECTED);
    assert_eq!(form.view().fields[0].value, "");
}

#[test]
fn the_form_disconnects_after_its_attempts() {
    let mut form = ClaimForm::new(2);
    for _ in 0..2 {
        assert!(matches!(
            form.input(ClaimInput::Enter),
            ClaimEffect::Submit(_)
        ));
        let after = form.rejected();
        if after == ClaimEffect::Disconnect {
            assert_eq!(form.view().notice.unwrap().text, strings::TOO_MANY);
            return;
        }
    }
    panic!("never disconnected");
}

#[test]
fn escape_and_quit_leave() {
    let mut form = ClaimForm::new(3);
    assert_eq!(form.input(ClaimInput::Escape), ClaimEffect::Disconnect);
    assert_eq!(form.input(ClaimInput::Quit), ClaimEffect::Disconnect);
}

#[test]
fn the_signed_in_placeholder_names_the_role_and_admits_it_is_a_placeholder() {
    let sysop = signed_in_view(Role::Sysop);
    assert_eq!(sysop.body, vec![strings::SIGNED_IN_SYSOP.to_owned()]);
    assert_eq!(sysop.note, Some(strings::PLACEHOLDER_NOTE));
    assert_eq!(
        signed_in_view(Role::Member).body,
        vec![strings::SIGNED_IN_MEMBER.to_owned()]
    );
}
