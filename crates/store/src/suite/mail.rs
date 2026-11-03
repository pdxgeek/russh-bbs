//! Mail against any backend (M3-I, D78, D90): sealed rows that hold no plaintext, a key that
//! must match, a row that will not move, one answer for an unknown handle, `mail_per_day`, the
//! inbox cap, reports and what the audit log may say.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

use russh_bbs_core::access::{Permission, Slug, Snapshot};
use russh_bbs_core::audit::{Action, AuditEntry, AuditQuery, AuditStore, Target, known_events};
use russh_bbs_core::drafts::Body;
use russh_bbs_core::limits::{Limit, LimitKind, LimitStore, set_member_limit};
use russh_bbs_core::mail::{
    self, INBOX_CAP, KeyCheckError, KeyId, MAX_OPEN_REPORTS, MailError, MailKey, MailKeys,
    MailStore, MessageId, body_limits, verify_keys,
};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountState, AccountStateStore, AccountStore, NewAccount, UnixSeconds,
};

use super::{Harness, TestStore};

const T0: i64 = 2_000_000;

fn at(n: i64) -> UnixSeconds {
    UnixSeconds::from_raw(T0 + n)
}

fn keys() -> MailKeys {
    MailKeys::single(KeyId::FIRST, MailKey::generate().unwrap())
}

async fn make<S: AccountStore>(store: &S, name: &str, role: Slug, state: AccountState) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role,
            state,
        })
        .await
        .unwrap()
}

async fn snap<S: TestStore>(store: &S, who: &Account) -> Snapshot {
    store.snapshot(Some(who.id)).await.unwrap()
}

fn body(text: &str) -> Body {
    Body::prepare(body_limits(), text).unwrap()
}

struct World {
    sysop: Account,
    ann: Account,
    bob: Account,
    cat: Account,
}

async fn world<S: TestStore>(s: &S) -> World {
    World {
        sysop: make(s, "ops", Slug::SYSOP, AccountState::Active).await,
        ann: make(s, "ann", Slug::MEMBER, AccountState::Active).await,
        bob: make(s, "bob", Slug::MEMBER, AccountState::Active).await,
        cat: make(s, "cat", Slug::MEMBER, AccountState::Active).await,
    }
}

const SUBJECT: &str = "Quarterly kumquat inventory";
const TEXT: &str = "The kumquat count is eleven. Do not tell anyone about the pelican.";

async fn send_one<S: TestStore>(
    s: &S,
    keys: &MailKeys,
    from: &Account,
    to: &str,
    n: i64,
) -> Result<mail::Sent, MailError> {
    mail::send(
        s,
        keys,
        &snap(s, from).await,
        at(n),
        to,
        SUBJECT,
        &body(TEXT),
    )
    .await
}

async fn all_audit<S: AuditStore>(s: &S) -> Vec<AuditEntry> {
    known_events(s.page(&AuditQuery::default()).await.unwrap())
}

pub(crate) async fn mail_round_trips_and_the_stored_row_holds_no_plaintext<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let keys = keys();
    let sent = send_one(s, &keys, &w.ann, "BOB", 1).await.unwrap();
    let bob = snap(s, &w.bob).await;

    let page = mail::inbox(s, &keys, &bob, 0).await.unwrap();
    assert_eq!((page.total, page.unread), (1, 1));
    assert_eq!(page.entries[0].subject.as_deref(), Some(SUBJECT));
    assert_eq!(page.entries[0].from.as_deref(), Some("ann"));
    assert!(page.entries[0].unread);
    let view = mail::read(s, &keys, &bob, at(2), sent.id).await.unwrap();
    assert_eq!(view.subject, SUBJECT);
    assert_eq!(view.body.as_str(), TEXT);
    assert!(view.can_reply && view.can_report);
    let page = mail::inbox(s, &keys, &bob, 0).await.unwrap();
    assert_eq!((page.total, page.unread), (1, 0), "reading marks it read");

    // What is really in the database: sealed bytes only. Search the raw bytes (as hex) of the
    // sealed columns and of every text column of every table for each word of the message.
    let subject_hex = h.hex("subject").await;
    let body_hex = h.hex("body").await;
    assert_eq!((subject_hex.len(), body_hex.len()), (1, 1));
    for plain in [SUBJECT, TEXT, "kumquat", "pelican"] {
        let needle = plain.bytes().fold(String::new(), |mut hex, b| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{b:02x}");
            hex
        });
        for stored in [&subject_hex[0], &body_hex[0]] {
            assert!(!stored.contains(&needle), "{plain} is in a stored row");
        }
    }
    // 24-byte nonce + text + 16-byte tag: the ciphertext is as long as the text plus 40.
    assert_eq!(subject_hex[0].len() / 2, SUBJECT.len() + 40);
    assert_eq!(body_hex[0].len() / 2, TEXT.len() + 40);
    // Nothing the audit log kept names a subject or a text, and a send writes no row at all.
    assert!(all_audit(s).await.iter().all(|e| {
        e.event.action != Action::MailReported && !e.event.detail.as_str().contains("kumquat")
    }));
    assert!(
        all_audit(s)
            .await
            .iter()
            .all(|e| !format!("{:?}", e.event).contains("pelican"))
    );
}

