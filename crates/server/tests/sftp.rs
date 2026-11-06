//! File areas over a real OpenSSH `sftp` client (M4-F2). The same scenario runs against the
//! local blob backend and, with `--ignored` and the `RUSSH_BBS_TEST_S3_*` variables, against
//! MinIO: upload to quarantine, invisible to others, staff approve, download as another member,
//! limits, auto-approve, the scan hook against a fake clamd, path traversal, and the operations
//! that are refused. Keys are generated here; the temporary directories hold everything.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    missing_docs
)]

mod common;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{HashAlg, PrivateKey};
use russh_bbs_core::access::{AccessStore, Fingerprint, Slug};
use russh_bbs_core::audit::{Action, AuditQuery, AuditStore, known_events};
use russh_bbs_core::blobs::{BlobKey, BlobStore};
use russh_bbs_core::files::{self, AreaId, FileId, FileState, FileStore};
use russh_bbs_core::limits::{Limit, LimitKind, LimitStore};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    AccountState, AccountStateStore, AccountStore, NewAccount, UnixSeconds,
};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::founder;
use russh_bbs_server::sftp::{ClamdScanner, ClamdTarget, FileServices, Scanner};
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_server::ui::Ui;
use russh_bbs_store::SqliteStore;
use russh_bbs_store::blobs::{Blobs, LocalBlobStore, S3BlobStore, S3Credentials, S3Settings};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Member {
    id: russh_bbs_core::store::AccountId,
    handle: String,
    key_file: PathBuf,
}

struct Rig {
    server: SshServer,
    store: SqliteStore,
    blobs: Blobs,
    dir: TempDir,
}

impl Rig {
    async fn start(blobs: Blobs, scanner: Scanner, sftp: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
        let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
        let mut console = Vec::new();
        let gate = founder::start(&store, &mut console).await.unwrap();
        drop(console);
        let limits = Limits {
            ip_rate: RatePolicy::new(10_000, Duration::from_millis(1)),
            key_rate: RatePolicy::new(10_000, Duration::from_millis(1)),
            ..common::fast_limits()
        };
        let config = ServerConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            limits,
        };
        let services = Arc::new(FileServices {
            blobs: blobs.clone(),
            scanner,
            sftp,
        });
        let ui = Ui::standard().unwrap().with_files(services);
        let server = SshServer::start_with_ui(
            &config,
            &host_key,
            Arc::new(store.clone()),
            Arc::new(gate),
            Arc::new(ui),
        )
        .await
        .unwrap();
        Self {
            server,
            store,
            blobs,
            dir,
        }
    }

    fn addr(&self) -> SocketAddr {
        self.server.local_addr()
    }

    /// An active member with a key, written to disk for the `sftp` client.
    async fn member(&self, name: &str, role: Slug) -> Member {
        let key = common::new_client_key();
        let account = self
            .store
            .create_account(NewAccount {
                username: sanitize_username(name).unwrap(),
                role,
                state: AccountState::Active,
            })
            .await
            .unwrap();
        let fingerprint =
            Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
        self.store
            .attach_fingerprint(account.id, &fingerprint)
            .await
            .unwrap();
        let key_file = self.dir.path().join(format!("{name}.key"));
        write_key(&key, &key_file);
        Member {
            id: account.id,
            handle: name.to_owned(),
            key_file,
        }
    }

    async fn snapshot(&self, who: &Member) -> russh_bbs_core::access::Snapshot {
        self.store.snapshot(Some(who.id)).await.unwrap()
    }

    async fn area(&self, by: &Member, name: &str, restricted: bool, auto: bool) -> AreaId {
        let snap = self.snapshot(by).await;
        files::create_area(&self.store, &snap, now(), name, "", restricted, auto)
            .await
            .unwrap()
    }

    /// Run `sftp` for `who` with these batch lines. Returns (success, stdout + stderr).
    async fn sftp(&self, who: &Member, workdir: &Path, lines: &[&str]) -> (bool, String) {
        let batch = workdir.join(format!("batch-{}.txt", rand_name()));
        std::fs::write(&batch, lines.join("\n") + "\n").unwrap();
        let addr = self.addr();
        let key = who.key_file.clone();
        let handle = who.handle.clone();
        let workdir = workdir.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let output = Command::new("sftp")
                .current_dir(workdir)
                .args(["-b"])
                .arg(&batch)
                .args([
                    "-o",
                    "StrictHostKeyChecking=no",
                    "-o",
                    "UserKnownHostsFile=/dev/null",
                    "-o",
                    "IdentitiesOnly=yes",
                    "-o",
                    "PreferredAuthentications=publickey",
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "LogLevel=ERROR",
                    "-o",
                ])
                .arg(format!("IdentityFile={}", key.display()))
                .arg("-P")
                .arg(addr.port().to_string())
                .arg(format!("{handle}@127.0.0.1"))
                .output()
                .unwrap();
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            (output.status.success(), text)
        })
        .await
        .unwrap()
    }

    async fn stop(self) {
        self.server.shutdown().await;
        self.store.close().await;
    }
}

