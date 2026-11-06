//! The door framework over a real SSH client and a real telnet client against the real SQLite
//! store (M4-D0, D98): the same walk on both transports. Open Doors from the main menu, open the
//! example coin toss, play, see the save in the store, leave, come back, cross a day boundary,
//! run the daily tick, and meet the reserved-name rule, a bar and a revoked permission.
//!
//! The clock the doors read is a manual one, so "tomorrow" is one line. No test sleeps to wait
//! for the server.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unused_async_trait_impl,
    clippy::match_same_arms,
    missing_docs
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Client, fast_limits, new_client_key};
use russh::Channel;
use russh::ChannelMsg;
use russh::client::{AuthResult, Handle, Msg};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh_bbs_core::access::{AccessStore, Fingerprint, Permission, Slug};
use russh_bbs_core::audit::{Action, AuditQuery, AuditStore, known_events};
use russh_bbs_core::doors::{self, DoorId, DoorStore, Revision};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::passwords::Backoff;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountId, AccountState, AccountStore, NewAccount};
use russh_bbs_core::terminal_settings::{TerminalSettings, TerminalSettingsStore};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_server::doors::Doors;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_server::telnet::{TelnetConfig, TelnetServer};
use russh_bbs_server::ui::Ui;
use russh_bbs_store::SqliteStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const SECRET: &str = "correct horse battery";
const WAIT: Duration = Duration::from_secs(20);
const DAY: i64 = 20_000;

struct Rig {
    ssh: SshServer,
    telnet: TelnetServer,
    store: SqliteStore,
    db: SqlitePool,
    clock: Arc<ManualClock>,
    doors: Doors,
    _dir: TempDir,
}

fn lenient(limits: Limits) -> Limits {
    let loose = RatePolicy::new(1_000, Duration::from_millis(1));
    Limits {
        ip_rate: loose,
        key_rate: loose,
        password_handle_backoff: Backoff::UNLIMITED,
        password_network_backoff: Backoff::UNLIMITED,
        max_auth_attempts: 3,
        ..limits
    }
}

impl Rig {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bbs.db");
        let store = SqliteStore::open(&path).await.unwrap();
        let db = SqlitePool::connect_with(SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap();
        let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
        let clock = Arc::new(ManualClock::new(Day::from_raw(DAY).start()));
        let doors = Doors::builtin()
            .unwrap()
            .with_clock(clock.clone() as Arc<dyn Clock>);
        let ui = Arc::new(Ui::standard().unwrap().with_doors(doors.clone()));
        let limits = lenient(fast_limits());
        let ssh = SshServer::start_with_ui(
            &ServerConfig {
                listen: "127.0.0.1:0".parse().unwrap(),
                limits,
            },
            &host_key,
            Arc::new(store.clone()),
            Arc::new(
                russh_bbs_server::founder::start(&store, &mut Vec::new())
                    .await
                    .unwrap(),
            ),
            Arc::clone(&ui),
        )
        .await
        .unwrap();
        let telnet = TelnetServer::start(
            &TelnetConfig {
                listen: "127.0.0.1:0".parse().unwrap(),
                allow_login: true,
                limits: Limits {
                    low_security: true,
                    ..limits
                },
            },
            Arc::new(store.clone()),
            ssh.registry(),
            ui,
        )
        .await
        .unwrap();
        Self {
            ssh,
            telnet,
            store,
            db,
            clock,
            doors,
            _dir: dir,
        }
    }

    async fn account(&self, name: &str, role: Slug) -> AccountId {
        let account = self
            .store
            .create_account(NewAccount {
                username: sanitize_username(name).unwrap(),
                role,
                state: AccountState::Active,
            })
            .await
            .unwrap();
        let settings = TerminalSettings::new(None, None, true, true, Some(Tier::Ascii)).unwrap();
        self.store
            .set_terminal_settings(account.id, &settings)
            .await
            .unwrap();
        let hash = russh_bbs_core::passwords::hash::hash(SECRET).unwrap();
        russh_bbs_core::passwords::PasswordStore::set_hash(
            &self.store,
            account.id,
            Some(&hash),
            &russh_bbs_core::audit::events::password_set(account.id, "key session"),
        )
        .await
        .unwrap();
        account.id
    }