pub(crate) async fn a_row_is_only_ever_reachable_through_its_recipients_mailbox<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let keys = keys();
    let sent = send_one(s, &keys, &w.ann, "bob", 1).await.unwrap();
    // Another member and the sysop: no such message, the same answer as for one that never was.
    for who in [&w.cat, &w.sysop, &w.ann] {
        let snapshot = snap(s, who).await;
        assert!(matches!(
            mail::read(s, &keys, &snapshot, at(2), sent.id).await,
            Err(MailError::NotFound)
        ));
        assert!(matches!(
            mail::read(s, &keys, &snapshot, at(2), MessageId::from_raw(12345)).await,
            Err(MailError::NotFound)
        ));
        assert!(matches!(
            mail::delete(s, &snapshot, sent.id).await,
            Err(MailError::NotFound)
        ));
        assert_eq!(
            mail::inbox(s, &keys, &snapshot, 0).await.unwrap().total,
            0,
            "no one's list shows it"
        );
    }
    // Still there for bob.
    assert_eq!(
        mail::inbox(s, &keys, &snap(s, &w.bob).await, 0)
            .await
            .unwrap()
            .total,
        1
    );
    // The store itself scopes by recipient.
    assert!(s.message(w.cat.id, sent.id).await.unwrap().is_none());
    assert!(!s.delete_mail(w.cat.id, sent.id).await.unwrap());
    // Without the permission, or without an account, nothing opens.
    assert!(matches!(
        mail::inbox(s, &keys, &Snapshot::new(Some(w.bob.id), vec![]), 0).await,
        Err(MailError::NotPermitted(Permission::MailRead))
    ));
    assert!(matches!(
        mail::inbox(s, &keys, &Snapshot::empty(), 0).await,
        Err(MailError::NotSignedIn)
    ));
    assert!(matches!(
        mail::send(
            s,
            &keys,
            &Snapshot::new(Some(w.bob.id), vec![]),
            at(3),
            "ann",
            "hi",
            &body("x")
        )
        .await,
        Err(MailError::NotPermitted(Permission::MailSend))
    ));
}

pub(crate) async fn a_tampered_moved_or_swapped_row_is_an_error_entry_not_a_crash<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let keys = keys();
    let first = send_one(s, &keys, &w.ann, "bob", 1).await.unwrap();
    let second = send_one(s, &keys, &w.ann, "bob", 2).await.unwrap();
    let third = send_one(s, &keys, &w.ann, "bob", 3).await.unwrap();
    let fourth = send_one(s, &keys, &w.ann, "bob", 4).await.unwrap();
    // a: the body replaced by the sealed subject (a subject presented as a body).
    h.raw(&format!(
        "UPDATE mail_message SET body = subject WHERE id = {}",
        first.id
    ))
    .await
    .unwrap();
    // b: the subject taken from message c (a sealed value moved to another message).
    h.raw(&format!(
        "UPDATE mail_message SET subject = (SELECT m.subject FROM mail_message m WHERE m.id = {}) \
         WHERE id = {}",
        third.id, second.id
    ))
    .await
    .unwrap();
    // d: moved to another mailbox, where its recipient id no longer matches what it was sealed for.
    h.raw(&format!(
        "UPDATE mail_message SET recipient_id = {} WHERE id = {}",
        w.cat.id, fourth.id
    ))
    .await
    .unwrap();

    let bob = snap(s, &w.bob).await;
    let page = mail::inbox(s, &keys, &bob, 0).await.unwrap();
    assert_eq!(page.entries.len(), 3, "a, b and c; d moved away");
    let by_id = |id: MessageId| page.entries.iter().find(|e| e.id == id).unwrap();
    assert_eq!(
        by_id(first.id).subject.as_deref(),
        Some(SUBJECT),
        "its subject is intact; its body is not"
    );
    assert_eq!(by_id(second.id).subject, None, "an error entry");
    assert_eq!(by_id(third.id).subject.as_deref(), Some(SUBJECT));
    assert!(matches!(
        mail::read(s, &keys, &bob, at(5), first.id).await,
        Err(MailError::Unreadable)
    ));
    assert!(matches!(
        mail::read(s, &keys, &bob, at(5), second.id).await,
        Err(MailError::Unreadable)
    ));
    // The moved row shows to its new mailbox as an error entry that can be deleted.
    let cat = snap(s, &w.cat).await;
    let page = mail::inbox(s, &keys, &cat, 0).await.unwrap();
    assert_eq!(
        (page.entries.len(), page.entries[0].subject.clone()),
        (1, None)
    );
    assert!(matches!(
        mail::read(s, &keys, &cat, at(5), fourth.id).await,
        Err(MailError::Unreadable)
    ));
    mail::delete(s, &cat, fourth.id).await.unwrap();
    // The untouched message still reads.
    assert_eq!(
        mail::read(s, &keys, &bob, at(5), third.id)
            .await
            .unwrap()
            .body
            .as_str(),
        TEXT
    );
    // An unreadable message was not marked read by being opened.
    assert!(!s.message(w.bob.id, second.id).await.unwrap().unwrap().read);
}

