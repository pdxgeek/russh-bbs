//! Drafts against any backend (M3-B, D80): per member and per context, capped, sanitized on
//! save, gone with the account.
#![allow(clippy::unwrap_used, clippy::panic)]

use russh_bbs_core::access::Slug;
use russh_bbs_core::drafts::{
    BodyError, BodyLimits, ContextKey, DraftError, DraftStore, MAX_BODY_CHARS,
    MAX_DRAFTS_PER_MEMBER, SaveOutcome, discard_draft, list_drafts, load_draft, save_draft,
};
use russh_bbs_core::sanitize::{RESERVED_CHROME, sanitize_username};
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, StoreError, UnixSeconds,
};

use super::Harness;

async fn active<S: AccountStore>(store: &S, name: &str) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

fn key(text: &str) -> ContextKey {
    ContextKey::parse(text).unwrap()
}

const T0: UnixSeconds = UnixSeconds::from_raw(1_000);

async fn save<S: DraftStore>(
    s: &S,
    who: AccountId,
    context: &str,
    text: &str,
    at: i64,
) -> Result<SaveOutcome, DraftError> {
    save_draft(
        s,
        who,
        &key(context),
        text,
        BodyLimits::MAX,
        UnixSeconds::from_raw(at),
    )
    .await
}

pub(crate) async fn a_saved_draft_reads_back_and_a_second_save_replaces_it<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    assert_eq!(load_draft(s, ann.id, &key("mail:new")).await.unwrap(), None);
    assert_eq!(
        save_draft(
            s,
            ann.id,
            &key("mail:new"),
            "first\nsecond",
            BodyLimits::MAX,
            T0
        )
        .await
        .unwrap(),
        SaveOutcome::Created
    );
    let draft = load_draft(s, ann.id, &key("mail:new"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(draft.body.as_str(), "first\nsecond");
    assert_eq!(draft.updated_at, T0);
    assert_eq!(draft.context, key("mail:new"));
    assert_eq!(
        save(s, ann.id, "mail:new", "third", 2_000).await.unwrap(),
        SaveOutcome::Replaced
    );
    let draft = load_draft(s, ann.id, &key("mail:new"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(draft.body.as_str(), "third");
    assert_eq!(draft.updated_at, UnixSeconds::from_raw(2_000));
    assert_eq!(list_drafts(s, ann.id).await.unwrap().len(), 1);
}

pub(crate) async fn drafts_are_per_member_and_per_context<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let (ann, bob) = (active(s, "ann").await, active(s, "bob").await);
    save(s, ann.id, "forum:reply:1", "ann one", 10)
        .await
        .unwrap();
    save(s, ann.id, "forum:reply:2", "ann two", 20)
        .await
        .unwrap();
    save(s, bob.id, "forum:reply:1", "bob one", 30)
        .await
        .unwrap();
    let body =
        |draft: Option<russh_bbs_core::drafts::Draft>| draft.map(|d| d.body.as_str().to_owned());
    assert_eq!(
        body(load_draft(s, ann.id, &key("forum:reply:1")).await.unwrap()),
        Some("ann one".into())
    );
    assert_eq!(
        body(load_draft(s, ann.id, &key("forum:reply:2")).await.unwrap()),
        Some("ann two".into())
    );
    assert_eq!(
        body(load_draft(s, bob.id, &key("forum:reply:1")).await.unwrap()),
        Some("bob one".into())
    );
    assert_eq!(
        body(load_draft(s, bob.id, &key("forum:reply:2")).await.unwrap()),
        None
    );
    // Deleting one member's draft touches nobody else's.
    assert!(
        discard_draft(s, ann.id, &key("forum:reply:1"))
            .await
            .unwrap()
    );
    assert!(
        !discard_draft(s, ann.id, &key("forum:reply:1"))
            .await
            .unwrap()
    );
    assert_eq!(
        body(load_draft(s, bob.id, &key("forum:reply:1")).await.unwrap()),
        Some("bob one".into())
    );
    assert_eq!(list_drafts(s, ann.id).await.unwrap().len(), 1);
}

pub(crate) async fn the_list_is_newest_first_without_text<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    save(s, ann.id, "c:old", "old text", 100).await.unwrap();
    save(s, ann.id, "c:new", "newer text here", 300)
        .await
        .unwrap();
    save(s, ann.id, "c:mid", "mid", 200).await.unwrap();
    let list = list_drafts(s, ann.id).await.unwrap();
    let order: Vec<&str> = list.iter().map(|d| d.context.as_str()).collect();
    assert_eq!(order, ["c:new", "c:mid", "c:old"]);
    assert_eq!(list.first().unwrap().chars, "newer text here".len());
    assert_eq!(list.first().unwrap().updated_at, UnixSeconds::from_raw(300));
}

pub(crate) async fn the_cap_refuses_a_new_context_but_always_allows_a_replacement<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let (ann, bob) = (active(s, "ann").await, active(s, "bob").await);
    for n in 0..MAX_DRAFTS_PER_MEMBER {
        save(s, ann.id, &format!("c:{n}"), "text", 10)
            .await
            .unwrap();
    }
    assert!(matches!(
        save(s, ann.id, "c:one-too-many", "text", 10).await,
        Err(DraftError::TooMany)
    ));
    assert_eq!(
        list_drafts(s, ann.id).await.unwrap().len(),
        MAX_DRAFTS_PER_MEMBER
    );
    // At the cap, replacing a draft that exists still works.
    assert_eq!(
        save(s, ann.id, "c:0", "changed", 20).await.unwrap(),
        SaveOutcome::Replaced
    );
    // The cap is per member.
    save(s, bob.id, "c:0", "bob's", 10).await.unwrap();
    // Freeing a slot (a discard, or blank text) makes room.
    assert!(discard_draft(s, ann.id, &key("c:1")).await.unwrap());
    save(s, ann.id, "c:one-too-many", "text", 30).await.unwrap();
    assert_eq!(
        save(s, ann.id, "c:2", "   \n ", 40).await.unwrap(),
        SaveOutcome::Removed
    );
    save(s, ann.id, "c:again", "text", 50).await.unwrap();
    assert_eq!(
        list_drafts(s, ann.id).await.unwrap().len(),
        MAX_DRAFTS_PER_MEMBER
    );
}

