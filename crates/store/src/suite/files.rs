//! File areas against any backend (M4-F2): areas as access resources, quarantine, the scan hook,
//! approve and reject, the upload limits, tags, search and download lists. Bytes go through a
//! real local blob store, so approving really moves them.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

use russh_bbs_core::access::{Permission, Slug, Snapshot};
use russh_bbs_core::audit::{Action, Actor, AuditQuery, AuditStore, known_events};
use russh_bbs_core::blobs::{BlobKey, BlobStore};
use russh_bbs_core::files::{
    self, AreaId, FileError, FileId, FileState, FileStore, ScanHook, ScanOutcome, ScanState,
    Uploaded,
};
use russh_bbs_core::limits::{Limit, LimitKind, LimitStore};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, UnixSeconds,
};
use tokio::io::{AsyncRead, AsyncReadExt};

use super::{Harness, TestStore};
use crate::blobs::LocalBlobStore;

const T0: i64 = 1_000_000;

fn at(n: i64) -> UnixSeconds {
    UnixSeconds::from_raw(T0 + n)
}

async fn make<S: AccountStore>(store: &S, name: &str, role: Slug) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

async fn snap<S: TestStore>(store: &S, who: &Account) -> Snapshot {
    store.snapshot(Some(who.id)).await.unwrap()
}

/// A scanner that answers what the test says.
struct Fixed(ScanOutcome);

impl ScanHook for Fixed {
    async fn scan<R>(&self, _size: u64, mut body: R) -> ScanOutcome
    where
        R: AsyncRead + Send + Unpin,
    {
        // Read it all, as a real scanner would.
        let mut sink = Vec::new();
        body.read_to_end(&mut sink).await.unwrap();
        self.0.clone()
    }
}

async fn blobs(dir: &tempfile::TempDir) -> LocalBlobStore {
    LocalBlobStore::open(dir.path(), 1 << 30).await.unwrap()
}

/// Upload `bytes` as `name` the way the SFTP front end does: plan, meter, put, record.
async fn upload<S: TestStore, H: ScanHook>(
    s: &S,
    store: &LocalBlobStore,
    hook: &H,
    who: &Snapshot,
    area: AreaId,
    name: &str,
    bytes: &[u8],
    now: UnixSeconds,
) -> Result<Uploaded, FileError> {
    let plan = files::plan_upload(s, who, area, name, now).await?;
    let mut meter = plan.meter();
    meter.accept(s, now, bytes.len()).await?;
    let meta = store.put(&plan.key, bytes, &plan.put_options()).await?;
    files::record_upload(s, store, hook, &plan, &meta, now).await
}

async fn read_all(store: &LocalBlobStore, key: &BlobKey) -> Vec<u8> {
    let mut out = Vec::new();
    store
        .get(key)
        .await
        .unwrap()
        .reader
        .read_to_end(&mut out)
        .await
        .unwrap();
    out
}

struct World {
    sysop: Account,
    mod_: Account,
    ann: Account,
    bob: Account,
    docs: AreaId,
    staff: AreaId,
}

async fn world<S: TestStore>(s: &S) -> World {
    let sysop = make(s, "ops", Slug::SYSOP).await;
    let mod_ = make(s, "mo", Slug::MODERATOR).await;
    let ann = make(s, "ann", Slug::MEMBER).await;
    let bob = make(s, "bob", Slug::MEMBER).await;
    let sys = snap(s, &sysop).await;
    let docs = files::create_area(s, &sys, at(0), "Docs", "Manuals", false, false)
        .await
        .unwrap();
    let staff = files::create_area(s, &sys, at(0), "Staff room", "Keys and rotas", true, false)
        .await
        .unwrap();
    files::allow_member(s, &sys, staff, "ann").await.unwrap();
    World {
        sysop,
        mod_,
        ann,
        bob,
        docs,
        staff,
    }
}