pub(crate) async fn a_different_key_cannot_open_and_start_up_checks_say_so<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let right = keys();
    // A new board has no mail, so any key will do.
    verify_keys(s, &right).await.unwrap();
    verify_keys(
        s,
        &MailKeys::single(KeyId::from_raw(7), MailKey::generate().unwrap()),
    )
    .await
    .unwrap();
    let sent = send_one(s, &right, &w.ann, "bob", 1).await.unwrap();
    send_one(s, &right, &w.ann, "bob", 2).await.unwrap();
    assert_eq!(s.key_ids_in_use().await.unwrap(), [KeyId::FIRST]);
    verify_keys(s, &right).await.unwrap();

    // Another key under the same id: it holds the id but opens nothing.
    let wrong = keys();
    assert!(matches!(
        verify_keys(s, &wrong).await,
        Err(KeyCheckError::DoesNotOpen)
    ));
    // A key set that does not hold the id the rows name.
    let other_id = MailKeys::single(KeyId::from_raw(2), MailKey::generate().unwrap());
    assert!(matches!(
        verify_keys(s, &other_id).await,
        Err(KeyCheckError::Missing(ids)) if ids == [1]
    ));
    // With the wrong key the member sees error entries, not a crash and not text.
    let bob = snap(s, &w.bob).await;
    let page = mail::inbox(s, &wrong, &bob, 0).await.unwrap();
    assert!(page.entries.iter().all(|e| e.subject.is_none()));
    assert!(matches!(
        mail::read(s, &wrong, &bob, at(3), sent.id).await,
        Err(MailError::Unreadable)
    ));
}

pub(crate) async fn unknown_and_not_active_handles_get_one_answer<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let keys = keys();
    make(s, "pat", Slug::MEMBER, AccountState::Pending).await;
    make(s, "ava", Slug::MEMBER, AccountState::Approved).await;
    let sus = make(s, "sam", Slug::MEMBER, AccountState::Active).await;
    s.change_state(
        sus.id,
        AccountState::Suspended,
        russh_bbs_core::audit::Actor::System,
    )
    .await
    .unwrap();
    for handle in ["nobody", "pat", "ava", "sam", "", "   ", &"x".repeat(80)] {
        let error = send_one(s, &keys, &w.ann, handle, 1).await.unwrap_err();
        assert!(
            matches!(error, MailError::UnknownRecipient),
            "{handle:?}: {error:?}"
        );
        assert_eq!(error.to_string(), "no such member");
    }
    // Nothing was stored for anyone, and no send was counted against the day.
    let ann = snap(s, &w.ann).await;
    assert_eq!(
        s.usage(
            w.ann.id,
            LimitKind::MailPerDay,
            russh_bbs_core::limits::Day::of(at(1))
        )
        .await
        .unwrap(),
        0
    );
    for who in [&w.bob, &w.cat, &sus] {
        assert_eq!(s.inbox_counts(who.id).await.unwrap(), (0, 0));
    }
    // A handle is matched ignoring case; mail to oneself works.
    send_one(s, &keys, &w.ann, "ANN", 2).await.unwrap();
    assert_eq!(mail::inbox(s, &keys, &ann, 0).await.unwrap().total, 1);
}

