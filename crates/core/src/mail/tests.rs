//! Pure rules: sealing, the key file format and text bounds. The service's behaviour against
//! real stores (limits, the inbox cap, no cross-mailbox reads, reports, audit) is in the shared
//! store suite, which runs on every backend.
#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

use super::*;
use crate::store::AccountId;

fn keys() -> MailKeys {
    MailKeys::single(KeyId::FIRST, MailKey::generate().unwrap())
}

fn ids() -> (MessageId, AccountId) {
    (MessageId::from_raw(77), AccountId::from_raw(5))
}

#[test]
fn sealed_text_opens_with_its_own_id_and_recipient() {
    let keys = keys();
    let (id, to) = ids();
    let (key, sealed) = seal::seal(&keys, Field::Body, id, to, "hello there").unwrap();
    assert_eq!(key, KeyId::FIRST);
    let text = seal::open(&keys, key, Field::Body, id, to, &sealed).unwrap();
    assert_eq!(text, "hello there");
}

#[test]
fn sealed_bytes_do_not_hold_the_text_and_differ_each_time() {
    let keys = keys();
    let (id, to) = ids();
    let text = "a recognisable sentence about nothing";
    let (_, a) = seal::seal(&keys, Field::Body, id, to, text).unwrap();
    let (_, b) = seal::seal(&keys, Field::Body, id, to, text).unwrap();
    assert_ne!(a, b, "a fresh random nonce per sealed value");
    assert!(!a.windows(text.len()).any(|w| w == text.as_bytes()));
}

#[test]
fn a_moved_or_changed_value_does_not_open() {
    let keys = keys();
    let (id, to) = ids();
    let (key, sealed) = seal::seal(&keys, Field::Body, id, to, "text").unwrap();
    let open = |field, id, to, bytes: &[u8]| seal::open(&keys, key, field, id, to, bytes);
    // Another recipient's mailbox.
    assert_eq!(
        open(Field::Body, id, AccountId::from_raw(6), &sealed),
        Err(SealError::Unreadable)
    );
    // Another message.
    assert_eq!(
        open(Field::Body, MessageId::from_raw(78), to, &sealed),
        Err(SealError::Unreadable)
    );
    // Body bytes presented as the subject.
    assert_eq!(
        open(Field::Subject, id, to, &sealed),
        Err(SealError::Unreadable)
    );
    // Every single flipped byte is caught, in the nonce, the text and the tag.
    for at in 0..sealed.len() {
        let mut bad = sealed.clone();
        bad[at] ^= 1;
        assert_eq!(open(Field::Body, id, to, &bad), Err(SealError::Unreadable));
    }
    // Truncated to nothing, and to less than a nonce.
    assert_eq!(open(Field::Body, id, to, &[]), Err(SealError::Unreadable));
    assert_eq!(
        open(Field::Body, id, to, &sealed[..10]),
        Err(SealError::Unreadable)
    );
}

#[test]
fn another_key_does_not_open_and_an_unheld_key_id_is_named() {
    let (id, to) = ids();
    let one = keys();
    let two = keys();
    let (key, sealed) = seal::seal(&one, Field::Subject, id, to, "s").unwrap();
    assert_eq!(
        seal::open(&two, key, Field::Subject, id, to, &sealed),
        Err(SealError::Unreadable)
    );
    assert_eq!(
        seal::open(&one, KeyId::from_raw(9), Field::Subject, id, to, &sealed),
        Err(SealError::UnknownKey)
    );
}

#[test]
fn a_retired_key_still_opens_what_it_sealed_and_the_active_one_seals_new_mail() {
    let (id, to) = ids();
    let old = MailKey::generate().unwrap();
    let old_file = old.to_file_bytes(KeyId::FIRST);
    let (_, old_copy) = MailKey::from_file_bytes(&old_file).unwrap();
    let before = MailKeys::single(KeyId::FIRST, old);
    let (_, sealed) = seal::seal(&before, Field::Body, id, to, "old mail").unwrap();
    let after = MailKeys::single(KeyId::from_raw(2), MailKey::generate().unwrap())
        .with_retired(KeyId::FIRST, old_copy);
    assert_eq!(after.active(), KeyId::from_raw(2));
    assert_eq!(
        seal::open(&after, KeyId::FIRST, Field::Body, id, to, &sealed).unwrap(),
        "old mail"
    );
    let (used, _) = seal::seal(&after, Field::Body, id, to, "new").unwrap();
    assert_eq!(used, KeyId::from_raw(2));
}