pub(crate) async fn areas_are_created_by_managers_only_with_unique_names_and_an_audit_row<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let ann = snap(s, &w.ann).await;
    assert!(matches!(
        files::create_area(s, &ann, at(1), "Mine", "", false, false).await,
        Err(FileError::NotPermitted(Permission::FilesManageAreas))
    ));
    let sys = snap(s, &w.sysop).await;
    assert!(matches!(
        files::create_area(s, &sys, at(1), "  docs ", "", false, false).await,
        Err(FileError::NameTaken)
    ));
    assert!(matches!(
        files::create_area(s, &sys, at(1), "   ", "", false, false).await,
        Err(FileError::Text(_))
    ));
    let rows = known_events(
        s.page(&AuditQuery {
            action: Some(Action::FileAreaCreated),
            ..AuditQuery::default()
        })
        .await
        .unwrap(),
    );
    assert_eq!(rows.len(), 2, "one audit row per area, none for refusals");
    for entry in &rows {
        assert!(!entry.event.detail.as_str().contains("Docs"));
    }
}

pub(crate) async fn quarantine_is_invisible_to_others_until_approved<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let dir = tempfile::tempdir().unwrap();
    let store = blobs(&dir).await;
    let w = world(s).await;
    let (ann, bob) = (snap(s, &w.ann).await, snap(s, &w.bob).await);
    let moderator = snap(s, &w.mod_).await;
    let up = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        w.docs,
        "manual.txt",
        b"hello files",
        at(1),
    )
    .await
    .unwrap();
    assert_eq!(up.state, FileState::Quarantine);
    assert_eq!(up.scan, ScanState::Unscanned);

    // Nobody but the uploader and approvers sees it: not in the listing, not by id, not in a
    // search, not for download, not in the queue.
    assert!(files::listing(s, &bob, w.docs).await.unwrap().is_empty());
    assert!(
        files::pending_listing(s, &bob, w.docs)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        files::file_view(s, &bob, up.id).await,
        Err(FileError::NotFound)
    ));
    assert!(matches!(
        files::open_download(s, &bob, up.id).await,
        Err(FileError::NotFound)
    ));
    assert!(matches!(
        files::list_add(s, &bob, up.id, at(2)).await,
        Err(FileError::NotFound)
    ));
    assert!(matches!(
        files::tag(s, &bob, up.id, "x").await,
        Err(FileError::NotFound)
    ));
    assert!(matches!(
        files::approve(s, &store, &bob, up.id, at(2)).await,
        Err(FileError::NotFound)
    ));
    assert!(
        files::search(s, &bob, "manual", 0)
            .await
            .unwrap()
            .files
            .is_empty()
    );
    assert!(files::queue(s, &bob, 0).await.unwrap().files.is_empty());
    // The uploader sees it as theirs, and an approver sees it in the queue.
    let own = files::pending_listing(s, &ann, w.docs).await.unwrap();
    assert_eq!(own.len(), 1);
    assert!(
        files::area_page(s, &ann, w.docs, 0)
            .await
            .unwrap()
            .files
            .is_empty()
    );
    let queued = files::queue(s, &moderator, 0).await.unwrap();
    assert_eq!(queued.files.len(), 1);
    // The uploader may read their own quarantined bytes; they are still in quarantine.
    let target = files::open_download(s, &ann, up.id).await.unwrap();
    assert_eq!(target.key.namespace().as_str(), "quarantine");
    assert_eq!(read_all(&store, &target.key).await, b"hello files");

    // A member who is not an approver cannot approve their own upload.
    assert!(matches!(
        files::approve(s, &store, &ann, up.id, at(2)).await,
        Err(FileError::NotPermitted(Permission::FilesApprove))
    ));
    files::approve(s, &store, &moderator, up.id, at(3))
        .await
        .unwrap();

    // Now everyone who may list and download sees it, and the bytes moved.
    let page = files::area_page(s, &bob, w.docs, 0).await.unwrap();
    assert_eq!(page.files.len(), 1);
    assert_eq!(page.area.unwrap().files, 1);
    let view = files::file_view(s, &bob, up.id).await.unwrap();
    assert!(
        view.download_path
            .unwrap()
            .ends_with(&format!("/{}-manual.txt", up.id))
    );
    let target = files::open_download(s, &bob, up.id).await.unwrap();
    assert_eq!(target.key.namespace().as_str(), "files");
    assert_eq!(read_all(&store, &target.key).await, b"hello files");
    assert!(
        store
            .head(&BlobKey::parse(&format!("quarantine/{}", target.key.id().as_str())).unwrap())
            .await
            .is_err(),
        "the quarantine copy is gone"
    );
    assert!(
        files::queue(s, &moderator, 0)
            .await
            .unwrap()
            .files
            .is_empty()
    );
    assert_eq!(s.file(up.id).await.unwrap().unwrap().downloads, 1);
    // Deciding twice is refused and changes nothing.
    assert!(matches!(
        files::approve(s, &store, &moderator, up.id, at(4)).await,
        Err(FileError::NotPending)
    ));
    let rows = known_events(
        s.page(&AuditQuery {
            action: Some(Action::FileApproved),
            ..AuditQuery::default()
        })
        .await
        .unwrap(),
    );
    assert_eq!(rows.len(), 1);
    assert!(
        !rows[0].event.detail.as_str().contains("manual"),
        "ids only"
    );
}

