//! Audit event shape, the read gate, and the sanitizing of detail text.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::events;
use super::*;
use crate::access::{
    Child, Granting, GroupChange, GroupId, NewGrant, Permission, ResourceId, ResourceKind,
    ResourceRef, Scope, Slug, Snapshot, Source, Subject, Via,
};
use crate::sanitize::RESERVED_CHROME;
use crate::store::{AccountId, AccountState, StoreError, UnixSeconds};

fn alice() -> AccountId {
    AccountId::from_raw(7)
}

#[test]
fn action_names_are_unique_area_dot_verb_and_round_trip() {
    let mut seen = HashSet::new();
    for &action in Action::ALL {
        let name = action.name();
        assert!(seen.insert(name), "{name} is used twice");
        let (area, verb) = name.split_once('.').expect("area.verb");
        assert!(!area.is_empty() && !verb.is_empty(), "{name}");
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
            "{name}"
        );
        assert_eq!(Action::parse(name), Some(action));
    }
    assert_eq!(Action::parse("nope"), None);
    assert_eq!(Action::parse(""), None);
}

#[test]
fn detail_goes_through_the_sanitizer_and_a_length_cap() {
    let hostile = format!(
        "ok\x1b[31mred\x1b[0m\x07\u{200b}\u{202e}tail\nline2\u{e0b0}{}",
        RESERVED_CHROME.iter().collect::<String>()
    );
    let detail = Detail::new(&hostile);
    let text = detail.as_str();
    assert!(!text.contains('\x1b'), "{text:?}");
    assert!(!text.contains('\x07') && !text.contains('\n'), "{text:?}");
    assert!(
        !text.contains('\u{200b}') && !text.contains('\u{202e}'),
        "{text:?}"
    );
    assert!(!text.contains('\u{e0b0}'), "{text:?}");
    assert!(
        RESERVED_CHROME.iter().all(|c| !text.contains(*c)),
        "reserved security-marker codepoints must not survive: {text:?}"
    );

    let long = Detail::new(&"x".repeat(DETAIL_MAX_CHARS * 5));
    assert_eq!(long.as_str().chars().count(), DETAIL_MAX_CHARS);
    // Characters, not bytes: a multi-byte run is cut on a boundary.
    let wide = Detail::new(&"é".repeat(DETAIL_MAX_CHARS * 2));
    assert_eq!(wide.as_str().chars().count(), DETAIL_MAX_CHARS);
}

#[test]
fn an_event_set_with_a_detail_holds_only_the_sanitized_text() {
    let event =
        AuditEvent::new(Actor::System, Action::SettingChanged, Target::None).detail("a\x1b[2Jb");
    assert!(!event.detail.as_str().contains('\x1b'));
}

#[test]
fn targets_round_trip_through_their_stored_columns() {
    let key = crate::access::Fingerprint::parse(&format!("SHA256:{:A>43}", 1)).unwrap();
    let targets = [
        Target::None,
        Target::Account(alice()),
        Target::Group(GroupId::from_raw(3)),
        Target::Role(Slug::parse("moderator").unwrap()),
        Target::Key(key.clone()),
    ];
    for target in targets {
        let (kind, reference) = target.parts();
        assert_eq!(Target::from_parts(kind, &reference), Some(target));
    }
    // A key target stores exactly the public fingerprint string.
    assert_eq!(Target::Key(key.clone()).parts().1, key.as_str());
    // Pairs the code could not have written are refused.
    for (kind, reference) in [
        ("", "x"),
        ("account", "not a number"),
        ("key", "SHA256:short"),
        ("role", "Has Spaces"),
        ("mystery", "1"),
    ] {
        assert_eq!(
            Target::from_parts(kind, reference),
            None,
            "{kind} {reference}"
        );
    }
}

#[test]
fn resources_round_trip_and_bad_pairs_are_refused() {
    let r = ResourceRef {
        kind: ResourceKind::Forum,
        id: ResourceId::new(4).unwrap(),
    };
    let event = AuditEvent::new(Actor::System, Action::GrantAdded, Target::None).on(r);
    let (kind, id) = event.resource_parts();
    assert_eq!(AuditEvent::resource_from_parts(kind, id), Some(Some(r)));
    assert_eq!(AuditEvent::resource_from_parts("", 0), Some(None));
    assert_eq!(AuditEvent::resource_from_parts("forum", 0), None);
    assert_eq!(AuditEvent::resource_from_parts("", 5), None);
    assert_eq!(AuditEvent::resource_from_parts("nope", 5), None);
}