fn now() -> UnixSeconds {
    UnixSeconds::from_raw(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .try_into()
            .unwrap(),
    )
}

fn rand_name() -> String {
    let id = russh_bbs_core::blobs::BlobId::generate();
    id.as_str()[..8].to_owned()
}

fn write_key(key: &PrivateKey, path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::write(path, key.to_openssh(LineEnding::LF).unwrap().as_bytes()).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

/// Deterministic bytes that are not all the same, so a swapped or short block shows.
fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|n| u8::try_from((n * 31 + n / 251) % 256).unwrap())
        .collect()
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// An empty working directory for one client.
fn workdir(rig: &Rig, name: &str) -> PathBuf {
    let dir = rig.dir.path().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn local_blobs(dir: &Path) -> Blobs {
    Blobs::Local(
        LocalBlobStore::open(dir.join("blobs"), 64 * 1024 * 1024)
            .await
            .unwrap(),
    )
}

fn names_in_listing(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|l| !l.starts_with("sftp>"))
        .filter_map(|l| l.split_whitespace().last())
        .map(|name| name.rsplit('/').next().unwrap_or(name).to_owned())
        .collect()
}

/// The whole story, once, on whatever blob backend `blobs` is.
async fn scenario(blobs: Blobs) {
    let rig = Rig::start(blobs, Scanner::None(russh_bbs_core::files::NoScan), true).await;
    let ops = rig.member("ops", Slug::SYSOP).await;
    let staff = rig.member("mo", Slug::MODERATOR).await;
    let ann = rig.member("ann", Slug::MEMBER).await;
    let bob = rig.member("bob", Slug::MEMBER).await;
    let docs = rig.area(&ops, "Docs", false, false).await;
    let fast = rig.area(&ops, "Fast lane", false, true).await;
    let secret = rig.area(&ops, "Staff only", true, false).await;
    let (docs_dir, fast_dir, secret_dir) = (
        format!("/{docs}-docs"),
        format!("/{fast}-fast-lane"),
        format!("/{secret}-staff-only"),
    );

    // ---- ann uploads into quarantine ------------------------------------------------------
    let ann_dir = workdir(&rig, "ann");
    let small = b"hello from ann\n".to_vec();
    let big = pattern(3 * 1024 * 1024 + 12_345);
    std::fs::write(ann_dir.join("hello.txt"), &small).unwrap();
    std::fs::write(ann_dir.join("big.bin"), &big).unwrap();
    let (ok, out) = rig
        .sftp(
            &ann,
            &ann_dir,
            &[
                &format!("put hello.txt {docs_dir}/hello.txt"),
                &format!("put big.bin {docs_dir}/big.bin"),
            ],
        )
        .await;
    assert!(ok, "upload failed: {out}");
    // The member sees both as pending, and the root lists the areas she may see (not the
    // restricted one) and the download list.
    let (ok, out) = rig
        .sftp(
            &ann,
            &ann_dir,
            &["ls -1 /", &format!("ls -1 {docs_dir}/pending")],
        )
        .await;
    assert!(ok, "{out}");
    assert!(
        out.contains(&format!("{docs}-docs")) && out.contains("downloads"),
        "{out}"
    );
    assert!(
        !out.contains("staff-only"),
        "a restricted area is not listed: {out}"
    );
    let pending = names_in_listing(&out);
    let hello_name = pending
        .iter()
        .find(|n| n.ends_with("-hello.txt"))
        .unwrap()
        .clone();
    let big_name = pending
        .iter()
        .find(|n| n.ends_with("-big.bin"))
        .unwrap()
        .clone();
    let hello_id: i64 = hello_name.split('-').next().unwrap().parse().unwrap();
    let big_id: i64 = big_name.split('-').next().unwrap().parse().unwrap();
    // Sizes are real.
    let (_, out) = rig
        .sftp(&ann, &ann_dir, &[&format!("ls -l {docs_dir}/pending")])
        .await;
    assert!(out.contains(&big.len().to_string()), "{out}");

    // ---- invisible to everyone else --------------------------------------------------------
    let bob_dir = workdir(&rig, "bob");
    let (_, out) = rig
        .sftp(
            &bob,
            &bob_dir,
            &[
                &format!("ls -1 {docs_dir}"),
                &format!("ls -1 {docs_dir}/pending"),
            ],
        )
        .await;
    assert!(!out.contains("hello") && !out.contains("big.bin"), "{out}");
    for path in [
        format!("{docs_dir}/pending/{hello_name}"),
        format!("{docs_dir}/{hello_name}"),
        format!("/{docs}/{hello_id}"),
    ] {
        let (ok, out) = rig
            .sftp(&bob, &bob_dir, &[&format!("get {path} stolen.txt")])
            .await;
        assert!(!ok && !bob_dir.join("stolen.txt").exists(), "{path}: {out}");
        assert!(
            out.to_lowercase().contains("no such file") || out.contains("not found"),
            "{out}"
        );
    }
    // The same answer for a file that does not exist and for the restricted area.
    let (_, missing) = rig
        .sftp(&bob, &bob_dir, &[&format!("get {docs_dir}/9999-nope x")])
        .await;
    let (_, hidden) = rig
        .sftp(&bob, &bob_dir, &[&format!("ls {secret_dir}")])
        .await;
    let (_, never) = rig.sftp(&bob, &bob_dir, &["ls /9999-never"]).await;
    assert_eq!(
        hidden
            .replace(&secret_dir, "X")
            .replace(&secret.to_string(), "N"),
        never.replace("/9999-never", "X").replace("9999", "N"),
        "a restricted area answers like one that does not exist"
    );
    assert!(missing.to_lowercase().contains("not found"), "{missing}");

    // ---- staff approve; another member downloads -------------------------------------------
    let staff_snap = rig.snapshot(&staff).await;
    // The approver can read what is in quarantine, to review it.
    let staff_dir = workdir(&rig, "mo");
    let (ok, out) = rig
        .sftp(
            &staff,
            &staff_dir,
            &[&format!("get {docs_dir}/pending/{hello_name} review.txt")],
        )
        .await;
    assert!(ok, "{out}");
    assert_eq!(std::fs::read(staff_dir.join("review.txt")).unwrap(), small);
    for id in [hello_id, big_id] {
        files::approve(
            &rig.store,
            &rig.blobs,
            &staff_snap,
            FileId::from_raw(id),
            now(),
        )
        .await
        .unwrap();
    }
    let (ok, out) = rig
        .sftp(
            &bob,
            &bob_dir,
            &[
                &format!("get {docs_dir}/{hello_name} got-hello.txt"),
                &format!("get {docs_dir}/{big_name} got-big.bin"),
            ],
        )
        .await;
    assert!(ok, "{out}");
    assert_eq!(std::fs::read(bob_dir.join("got-hello.txt")).unwrap(), small);
    assert_eq!(
        sha(&std::fs::read(bob_dir.join("got-big.bin")).unwrap()),
        sha(&big)
    );
    // A resume continues where a partial download stopped (the server reads from the start).
    let partial = &big[..1_000_000];
    std::fs::write(bob_dir.join("part.bin"), partial).unwrap();
    let (ok, out) = rig
        .sftp(
            &bob,
            &bob_dir,
            &[&format!("reget {docs_dir}/{big_name} part.bin")],
        )
        .await;
    assert!(ok, "{out}");
    assert_eq!(
        sha(&std::fs::read(bob_dir.join("part.bin")).unwrap()),
        sha(&big)
    );
    // Approved files are listed; the pending directory is empty again.
    let (_, out) = rig
        .sftp(&bob, &bob_dir, &[&format!("ls -1 {docs_dir}")])
        .await;
    assert!(
        out.contains(&hello_name) && out.contains(&big_name),
        "{out}"
    );
    let (_, out) = rig
        .sftp(&ann, &ann_dir, &[&format!("ls -1 {docs_dir}/pending")])
        .await;
    assert!(!out.contains("hello"), "{out}");
    // The bytes really are in the live namespace, and the audit trail has the approvals.
    let row = rig
        .store
        .file(FileId::from_raw(hello_id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, FileState::Approved);
    assert!(row.blob_key.starts_with("files/"), "{}", row.blob_key);
    assert!(
        rig.blobs
            .head(&BlobKey::parse(&row.blob_key).unwrap())
            .await
            .is_ok()
    );
    let approvals = known_events(
        rig.store
            .page(&AuditQuery {
                action: Some(Action::FileApproved),
                ..AuditQuery::default()
            })
            .await
            .unwrap(),
    );
    assert_eq!(approvals.len(), 2);

    // ---- the download list ------------------------------------------------------------------
    let bob_snap = rig.snapshot(&bob).await;
    files::list_add(&rig.store, &bob_snap, FileId::from_raw(hello_id), now())
        .await
        .unwrap();
    let (ok, out) = rig
        .sftp(
            &bob,
            &bob_dir,
            &[
                "ls -1 /downloads",
                &format!("get /downloads/{hello_name} listed.txt"),
            ],
        )
        .await;
    assert!(ok && out.contains(&hello_name), "{out}");
    assert_eq!(std::fs::read(bob_dir.join("listed.txt")).unwrap(), small);

    // ---- auto-approve ----------------------------------------------------------------------
    std::fs::write(ann_dir.join("quick.txt"), b"quick").unwrap();
    let (ok, out) = rig
        .sftp(
            &ann,
            &ann_dir,
            &[&format!("put quick.txt {fast_dir}/quick.txt")],
        )
        .await;
    assert!(ok, "{out}");
    let (_, out) = rig
        .sftp(&bob, &bob_dir, &[&format!("ls -1 {fast_dir}")])
        .await;
    assert!(
        out.contains("-quick.txt"),
        "an auto-approve area is live at once: {out}"
    );

    // ---- limits ----------------------------------------------------------------------------
    let ann_account = ann.id;
    rig.store
        .set_member_limit(
            ann_account,
            LimitKind::UploadBytesPerFile,
            Some(Limit::AtMost(1000)),
            russh_bbs_core::audit::Actor::System,
        )
        .await
        .unwrap();
    std::fs::write(ann_dir.join("toobig.bin"), pattern(5000)).unwrap();
    let (ok, out) = rig
        .sftp(
            &ann,
            &ann_dir,
            &[&format!("put toobig.bin {docs_dir}/toobig.bin")],
        )
        .await;
    assert!(!ok, "{out}");
    assert!(
        out.contains("limit") || out.contains("denied") || out.contains("Failure"),
        "{out}"
    );
    let (_, out) = rig
        .sftp(&ann, &ann_dir, &[&format!("ls -1 {docs_dir}/pending")])
        .await;
    assert!(
        !out.contains("toobig"),
        "a refused upload leaves nothing: {out}"
    );
    rig.store
        .set_member_limit(
            ann_account,
            LimitKind::UploadBytesPerFile,
            None,
            russh_bbs_core::audit::Actor::System,
        )
        .await
        .unwrap();
    rig.store
        .set_member_limit(
            ann_account,
            LimitKind::UploadBytesPerDay,
            Some(Limit::AtMost(0)),
            russh_bbs_core::audit::Actor::System,
        )
        .await
        .unwrap();
    let (ok, out) = rig
        .sftp(
            &ann,
            &ann_dir,
            &[&format!("put hello.txt {docs_dir}/again.txt")],
        )
        .await;
    assert!(!ok, "{out}");
    rig.store
        .set_member_limit(
            ann_account,
            LimitKind::UploadBytesPerDay,
            None,
            russh_bbs_core::audit::Actor::System,
        )
        .await
        .unwrap();

    // ---- a restricted area the member was let into ------------------------------------------
    std::fs::write(bob_dir.join("peek.txt"), b"x").unwrap();
    let (ok, out) = rig
        .sftp(
            &bob,
            &bob_dir,
            &[&format!("put peek.txt {secret_dir}/peek.txt")],
        )
        .await;
    assert!(!ok && out.to_lowercase().contains("no such file"), "{out}");

    // ---- paths: traversal, odd names, and everything the tree does not offer ---------------
    for path in [
        "/../../../etc/passwd",
        &format!("{docs_dir}/../../etc/passwd"),
        "../../../../etc/passwd",
        "/etc/passwd",
        "~/.ssh/authorized_keys",
        "C:/windows/win.ini",
        "//etc//passwd",
    ] {
        let target = bob_dir.join("trav.out");
        let _ = std::fs::remove_file(&target);
        let (ok, out) = rig
            .sftp(&bob, &bob_dir, &[&format!("get {path} trav.out")])
            .await;
        assert!(!ok && !target.exists(), "{path}: {out}");
    }
    let (_, out) = rig
        .sftp(&bob, &bob_dir, &["cd ..", "pwd", "cd /", "pwd"])
        .await;
    assert!(out.contains("Remote working directory: /"), "{out}");
    for line in [
        format!("mkdir {docs_dir}/sub"),
        format!("rename {docs_dir}/{hello_name} {docs_dir}/moved"),
        format!("rm {docs_dir}/{hello_name}"),
        format!("ln -s /etc/passwd {docs_dir}/link"),
        format!("chmod 777 {docs_dir}/{hello_name}"),
        format!("rmdir {docs_dir}"),
        format!("put peek.txt {docs_dir}/{hello_name}"),
    ] {
        let (ok, out) = rig.sftp(&bob, &bob_dir, &[&line]).await;
        assert!(!ok, "`{line}` should be refused: {out}");
    }
    // Nothing changed.
    let (_, out) = rig
        .sftp(
            &bob,
            &bob_dir,
            &[&format!("get {docs_dir}/{hello_name} again.txt")],
        )
        .await;
    assert_eq!(
        std::fs::read(bob_dir.join("again.txt")).unwrap(),
        small,
        "{out}"
    );
    // A member can remove their own upload while it waits.
    std::fs::write(ann_dir.join("oops.txt"), b"oops").unwrap();
    let (ok, out) = rig
        .sftp(
            &ann,
            &ann_dir,
            &[&format!("put oops.txt {docs_dir}/oops.txt")],
        )
        .await;
    assert!(ok, "{out}");
    let (_, out) = rig
        .sftp(&ann, &ann_dir, &[&format!("ls -1 {docs_dir}/pending")])
        .await;
    let oops = names_in_listing(&out)
        .into_iter()
        .find(|n| n.ends_with("-oops.txt"))
        .unwrap();
    let (ok, out) = rig
        .sftp(&ann, &ann_dir, &[&format!("rm {docs_dir}/pending/{oops}")])
        .await;
    assert!(ok, "{out}");
    let (_, out) = rig
        .sftp(&ann, &ann_dir, &[&format!("ls -1 {docs_dir}/pending")])
        .await;
    assert!(!out.contains("oops"), "{out}");

    rig.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_scan_approve_and_download_over_sftp_on_the_local_backend() {
    let dir = tempfile::tempdir().unwrap();
    scenario(local_blobs(dir.path()).await).await;
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} is not set; these tests need MinIO (see docs/deploy.md and the F1 notes)")
    })
}