pub(crate) async fn reject_deletes_the_bytes_and_hides_the_row<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let dir = tempfile::tempdir().unwrap();
    let store = blobs(&dir).await;
    let w = world(s).await;
    let (ann, moderator) = (snap(s, &w.ann).await, snap(s, &w.mod_).await);
    let up = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        w.docs,
        "bad.bin",
        b"nope",
        at(1),
    )
    .await
    .unwrap();
    let key = files::open_download(s, &ann, up.id).await.unwrap().key;
    files::reject(s, &store, &moderator, up.id, at(2))
        .await
        .unwrap();
    assert!(store.head(&key).await.is_err(), "the bytes are gone");
    let row = s.file(up.id).await.unwrap().unwrap();
    assert_eq!(row.state, FileState::Rejected);
    for who in [&ann, &moderator] {
        assert!(matches!(
            files::file_view(s, who, up.id).await,
            Err(FileError::NotFound)
        ));
        assert!(matches!(
            files::open_download(s, who, up.id).await,
            Err(FileError::NotFound)
        ));
    }
    assert!(
        files::pending_listing(s, &ann, w.docs)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        known_events(
            s.page(&AuditQuery {
                action: Some(Action::FileRejected),
                ..AuditQuery::default()
            })
            .await
            .unwrap()
        )
        .len(),
        1
    );
}