pub(crate) async fn concurrent_saves_never_pass_the_cap<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let mut tasks = Vec::new();
    for n in 0..(MAX_DRAFTS_PER_MEMBER + 12) {
        let store = s.clone();
        let id = ann.id;
        tasks.push(tokio::spawn(async move {
            save(&store, id, &format!("race:{n}"), "text", 10).await
        }));
    }
    let (mut saved, mut refused) = (0, 0);
    for task in tasks {
        match task.await.unwrap() {
            Ok(SaveOutcome::Created) => saved += 1,
            Err(DraftError::TooMany) => refused += 1,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(saved, MAX_DRAFTS_PER_MEMBER);
    assert_eq!(refused, 12);
    assert_eq!(
        list_drafts(s, ann.id).await.unwrap().len(),
        MAX_DRAFTS_PER_MEMBER
    );
}

pub(crate) async fn the_text_is_sanitized_on_save_and_bounded<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let hostile =
        "hi\u{1b}[31m red\u{7}\r\nmark \u{27E6}\u{2299}\u{27E7} glyph \u{e000}\u{f0001} end";
    save(s, ann.id, "c:x", hostile, 10).await.unwrap();
    // What the database holds, not only what the API returns.
    let stored = h
        .texts("SELECT CAST(body AS TEXT) FROM draft")
        .await
        .into_iter()
        .next()
        .unwrap();
    assert!(!stored.contains('\u{1b}') && !stored.contains('\u{7}') && !stored.contains('\r'));
    assert!(!stored.chars().any(|c| RESERVED_CHROME.contains(&c)));
    assert!(!stored.contains('\u{e000}') && !stored.contains('\u{f0001}'));
    assert!(stored.starts_with("hi red\nmark"), "{stored:?}");
    // Over the caller's limits: refused, and nothing is stored or replaced.
    let narrow = BodyLimits::new(5, 2);
    let too_long = save_draft(s, ann.id, &key("c:x"), "123456", narrow, T0).await;
    assert!(matches!(
        too_long,
        Err(DraftError::Body(BodyError::TooLong { max: 5 }))
    ));
    let too_many = save_draft(s, ann.id, &key("c:x"), "a\nb\nc", narrow, T0).await;
    assert!(matches!(
        too_many,
        Err(DraftError::Body(BodyError::TooManyLines { max: 2 }))
    ));
    let huge = "x".repeat(MAX_BODY_CHARS + 1);
    assert!(matches!(
        save(s, ann.id, "c:y", &huge, 10).await,
        Err(DraftError::Body(BodyError::TooLong { .. }))
    ));
    let stored = load_draft(s, ann.id, &key("c:x")).await.unwrap().unwrap();
    assert!(stored.body.as_str().starts_with("hi"));
    assert_eq!(load_draft(s, ann.id, &key("c:y")).await.unwrap(), None);
    let at_the_limit = "y".repeat(MAX_BODY_CHARS);
    save(s, ann.id, "c:y", &at_the_limit, 10).await.unwrap();
    assert_eq!(
        load_draft(s, ann.id, &key("c:y"))
            .await
            .unwrap()
            .unwrap()
            .body
            .chars(),
        MAX_BODY_CHARS
    );
}