pub(crate) async fn mail_per_day_is_enforced_per_utc_day<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let keys = keys();
    let sys = snap(s, &w.sysop).await;
    let granted = sys.require(Permission::LimitsEdit, None).unwrap();
    set_member_limit(
        s,
        &granted,
        w.ann.id,
        LimitKind::MailPerDay,
        Limit::AtMost(2),
    )
    .await
    .unwrap();
    // 2_000_000 is 23 days and 13_600 s in: 72_800 s left today.
    send_one(s, &keys, &w.ann, "bob", 0).await.unwrap();
    send_one(s, &keys, &w.ann, "cat", 10).await.unwrap();
    let refused = send_one(s, &keys, &w.ann, "bob", 20).await.unwrap_err();
    let MailError::Limit(refusal) = refused else {
        panic!("not a limit: {refused:?}");
    };
    assert_eq!(refusal.kind, LimitKind::MailPerDay);
    assert_eq!(refusal.limit, 2);
    assert_eq!(
        refusal.resets_in,
        Some((86_400 - (T0 + 20) % 86_400) as u64)
    );
    assert_eq!(
        s.inbox_counts(w.bob.id).await.unwrap().0,
        1,
        "nothing was written"
    );
    // Replies count too; another member's day is their own.
    let bob = snap(s, &w.bob).await;
    let first = mail::inbox(s, &keys, &bob, 0).await.unwrap().entries[0].id;
    mail::reply(s, &keys, &bob, at(30), first, "Re: hi", &body("thanks"))
        .await
        .unwrap();
    // The next UTC day starts again.
    send_one(s, &keys, &w.ann, "bob", 86_400).await.unwrap();
    // The sysop is unlimited by default.
    for n in 0..5 {
        send_one(s, &keys, &w.sysop, "bob", 40 + n).await.unwrap();
    }
}

pub(crate) async fn the_inbox_cap_refuses_new_mail_and_deleting_makes_room<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let keys = std::sync::Arc::new(keys());
    // The sysop has no daily limit, so one sender can fill a mailbox.
    for n in 0..INBOX_CAP {
        mail::send(
            s,
            &keys,
            &snap(s, &w.sysop).await,
            at(i64::try_from(n).unwrap()),
            "bob",
            "x",
            &body("y"),
        )
        .await
        .unwrap();
    }
    assert_eq!(s.inbox_counts(w.bob.id).await.unwrap().0, INBOX_CAP);
    let full = send_one(s, &keys, &w.sysop, "bob", 999).await.unwrap_err();
    assert!(matches!(full, MailError::InboxFull), "{full:?}");
    // The store's own check refuses too (the preflight in core is only a courtesy).
    let bob = snap(s, &w.bob).await;
    let page = mail::inbox(s, &keys, &bob, 0).await.unwrap();
    assert_eq!(page.entries.len(), mail::PAGE_SIZE as usize);
    assert!(page.more);
    assert_eq!(page.total, INBOX_CAP);
    mail::delete(s, &bob, page.entries[0].id).await.unwrap();
    send_one(s, &keys, &w.sysop, "bob", 1000).await.unwrap();
    // Two sends racing for the last place: exactly one passes.
    let page = mail::inbox(s, &keys, &bob, 0).await.unwrap();
    mail::delete(s, &bob, page.entries[0].id).await.unwrap();
    let mut tasks = Vec::new();
    for n in 0..4 {
        let (store, keys_n, sys) = (s.clone(), keys.clone(), snap(s, &w.sysop).await);
        tasks.push(tokio::spawn(async move {
            mail::send(&store, &keys_n, &sys, at(2000 + n), "bob", "r", &body("r"))
                .await
                .is_ok()
        }));
    }
    let mut passed = 0;
    for task in tasks {
        passed += usize::from(task.await.unwrap());
    }
    assert_eq!(passed, 1);
    assert_eq!(s.inbox_counts(w.bob.id).await.unwrap().0, INBOX_CAP);
}

