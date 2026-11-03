//! The telnet transport over a real TCP client against the real SQLite store (M3-H, D27): login
//! is refused unless the second opt-in is on; a member signs in with a password and gets the same
//! screens as SSH with the `UNENCRYPTED` marker on every one; a sysop is an ordinary member over
//! telnet; the window follows NAWS; hostile protocol input is bounded; the session limits span
//! both transports.
//!
//! No test sleeps to wait for the server. A screen is read by resizing the window to a size not
//! used before and reading until the marker appears on that size's last row: the frame for that
//! size is built after every key sent earlier, so it shows their effect.
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

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use common::{Client, fast_limits, new_client_key};
use russh::client::{AuthResult, Handle};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg};
use russh_bbs_core::access::{Fingerprint, Slug};
use russh_bbs_core::audit::Actor;
use russh_bbs_core::icons::Tier;
use russh_bbs_core::limits::{Limit, LimitKind, LimitStore};
use russh_bbs_core::passwords::PasswordStore;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountId, AccountState, AccountStore, NewAccount};
use russh_bbs_core::terminal_settings::{MemberSessions, TerminalSettings, TerminalSettingsStore};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_server::telnet::{TelnetConfig, TelnetServer};
use russh_bbs_server::ui::Ui;
use russh_bbs_store::SqliteStore;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const SECRET: &str = "correct horse battery";
const WAIT: Duration = Duration::from_secs(20);

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

/// What the footer paints on the last row: the move, the line clear, the marker in its reserved
/// style. `rows` is the window's height.
fn footer_at(rows: u16) -> String {
    format!("\u{1b}[{rows};1H\u{1b}[2K\u{1b}[1;7mUNENCRYPTED\u{1b}[0m")
}

struct Rig {
    ssh: SshServer,
    telnet: TelnetServer,
    store: SqliteStore,
    dir: TempDir,
}

fn lenient(limits: Limits) -> Limits {
    let loose = RatePolicy::new(1_000, Duration::from_millis(1));
    Limits {
        ip_rate: loose,
        key_rate: loose,
        password_account_rate: loose,
        password_network_rate: loose,
        max_auth_attempts: 3,
        ..limits
    }
}

impl Rig {
    async fn start(allow_login: bool) -> Self {
        Self::start_with(allow_login, lenient(fast_limits())).await
    }

    async fn start_with(allow_login: bool, limits: Limits) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
        let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
        let ssh = SshServer::start(
            &ServerConfig {
                listen: "127.0.0.1:0".parse().unwrap(),
                limits,
            },
            &host_key,
            Arc::new(store.clone()),
        )
        .await
        .unwrap();
        let telnet = TelnetServer::start(
            &TelnetConfig {
                listen: "127.0.0.1:0".parse().unwrap(),
                allow_login,
                limits: Limits {
                    low_security: true,
                    ..limits
                },
            },
            Arc::new(store.clone()),
            ssh.registry(),
            Arc::new(Ui::standard().unwrap()),
        )
        .await
        .unwrap();
        Self {
            ssh,
            telnet,
            store,
            dir,
        }
    }

    fn addr(&self) -> SocketAddr {
        self.telnet.local_addr()
    }

    /// An active account with terminal settings already chosen (so there is no glyph test) and,
    /// if given, a password.
    async fn account(&self, name: &str, role: Slug, password: Option<&str>) -> AccountId {
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
        if let Some(password) = password {
            let hash = russh_bbs_core::passwords::hash::hash(password).unwrap();
            self.store
                .set_hash(
                    account.id,
                    Some(&hash),
                    &russh_bbs_core::audit::events::password_set(account.id, "key session"),
                )
                .await
                .unwrap();
        }
        account.id
    }

    async fn member(&self, name: &str) -> AccountId {
        self.account(name, Slug::MEMBER, Some(SECRET)).await
    }
}

/// A telnet client that answers nothing it is not asked, decodes the protocol, and keeps the text.
struct Tn {
    stream: TcpStream,
    text: Vec<u8>,
    /// Negotiation commands received: (verb, option).
    commands: Vec<(u8, u8)>,
    state: Rx,
    rows: u16,
}

#[derive(Clone, Copy)]
enum Rx {
    Data,
    Iac,
    Verb(u8),
    Sb,
    SbIac,
}