pub(crate) async fn the_scan_decides_what_auto_approve_may_do<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let dir = tempfile::tempdir().unwrap();
    let store = blobs(&dir).await;
    let w = world(s).await;
    let sys = snap(s, &w.sysop).await;
    let ann = snap(s, &w.ann).await;
    let auto = files::create_area(s, &sys, at(0), "Fast lane", "", false, true)
        .await
        .unwrap();

    // Clean in an auto-approve area: live at once, scanned clean.
    let up = upload(
        s,
        &store,
        &Fixed(ScanOutcome::Clean),
        &ann,
        auto,
        "ok.txt",
        b"fine",
        at(1),
    )
    .await
    .unwrap();
    assert_eq!((up.state, up.scan), (FileState::Approved, ScanState::Clean));
    // No scanner: auto-approve still applies, and the file is labelled unscanned.
    let up2 = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        auto,
        "raw.txt",
        b"raw",
        at(1),
    )
    .await
    .unwrap();
    assert_eq!(
        (up2.state, up2.scan),
        (FileState::Approved, ScanState::Unscanned)
    );
    // Infected: rejected whatever the area says, bytes gone, the signature kept (cleaned).
    let bad = upload(
        s,
        &store,
        &Fixed(ScanOutcome::Infected("Eicar-\u{1b}[31mTest".into())),
        &ann,
        auto,
        "virus.com",
        b"X5O!",
        at(1),
    )
    .await
    .unwrap();
    assert_eq!(
        (bad.state, bad.scan),
        (FileState::Rejected, ScanState::Infected)
    );
    assert!(!bad.note.contains('\u{1b}'));
    let row = s.file(bad.id).await.unwrap().unwrap();
    assert!(
        store
            .head(&BlobKey::parse(&row.blob_key).unwrap())
            .await
            .is_err()
    );
    assert_eq!(
        known_events(
            s.page(&AuditQuery {
                action: Some(Action::FileRejected),
                ..AuditQuery::default()
            })
            .await
            .unwrap()
        )[0]
        .event
        .detail
        .as_str(),
        format!("file {} infected", bad.id)
    );
    // A scanner that cannot answer never auto-approves: the file waits for staff.
    let failed = upload(
        s,
        &store,
        &Fixed(ScanOutcome::Failed("clamd is down".into())),
        &ann,
        auto,
        "maybe.txt",
        b"hm",
        at(1),
    )
    .await
    .unwrap();
    assert_eq!(
        (failed.state, failed.scan),
        (FileState::Quarantine, ScanState::Failed)
    );
    // In a manual area a clean file still waits.
    let manual = upload(
        s,
        &store,
        &Fixed(ScanOutcome::Clean),
        &ann,
        w.docs,
        "m.txt",
        b"m",
        at(1),
    )
    .await
    .unwrap();
    assert_eq!(
        (manual.state, manual.scan),
        (FileState::Quarantine, ScanState::Clean)
    );
}

pub(crate) async fn restricted_areas_are_not_reached_through_a_global_holding<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let dir = tempfile::tempdir().unwrap();
    let store = blobs(&dir).await;
    let w = world(s).await;
    let (ann, bob, sys) = (
        snap(s, &w.ann).await,
        snap(s, &w.bob).await,
        snap(s, &w.sysop).await,
    );
    let up = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        w.staff,
        "keys.txt",
        b"k",
        at(1),
    )
    .await
    .unwrap();
    files::approve(s, &store, &sys, up.id, at(2)).await.unwrap();
    let names =
        |list: &[files::AreaSummary]| list.iter().map(|a| a.name.clone()).collect::<Vec<_>>();
    assert_eq!(
        names(&files::areas(s, &ann).await.unwrap()),
        ["Docs", "Staff room"]
    );
    assert_eq!(names(&files::areas(s, &bob).await.unwrap()), ["Docs"]);
    // Bob gets the same answer for the area that exists and for one that does not.
    let missing = AreaId::from_raw(9_999);
    for id in [w.staff, missing] {
        assert!(matches!(
            files::area_page(s, &bob, id, 0).await,
            Err(FileError::NotFound)
        ));
        assert!(matches!(
            files::listing(s, &bob, id).await,
            Err(FileError::NotFound)
        ));
        assert!(matches!(
            files::pending_listing(s, &bob, id).await,
            Err(FileError::NotFound)
        ));
        assert!(matches!(
            files::plan_upload(s, &bob, id, "x.txt", at(3))
                .await
                .map(|_| ()),
            Err(FileError::NotFound)
        ));
    }
    assert!(matches!(
        files::file_view(s, &bob, up.id).await,
        Err(FileError::NotFound)
    ));
    assert!(matches!(
        files::open_download(s, &bob, up.id).await,
        Err(FileError::NotFound)
    ));
    assert!(
        files::search(s, &bob, "keys", 0)
            .await
            .unwrap()
            .files
            .is_empty()
    );
    assert_eq!(
        files::search(s, &ann, "keys", 0).await.unwrap().files.len(),
        1
    );
    assert!(files::queue(s, &bob, 0).await.unwrap().files.is_empty());
    // Staff who manage areas reach it too.
    assert_eq!(files::areas(s, &sys).await.unwrap().len(), 2);
}