pub(crate) async fn blank_text_removes_the_draft<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    save(s, ann.id, "c:x", "something", 10).await.unwrap();
    assert_eq!(
        save(s, ann.id, "c:x", " \n\t", 20).await.unwrap(),
        SaveOutcome::Removed
    );
    assert_eq!(load_draft(s, ann.id, &key("c:x")).await.unwrap(), None);
    // Blank text for a context with no draft is also a no-op, and stores nothing.
    assert_eq!(
        save(s, ann.id, "c:none", "", 20).await.unwrap(),
        SaveOutcome::Removed
    );
    assert_eq!(list_drafts(s, ann.id).await.unwrap().len(), 0);
}

pub(crate) async fn saving_a_draft_for_an_unknown_account_fails<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let result = save(s, AccountId::from_raw(9_999), "c:x", "text", 10).await;
    assert!(matches!(
        result,
        Err(DraftError::Store(StoreError::AccountNotFound))
    ));
}

pub(crate) async fn deleting_an_account_deletes_its_drafts<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let (ann, bob) = (active(s, "ann").await, active(s, "bob").await);
    save(s, ann.id, "c:x", "ann", 10).await.unwrap();
    save(s, bob.id, "c:x", "bob", 10).await.unwrap();
    h.raw(&format!(
        "DELETE FROM accounts WHERE id = {}",
        ann.id.as_raw()
    ))
    .await
    .unwrap();
    assert_eq!(
        h.texts("SELECT CAST(count(*) AS TEXT) FROM draft").await,
        ["1"]
    );
    assert!(load_draft(s, bob.id, &key("c:x")).await.unwrap().is_some());
}

pub(crate) async fn the_database_refuses_what_core_would_never_write<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let id = ann.id.as_raw();
    let insert = |context: &str, body: &str| {
        format!(
            "INSERT INTO draft (account_id, context, body, updated_at) VALUES ({id}, '{context}', '{body}', 1)"
        )
    };
    assert!(h.raw(&insert("ok", "x")).await.is_ok());
    assert!(h.raw(&insert("", "x")).await.is_err());
    assert!(h.raw(&insert(&"c".repeat(65), "x")).await.is_err());
    assert!(h.raw(&insert("empty-body", "")).await.is_err());
    assert!(h.raw(&insert("huge", &"b".repeat(16_001))).await.is_err());
    assert!(
        h.raw(&insert("ok", "again")).await.is_err(),
        "one row per context"
    );
    assert!(
        h.raw(
            "INSERT INTO draft (account_id, context, body, updated_at) VALUES (999999, 'x', 'x', 1)"
        )
        .await
        .is_err()
    );
}

pub(crate) async fn a_stored_row_the_code_could_not_have_written_is_corrupt_not_shown<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let id = ann.id.as_raw();
    h.raw(&format!(
        "INSERT INTO draft (account_id, context, body, updated_at) VALUES ({id}, 'bad key!', 'x', 1)"
    ))
    .await
    .unwrap();
    assert!(matches!(
        list_drafts(s, ann.id).await,
        Err(DraftError::Store(StoreError::Corrupt(_)))
    ));
    // A body edited by hand to carry an escape is cleaned when read, never shown raw.
    h.raw(&format!("DELETE FROM draft WHERE account_id = {id}"))
        .await
        .unwrap();
    h.raw(&format!(
        "INSERT INTO draft (account_id, context, body, updated_at) VALUES ({id}, 'edited', 'a\u{1b}[2Jb', 1)"
    ))
    .await
    .unwrap();
    let draft = load_draft(s, ann.id, &key("edited"))
        .await
        .unwrap()
        .unwrap();
    assert!(!draft.body.as_str().contains('\u{1b}'));
    assert_eq!(draft.body.as_str(), "ab");
}

pub(crate) async fn drafts_survive_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    let ann = active(h.store(), "ann").await;
    save(h.store(), ann.id, "c:x", "kept\nacross", 10)
        .await
        .unwrap();
    h.reopen().await;
    let draft = load_draft(h.store(), ann.id, &key("c:x"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(draft.body.as_str(), "kept\nacross");
}

pub(crate) async fn a_broken_store_errors_instead_of_pretending_there_is_no_draft<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    save(s, ann.id, "c:x", "text", 10).await.unwrap();
    h.close().await;
    assert!(matches!(
        load_draft(s, ann.id, &key("c:x")).await,
        Err(DraftError::Store(_))
    ));
    assert!(save(s, ann.id, "c:y", "text", 10).await.is_err());
    assert!(list_drafts(s, ann.id).await.is_err());
    assert!(discard_draft(s, ann.id, &key("c:x")).await.is_err());
}