    /// An account that also has an SSH key bound.
    async fn member(&self, name: &str) -> (AccountId, PrivateKey) {
        let id = self.account(name, Slug::MEMBER).await;
        let key = new_client_key();
        self.store
            .attach_fingerprint(id, &fingerprint_of(&key))
            .await
            .unwrap();
        common::remember(fingerprint_of(&key), name);
        (id, key)
    }

    fn door() -> DoorId {
        DoorId::parse("coin-toss").unwrap()
    }

    async fn tick(&self) {
        let door = self.doors.all()[0].clone();
        run_due(
            &self.store,
            door.as_ref(),
            self.doors.clock().as_ref(),
            TickPolicy::default(),
        )
        .await
        .unwrap();
    }
}

fn fingerprint_of(key: &PrivateKey) -> Fingerprint {
    Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap()
}

// ------------------------------------------------------------------ the two terminals

/// A person at a terminal on either transport: send keys, read the screen they led to as words.
trait Term {
    async fn keys(&mut self, keys: &str) -> String;
}

/// The screen as one line of words, so a message that wrapped across rows can be matched.
fn flat(screen: &str) -> String {
    screen
        .replace(['│', '┌', '┐', '└', '┘', '─', '|', '+'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

struct Ssh {
    channel: Channel<Msg>,
    _session: Handle<Client>,
}

impl Ssh {
    async fn at_menu(rig: &Rig, key: &PrivateKey, user: &str) -> Self {
        let mut session = common::connect(rig.ssh.local_addr()).await;
        let result = session
            .authenticate_publickey(
                &common::user_for(key, user),
                PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
            )
            .await
            .unwrap();
        assert!(matches!(result, AuthResult::Success), "{result:?}");
        let mut channel = common::open_shell(&session, 80, 24).await;
        drain(&mut channel).await;
        let mut ssh = Self {
            channel,
            _session: session,
        };
        let menu = ssh.keys("x").await;
        assert!(menu.contains("Main menu"), "{menu}");
        ssh
    }
}

async fn drain(channel: &mut Channel<Msg>) -> String {
    let mut seen = String::new();
    while let Ok(msg) = timeout(Duration::from_millis(120), channel.wait()).await {
        match msg {
            Some(ChannelMsg::Data { data }) => seen.push_str(&String::from_utf8_lossy(&data)),
            Some(_) => {}
            None => break,
        }
    }
    seen
}

/// Replay a terminal byte stream onto a blank screen (printable text, `H` and `C` only).
fn replay(raw: &str) -> String {
    let mut rows: Vec<Vec<char>> = Vec::new();
    let (mut row, mut col) = (0_usize, 0_usize);
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' if chars.peek() == Some(&'[') => {
                chars.next();
                let mut params = String::new();
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        let numbers: Vec<usize> =
                            params.split(';').map(|p| p.parse().unwrap_or(1)).collect();
                        match next {
                            'H' => {
                                row = numbers.first().copied().unwrap_or(1).saturating_sub(1);
                                col = numbers.get(1).copied().unwrap_or(1).saturating_sub(1);
                            }
                            'C' => col += numbers.first().copied().unwrap_or(1),
                            _ => {}
                        }
                        break;
                    }
                    params.push(next);
                }
            }
            '\u{1b}' => {
                chars.next();
            }
            c if c.is_control() => {}
            c => {
                while rows.len() <= row {
                    rows.push(Vec::new());
                }
                let line = &mut rows[row];
                if line.len() <= col {
                    line.resize(col + 1, ' ');
                }
                line[col] = c;
                col += 1;
            }
        }
    }
    rows.iter()
        .map(|r| r.iter().collect::<String>().trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

impl Term for Ssh {
    async fn keys(&mut self, keys: &str) -> String {
        self.channel
            .data_bytes(keys.as_bytes().to_vec())
            .await
            .unwrap();
        drain(&mut self.channel).await;
        self.channel.window_change(80, 24, 0, 0).await.unwrap();
        flat(&replay(&drain(&mut self.channel).await))
    }
}

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_NAWS: u8 = 31;

fn footer_at(rows: u16) -> String {
    format!("\u{1b}[{rows};1H\u{1b}[2K\u{1b}[1;7mUNENCRYPTED\u{1b}[0m")
}

struct Tn {
    stream: TcpStream,
    text: Vec<u8>,
    state: u8,
    rows: u16,
}

fn naws(cols: u16, rows: u16) -> Vec<u8> {
    let mut out = vec![IAC, SB, OPT_NAWS];
    for byte in cols.to_be_bytes().into_iter().chain(rows.to_be_bytes()) {
        out.push(byte);
        if byte == IAC {
            out.push(IAC);
        }
    }
    out.extend([IAC, SE]);
    out
}

impl Tn {
    async fn send(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.unwrap();
    }

    /// Feed one byte of the server's stream through a minimal IAC filter.
    /// States: 0 data, 1 after IAC, 2 after a verb, 3 in a subnegotiation, 4 SB then IAC.
    fn rx(&mut self, byte: u8) {
        self.state = match (self.state, byte) {
            (0, IAC) => 1,
            (0, _) => {
                self.text.push(byte);
                0
            }
            (1, IAC) => {
                self.text.push(IAC);
                0
            }
            (1, WILL | WONT | DO | DONT) => 2,
            (1, SB) => 3,
            (1, _) => 0,
            (2, _) => 0,
            (3, IAC) => 4,
            (3, _) => 3,
            (4, SE) => 0,
            _ => 3,
        };
    }

    async fn fill(&mut self) -> bool {
        let mut buffer = [0_u8; 4096];
        let n = timeout(WAIT, self.stream.read(&mut buffer))
            .await
            .expect("the server went quiet")
            .unwrap_or(0);
        for &byte in &buffer[..n] {
            self.rx(byte);
        }
        n > 0
    }

    async fn read_until(&mut self, needle: &str) -> String {
        loop {
            let seen = String::from_utf8_lossy(&self.text).into_owned();
            if let Some(at) = seen.find(needle) {
                let end = at + needle.len();
                self.text.drain(..end);
                return seen[..end].to_owned();
            }
            assert!(self.fill().await, "closed before {needle:?}; saw {seen:?}");
        }
    }

    async fn at_menu(rig: &Rig, user: &str) -> Self {
        let stream = timeout(WAIT, TcpStream::connect(rig.telnet.local_addr()))
            .await
            .unwrap()
            .unwrap();
        let mut tn = Self {
            stream,
            text: Vec::new(),
            state: 0,
            rows: 24,
        };
        let mut hello = vec![
            IAC, DO, OPT_ECHO, IAC, DO, OPT_SGA, IAC, WILL, OPT_SGA, IAC, WILL, OPT_NAWS,
        ];
        hello.extend(naws(80, 24));
        tn.send(&hello).await;
        tn.read_until("login: ").await;
        tn.send(format!("{user}\r\0").as_bytes()).await;
        tn.read_until("password: ").await;
        tn.send(format!("{SECRET}\r\0").as_bytes()).await;
        tn.read_until(&footer_at(24)).await;
        let menu = tn.keys("x").await;
        assert!(menu.contains("Main menu"), "{menu}");
        tn
    }
}

impl Term for Tn {
    async fn keys(&mut self, keys: &str) -> String {
        // Enter is CR NUL on the wire.
        self.send(keys.replace('\r', "\r\0").as_bytes()).await;
        if keys.ends_with('\u{1b}') {
            // A lone Esc byte is only the Escape key once the parser's timeout has passed
            // (`ESC_TIMEOUT`, 100 ms): wait it out, or the frame below could beat the key.
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        // A window size not used before makes the next frame unmistakably newer than the keys.
        self.rows = if self.rows == 24 { 25 } else { 24 };
        self.send(&naws(80, self.rows)).await;
        let frame = self.read_until(&footer_at(self.rows)).await;
        // Drop what is left of the previous frame's footer note before this frame's clear.
        let from = frame.find("\u{1b}[2J").unwrap_or(0);
        flat(&replay(&frame[from..]))
    }
}

// ------------------------------------------------------------------ the shared walk

/// The walk both transports make. `account` is the member's id; `me` their handle.
async fn walk<T: Term>(term: &mut T, rig: &Rig, account: AccountId) {
    let id = Rig::door();

    // Doors is on the main menu, opened by `d`.
    let list = term.keys("d").await;
    assert!(
        list.contains("Doors") && list.contains("Coin Toss"),
        "{list}"
    );
    assert!(list.contains("Pick a door"), "{list}");

    // Opening a door shows the fresh game: five tosses, five coins, nothing saved yet.
    let game = term.keys("\r").await;
    assert!(game.contains("Tosses left today: 5"), "{game}");
    assert!(game.contains("Coins: 5"), "{game}");
    assert!(rig.store.load_save(&id, account).await.unwrap().is_none());

    // Three tosses: each is saved through the store as it happens.
    let played = term.keys("hth").await;
    assert!(played.contains("Tosses left today: 2"), "{played}");
    let saved = rig.store.load_save(&id, account).await.unwrap().unwrap();
    assert_eq!(
        (saved.revision, saved.version, saved.season),
        (Revision::from_raw(3), 2, 1)
    );
    let world = rig.store.load_world(&id).await.unwrap().unwrap();
    assert_eq!(
        world.revision,
        Revision::from_raw(3),
        "each toss fed the shared world"
    );

    // A name goes through the reserved-name rule: `sysop` is refused, a real name is kept.
    assert!(term.keys("n").await.contains("New name:"));
    let refused = term.keys("sysop\r").await;
    assert!(
        refused.contains("That name is not allowed here."),
        "{refused}"
    );
    term.keys("n").await;
    let named = term.keys("Brindle\r").await;
    assert!(named.contains("Now known as Brindle."), "{named}");
    assert!(named.contains("Brindle - day 20000"), "{named}");

    // Leaving the door lands on the door list, then Esc... the list's Esc is the main menu.
    let back = term.keys("q").await;
    assert!(back.contains("Pick a door"), "{back}");
    // Reopening loads the save: same allowance, same name.
    let again = term.keys("1").await;
    assert!(again.contains("Tosses left today: 2"), "{again}");
    assert!(again.contains("Brindle"), "{again}");
    term.keys("q").await;

    // Tomorrow: the daily tick rolls the world, and the allowance is refilled on opening.
    rig.clock.set(Day::from_raw(DAY + 1).start());
    rig.tick().await;
    let tomorrow = term.keys("\r").await;
    assert!(tomorrow.contains("Tosses left today: 5"), "{tomorrow}");
    assert!(
        tomorrow.contains("0 coins today and 3 yesterday"),
        "{tomorrow}"
    );
    term.keys("q").await;
    rig.clock.set(Day::from_raw(DAY).start());

    // Esc at the door list hands the session back to the main menu.
    let menu = term.keys("\u{1b}").await;
    assert!(
        menu.contains("Main menu") && !menu.contains("Pick a door"),
        "{menu}"
    );
}

/// The reserved look-around key (D98) shows the door's text view on either transport, any key
/// returns to the game without reaching it, and a key sent after the world went stale (the
/// board re-reads the world first) still plays normally.
async fn look_around<T: Term>(term: &mut T, rig: &Rig) {
    term.keys("d").await;
    let game = term.keys("\r").await;
    assert!(game.contains("Tosses left today: 5"), "{game}");

    let view = term.keys("?").await;
    assert!(view.contains("Look around"), "{view}");
    assert!(view.contains("Tosses left today: 5"), "{view}");

    // The key that closes the view is not passed to the door: `h` would have been a toss.
    let back = term.keys("h").await;
    assert!(!back.contains("Look around"), "{back}");
    assert!(back.contains("Tosses left today: 5"), "{back}");

    // Time passes: the next key goes through the world re-read and still tosses.
    rig.clock.set(russh_bbs_core::store::UnixSeconds::from_raw(
        Day::from_raw(DAY).start().as_raw() + 5,
    ));
    let tossed = term.keys("h").await;
    assert!(tossed.contains("Tosses left today: 4"), "{tossed}");
    term.keys("q").await;
    rig.clock.set(Day::from_raw(DAY).start());
}

#[tokio::test]
async fn the_look_around_key_works_over_ssh() {
    let rig = Rig::start().await;
    let (_, key) = rig.member("cat").await;
    let mut ssh = Ssh::at_menu(&rig, &key, "anyone").await;
    look_around(&mut ssh, &rig).await;
}

#[tokio::test]
async fn the_look_around_key_works_over_telnet() {
    let rig = Rig::start().await;
    rig.account("dov", Slug::MEMBER).await;
    let mut tn = Tn::at_menu(&rig, "dov").await;
    look_around(&mut tn, &rig).await;
}

#[tokio::test]
async fn the_example_door_works_over_ssh() {
    let rig = Rig::start().await;
    let (account, key) = rig.member("ann").await;
    let mut ssh = Ssh::at_menu(&rig, &key, "anyone").await;
    walk(&mut ssh, &rig, account).await;
    // Ctrl-C logs off from inside a door: the channel closes and the session is gone.
    ssh.keys("d").await;
    ssh.keys("\r").await;
    ssh.channel.data_bytes(vec![3]).await.unwrap();
    timeout(WAIT, async { while ssh.channel.wait().await.is_some() {} })
        .await
        .expect("Ctrl-C inside a door did not end the session");
}

#[tokio::test]
async fn the_example_door_works_over_telnet() {
    let rig = Rig::start().await;
    let account = rig.account("bea", Slug::MEMBER).await;
    let mut tn = Tn::at_menu(&rig, "bea").await;
    walk(&mut tn, &rig, account).await;
}

/// The same keys give the same screen on both transports, below the footer row telnet adds.
#[tokio::test]
async fn both_transports_draw_the_same_door_screen() {
    let rig = Rig::start().await;
    let (_, key) = rig.member("ann").await;
    rig.account("bea", Slug::MEMBER).await;
    let mut ssh = Ssh::at_menu(&rig, &key, "anyone").await;
    let mut tn = Tn::at_menu(&rig, "bea").await;
    for keys in ["d", "\r", "hh"] {
        let a = ssh.keys(keys).await;
        let b = tn.keys(keys).await;
        // The handles differ, and the telnet screen has its marker row; the door's words do not.
        let strip = |s: String| {
            s.replace("ann", "NAME")
                .replace("bea", "NAME")
                .replace('-', " ")
                .replace("UNENCRYPTED", "")
                .replace("anyone on the network can read this session", "")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        let (a, b) = (strip(a), strip(b));
        if keys == "hh" {
            // Coin outcomes are random per session; compare the fixed lines.
            for fixed in ["Coin Toss", "Tosses left today: 3", "Coins:"] {
                assert!(
                    a.contains(fixed) && b.contains(fixed),
                    "{fixed}\nssh: {a}\ntelnet: {b}"
                );
            }
        } else {
            assert_eq!(
                a, b,
                "the same keys must give the same screen on both transports"
            );
        }
    }
}

#[tokio::test]
async fn a_barred_player_is_refused_and_a_revoked_permission_hides_the_door() {
    let rig = Rig::start().await;
    let (ann, key) = rig.member("ann").await;
    let staff = rig.account("cora", Slug::COSYSOP).await;
    let snapshot = rig.store.snapshot(Some(staff)).await.unwrap();
    doors::bar_player(&rig.store, &snapshot, &Rig::door(), ann)
        .await
        .unwrap();
    let mut ssh = Ssh::at_menu(&rig, &key, "anyone").await;
    ssh.keys("d").await;
    let refused = ssh.keys("\r").await;
    assert!(
        refused.contains("You are not allowed to play this door."),
        "{refused}"
    );
    assert!(!refused.contains("Tosses left"), "{refused}");
    // The bar was audited, and lifting it lets the player in.
    let rows = known_events(rig.store.page(&AuditQuery::default()).await.unwrap());
    assert!(
        rows.iter()
            .any(|r| r.event.action == Action::DoorPlayerBarred)
    );
    doors::unbar_player(&rig.store, &snapshot, &Rig::door(), ann)
        .await
        .unwrap();
    assert!(ssh.keys("\r").await.contains("Tosses left today: 5"));
    drop(ssh);

    // Taking `doors.run` away hides the menu item on the next sign-in.
    sqlx::query("DELETE FROM role_permission WHERE role = 'member' AND permission = 'doors.run'")
        .execute(&rig.db)
        .await
        .unwrap();
    let mut ssh = Ssh::at_menu(&rig, &key, "anyone").await;
    let menu = ssh.keys("d").await;
    assert!(
        !menu.contains("Coin Toss") && menu.contains("Main menu"),
        "{menu}"
    );
    assert!(!menu.contains("Doors"), "{menu}");
    let _ = Permission::DoorsRun;
    let _ = AccountId::from_raw(0);
}