pub(crate) async fn the_upload_limits_are_enforced_per_file_and_per_day<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let dir = tempfile::tempdir().unwrap();
    let store = blobs(&dir).await;
    let w = world(s).await;
    let sys = snap(s, &w.sysop).await;
    s.set_member_limit(
        w.ann.id,
        LimitKind::UploadBytesPerFile,
        Some(Limit::AtMost(100)),
        Actor::System,
    )
    .await
    .unwrap();
    s.set_member_limit(
        w.ann.id,
        LimitKind::UploadBytesPerDay,
        Some(Limit::AtMost(150)),
        Actor::System,
    )
    .await
    .unwrap();
    let ann = snap(s, &w.ann).await;
    let up = |name: &'static str, len: usize, now: i64| {
        let (s, store, ann, docs) = (s.clone(), store.clone(), ann.clone(), w.docs);
        async move {
            upload(
                &s,
                &store,
                &files::NoScan,
                &ann,
                docs,
                name,
                &vec![7_u8; len],
                at(now),
            )
            .await
        }
    };
    // Over the per-file limit: refused when the bytes arrive, nothing stored.
    match up("big.bin", 101, 1).await {
        Err(FileError::Limit(refusal)) => assert_eq!(refusal.kind, LimitKind::UploadBytesPerFile),
        other => panic!("{other:?}"),
    }
    assert!(s.pending_in(&[w.docs], 10, 0).await.unwrap().is_empty());
    // Exactly at the limit is fine; the day's count is the 100 bytes (reserved up to the cap).
    up("ok1.bin", 100, 2).await.unwrap();
    // The next 100 would pass the day's 150: refused on the first byte past it, per-day.
    match up("ok2.bin", 100, 3).await {
        Err(FileError::Limit(refusal)) => {
            assert_eq!(refusal.kind, LimitKind::UploadBytesPerDay);
            assert!(refusal.resets_in.is_some());
        }
        other => panic!("{other:?}"),
    }
    // 50 more fits exactly; then the day is used up and planning refuses before any byte.
    up("ok3.bin", 50, 4).await.unwrap();
    assert!(matches!(
        files::plan_upload(s, &ann, w.docs, "late.bin", at(5)).await.map(|_| ()),
        Err(FileError::Limit(r)) if r.kind == LimitKind::UploadBytesPerDay
    ));
    // The next UTC day starts fresh.
    let tomorrow = at(86_400);
    up("next.bin", 10, 86_400).await.unwrap();
    let day = russh_bbs_core::limits::Day::of(tomorrow);
    assert!(
        s.usage(w.ann.id, LimitKind::UploadBytesPerDay, day)
            .await
            .unwrap()
            >= 10
    );
    // The sysop is unlimited.
    let big = vec![1_u8; 6 * 1024 * 1024];
    upload(
        s,
        &store,
        &files::NoScan,
        &sys,
        w.docs,
        "huge.bin",
        &big,
        at(6),
    )
    .await
    .unwrap();
    // A member at the default limits cannot send 6 MiB.
    assert!(matches!(
        upload(s, &store, &files::NoScan, &snap(s, &w.bob).await, w.docs, "huge.bin", &big, at(6)).await,
        Err(FileError::Limit(r)) if r.kind == LimitKind::UploadBytesPerFile
    ));
}