/// A fresh prefix cannot be chosen in a bucket, so the scenario runs against whatever the
/// bucket holds; keys are random, so earlier runs do not interfere.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs an S3-compatible server: set RUSSH_BBS_TEST_S3_ENDPOINT, _BUCKET, _ACCESS_KEY, _SECRET_KEY"]
async fn upload_scan_approve_and_download_over_sftp_on_minio() {
    let store = S3BlobStore::connect(S3Settings {
        endpoint: Some(env("RUSSH_BBS_TEST_S3_ENDPOINT")),
        region: None,
        bucket: env("RUSSH_BBS_TEST_S3_BUCKET"),
        path_style: true,
        ca_file: None,
        credentials: Some(S3Credentials::new(
            env("RUSSH_BBS_TEST_S3_ACCESS_KEY"),
            env("RUSSH_BBS_TEST_S3_SECRET_KEY"),
            None,
        )),
        multipart_threshold: 5 * 1024 * 1024,
        max_object_bytes: 64 * 1024 * 1024,
    })
    .await
    .unwrap();
    scenario(Blobs::S3(store)).await;
}

// ---- the scan hook, against a fake clamd --------------------------------------------------

/// A fake clamd on a TCP port: speaks `INSTREAM`, calls anything holding `EICAR` infected, and
/// answers an error when it holds `BROKEN`.
async fn fake_clamd() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut command = [0_u8; 10];
                if stream.read_exact(&mut command).await.is_err() || &command != b"zINSTREAM\0" {
                    return;
                }
                let mut content = Vec::new();
                loop {
                    let mut len = [0_u8; 4];
                    if stream.read_exact(&mut len).await.is_err() {
                        return;
                    }
                    let len = u32::from_be_bytes(len) as usize;
                    if len == 0 {
                        break;
                    }
                    let mut chunk = vec![0_u8; len];
                    if stream.read_exact(&mut chunk).await.is_err() {
                        return;
                    }
                    content.extend_from_slice(&chunk);
                }
                let has = |needle: &[u8]| content.windows(needle.len()).any(|w| w == needle);
                let reply: &[u8] = if has(b"EICAR") {
                    b"stream: Eicar-Test-Signature FOUND\0"
                } else if has(b"BROKEN") {
                    b"INSTREAM size limit exceeded. ERROR\0"
                } else {
                    b"stream: OK\0"
                };
                let _ = stream.write_all(reply).await;
            });
        }
    });
    (address, task)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_scanner_rejects_what_it_finds_and_never_auto_approves_what_it_cannot_check() {
    let dir = tempfile::tempdir().unwrap();
    let (address, clamd) = fake_clamd().await;
    let rig = Rig::start(
        local_blobs(dir.path()).await,
        Scanner::Clamd(ClamdScanner::new(ClamdTarget::Tcp(address))),
        true,
    )
    .await;
    let ops = rig.member("ops", Slug::SYSOP).await;
    let ann = rig.member("ann", Slug::MEMBER).await;
    let bob = rig.member("bob", Slug::MEMBER).await;
    let fast = rig.area(&ops, "Fast lane", false, true).await;
    let fast_dir = format!("/{fast}-fast-lane");
    let work = workdir(&rig, "ann");
    std::fs::write(work.join("clean.txt"), b"nothing to see").unwrap();
    std::fs::write(work.join("virus.com"), b"X5O!P%@AP EICAR-STANDARD").unwrap();
    std::fs::write(work.join("odd.txt"), b"BROKEN scanner input").unwrap();

    let (ok, out) = rig
        .sftp(
            &ann,
            &work,
            &[&format!("put clean.txt {fast_dir}/clean.txt")],
        )
        .await;
    assert!(ok, "{out}");
    let (ok, out) = rig
        .sftp(
            &ann,
            &work,
            &[&format!("put virus.com {fast_dir}/virus.com")],
        )
        .await;
    assert!(!ok, "the scanner's finding fails the transfer: {out}");
    let (ok, out) = rig
        .sftp(&ann, &work, &[&format!("put odd.txt {fast_dir}/odd.txt")])
        .await;
    assert!(ok, "a scan error leaves the file in quarantine: {out}");

    let bob_work = workdir(&rig, "bob");
    let (_, out) = rig
        .sftp(&bob, &bob_work, &[&format!("ls -1 {fast_dir}")])
        .await;
    assert!(
        out.contains("-clean.txt"),
        "clean + auto-approve is live: {out}"
    );
    assert!(!out.contains("virus") && !out.contains("odd.txt"), "{out}");
    // Rows: infected -> rejected with its bytes gone; failed -> still in quarantine, not live.
    let all = rig.store.pending_in(&[fast], 50, 0).await.unwrap();
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].scan, files::ScanState::Failed);
    let live = rig
        .store
        .files(fast, FileState::Approved, 50, 0)
        .await
        .unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].scan, files::ScanState::Clean);
    let rejected = known_events(
        rig.store
            .page(&AuditQuery {
                action: Some(Action::FileRejected),
                ..AuditQuery::default()
            })
            .await
            .unwrap(),
    );
    assert_eq!(rejected.len(), 1);
    assert!(rejected[0].event.detail.as_str().ends_with("infected"));
    let stored = rig
        .blobs
        .list(
            &russh_bbs_core::blobs::BlobPrefix::namespace(
                russh_bbs_core::blobs::Namespace::new("quarantine").unwrap(),
            ),
            None,
            100,
        )
        .await
        .unwrap();
    assert_eq!(
        stored.keys.len(),
        1,
        "only the unscannable file waits in quarantine"
    );

    // With the scanner down, nothing auto-approves and nothing is lost.
    clamd.abort();
    std::fs::write(work.join("late.txt"), b"late").unwrap();
    let (ok, out) = rig
        .sftp(&ann, &work, &[&format!("put late.txt {fast_dir}/late.txt")])
        .await;
    assert!(ok, "{out}");
    let (_, out) = rig
        .sftp(&bob, &bob_work, &[&format!("ls -1 {fast_dir}")])
        .await;
    assert!(!out.contains("late.txt"), "{out}");
    rig.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sftp_is_for_signed_in_members_and_off_when_the_sysop_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let rig = Rig::start(
        local_blobs(dir.path()).await,
        Scanner::None(russh_bbs_core::files::NoScan),
        false,
    )
    .await;
    let ann = rig.member("ann", Slug::MEMBER).await;
    let work = workdir(&rig, "w");
    let (ok, out) = rig.sftp(&ann, &work, &["ls /"]).await;
    assert!(!ok, "[files] sftp = false refuses the subsystem: {out}");
    rig.stop().await;

    let dir = tempfile::tempdir().unwrap();
    let rig = Rig::start(
        local_blobs(dir.path()).await,
        Scanner::None(russh_bbs_core::files::NoScan),
        true,
    )
    .await;
    // A key that is not a member's gets the waiting room at best, and no sftp.
    let stranger = Member {
        id: russh_bbs_core::store::AccountId::from_raw(0),
        handle: "new".into(),
        key_file: dir.path().join("stranger.key"),
    };
    write_key(&common::new_client_key(), &stranger.key_file);
    let work = workdir(&rig, "s");
    let (ok, out) = rig.sftp(&stranger, &work, &["ls /"]).await;
    assert!(!ok, "{out}");
    // A suspended member's open session loses everything: nothing lists, nothing opens.
    let bob = rig.member("bob", Slug::MEMBER).await;
    let (ok, out) = rig.sftp(&bob, &work, &["ls /"]).await;
    assert!(ok, "{out}");
    rig.store
        .change_state(
            bob.id,
            AccountState::Suspended,
            russh_bbs_core::audit::Actor::System,
        )
        .await
        .unwrap();
    let (ok, _) = rig.sftp(&bob, &work, &["ls /"]).await;
    assert!(!ok, "a suspended member cannot connect");
    rig.stop().await;
}
