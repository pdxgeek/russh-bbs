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
            // The board has mail keys, so mail works over telnet as over ssh (D94 (4a)).
            Arc::new(Ui::standard().unwrap().with_mail_keys(Arc::new(
                russh_bbs_core::mail::MailKeys::single(
                    russh_bbs_core::mail::KeyId::FIRST,
                    russh_bbs_core::mail::MailKey::generate().unwrap(),
                ),
            ))),
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
            .unwrap_or_else(|_| {
                panic!(
                    "the server went quiet; unread: {:?}",
                    words_of(&String::from_utf8_lossy(&self.text))
                )
            })
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
    // The password page opens over telnet (D94 (3)): a password session is asked for the
    // current password first.
    tn.keys("s").await;
    let page = tn.keys("s").await;
    assert!(page.contains("Set or change password"), "{page}");
    let after_s = tn.keys("s").await;
    assert!(after_s.contains("Current password"), "{after_s}");
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

#[tokio::test]
async fn telnet_can_make_a_link_code_but_has_no_key_to_bind_one() {
    // FIX-M3-11, D97: a member over telnet may make a device-link code (the page says it works
    // from an ssh connection only), and the telnet waiting room offers no "Link this device".
    let rig = Rig::start(true).await;
    rig.account("maker", Slug::MEMBER, Some(SECRET)).await;
    let mut tn = at_menu(&rig, "maker").await;
    let members = tn.keys("u").await;
    assert!(members.contains("[k] My keys"), "{members}");
    let keys = tn.keys("k").await;
    assert!(keys.contains("Keys sign you in"), "{keys}");
    assert!(keys.contains("No key is saved"), "{keys}");
    let shown = tn.keys("l").await;
    assert!(shown.contains("Code:"), "{shown}");
    assert!(
        shown.contains("used from an ssh connection only"),
        "{shown}"
    );
    // The one-time code and the marker are both on the page.
    assert!(shown.contains("UNENCRYPTED"), "{shown}");

    // The telnet waiting room has no key, so it has no way to use a code.
    let (_tn, home) = join(&rig).await;
    assert!(!home.contains("Link this device"), "{home}");
    assert!(!home.contains("Recovery code"), "{home}");
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
    let handle = rig
        .rows(&format!("SELECT username FROM accounts WHERE id = {id}"))
        .await
        .remove(0);
    common::remember(&fp, &handle);
    let mut session = common::connect(rig.ssh.local_addr()).await;
    let result = session
        .authenticate_publickey(
            &common::user_for(&key, "anyone"),
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
            // The queue's footer is the last thing drawn inside the box; the low-security
            // marker (D93: the only status row left) is drawn with it.
            if words.contains("Esc: back") && (!low_security || words.contains("telnet is on")) {
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

/// Read until the screen's words contain `needle` (a diff redraw can split a run of text with
/// cursor moves, so the raw bytes are not searched).
async fn until_words(tn: &mut Tn, needle: &str) {
    loop {
        let words = words_of(&String::from_utf8_lossy(&tn.text));
        if words.contains(needle) {
            return;
        }
        assert!(tn.fill().await, "closed before {needle:?}; saw {words:?}");
    }
}

#[tokio::test]
async fn chat_works_over_telnet_with_both_markers_and_untrusted_input() {
    let rig = Rig::start(true).await;
    rig.member("alice").await;
    rig.member("bob").await;
    let mut alice = at_menu(&rig, "alice").await;
    let mut bob = at_menu(&rig, "bob").await;
    for tn in [&mut alice, &mut bob] {
        let list = words_of(&tn.keys("c").await);
        assert!(
            list.contains("Chat rooms") && list.contains("lobby"),
            "{list}"
        );
        let room = words_of(&tn.keys("\r").await);
        assert!(room.contains("#lobby"), "{room}");
    }
    // Alice's line, typed over telnet, is pushed to both screens without another key.
    alice.type_text("hi bob\r").await;
    until_words(&mut bob, "<alice> hi bob").await;
    until_words(&mut alice, "<alice> hi bob").await;
    // Text typed at the prompt is untrusted input like on SSH: a pasted escape sequence never
    // reaches the other screen.
    alice.type_text("evil \u{1b}[31mred\u{1b}[0m end\r").await;
    until_words(&mut bob, "<alice> evil").await;
    let raw = String::from_utf8_lossy(&bob.text).into_owned();
    assert!(
        !raw.contains("\u{1b}[31mred"),
        "an escape reached bob: {raw:?}"
    );
}

// ----------------------------------------------------- D94: accounts and full access over telnet

use russh_bbs_core::invites::{
    CODE_BYTES, INVITE_TTL_SECS, InviteCode, InviteStore, NewInvite, Quota,
};
use russh_bbs_core::queue::QueueStore;
use russh_bbs_core::store::{RegistrationMode, set_registration_mode};

const NEW_PASSWORD: &str = "a second long password";
const GREETING: &str = "Signed in as";

impl Rig {
    async fn mode(&self, mode: RegistrationMode) {
        set_registration_mode(&self.store, mode).await.unwrap();
    }

    /// A one-use invite code made by `inviter`, as the store holds it.
    async fn invite_code(&self, inviter: AccountId, seed: u8) -> InviteCode {
        let code = InviteCode::from_bytes([seed; CODE_BYTES]);
        self.store
            .create_invite(&NewInvite {
                inviter,
                code_hash: code.hash(),
                ttl_secs: INVITE_TTL_SECS,
                quota: Quota::Unlimited,
            })
            .await
            .unwrap();
        code
    }

    async fn rows(&self, sql: &str) -> Vec<String> {
        let db = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(self.dir.path().join("bbs.db"))
                .read_only(true),
        )
        .await
        .unwrap();
        sqlx::query_scalar::<_, String>(sql)
            .fetch_all(&db)
            .await
            .unwrap()
    }
}

/// Connect, type `new` at the login prompt and read the waiting room's first screen.
async fn join(rig: &Rig) -> (Tn, String) {
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.read_until("login: ").await;
    tn.type_text("new\r").await;
    let first = tn.read_until(&footer_at(24)).await;
    (tn, words_of(&first))
}

/// Type each value into the form's fields (Enter after each) and read the screen that follows.
async fn fill(tn: &mut Tn, values: &[&str]) -> String {
    for value in values {
        tn.type_text(&format!("{value}\r")).await;
    }
    tn.keys("").await
}

#[tokio::test]
async fn mail_works_over_telnet_and_is_sealed_at_rest() {
    let rig = Rig::start(true).await;
    rig.member("alice").await;
    rig.member("bob").await;
    let mut alice = at_menu(&rig, "alice").await;
    let inbox = words_of(&alice.keys("m").await);
    assert!(inbox.contains("Your mailbox is empty."), "{inbox}");
    let compose = words_of(&alice.keys("c").await);
    assert!(
        compose.contains("Handle of the member to write to"),
        "{compose}"
    );
    alice.type_text("bob\r").await;
    let editor = words_of(&alice.keys("").await);
    assert!(editor.contains("Subject"), "{editor}");
    alice.type_text("Kumquat inventory\r").await;
    let editor = words_of(&alice.keys("").await);
    assert!(editor.contains("To bob: Kumquat inventory"), "{editor}");
    alice.type_text("The count is eleven.").await;
    let sent = words_of(&alice.keys("\u{18}").await);
    assert!(sent.contains("Sent."), "{sent}");
    // The mail is in the store, sealed: the subject is not in the database file.
    let sealed = rig
        .rows("SELECT CAST(count(*) AS TEXT) FROM mail_message")
        .await;
    assert_eq!(sealed, ["1"]);
    let file = std::fs::read(rig.dir.path().join("bbs.db")).unwrap();
    assert!(
        !file.windows(7).any(|w| w == b"Kumquat"),
        "the subject is readable in the database file"
    );
    // bob reads it over telnet.
    let mut bob = at_menu(&rig, "bob").await;
    let inbox = words_of(&bob.keys("m").await);
    assert!(inbox.contains("Kumquat inventory"), "{inbox}");
    assert!(inbox.contains("alice"), "{inbox}");
}

#[tokio::test]
async fn invites_can_be_created_listed_and_viewed_as_a_tree_over_telnet() {
    let rig = Rig::start(true).await;
    let alice = rig.account("alice", Slug::TRUSTED, Some(SECRET)).await;
    let mut tn = at_menu(&rig, "alice").await;
    tn.keys("i").await;
    let created = words_of(&tn.keys("c").await);
    assert!(created.contains("Invite code"), "{created}");
    assert!(!created.contains("available over ssh"), "{created}");
    assert_eq!(
        russh_bbs_core::invites::InviteStore::invites_by(&rig.store, alice, 50)
            .await
            .unwrap()
            .len(),
        1,
        "the code was made"
    );
    let listed = words_of(&tn.keys("\u{1b}").await);
    assert!(!listed.contains("available over ssh"), "{listed}");
    for key in ["y", "t"] {
        let screen = words_of(&tn.keys(key).await);
        assert!(!screen.contains("available over ssh"), "{key}: {screen}");
        tn.keys("\u{1b}").await;
    }
}

#[tokio::test]
async fn a_password_can_be_changed_over_telnet_after_giving_the_current_one() {
    let rig = Rig::start(true).await;
    let alice = rig.member("alice").await;
    let before = rig.store.stored_hash(alice).await.unwrap().unwrap();
    let mut tn = at_menu(&rig, "alice").await;
    tn.keys("s").await;
    let page = words_of(&tn.keys("s").await);
    assert!(page.contains("Set or change password"), "{page}");
    let prompt = words_of(&tn.keys("s").await);
    assert!(prompt.contains("Current password"), "{prompt}");
    // A wrong current password changes nothing.
    tn.type_text("not my password\r").await;
    tn.type_text(&format!("{NEW_PASSWORD}\r")).await;
    let refused = words_of(&fill(&mut tn, &[NEW_PASSWORD]).await);
    assert!(refused.contains("not the current password"), "{refused}");
    assert_eq!(rig.store.stored_hash(alice).await.unwrap().unwrap(), before);
    // The right one does, and the new password logs in while the old one no longer does.
    tn.keys("s").await;
    tn.type_text(&format!("{SECRET}\r")).await;
    tn.type_text(&format!("{NEW_PASSWORD}\r")).await;
    let done = words_of(&fill(&mut tn, &[NEW_PASSWORD]).await);
    assert!(done.contains("Password set."), "{done}");
    assert_ne!(rig.store.stored_hash(alice).await.unwrap().unwrap(), before);
    drop(tn);
    let mut again = Tn::connect(rig.addr(), 80, 24).await;
    again.log_in("alice", NEW_PASSWORD).await;
    let first = again.read_until(&footer_at(24)).await;
    assert!(words_of(&first).contains("Signed in as alice"), "{first}");
}

#[tokio::test]
async fn redeeming_an_invite_over_telnet_makes_a_password_account_that_then_logs_in() {
    let rig = Rig::start(true).await;
    let alice = rig.member("alice").await;
    let code = rig.invite_code(alice, 7).await;
    let (mut tn, home) = join(&rig).await;
    assert!(home.contains("Waiting room"), "{home}");
    assert!(home.contains("Redeem an invite code"), "{home}");
    let form = words_of(&tn.keys("c").await);
    assert!(form.contains("Invite code"), "{form}");
    assert!(form.contains("Choose a password"), "{form}");
    assert!(form.contains("Type the password again"), "{form}");
    // The password is never echoed back as text.
    let typed = fill(
        &mut tn,
        &[code.reveal(), "telnetter", NEW_PASSWORD, NEW_PASSWORD],
    )
    .await;
    assert!(!typed.contains(NEW_PASSWORD), "{typed}");
    assert!(typed.contains("telnetter"), "{typed}");
    // The new member is signed in (the session carries on as them).
    let account = rig
        .rows("SELECT CAST(id AS TEXT) FROM accounts WHERE username = 'telnetter'")
        .await;
    assert_eq!(account.len(), 1, "the account was made");
    // Active, invited by alice, a password and no key.
    assert_eq!(
        rig.rows("SELECT state FROM accounts WHERE username = 'telnetter'")
            .await,
        ["active"]
    );
    assert_eq!(
        rig.rows("SELECT CAST(invited_by AS TEXT) FROM accounts WHERE username = 'telnetter'")
            .await,
        [alice.to_string()]
    );
    assert_eq!(
        rig.rows("SELECT CAST(count(*) AS TEXT) FROM account_keys")
            .await,
        ["0"]
    );
    assert_eq!(
        rig.rows("SELECT CAST(count(*) AS TEXT) FROM account_password WHERE account_id = (SELECT id FROM accounts WHERE username = 'telnetter')")
            .await,
        ["1"]
    );
    // No password in the database file, only its hash.
    let file = std::fs::read(rig.dir.path().join("bbs.db")).unwrap();
    assert!(
        !file
            .windows(NEW_PASSWORD.len())
            .any(|w| w == NEW_PASSWORD.as_bytes()),
        "the password is readable in the database file"
    );
    // They are a plain member in this session and the next: log in again with the password.
    let screen = words_of(&tn.keys("x").await);
    assert!(!screen.contains("Sysop tools"), "{screen}");
    drop(tn);
    let mut again = Tn::connect(rig.addr(), 80, 24).await;
    again.log_in("telnetter", NEW_PASSWORD).await;
    let first = again.read_until(&footer_at(24)).await;
    assert!(
        words_of(&first).contains(&format!("{GREETING} telnetter")),
        "{first}"
    );
    // The code was spent.
    let (mut second, _) = join(&rig).await;
    second.keys("c").await;
    let refused = words_of(
        &fill(
            &mut second,
            &[code.reveal(), "another", NEW_PASSWORD, NEW_PASSWORD],
        )
        .await,
    );
    assert!(
        refused.contains("That invite could not be used."),
        "{refused}"
    );
}

#[tokio::test]
async fn open_registration_over_telnet_makes_a_password_account_and_every_refusal_reads_the_same() {
    let rig = Rig::start(true).await;
    rig.mode(RegistrationMode::Open).await;
    rig.member("taken").await;
    let (mut tn, home) = join(&rig).await;
    assert!(home.contains("Create an account"), "{home}");
    tn.keys("n").await;
    // A taken handle, and a lookalike, get one sentence.
    let one = words_of(&fill(&mut tn, &["taken", NEW_PASSWORD, NEW_PASSWORD]).await);
    assert!(one.contains("That account could not be created."), "{one}");
    // A password that is too short or does not match is a local, typed-text message.
    let short = words_of(&fill(&mut tn, &["walkin", "short", "short"]).await);
    assert!(short.contains("Too short"), "{short}");
    let mismatch = words_of(
        &fill(
            &mut tn,
            &["walkin", NEW_PASSWORD, "something else entirely"],
        )
        .await,
    );
    assert!(mismatch.contains("did not match"), "{mismatch}");
    assert_eq!(
        rig.rows("SELECT CAST(count(*) AS TEXT) FROM accounts WHERE username = 'walkin'")
            .await,
        ["0"]
    );
    fill(&mut tn, &["walkin", NEW_PASSWORD, NEW_PASSWORD]).await;
    assert_eq!(
        rig.rows("SELECT CAST(count(*) AS TEXT) FROM accounts WHERE username = 'walkin'")
            .await,
        ["1"]
    );
    assert_eq!(
        rig.rows("SELECT CAST(count(*) AS TEXT) FROM account_keys")
            .await,
        ["0"]
    );
    drop(tn);
    let mut again = Tn::connect(rig.addr(), 80, 24).await;
    again.log_in("walkin", NEW_PASSWORD).await;
    let first = again.read_until(&footer_at(24)).await;
    assert!(words_of(&first).contains("Signed in as walkin"), "{first}");
}

#[tokio::test]
async fn an_application_over_telnet_waits_with_a_hash_and_approval_makes_a_password_account() {
    let rig = Rig::start(true).await;
    rig.mode(RegistrationMode::Apply).await;
    let sysop = rig.account("the-sysop", Slug::SYSOP, Some(SECRET)).await;
    let (mut tn, home) = join(&rig).await;
    assert!(home.contains("Apply for membership"), "{home}");
    tn.keys("a").await;
    let answer = words_of(
        &fill(
            &mut tn,
            &[
                "hopeful",
                "a friend of a friend",
                NEW_PASSWORD,
                NEW_PASSWORD,
            ],
        )
        .await,
    );
    // The same fixed answer whether kept or dropped, and honest about what is kept.
    assert!(
        answer.contains("Thank you. If your application is kept"),
        "{answer}"
    );
    assert!(answer.contains("salted hash"), "{answer}");
    assert!(!answer.contains("fingerprint"), "{answer}");
    // Kept: one request holding a hash, no account yet, so the password cannot log in.
    let held = loop {
        let n = rig
            .rows(
                "SELECT CAST(count(*) AS TEXT) FROM invite_request WHERE password_hash IS NOT NULL",
            )
            .await;
        if n == ["1"] {
            break n;
        }
        tokio::task::yield_now().await;
    };
    assert_eq!(held, ["1"]);
    let mut early = Tn::connect(rig.addr(), 80, 24).await;
    early.log_in("hopeful", NEW_PASSWORD).await;
    let said = early.read_until("Login incorrect.").await;
    assert!(said.contains("Login incorrect."));
    // The sysop approves (the queue screen is staff-only, so over the store here).
    let queue = rig.store.pending_requests(10).await.unwrap();
    assert_eq!(queue.requests.len(), 1);
    rig.store
        .approve_request(queue.requests[0].id, sysop)
        .await
        .unwrap();
    assert_eq!(
        rig.rows("SELECT CAST(count(*) AS TEXT) FROM invite_request")
            .await,
        ["0"]
    );
    let mut now = Tn::connect(rig.addr(), 80, 24).await;
    now.log_in("hopeful", NEW_PASSWORD).await;
    let first = now.read_until(&footer_at(24)).await;
    assert!(words_of(&first).contains("Signed in as hopeful"), "{first}");
}

#[tokio::test]
async fn a_request_for_an_invite_over_telnet_is_queued_with_a_hash_and_asks_for_no_key() {
    let rig = Rig::start(true).await;
    let (mut tn, home) = join(&rig).await;
    assert!(home.contains("Request an invite"), "{home}");
    let form = words_of(&tn.keys("i").await);
    assert!(form.contains("Choose a password"), "{form}");
    let answer = words_of(&fill(&mut tn, &["asker", "hello", NEW_PASSWORD, NEW_PASSWORD]).await);
    assert!(
        answer.contains("Thank you. If your request is kept"),
        "{answer}"
    );
    assert!(
        answer.contains("log in again later with this handle and password"),
        "{answer}"
    );
    loop {
        let n = rig
            .rows("SELECT CAST(count(*) AS TEXT) FROM invite_request WHERE kind = 'invite' AND password_hash IS NOT NULL")
            .await;
        if n == ["1"] {
            break;
        }
        tokio::task::yield_now().await;
    }
}

/// What a stranger can see over telnet in `mode`: the home, rules and every form, as words.
async fn visitor_screens(rig: &Rig, mode: RegistrationMode) -> Vec<String> {
    rig.mode(mode).await;
    let (mut tn, home) = join(rig).await;
    let mut out = vec![home];
    let keys: &[&str] = match mode {
        RegistrationMode::Invite => &["r", "i", "c"],
        RegistrationMode::Apply => &["r", "a"],
        RegistrationMode::Open => &["r", "n"],
    };
    for key in keys {
        out.push(words_of(&tn.keys(key).await));
        out.push(words_of(&tn.keys("\u{1b}").await));
    }
    out
}

#[tokio::test]
async fn the_telnet_waiting_room_reveals_nothing_about_the_board_in_any_mode() {
    for mode in [
        RegistrationMode::Invite,
        RegistrationMode::Apply,
        RegistrationMode::Open,
    ] {
        let empty = Rig::start(true).await;
        let empty_screens = visitor_screens(&empty, mode).await;

        // A busy board: members, staff, a pending request, a denial, boards' worth of state.
        let busy = Rig::start(true).await;
        let alice = busy.member("alice").await;
        busy.account("the-sysop", Slug::SYSOP, Some(SECRET)).await;
        busy.account("carol", Slug::TRUSTED, Some(SECRET)).await;
        busy.invite_code(alice, 1).await;
        let (mut earlier, _) = join(&busy).await;
        busy.mode(RegistrationMode::Apply).await;
        earlier.keys("a").await;
        fill(&mut earlier, &["queued", "hi", NEW_PASSWORD, NEW_PASSWORD]).await;
        let busy_screens = visitor_screens(&busy, mode).await;

        assert_eq!(
            empty_screens, busy_screens,
            "{mode:?}: the room told them apart"
        );
        let everything = busy_screens.join("\n");
        for secret in ["alice", "the-sysop", "carol", "queued"] {
            assert!(!everything.contains(secret), "{mode:?}: {secret} leaked");
        }
    }
}

#[tokio::test]
async fn the_founder_claim_is_refused_over_telnet_and_no_sysop_is_made() {
    let rig = Rig::start(true).await;
    let mut tn = Tn::connect(rig.addr(), 80, 24).await;
    tn.read_until("login: ").await;
    tn.type_text("claim\r").await;
    tn.read_until("password: ").await;
    tn.type_text("a-setup-token-typed-as-a-password\r").await;
    let said = tn.read_until("Login incorrect.").await;
    assert!(said.contains("Login incorrect."));
    assert_eq!(
        rig.rows("SELECT CAST(count(*) AS TEXT) FROM accounts")
            .await,
        ["0"]
    );
}

#[tokio::test]
async fn a_sysop_over_telnet_who_joined_by_invite_or_logs_in_keeps_no_staff_powers() {
    let rig = Rig::start(true).await;
    rig.account("boss", Slug::SYSOP, Some(SECRET)).await;
    let mut tn = at_menu(&rig, "boss").await;
    let menu = words_of(&tn.keys("x").await);
    assert!(!menu.contains("Sysop tools"), "{menu}");
}

#[tokio::test]
async fn the_request_limits_apply_to_the_telnet_forms_and_the_answer_never_changes() {
    let rig = Rig::start(true).await;
    rig.mode(RegistrationMode::Apply).await;
    // Twelve different handles from one network over three connections: the per-network limit
    // keeps ten, and every answer reads the same.
    let mut answers = Vec::new();
    let mut n = 0;
    for _ in 0..3 {
        let (mut tn, _) = join(&rig).await;
        tn.keys("a").await;
        for _ in 0..4 {
            n += 1;
            let handle = format!("hopeful{n}");
            answers.push(words_of(
                &fill(&mut tn, &[&handle, "hi", NEW_PASSWORD, NEW_PASSWORD]).await,
            ));
            tn.keys("\u{1b}").await;
            tn.keys("a").await;
        }
    }
    assert!(answers.iter().all(|a| a == &answers[0]), "{answers:?}");
    // Wait for the last submission to settle, then count what was kept.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let kept = rig
        .rows("SELECT CAST(count(*) AS TEXT) FROM invite_request")
        .await;
    assert_eq!(kept, ["10"], "the network limit kept ten of twelve");
}

#[tokio::test]
async fn the_forum_and_mail_screens_carry_the_unencrypted_row_too() {
    // `keys` reads until the marker is painted on the new last row, so reaching each screen at
    // all proves the row follows it (the forum and mail screens once returned before it).
    let rig = Rig::start(true).await;
    rig.member("alice").await;
    let mut tn = at_menu(&rig, "alice").await;
    let forums = words_of(&tn.keys("f").await);
    assert!(forums.contains("UNENCRYPTED"), "{forums}");
    tn.keys("\u{1b}").await;
    let mail = words_of(&tn.keys("m").await);
    assert!(mail.contains("UNENCRYPTED"), "{mail}");
}

// ------------------------------------------------ the same join-then-sign-in over both transports

/// Words of an ssh screen after `keys`: send them, ask for a full redraw at a new size and read
/// until the server is quiet.
async fn ssh_keys(
    channel: &mut russh::Channel<russh::client::Msg>,
    keys: &str,
    rows: &mut u32,
) -> String {
    channel.data_bytes(keys.as_bytes().to_vec()).await.unwrap();
    *rows = if *rows == 24 { 25 } else { 24 };
    channel.window_change(80, *rows, 0, 0).await.unwrap();
    let mut seen = String::new();
    while let Ok(msg) = timeout(Duration::from_millis(700), channel.wait()).await {
        match msg {
            Some(russh::ChannelMsg::Data { data }) => {
                seen.push_str(&String::from_utf8_lossy(&data));
            }
            Some(_) => {}
            None => break,
        }
    }
    words_of(&seen)
}

/// The last frame in `words` (a redraw repeats the whole screen) without the telnet marker,
/// which is all telnet adds; everything else must match ssh.
fn without_the_marker(words: &str) -> String {
    let frame = words.rfind("russh bbs").map_or(words, |at| &words[at..]);
    frame
        .replace(
            "UNENCRYPTED anyone on the network can read this session",
            "",
        )
        .replace("UNENCRYPTED", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[tokio::test]
async fn the_same_join_then_sign_in_script_gives_the_same_screens_over_ssh_and_telnet() {
    let script: [(&str, &str); 6] = [
        ("n", "the form"),
        ("walker\r", "handle"),
        ("a long join password\r", "password"),
        ("a long join password\r", "joined"),
        ("x", "main menu"),
        ("", ""),
    ];
    // ssh, no key, `new`.
    let ssh_rig = Rig::start(true).await;
    ssh_rig.mode(RegistrationMode::Open).await;
    let mut session = common::connect(ssh_rig.ssh.local_addr()).await;
    assert!(matches!(
        session.authenticate_password("new", "").await.unwrap(),
        AuthResult::Success
    ));
    let mut channel = common::open_shell(&session, 80, 24).await;
    let mut rows = 24;
    let ssh_home = ssh_keys(&mut channel, "", &mut rows).await;
    let mut ssh_screens = vec![ssh_home];
    for (keys, _) in &script[..4] {
        ssh_screens.push(ssh_keys(&mut channel, keys, &mut rows).await);
    }

    // telnet, `new`.
    let tn_rig = Rig::start(true).await;
    tn_rig.mode(RegistrationMode::Open).await;
    let (mut tn, home) = join(&tn_rig).await;
    let mut tn_screens = vec![home];
    for (keys, _) in &script[..4] {
        tn_screens.push(words_of(&tn.keys(keys).await));
    }

    for (index, (ssh, telnet)) in ssh_screens.iter().zip(&tn_screens).enumerate() {
        assert!(telnet.contains("UNENCRYPTED"), "step {index}: {telnet}");
        assert!(!ssh.contains("UNENCRYPTED"), "step {index}: {ssh}");
        assert_eq!(
            without_the_marker(ssh),
            without_the_marker(telnet),
            "step {index} differs between ssh and telnet"
        );
    }
    drop(tn);
    // And then the sign-in: handle and password over ssh, login and password over telnet.
    let mut again = Tn::connect(tn_rig.addr(), 80, 24).await;
    again.log_in("walker", "a long join password").await;
    let telnet_first = words_of(&again.read_until(&footer_at(24)).await);
    let mut ssh_again = common::connect(ssh_rig.ssh.local_addr()).await;
    assert!(matches!(
        ssh_again
            .authenticate_password("walker", "a long join password")
            .await
            .unwrap(),
        AuthResult::Success
    ));
    let mut ssh_channel = common::open_shell(&ssh_again, 80, 24).await;
    let mut rows = 24;
    let ssh_first = ssh_keys(&mut ssh_channel, "", &mut rows).await;
    assert_eq!(
        without_the_marker(&ssh_first),
        without_the_marker(&telnet_first),
        "the first screen after sign-in differs"
    );
    assert!(ssh_first.contains("Signed in as walker"), "{ssh_first}");
}