pub(crate) async fn tags_descriptions_and_search_obey_access<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let dir = tempfile::tempdir().unwrap();
    let store = blobs(&dir).await;
    let w = world(s).await;
    let (ann, bob, sys) = (
        snap(s, &w.ann).await,
        snap(s, &w.bob).await,
        snap(s, &w.sysop).await,
    );
    let a = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        w.docs,
        "Router manual.pdf",
        b"a",
        at(1),
    )
    .await
    .unwrap();
    let b = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        w.staff,
        "Router keys.txt",
        b"b",
        at(1),
    )
    .await
    .unwrap();
    for id in [a.id, b.id] {
        files::approve(s, &store, &sys, id, at(2)).await.unwrap();
    }
    // Tags: members hold files.tag; a tag is refused, not rewritten, when it is odd.
    files::tag(s, &bob, a.id, " Hardware ").await.unwrap();
    files::tag(s, &bob, a.id, "hardware").await.unwrap();
    assert!(matches!(
        files::tag(s, &bob, a.id, "bad tag").await,
        Err(FileError::Text(_))
    ));
    assert!(matches!(
        files::tag(s, &bob, a.id, "").await,
        Err(FileError::Text(_))
    ));
    for n in 0..7 {
        files::tag(s, &bob, a.id, &format!("t{n}")).await.unwrap();
    }
    assert!(matches!(
        files::tag(s, &bob, a.id, "ninth").await,
        Err(FileError::TooManyTags)
    ));
    let view = files::file_view(s, &bob, a.id).await.unwrap();
    assert_eq!(view.file.tags.len(), 8);
    assert!(view.file.tags.contains(&"hardware".to_owned()));
    files::untag(s, &bob, a.id, "t0").await.unwrap();
    assert_eq!(
        files::file_view(s, &bob, a.id)
            .await
            .unwrap()
            .file
            .tags
            .len(),
        7
    );
    // A file bob cannot see cannot be tagged, and says nothing about it.
    assert!(matches!(
        files::tag(s, &bob, b.id, "x").await,
        Err(FileError::NotFound)
    ));
    // Descriptions: the uploader or an approver, cleaned.
    files::describe(s, &ann, a.id, "A manual\u{1b}[2J for routers")
        .await
        .unwrap();
    let view = files::file_view(s, &bob, a.id).await.unwrap();
    assert!(!view.description.as_str().contains('\u{1b}'));
    assert!(view.description.as_str().contains("routers"));
    assert!(matches!(
        files::describe(s, &bob, a.id, "mine now").await,
        Err(FileError::NotPermitted(_))
    ));
    // Search: every term must match the name, description or a tag; case does not matter.
    let found = |page: files::FilePage| page.files.iter().map(|f| f.id).collect::<Vec<_>>();
    assert_eq!(
        found(files::search(s, &bob, "ROUTER", 0).await.unwrap()),
        [a.id]
    );
    assert_eq!(
        found(files::search(s, &bob, "router hardware", 0).await.unwrap()),
        [a.id]
    );
    assert_eq!(
        found(
            files::search(s, &bob, "router manual routers", 0)
                .await
                .unwrap()
        ),
        [a.id]
    );
    assert!(
        files::search(s, &bob, "router nothere", 0)
            .await
            .unwrap()
            .files
            .is_empty()
    );
    assert!(
        files::search(s, &bob, "\"'; DROP TABLE file; --", 0)
            .await
            .unwrap()
            .files
            .is_empty()
    );
    assert!(
        files::search(s, &bob, "   ", 0)
            .await
            .unwrap()
            .files
            .is_empty()
    );
    // Ann reaches both areas; bob only the open one.
    assert_eq!(
        found(files::search(s, &ann, "router", 0).await.unwrap()).len(),
        2
    );
}