pub(crate) async fn replies_go_to_the_sender_by_account_and_only_while_they_are_active<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let keys = keys();
    let sent = send_one(s, &keys, &w.ann, "bob", 1).await.unwrap();
    let bob = snap(s, &w.bob).await;
    let view = mail::read(s, &keys, &bob, at(2), sent.id).await.unwrap();
    assert_eq!(view.reply_subject(), format!("Re: {SUBJECT}"));
    assert!(view.quoted().as_str().contains("> The kumquat"));
    let reply = mail::reply(
        s,
        &keys,
        &bob,
        at(3),
        sent.id,
        &view.reply_subject(),
        &body("yes"),
    )
    .await
    .unwrap();
    let ann = snap(s, &w.ann).await;
    let got = mail::read(s, &keys, &ann, at(4), reply.id).await.unwrap();
    assert_eq!(got.from.as_deref(), Some("bob"));
    assert_eq!(got.subject, format!("Re: {SUBJECT}"));
    // A reply to a message that is not yours, or that is gone, is a not-found.
    assert!(matches!(
        mail::reply(
            s,
            &keys,
            &snap(s, &w.cat).await,
            at(5),
            sent.id,
            "Re: x",
            &body("z")
        )
        .await,
        Err(MailError::NotFound)
    ));
    // The sender is suspended: the reply is refused with the one recipient answer.
    s.change_state(
        w.ann.id,
        AccountState::Suspended,
        russh_bbs_core::audit::Actor::System,
    )
    .await
    .unwrap();
    assert!(matches!(
        mail::reply(s, &keys, &bob, at(6), sent.id, "Re: x", &body("z")).await,
        Err(MailError::UnknownRecipient)
    ));
    // Text is refused, not cut.
    assert!(matches!(
        mail::send(s, &keys, &bob, at(7), "cat", &"s".repeat(121), &body("b")).await,
        Err(MailError::Text(_))
    ));
    assert!(matches!(
        mail::send(s, &keys, &bob, at(7), "cat", "  ", &body("b")).await,
        Err(MailError::Text(_))
    ));
    assert!(matches!(
        mail::send(s, &keys, &bob, at(7), "cat", "s", &body("   ")).await,
        Err(MailError::Text(_))
    ));
}

pub(crate) async fn reports_copy_what_the_reporter_sends_and_are_audited_by_id<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let keys = keys();
    let sent = send_one(s, &keys, &w.ann, "bob", 1).await.unwrap();
    let bob = snap(s, &w.bob).await;
    let sys = snap(s, &w.sysop).await;
    // Staff see no report, and cannot read the message, before anyone reports it.
    assert!(mail::reports(s, &sys, 0).await.unwrap().is_empty());
    assert!(matches!(
        mail::reports(s, &bob, 0).await,
        Err(MailError::NotPermitted(Permission::MailModerate))
    ));
    // Someone who does not have the message cannot report it.
    assert!(matches!(
        mail::report(s, &snap(s, &w.cat).await, at(2), sent.id, "x", &body("y")).await,
        Err(MailError::NotFound)
    ));
    // Bob reports, sending only the part he chooses (here: not the whole message).
    let report = mail::report(s, &bob, at(3), sent.id, "spam", &body("the pelican line"))
        .await
        .unwrap();
    assert!(matches!(
        mail::report(s, &bob, at(4), sent.id, "again", &body("again")).await,
        Err(MailError::AlreadyReported)
    ));
    let listed = mail::reports(s, &sys, 0).await.unwrap();
    assert_eq!(listed.len(), 1);
    let r = &listed[0];
    assert_eq!(r.id, report);
    assert_eq!(r.copy_subject, "spam");
    assert_eq!(r.copy_body.as_str(), "the pelican line");
    assert_eq!(
        (r.reporter.as_deref(), r.sender.as_deref(), r.open),
        (Some("bob"), Some("ann"), true)
    );
    // The sealed message was not touched, and the sender comes from the stored row.
    assert_eq!(s.inbox_counts(w.bob.id).await.unwrap().0, 1);
    let audit = known_events(
        s.page(&AuditQuery {
            action: Some(Action::MailReported),
            ..AuditQuery::default()
        })
        .await
        .unwrap(),
    );
    assert_eq!(audit.len(), 1);
    let event = &audit[0].event;
    assert_eq!(event.target, Target::Account(w.ann.id));
    assert_eq!(
        event.detail.as_str(),
        format!("report {report} message {}", sent.id)
    );
    for entry in all_audit(s).await {
        let text = format!("{:?}", entry.event);
        for secret in ["spam", "pelican", "kumquat"] {
            assert!(!text.contains(secret), "{secret} reached the audit log");
        }
    }
    // The report outlives the message.
    mail::delete(s, &bob, sent.id).await.unwrap();
    assert_eq!(mail::reports(s, &sys, 0).await.unwrap().len(), 1);
    // Closing is staff only, audited, and once.
    assert!(matches!(
        mail::close_report(s, &bob, report).await,
        Err(MailError::NotPermitted(Permission::MailModerate))
    ));
    mail::close_report(s, &sys, report).await.unwrap();
    assert!(matches!(
        mail::close_report(s, &sys, report).await,
        Err(MailError::NotFound)
    ));
    let closed = known_events(
        s.page(&AuditQuery {
            action: Some(Action::MailReportClosed),
            ..AuditQuery::default()
        })
        .await
        .unwrap(),
    );
    assert_eq!(closed.len(), 1);
    assert!(!mail::reports(s, &sys, 0).await.unwrap()[0].open);
}