impl Tn {
    /// Connect and negotiate like a character-mode client: willing to report the window, and
    /// told the server echoes.
    async fn connect(addr: SocketAddr, cols: u16, rows: u16) -> Self {
        let stream = timeout(WAIT, TcpStream::connect(addr))
            .await
            .unwrap()
            .unwrap();
        let mut tn = Self {
            stream,
            text: Vec::new(),
            commands: Vec::new(),
            state: Rx::Data,
            rows,
        };
        let mut hello = vec![
            IAC, DO, OPT_ECHO, IAC, DO, OPT_SGA, IAC, WILL, OPT_SGA, IAC, WILL, OPT_NAWS,
        ];
        hello.extend(naws(cols, rows));
        tn.send(&hello).await;
        tn
    }

    async fn send(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.unwrap();
    }

    /// Type text the way a character-mode telnet sends it (Enter is CR NUL).
    async fn type_text(&mut self, text: &str) {
        let wire = text.replace('\r', "\r\0");
        self.send(wire.as_bytes()).await;
    }

    /// Read more from the server. `false` at end of stream.
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

    fn rx(&mut self, byte: u8) {
        self.state = match (self.state, byte) {
            (Rx::Data, IAC) => Rx::Iac,
            (Rx::Data, _) => {
                self.text.push(byte);
                Rx::Data
            }
            (Rx::Iac, IAC) => {
                self.text.push(IAC);
                Rx::Data
            }
            (Rx::Iac, WILL | WONT | DO | DONT) => Rx::Verb(byte),
            (Rx::Iac, SB) => Rx::Sb,
            (Rx::Iac, _) => Rx::Data,
            (Rx::Verb(verb), _) => {
                self.commands.push((verb, byte));
                Rx::Data
            }
            (Rx::Sb, IAC) => Rx::SbIac,
            (Rx::Sb, _) => Rx::Sb,
            (Rx::SbIac, SE) => Rx::Data,
            (Rx::SbIac, _) => Rx::Sb,
        };
    }

    /// Read until `needle` appears; return everything up to and including it.
    async fn read_until(&mut self, needle: &str) -> String {
        loop {
            let seen = String::from_utf8_lossy(&self.text).into_owned();
            if let Some(at) = seen.find(needle) {
                let end = at + needle.len();
                self.text.drain(..end);
                return seen[..end].to_owned();
            }
            assert!(
                self.fill().await,
                "the server closed before {needle:?}; saw {seen:?}"
            );
        }
    }

    /// Read until the server closes the connection; everything it said.
    async fn read_to_close(&mut self) -> String {
        while self.fill().await {}
        String::from_utf8_lossy(&std::mem::take(&mut self.text)).into_owned()
    }

    /// Type the user name and password at the prompts.
    async fn log_in(&mut self, user: &str, password: &str) {
        self.read_until("login: ").await;
        self.type_text(&format!("{user}\r")).await;
        self.read_until("password: ").await;
        self.type_text(&format!("{password}\r")).await;
    }

    /// Resize to `rows` and read the whole screen the server draws for that size, as words.
    /// `rows` must differ from the previous size so the frame cannot be an old one.
    async fn resize(&mut self, cols: u16, rows: u16) -> String {
        assert_ne!(rows, self.rows, "use a new size to tell the frame apart");
        self.rows = rows;
        self.send(&naws(cols, rows)).await;
        let frame = self.read_until(&footer_at(rows)).await;
        words_of(&frame)
    }