pub(crate) async fn download_lists_hold_only_what_the_member_may_still_fetch<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let dir = tempfile::tempdir().unwrap();
    let store = blobs(&dir).await;
    let w = world(s).await;
    let (ann, bob, sys) = (
        snap(s, &w.ann).await,
        snap(s, &w.bob).await,
        snap(s, &w.sysop).await,
    );
    let a = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        w.docs,
        "a.txt",
        b"a",
        at(1),
    )
    .await
    .unwrap();
    let b = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        w.staff,
        "b.txt",
        b"b",
        at(1),
    )
    .await
    .unwrap();
    for id in [a.id, b.id] {
        files::approve(s, &store, &sys, id, at(2)).await.unwrap();
    }
    files::list_add(s, &bob, a.id, at(3)).await.unwrap();
    files::list_add(s, &bob, a.id, at(3)).await.unwrap();
    assert!(matches!(
        files::list_add(s, &bob, b.id, at(3)).await,
        Err(FileError::NotFound)
    ));
    let list = files::list_files(s, &bob).await.unwrap();
    assert_eq!(list.iter().map(|f| f.id).collect::<Vec<_>>(), [a.id]);
    // Other members' lists are separate.
    assert!(files::list_files(s, &ann).await.unwrap().is_empty());
    // A row that is on the list but whose area the member can no longer download from is not
    // served: simulate by adding directly to the store (as an older grant would have allowed).
    s.list_add(w.bob.id, b.id, at(4), 100).await.unwrap();
    assert_eq!(files::list_files(s, &bob).await.unwrap().len(), 1);
    files::list_remove(s, &bob, a.id).await.unwrap();
    assert!(files::list_files(s, &bob).await.unwrap().is_empty());
    // The cap.
    for n in 0..files::MAX_DOWNLOAD_LIST {
        let up = upload(
            s,
            &store,
            &files::NoScan,
            &sys,
            w.docs,
            &format!("f{n}.txt"),
            b"x",
            at(5),
        )
        .await
        .unwrap();
        files::approve(s, &store, &sys, up.id, at(5)).await.unwrap();
        let outcome = files::list_add(s, &ann, up.id, at(6)).await;
        assert!(outcome.is_ok(), "{n}: {outcome:?}");
    }
    let one_more = upload(
        s,
        &store,
        &files::NoScan,
        &sys,
        w.docs,
        "last.txt",
        b"x",
        at(7),
    )
    .await
    .unwrap();
    files::approve(s, &store, &sys, one_more.id, at(7))
        .await
        .unwrap();
    assert!(matches!(
        files::list_add(s, &ann, one_more.id, at(8)).await,
        Err(FileError::ListFull)
    ));
    files::list_clear(s, &ann).await.unwrap();
    assert!(files::list_files(s, &ann).await.unwrap().is_empty());
}

pub(crate) async fn removal_follows_who_owns_the_file_and_what_state_it_is_in<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let dir = tempfile::tempdir().unwrap();
    let store = blobs(&dir).await;
    let w = world(s).await;
    let (ann, bob, moderator) = (
        snap(s, &w.ann).await,
        snap(s, &w.bob).await,
        snap(s, &w.mod_).await,
    );
    let mine = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        w.docs,
        "mine.txt",
        b"m",
        at(1),
    )
    .await
    .unwrap();
    // Not mine, not visible: not found. Visible but not an approver: not permitted.
    assert!(matches!(
        files::remove(s, &store, &bob, mine.id).await,
        Err(FileError::NotFound)
    ));
    let key = files::open_download(s, &ann, mine.id).await.unwrap().key;
    files::remove(s, &store, &ann, mine.id).await.unwrap();
    assert!(store.head(&key).await.is_err());
    assert!(s.file(mine.id).await.unwrap().is_none());
    // Once approved only an approver may remove it.
    let live = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        w.docs,
        "live.txt",
        b"l",
        at(2),
    )
    .await
    .unwrap();
    files::approve(s, &store, &moderator, live.id, at(3))
        .await
        .unwrap();
    assert!(matches!(
        files::remove(s, &store, &ann, live.id).await,
        Err(FileError::NotPermitted(Permission::FilesApprove))
    ));
    files::remove(s, &store, &moderator, live.id).await.unwrap();
    assert!(matches!(
        files::open_download(s, &bob, live.id).await,
        Err(FileError::NotFound)
    ));
    let rows = known_events(
        s.page(&AuditQuery {
            action: Some(Action::FileRemoved),
            ..AuditQuery::default()
        })
        .await
        .unwrap(),
    );
    assert_eq!(rows.len(), 2);
}