#[test]
fn the_key_file_round_trips_and_refuses_damage() {
    let key = MailKey::generate().unwrap();
    let file = key.to_file_bytes(KeyId::from_raw(3));
    assert_eq!(file.len(), FILE_BYTES);
    let (id, back) = MailKey::from_file_bytes(&file).unwrap();
    assert_eq!(id, KeyId::from_raw(3));
    let (_, sealed) = seal::seal(
        &MailKeys::single(id, key),
        Field::Body,
        MessageId::from_raw(1),
        AccountId::from_raw(1),
        "x",
    )
    .unwrap();
    // The decoded key is the same key.
    assert!(
        seal::open(
            &MailKeys::single(id, back),
            id,
            Field::Body,
            MessageId::from_raw(1),
            AccountId::from_raw(1),
            &sealed
        )
        .is_ok()
    );
    for at in 0..FILE_BYTES {
        let mut bad = file.to_vec();
        bad[at] ^= 1;
        assert!(MailKey::from_file_bytes(&bad).is_err(), "byte {at}");
    }
    assert!(MailKey::from_file_bytes(&file[..FILE_BYTES - 1]).is_err());
    assert!(MailKey::from_file_bytes(&[]).is_err());
    let mut long = file.to_vec();
    long.push(0);
    assert!(MailKey::from_file_bytes(&long).is_err());
}

#[test]
fn debug_output_never_shows_key_bytes() {
    let key = MailKey::from_bytes([0xAB; KEY_BYTES]);
    let alone = format!("{key:?}");
    let shown = format!("{alone} {:?}", MailKeys::single(KeyId::FIRST, key));
    assert!(!shown.contains("171") && !shown.to_lowercase().contains("ab, ab"));
    assert!(!shown.contains("0xab"));
}

#[test]
fn message_ids_are_random_positive_and_nonzero() {
    let a = MessageId::random().unwrap();
    let b = MessageId::random().unwrap();
    assert_ne!(a, b);
    assert!(a.as_raw() > 0 && b.as_raw() > 0);
}

#[test]
fn subjects_are_cleaned_bounded_and_not_cut() {
    assert_eq!(prepare_subject("  hi  ").unwrap().as_str(), "hi");
    assert_eq!(prepare_subject("   "), Err(MailTextError::Empty));
    assert_eq!(
        prepare_subject(&"x".repeat(SUBJECT_MAX_CHARS + 1)),
        Err(MailTextError::TooLong {
            max: SUBJECT_MAX_CHARS
        })
    );
    assert!(prepare_subject(&"x".repeat(SUBJECT_MAX_CHARS)).is_ok());
    // Control characters and escapes never survive.
    let cleaned = prepare_subject("a\u{1b}[31mb\u{7}c").unwrap();
    assert!(!cleaned.as_str().contains('\u{1b}') && !cleaned.as_str().contains('\u{7}'));
}

#[test]
fn bodies_are_bounded_and_blank_is_refused() {
    use crate::drafts::{Body, BodyLimits};
    let body = Body::prepare(BodyLimits::MAX, "   \n ").unwrap();
    assert_eq!(check_body(&body), Err(MailTextError::Empty));
    let big = Body::prepare(BodyLimits::MAX, &"y".repeat(BODY_MAX_CHARS + 1)).unwrap();
    assert_eq!(check_body(&big), Err(MailTextError::Body));
    let ok = Body::prepare(BodyLimits::MAX, "fine").unwrap();
    assert!(check_body(&ok).is_ok());
}