#[test]
fn the_events_the_store_writes_say_what_changed() {
    let founder = events::founder_claimed(alice());
    assert_eq!(founder.action, Action::FounderClaimed);
    assert_eq!(founder.actor, Actor::Account(alice()));
    assert_eq!(founder.target, Target::Account(alice()));

    let created = events::account_created(
        alice(),
        &Slug::parse("member").unwrap(),
        AccountState::Active,
    );
    assert_eq!(created.actor, Actor::System);
    assert_eq!(created.detail.as_str(), "role member state active");

    let admin = Actor::Account(AccountId::from_raw(1));
    let group = GroupId::from_raw(2);
    let added = events::group_changed(
        admin,
        GroupChange::Add {
            group,
            child: Child::Account(alice()),
        },
        None,
    );
    assert_eq!(added.action, Action::GroupMemberAdded);
    assert_eq!(added.target, Target::Group(group));
    assert_eq!(added.detail.as_str(), "account 7");
    let deleted = events::group_changed(
        admin,
        GroupChange::Delete(group),
        Some(&Slug::parse("lounge").unwrap()),
    );
    assert_eq!(deleted.action, Action::GroupDeleted);
    assert_eq!(deleted.detail.as_str(), "name lounge");

    let resource = ResourceRef {
        kind: ResourceKind::Room,
        id: ResourceId::new(9).unwrap(),
    };
    let grant = events::grant_added(
        admin,
        &NewGrant {
            subject: Subject::Account(alice()),
            granting: Granting::Permission(Permission::ForumRead),
            scope: Scope::Resource(resource),
        },
    );
    assert_eq!(grant.action, Action::GrantAdded);
    assert_eq!(grant.target, Target::Account(alice()));
    assert_eq!(grant.resource, Some(resource));
    assert_eq!(grant.detail.as_str(), "permission forum.read");

    let setting = events::terminal_settings_changed(alice());
    assert_eq!(setting.action, Action::SettingChanged);
    assert_eq!(setting.actor, Actor::Account(alice()));
}

#[test]
fn the_query_page_size_is_bounded() {
    let mut q = AuditQuery::default();
    assert_eq!(q.page_size(), MAX_PAGE);
    q.limit = 0;
    assert_eq!(q.page_size(), 1);
    q.limit = u32::MAX;
    assert_eq!(q.page_size(), MAX_PAGE);
    q.limit = 10;
    assert_eq!(q.page_size(), 10);
}

#[test]
fn the_fixed_clock_is_set_by_hand() {
    let clock = FixedClock::new(100);
    assert_eq!(clock.now(), UnixSeconds::from_raw(100));
    clock.set(250);
    assert_eq!(clock.now(), UnixSeconds::from_raw(250));
}

/// A store that counts reads, to prove a denied read never reaches it.
#[derive(Default)]
struct CountingStore {
    pages: AtomicUsize,
}

impl AuditStore for CountingStore {
    async fn append(&self, _: &AuditEvent) -> Result<AuditId, StoreError> {
        Ok(AuditId::from_raw(1))
    }

    async fn page(&self, _: &AuditQuery) -> Result<Vec<AuditEntry>, StoreError> {
        self.pages.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}

fn holding(permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(alice()),
        &Slug::parse("cosysop").unwrap(),
        40,
        &Via::Account,
        permissions.iter().copied(),
    )
}

#[tokio::test]
async fn reading_the_log_needs_audit_view() {
    let store = CountingStore::default();
    let query = AuditQuery::default();

    for without in [
        Snapshot::empty(),
        holding(&[Permission::MembersView, Permission::SettingsEdit]),
    ] {
        let result = view_log(&store, &without, &query).await;
        assert!(
            matches!(result, Err(AuditViewError::Denied(d)) if d.permission == Permission::AuditView),
            "{result:?}"
        );
    }
    assert_eq!(
        store.pages.load(Ordering::SeqCst),
        0,
        "a denied read must not reach the store"
    );

    let with = holding(&[Permission::AuditView]);
    assert!(view_log(&store, &with, &query).await.unwrap().is_empty());
    assert_eq!(store.pages.load(Ordering::SeqCst), 1);
}

#[test]
fn a_permission_on_one_resource_does_not_open_the_log() {
    // `audit.view` is checked globally; holding it on a forum board is not the log.
    let resource = ResourceRef {
        kind: ResourceKind::Forum,
        id: ResourceId::new(1).unwrap(),
    };
    let snapshot = Snapshot::new(
        Some(alice()),
        vec![crate::access::Held {
            permission: Permission::AuditView,
            scope: Scope::Resource(resource),
            source: Source::Role {
                role: Slug::parse("moderator").unwrap(),
                rank: 30,
                via: Via::Account,
            },
        }],
    );
    assert!(snapshot.require(Permission::AuditView, None).is_err());
}