pub(crate) async fn two_deciders_cannot_both_win<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let dir = tempfile::tempdir().unwrap();
    let store = blobs(&dir).await;
    let w = world(s).await;
    let ann = snap(s, &w.ann).await;
    let up = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        w.docs,
        "race.txt",
        b"r",
        at(1),
    )
    .await
    .unwrap();
    let moderator = snap(s, &w.mod_).await;
    let sys = snap(s, &w.sysop).await;
    let (a, b) = tokio::join!(
        files::approve(s, &store, &moderator, up.id, at(2)),
        files::reject(s, &store, &sys, up.id, at(2)),
    );
    let ok = [a.is_ok(), b.is_ok()];
    assert_eq!(ok.iter().filter(|x| **x).count(), 1, "{a:?} {b:?}");
    let row = s.file(up.id).await.unwrap().unwrap();
    assert_ne!(row.state, FileState::Quarantine);
    // Whichever won, the bytes and the state agree.
    let live = BlobKey::parse(&row.blob_key).unwrap();
    match row.state {
        FileState::Approved => assert_eq!(read_all(&store, &live).await, b"r"),
        FileState::Rejected => assert!(store.head(&live).await.is_err()),
        FileState::Quarantine => panic!("still in quarantine"),
    }
}

pub(crate) async fn a_deleted_account_takes_its_download_list_and_not_the_file<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let dir = tempfile::tempdir().unwrap();
    let store = blobs(&dir).await;
    let w = world(s).await;
    let (ann, sys) = (snap(s, &w.ann).await, snap(s, &w.sysop).await);
    let up = upload(
        s,
        &store,
        &files::NoScan,
        &ann,
        w.docs,
        "keep.txt",
        b"k",
        at(1),
    )
    .await
    .unwrap();
    files::approve(s, &store, &sys, up.id, at(2)).await.unwrap();
    s.list_add(w.bob.id, up.id, at(3), 100).await.unwrap();
    h.raw(&format!(
        "DELETE FROM accounts WHERE id = {}",
        w.bob.id.as_raw()
    ))
    .await
    .unwrap();
    assert!(s.list_files(w.bob.id).await.unwrap().is_empty());
    h.raw(&format!(
        "DELETE FROM accounts WHERE id = {}",
        w.ann.id.as_raw()
    ))
    .await
    .unwrap();
    let row = s.file(up.id).await.unwrap().unwrap();
    assert_eq!(row.uploader_id, None::<AccountId>);
    assert_eq!(row.uploader, None);
    assert_eq!(row.state, FileState::Approved);
    // Deleting an area takes its files' rows with it.
    h.raw(&format!(
        "DELETE FROM file_area WHERE id = {}",
        w.docs.as_raw()
    ))
    .await
    .unwrap();
    assert!(s.file(up.id).await.unwrap().is_none());
    let _ = FileId::from_raw(1);
}

pub(crate) async fn a_broken_store_fails_file_calls<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let ann = snap(s, &w.ann).await;
    h.close().await;
    assert!(files::areas(s, &ann).await.is_err());
    assert!(files::area_page(s, &ann, w.docs, 0).await.is_err());
    assert!(files::search(s, &ann, "x", 0).await.is_err());
    assert!(files::list_files(s, &ann).await.is_err());
}

pub(crate) async fn the_database_refuses_bad_file_rows<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let ok = format!(
        "INSERT INTO file (area_id, name, original_name, size, sha256, blob_key, state, scan, created_at) \
         VALUES ({}, 'a', 'a', 1, '{}', 'quarantine/x', 'quarantine', 'clean', 1)",
        w.docs.as_raw(),
        "0".repeat(64)
    );
    h.raw(&ok).await.unwrap();
    assert!(
        h.raw(&ok.replace("'quarantine', 'clean'", "'live', 'clean'"))
            .await
            .is_err()
    );
    assert!(
        h.raw(&ok.replace("'quarantine', 'clean'", "'quarantine', 'maybe'"))
            .await
            .is_err()
    );
    assert!(h.raw(&ok.replace(&"0".repeat(64), "XYZ")).await.is_err());
    assert!(h.raw(&ok.replace("1, '0", "-1, '0")).await.is_err());
    assert!(
        h.raw(&format!(
            "INSERT INTO file_tag (file_id, tag) VALUES (1, 'Bad Tag')"
        ))
        .await
        .is_err()
    );
}