pub(crate) async fn the_open_report_cap_counts_open_reports_only<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let keys = keys();
    let bob = snap(s, &w.bob).await;
    let sys = snap(s, &w.sysop).await;
    let mut ids = Vec::new();
    for n in 0..=MAX_OPEN_REPORTS {
        ids.push(
            mail::send(
                s,
                &keys,
                &sys,
                at(i64::try_from(n).unwrap()),
                "bob",
                "s",
                &body("b"),
            )
            .await
            .unwrap()
            .id,
        );
    }
    for id in &ids[..usize::try_from(MAX_OPEN_REPORTS).unwrap()] {
        mail::report(s, &bob, at(100), *id, "r", &body("r"))
            .await
            .unwrap();
    }
    let last = ids[usize::try_from(MAX_OPEN_REPORTS).unwrap()];
    assert!(matches!(
        mail::report(s, &bob, at(101), last, "r", &body("r")).await,
        Err(MailError::TooManyReports)
    ));
    let first = mail::reports(s, &sys, 0).await.unwrap().last().unwrap().id;
    mail::close_report(s, &sys, first).await.unwrap();
    mail::report(s, &bob, at(102), last, "r", &body("r"))
        .await
        .unwrap();
}

pub(crate) async fn mail_cannot_be_read_back_through_a_report_or_the_audit_log<H: Harness>() {
    // The whole feature, end to end, then: nothing outside the mailbox table holds its text.
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let keys = keys();
    let sent = send_one(s, &keys, &w.ann, "bob", 1).await.unwrap();
    let bob = snap(s, &w.bob).await;
    mail::report(s, &bob, at(2), sent.id, "subject copy", &body("body copy"))
        .await
        .unwrap();
    mail::delete(s, &bob, sent.id).await.unwrap();
    let audit = h
        .texts("SELECT action || ' ' || detail FROM audit_log ORDER BY id")
        .await
        .join("\n");
    for secret in [SUBJECT, "kumquat", "pelican", "copy"] {
        assert!(!audit.contains(secret), "{secret} is in the audit log");
    }
}

pub(crate) async fn mail_survives_a_reopen_and_a_closed_store_fails_closed<H: Harness>() {
    let mut h = H::fresh().await;
    let w = world(h.store()).await;
    let keys = keys();
    let sent = send_one(h.store(), &keys, &w.ann, "bob", 1).await.unwrap();
    h.reopen().await;
    let s = h.store();
    let bob = snap(s, &w.bob).await;
    assert_eq!(
        mail::read(s, &keys, &bob, at(2), sent.id)
            .await
            .unwrap()
            .body
            .as_str(),
        TEXT
    );
    h.close().await;
    let s = h.store();
    assert!(matches!(
        mail::inbox(s, &keys, &bob, 0).await,
        Err(MailError::Store(_))
    ));
}

pub(crate) async fn deleting_an_account_deletes_its_mailbox_and_keeps_the_senders_mail<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let keys = keys();
    let to_bob = send_one(s, &keys, &w.ann, "bob", 1).await.unwrap();
    let to_ann = send_one(s, &keys, &w.bob, "ann", 2).await.unwrap();
    h.raw(&format!("DELETE FROM accounts WHERE id = {}", w.bob.id))
        .await
        .unwrap();
    assert!(s.message(w.bob.id, to_bob.id).await.unwrap().is_none());
    // ann keeps what bob sent her; the sender is shown as gone, and she cannot reply to it.
    let ann = snap(s, &w.ann).await;
    let view = mail::read(s, &keys, &ann, at(3), to_ann.id).await.unwrap();
    assert_eq!(view.from, None);
    assert!(!view.can_reply);
    assert!(matches!(
        mail::reply(s, &keys, &ann, at(4), to_ann.id, "Re: x", &body("z")).await,
        Err(MailError::UnknownRecipient)
    ));
}