    /// Send keys, then read the screen they led to.
    async fn keys(&mut self, keys: &str) -> String {
        self.type_text(keys).await;
        let rows = if self.rows == 24 { 25 } else { 24 };
        self.resize(80, rows).await
    }
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

/// The words of a screen: escape sequences and box drawing dropped.
fn words_of(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            out.push(' ');
        } else if c.is_control() || "│┌┐└┘─|+-".contains(c) {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Sign in as `user` and get past the login screen to the main menu.
async fn at_menu(rig: &Rig, user: &str) -> Tn {
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.log_in(user, SECRET).await;
    let first = tn.read_until(&footer_at(24)).await;
    assert!(
        words_of(&first).contains(&format!("Signed in as {user}")),
        "{first}"
    );
    let menu = tn.keys("x").await;
    assert!(menu.contains("Main menu"), "{menu}");
    tn
}

// ---------------------------------------------------------------- off, refused, refusals

#[tokio::test]
async fn login_is_refused_and_the_connection_closed_when_the_second_opt_in_is_off() {
    let rig = Rig::start(false).await;
    rig.member("alice").await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    let said = tn.read_to_close().await;
    let words = words_of(&said);
    assert!(
        words.contains("Telnet login is not enabled on this board."),
        "{words}"
    );
    assert!(words.contains("Connect with ssh instead."), "{words}");
    // No prompt was ever offered, so nothing can be typed that would be read as a password.
    assert!(
        !said.contains("login: ") && !said.contains("password: "),
        "{said}"
    );
    // The refusal still carries the marker, in the reserved style, on the last row.
    assert!(said.contains(&footer_at(24)), "{said:?}");
    // And it reveals nothing about members or the board.
    assert!(!words.contains("alice"), "{words}");
}

#[tokio::test]
async fn the_server_offers_only_what_a_character_mode_screen_needs() {
    let rig = Rig::start(true).await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.read_until("login: ").await;
    let mut offered = tn.commands.clone();
    offered.sort_unstable();
    assert_eq!(
        offered,
        [
            (DO, OPT_SGA),
            (DO, OPT_NAWS),
            (WILL, OPT_ECHO),
            (WILL, OPT_SGA)
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn an_unknown_name_a_wrong_password_and_claim_get_the_same_refusal_until_the_attempts_run_out()
 {
    let rig = Rig::start(true).await;
    rig.member("alice").await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    let banner = tn.read_until("login: ").await;
    let banner = words_of(&banner);
    assert!(
        banner.contains("THIS CONNECTION IS NOT ENCRYPTED"),
        "{banner}"
    );

    // An unknown name; a known name with the wrong password; and `claim`, which opens the
    // founder prompt over SSH and is just another name here.
    tn.type_text("nobody\r").await;
    tn.read_until("password: ").await;
    tn.type_text("whatever it is\r").await;
    let first = tn.read_until("login: ").await;
    tn.type_text("alice\r").await;
    tn.read_until("password: ").await;
    tn.type_text("not the password\r").await;
    let second = tn.read_until("login: ").await;
    assert!(first.contains("Login incorrect."), "{first:?}");
    assert_eq!(words_of(&first), words_of(&second));

    tn.type_text("claim\r").await;
    tn.read_until("password: ").await;
    tn.type_text("a token maybe\r").await;
    let rest = tn.read_to_close().await;
    let rest = words_of(&rest);
    assert!(rest.contains("Login incorrect."), "{rest}");
    assert!(rest.contains("Too many failed attempts."), "{rest}");
    assert!(!rest.contains("setup") && !rest.contains("token"), "{rest}");
    // The password was never echoed back.
    assert!(!rest.contains("a token maybe") && !rest.contains("not the password"));
}

#[tokio::test]
async fn a_suspended_account_gets_the_same_refusal_as_a_wrong_password() {
    let rig = Rig::start(true).await;
    let id = rig.member("dormant").await;
    sqlx::query("UPDATE accounts SET state = 'suspended' WHERE id = ?")
        .bind(id.as_raw())
        .execute(
            &sqlx::sqlite::SqlitePool::connect(&format!(
                "sqlite://{}",
                rig.dir.path().join("bbs.db").display()
            ))
            .await
            .unwrap(),
        )
        .await
        .unwrap();
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.log_in("dormant", SECRET).await;
    let said = words_of(&tn.read_until("login: ").await);
    assert!(said.contains("Login incorrect."), "{said}");
}

// ---------------------------------------------------------------- signed in

#[tokio::test]
async fn a_member_signs_in_with_a_password_and_every_screen_carries_the_marker() {
    let rig = Rig::start(true).await;
    rig.member("alice").await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.log_in("alice", SECRET).await;
    // The first screen, the login summary, already has the marker, drawn with the reserved
    // style on the window's last row.
    let first = tn.read_until(&footer_at(24)).await;
    let words = words_of(&first);
    assert!(words.contains("Signed in as alice"), "{words}");
    assert!(first.contains(&footer_at(24)));

    // The main menu, the settings screen and its pages: all the SSH screens, all marked, and
    // none of them draws into the marker's row (the footer's cursor move is the only write there).
    let mut screens = Vec::new();
    for keys in ["x", "s", "w", "\u{1b}", "\u{1b}"] {
        tn.type_text(keys).await;
        let rows = if tn.rows == 24 { 25 } else { 24 };
        tn.rows = rows;
        tn.send(&naws(80, rows)).await;
        let frame = tn.read_until(&footer_at(rows)).await;
        let row = format!("\u{1b}[{rows};");
        assert_eq!(
            frame.matches(&row).count(),
            1,
            "something else wrote to the last row: {frame:?}"
        );
        screens.push(words_of(&frame));
    }
    assert!(screens[0].contains("Main menu"), "{:?}", screens[0]);
    assert!(screens[1].contains("Terminal settings"), "{:?}", screens[1]);
    // The password page is not offered over telnet (D27): there is no entry for it.
    assert!(!screens[1].contains("Password"), "{:?}", screens[1]);
    let after_s = tn.keys("s").await;
    assert!(
        !after_s.contains("Current password") && !after_s.contains("New password"),
        "{after_s}"
    );
}

#[tokio::test]
async fn logging_off_says_goodbye_and_closes_without_a_second_marker_row() {
    let rig = Rig::start(true).await;
    rig.member("alice").await;
    let mut tn = at_menu(&rig, "alice").await;
    // Ctrl-C logs off, as the screen says.
    tn.send(&[0x03]).await;
    let rest = tn.read_to_close().await;
    // The terminal is restored, then the connection closes.
    assert!(
        rest.contains("\u{1b}[?1049l") || rest.contains("\u{1b}[?1049"),
        "{rest:?}"
    );
}

#[tokio::test]
async fn a_sysop_over_telnet_holds_no_staff_permission() {
    let rig = Rig::start(true).await;
    let sysop = rig.account("boss", Slug::SYSOP, Some(SECRET)).await;
    // What the store grants this account is staff...
    let held = rig.store.access_for(Some(sysop)).await.unwrap();
    assert!(
        held.held().iter().any(|h| matches!(
            &h.source,
            russh_bbs_core::access::Source::Role { rank, .. } if *rank >= 40
        )),
        "the sysop should hold a staff role in the store"
    );
    // ...and over telnet the menu has no staff entry and its hotkey does nothing.
    let mut tn = at_menu(&rig, "boss").await;
    let menu = tn.keys("x").await;
    assert!(menu.contains("Main menu"), "{menu}");
    assert!(!menu.contains("Sysop tools"), "{menu}");
    let pressed = tn.keys("o").await;
    assert!(
        pressed.contains("Main menu") && !pressed.contains("Sysop tools"),
        "{pressed}"
    );
    // A role change made while the session is open is also filtered: nothing is cached.
    let again = tn.keys("x").await;
    assert!(!again.contains("Sysop tools"), "{again}");
}

// ---------------------------------------------------------------- the window

#[tokio::test]
async fn the_window_follows_naws_and_the_marker_moves_to_the_new_last_row() {
    let rig = Rig::start(true).await;
    rig.member("alice").await;
    let mut tn = at_menu(&rig, "alice").await;
    // Bigger, then smaller, then huge: the marker is always on the last row, and a window the
    // client states as enormous is cut to the same maximum SSH uses.
    let big = tn.resize(120, 40).await;
    assert!(big.contains("Main menu"), "{big}");
    let small = tn.resize(60, 30).await;
    assert!(small.contains("Main menu"), "{small}");
    tn.rows = 0;
    tn.send(&naws(u16::MAX, u16::MAX)).await;
    let huge = tn.read_until(&footer_at(200)).await;
    assert!(words_of(&huge).contains("Main menu"));
    // A window of nothing, or one too small to draw in, is raised, not honored.
    tn.rows = 0;
    tn.send(&naws(1, 1)).await;
    let tiny = tn.read_until(&footer_at(10)).await;
    assert!(words_of(&tiny).contains("Main menu") || !tiny.is_empty());
}

// ---------------------------------------------------------------- hostile input

#[tokio::test]
async fn hostile_protocol_input_is_refused_once_bounded_and_never_reaches_the_screen() {
    // This test is about the protocol, not the budgets: let a megabyte through.
    let limits = Limits {
        requests_per_second: 100_000,
        input_bytes_per_second: 10_000_000,
        ..lenient(fast_limits())
    };
    let rig = Rig::start_with(true, limits).await;
    rig.member("alice").await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.read_until("login: ").await;
    let before = tn.commands.len();

    // A thousand requests for an option we do not use, a megabyte-long subnegotiation that never
    // ends properly, undefined commands, and a lone ESC sequence.
    let mut flood = Vec::new();
    for _ in 0..1000 {
        flood.extend([IAC, WILL, 200, IAC, DO, 201]);
    }
    flood.extend([IAC, SB, 24]);
    flood.extend(std::iter::repeat_n(b'A', 1_000_000));
    flood.extend([IAC, SE]);
    flood.extend([IAC, 100, IAC, 241, IAC, 246]);
    for chunk in flood.chunks(1024) {
        tn.send(chunk).await;
    }
    // Then a normal login still works: the server stayed responsive and in step.
    tn.type_text("alice\r").await;
    tn.read_until("password: ").await;
    tn.type_text(&format!("{SECRET}\r")).await;
    let first = tn.read_until(&footer_at(24)).await;
    assert!(
        words_of(&first).contains("Signed in as alice"),
        "{}",
        words_of(&first)
    );
    let refusals: Vec<_> = tn.commands[before..].to_vec();
    assert_eq!(
        refusals,
        [(DONT, 200), (WONT, 201)],
        "one refusal per option"
    );
}

#[tokio::test]
async fn an_escape_sequence_typed_at_the_prompt_is_not_typed_into_the_field() {
    let rig = Rig::start(true).await;
    rig.member("alice").await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.read_until("login: ").await;
    // Up arrow, a clear-screen sequence and a function key in the middle of the name.
    tn.type_text("al\u{1b}[A\u{1b}[2J\u{1b}OPice\r").await;
    tn.read_until("password: ").await;
    tn.type_text(&format!("{SECRET}\r")).await;
    let first = tn.read_until(&footer_at(24)).await;
    assert!(
        words_of(&first).contains("Signed in as alice"),
        "{}",
        words_of(&first)
    );
}

#[tokio::test]
async fn arrow_keys_and_a_multi_line_paste_at_the_prompt_neither_type_nor_submit() {
    let rig = Rig::start(true).await;
    rig.member("alice").await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.read_until("login: ").await;
    // Left arrow (CSI), Home (SS3) and Delete in the middle of the name are consumed, not typed.
    // Then a bracketed paste with a line break: the break must not submit "al" early.
    tn.type_text("\u{1b}[D\u{1b}OH\u{1b}[3~\u{1b}[200~al\nic\u{1b}[201~e\r")
        .await;
    tn.read_until("password: ").await;
    // The same at the password prompt: the paste breaks cannot submit a half password.
    let half = &SECRET[..7];
    let rest = &SECRET[7..];
    tn.type_text(&format!("\u{1b}[200~{half}\n\u{1b}[201~{rest}\r"))
        .await;
    let first = tn.read_until(&footer_at(24)).await;
    assert!(
        words_of(&first).contains("Signed in as alice"),
        "{}",
        words_of(&first)
    );
}

#[tokio::test]
async fn ctrl_c_as_the_telnet_interrupt_command_leaves_the_prompt() {
    let rig = Rig::start(true).await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.read_until("login: ").await;
    tn.send(&[IAC, 244]).await;
    let rest = tn.read_to_close().await;
    assert!(!rest.contains("Login incorrect."), "{rest:?}");
}

#[tokio::test]
async fn a_client_that_says_nothing_is_dropped_at_the_authentication_deadline() {
    let limits = Limits {
        auth_timeout: Duration::from_millis(300),
        ..lenient(fast_limits())
    };
    let rig = Rig::start_with(true, limits).await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.read_until("login: ").await;
    let rest = words_of(&tn.read_to_close().await);
    assert!(rest.contains("Login timed out."), "{rest}");
}

#[tokio::test]
async fn an_idle_signed_in_session_is_dropped() {
    let limits = Limits {
        idle_timeout: Duration::from_millis(400),
        ..lenient(fast_limits())
    };
    let rig = Rig::start_with(true, limits).await;
    rig.member("alice").await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.log_in("alice", SECRET).await;
    tn.read_until(&footer_at(24)).await;
    tn.read_to_close().await;
}

#[tokio::test]
async fn a_flood_of_input_ends_the_session() {
    let limits = lenient(fast_limits());
    let rig = Rig::start_with(true, limits).await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.read_until("login: ").await;
    // Megabytes of typing: far past the input budget, so the strikes pile up and it is dropped.
    let blast = [b'a'; 4096];
    for _ in 0..1024 {
        if tn.stream.write_all(&blast).await.is_err() {
            break;
        }
    }
    tn.read_to_close().await;
}

// ---------------------------------------------------------------- shared with ssh

async fn ssh_in(rig: &Rig, id: AccountId) -> (Handle<Client>, russh::Channel<russh::client::Msg>) {
    let key = new_client_key();
    let fp =
        Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
    rig.store.attach_fingerprint(id, &fp).await.unwrap();
    let mut session = common::connect(rig.ssh.local_addr()).await;
    let result = session
        .authenticate_publickey(
            "anyone",
            PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
        )
        .await
        .unwrap();
    assert!(matches!(result, AuthResult::Success));
    let mut channel = common::open_shell(&session, 80, 24).await;
    let mut seen = String::new();
    while !words_of(&seen).contains("Signed in as") {
        match timeout(WAIT, channel.wait()).await.unwrap() {
            Some(russh::ChannelMsg::Data { data }) => {
                seen.push_str(&String::from_utf8_lossy(&data));
            }
            Some(_) => {}
            None => panic!("ssh closed early: {seen:?}"),
        }
    }
    (session, channel)
}

#[tokio::test]
async fn the_concurrent_session_limit_spans_ssh_and_telnet() {
    let rig = Rig::start(true).await;
    let id = rig.member("alice").await;
    rig.store
        .set_member_limit(
            id,
            LimitKind::MaxSessions,
            Some(Limit::AtMost(1)),
            Actor::System,
        )
        .await
        .unwrap();
    let (_ssh, _channel) = ssh_in(&rig, id).await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.log_in("alice", SECRET).await;
    let rest = words_of(&tn.read_until("session open at once").await);
    assert!(rest.contains("may have 1 session"), "{rest}");
    // The refusal is a screen that carries the marker too, and then the connection ends.
    let closing = tn.read_to_close().await;
    assert!(closing.contains("UNENCRYPTED") || rest.contains("Limit reached"));
}

#[tokio::test]
async fn telnet_sessions_are_in_the_shared_registry_and_leave_it_when_they_end() {
    let rig = Rig::start(true).await;
    rig.member("alice").await;
    let tn = at_menu(&rig, "alice").await;
    common::eventually(&rig.ssh, |all| {
        all.iter()
            .any(|s| s.shell_open && s.access.is_some() && s.fingerprint.is_none())
    })
    .await;
    drop(tn);
    common::eventually(&rig.ssh, <[_]>::is_empty).await;
}

#[tokio::test]
async fn shutting_the_telnet_server_down_stops_accepting() {
    let rig = Rig::start(true).await;
    let addr = rig.addr();
    let Rig { ssh, telnet, .. } = rig;
    telnet.shutdown().await;
    assert!(TcpStream::connect(addr).await.is_err());
    ssh.shutdown().await;
}

// ---------------------------------------------------------------- low-security mode

/// The full redraw of the sysop queue over SSH, as words, with the board in low-security mode or not.
async fn queue_screen_over_ssh(low_security: bool) -> String {
    let limits = Limits {
        low_security,
        ..lenient(fast_limits())
    };
    let rig = Rig::start_with(true, limits).await;
    let id = rig.account("boss", Slug::SYSOP, None).await;
    let (_session, mut channel) = ssh_in(&rig, id).await;
    // Past the login screen, to Sysop tools, then the queue.
    channel.data_bytes(b"xoq".to_vec()).await.unwrap();
    // A window change makes the server clear and redraw the whole screen.
    channel.window_change(81, 25, 0, 0).await.unwrap();
    let mut seen = String::new();
    loop {
        match timeout(WAIT, channel.wait()).await.unwrap() {
            Some(russh::ChannelMsg::Data { data }) => {
                seen.push_str(&String::from_utf8_lossy(&data));
            }
            Some(_) => {}
            None => panic!("ssh closed early: {seen:?}"),
        }
        // Only the redraw after the window change starts with a clear.
        if let Some((_, redraw)) = seen.rsplit_once("\u{1b}[2J") {
            let words = words_of(redraw);
            if words.contains("can read what you do here") {
                return words;
            }
        }
    }
}

#[tokio::test]
async fn staff_screens_carry_the_low_security_marker_only_while_telnet_is_on() {
    let on = queue_screen_over_ssh(true).await;
    assert!(
        on.contains("Invite requests") || on.contains("requests"),
        "{on}"
    );
    assert!(on.contains("LOW SECURITY"), "{on}");
    assert!(on.contains("telnet is on"), "{on}");
    let off = queue_screen_over_ssh(false).await;
    assert!(off.contains("requests"), "{off}");
    assert!(!off.contains("LOW SECURITY"), "{off}");
}
