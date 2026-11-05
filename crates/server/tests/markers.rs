//! FIX-M35-1 (QA M3.5 M1, D39) over a real SSH client: a staff session of a board that has
//! telnet on (`low_security`) shows the `LOW-SECURITY` marker in the first cell of the last row,
//! in the reserved style, on every screen the menu reaches: forums, a thread's reader, the
//! editor, mail, chat, members, community, invites, settings, sysop tools, the door list and a
//! running door of each of the six games. The reserved style (bold and reverse together) is on no
//! other row. A plain member never sees it. The raw bytes are replayed onto a terminal grid, so
//! this reads what a person's terminal shows. (The grid is the same as the one in
//! `ssh/screen/marker_tests.rs`, which walks every tier, layout and size over both transports.)
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unused_async_trait_impl,
    missing_docs
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{fast_limits, new_client_key};
use russh::Channel;
use russh::ChannelMsg;
use russh::client::Msg;
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh_bbs_core::access::{Fingerprint, Slug};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::passwords::Backoff;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountState, AccountStore, NewAccount};
use russh_bbs_core::terminal_settings::{TerminalSettings, TerminalSettingsStore};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::doors::Doors;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_server::ui::Ui;
use russh_bbs_store::SqliteStore;
use tempfile::TempDir;
use tokio::time::timeout;

/// One cell of the terminal the bytes were replayed onto.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    ch: char,
    bold: bool,
    reverse: bool,
}

const BLANK: Cell = Cell {
    ch: ' ',
    bold: false,
    reverse: false,
};

/// A terminal that understands what ratatui's crossterm backend and the telnet footer send:
/// absolute moves, forward moves, erase, and the bold and reverse attributes. Everything else
/// (colours, modes) is skipped.
struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
    row: usize,
    col: usize,
    bold: bool,
    reverse: bool,
}

/// Terminal columns taken by `c`: two for the wide ranges the games and the emoji use.
fn width_of(c: char) -> usize {
    match u32::from(c) {
        0x1100..=0x115F
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

impl Grid {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![BLANK; cols * rows],
            row: 0,
            col: 0,
            bold: false,
            reverse: false,
        }
    }

    fn at(&self, row: usize, col: usize) -> Cell {
        self.cells[row * self.cols + col]
    }

    fn put(&mut self, c: char) {
        if self.row < self.rows && self.col < self.cols {
            self.cells[self.row * self.cols + self.col] = Cell {
                ch: c,
                bold: self.bold,
                reverse: self.reverse,
            };
        }
        self.col += width_of(c);
    }

    fn sgr(&mut self, params: &str) {
        let numbers: Vec<u32> = params.split(';').map(|p| p.parse().unwrap_or(0)).collect();
        let mut at = 0;
        while at < numbers.len() {
            match numbers[at] {
                0 => {
                    self.bold = false;
                    self.reverse = false;
                }
                1 => self.bold = true,
                7 => self.reverse = true,
                22 => self.bold = false,
                27 => self.reverse = false,
                // An extended colour: the numbers after it are not attributes.
                38 | 48 => {
                    at += match numbers.get(at + 1) {
                        Some(5) => 2,
                        Some(2) => 4,
                        _ => 0,
                    };
                }
                _ => {}
            }
            at += 1;
        }
    }

    fn erase(&mut self, params: &str, kind: char) {
        let mode = params.parse::<u32>().unwrap_or(0);
        let range = match (kind, mode) {
            ('J', 2 | 3) => 0..self.cells.len(),
            ('J', 0) => (self.row * self.cols + self.col).min(self.cells.len())..self.cells.len(),
            ('K', 2) => self.row * self.cols..(self.row + 1) * self.cols,
            ('K', 0) => {
                (self.row * self.cols + self.col).min(self.cells.len())
                    ..((self.row + 1) * self.cols).min(self.cells.len())
            }
            _ => 0..0,
        };
        for cell in &mut self.cells[range] {
            *cell = BLANK;
        }
    }

    /// Replay `bytes` onto the grid.
    fn feed(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\u{1b}' if chars.peek() == Some(&'[') => {
                    chars.next();
                    let mut params = String::new();
                    for next in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&next) {
                            self.control(next, &params);
                            break;
                        }
                        params.push(next);
                    }
                }
                '\u{1b}' => {
                    chars.next();
                }
                '\r' => self.col = 0,
                c if c.is_control() => {}
                c => self.put(c),
            }
        }
    }

    fn control(&mut self, kind: char, params: &str) {
        let first = |default: usize| {
            params
                .split(';')
                .next()
                .and_then(|p| p.parse::<usize>().ok())
                .unwrap_or(default)
        };
        match kind {
            'H' | 'f' => {
                let mut parts = params.split(';');
                let row = parts.next().and_then(|p| p.parse::<usize>().ok());
                let col = parts.next().and_then(|p| p.parse::<usize>().ok());
                self.row = row.unwrap_or(1).saturating_sub(1);
                self.col = col.unwrap_or(1).saturating_sub(1);
            }
            'C' => self.col += first(1),
            'D' => self.col = self.col.saturating_sub(first(1)),
            'A' => self.row = self.row.saturating_sub(first(1)),
            'B' => self.row += first(1),
            'G' => self.col = first(1).saturating_sub(1),
            'J' | 'K' => self.erase(params, kind),
            'm' => self.sgr(params),
            _ => {}
        }
    }

    /// The text of a row, trimmed.
    fn row_text(&self, row: usize) -> String {
        (0..self.cols)
            .map(|col| self.at(row, col).ch)
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    /// Every cell in the reserved marker style (bold and reverse together), as (row, col).
    fn reserved(&self) -> Vec<(usize, usize)> {
        let mut found = Vec::new();
        for row in 0..self.rows {
            for col in 0..self.cols {
                let cell = self.at(row, col);
                if cell.bold && cell.reverse && cell.ch != ' ' {
                    found.push((row, col));
                }
            }
        }
        found
    }

    fn dump(&self) -> String {
        (0..self.rows)
            .map(|row| self.row_text(row))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

struct Rig {
    ssh: SshServer,
    store: SqliteStore,
    _dir: TempDir,
}

impl Rig {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
        let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
        let doors = Doors::builtin().unwrap();
        let ui = Arc::new(Ui::standard().unwrap().with_doors(doors));
        let loose = RatePolicy::new(1_000, Duration::from_millis(1));
        // The board has telnet on: its ssh sessions carry the low-security mode.
        let limits = Limits {
            ip_rate: loose,
            key_rate: loose,
            password_handle_backoff: Backoff::UNLIMITED,
            password_network_backoff: Backoff::UNLIMITED,
            low_security: true,
            ..fast_limits()
        };
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
            ui,
        )
        .await
        .unwrap();
        Self {
            ssh,
            store,
            _dir: dir,
        }
    }

    async fn account(&self, name: &str, role: Slug) -> PrivateKey {
        let key = new_client_key();
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
        common::remember(fingerprint, name);
        let settings = TerminalSettings::new(None, None, true, true, Some(Tier::Ascii)).unwrap();
        self.store
            .set_terminal_settings(account.id, &settings)
            .await
            .unwrap();
        key
    }
}

/// One person's terminal: the channel, the grid the bytes were replayed onto, and the window.
struct Term {
    channel: Channel<Msg>,
    grid: Grid,
    cols: u32,
    rows: u32,
    /// The reserved cells counted on the raw bytes of every frame, for the report.
    seen: Vec<(String, usize)>,
    _session: russh::client::Handle<common::Client>,
}

impl Term {
    async fn open(rig: &Rig, key: &PrivateKey, cols: u32, rows: u32) -> Self {
        let mut session = common::connect(rig.ssh.local_addr()).await;
        let result = session
            .authenticate_publickey(
                &common::user_for(key, "anyone"),
                PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
            )
            .await
            .unwrap();
        assert!(matches!(result, russh::client::AuthResult::Success));
        let channel = common::open_shell(&session, cols, rows).await;
        let mut term = Self {
            channel,
            grid: Grid::new(cols as usize, rows as usize),
            cols,
            rows,
            seen: Vec::new(),
            _session: session,
        };
        term.drain().await;
        term
    }

    async fn drain(&mut self) {
        while let Ok(msg) = timeout(Duration::from_millis(120), self.channel.wait()).await {
            match msg {
                Some(ChannelMsg::Data { data }) => self.grid.feed(&data),
                Some(_) => {}
                None => break,
            }
        }
    }

    /// Send keys, let the screen settle, then ask for a full repaint and read it.
    async fn press(&mut self, keys: &str) -> String {
        self.channel
            .data_bytes(keys.as_bytes().to_vec())
            .await
            .unwrap();
        self.drain().await;
        self.channel
            .window_change(self.cols, self.rows, 0, 0)
            .await
            .unwrap();
        self.drain().await;
        self.grid.dump()
    }

    async fn line(&mut self, text: &str) -> String {
        self.press(&format!("{text}\r")).await
    }

    /// What this screen must show: in a staff session the marker in the first cell of the last
    /// row and the reserved style on no other row; in any other session no reserved cell at all.
    fn check(&mut self, staff: bool, screen: &str) {
        let last = self.rows as usize - 1;
        let reserved = self.grid.reserved();
        self.seen.push((screen.to_owned(), reserved.len()));
        let at = format!("{}x{} {screen}", self.cols, self.rows);
        if staff {
            let first = self.grid.at(last, 0);
            assert!(
                first.bold && first.reverse && first.ch != ' ',
                "{at}: no marker in the first cell of the last row\n{}",
                self.grid.dump()
            );
            assert!(
                self.grid.row_text(last).starts_with("LOW-SECURITY"),
                "{at}: {}\n{}",
                self.grid.row_text(last),
                self.grid.dump()
            );
            assert!(
                reserved.iter().all(|(row, col)| *row == last && *col < 20),
                "{at}: the reserved style outside the marker: {reserved:?}\n{}",
                self.grid.dump()
            );
        } else {
            assert!(
                reserved.is_empty(),
                "{at}: a plain member sees the reserved style: {reserved:?}\n{}",
                self.grid.dump()
            );
        }
    }
}

const SEND: &str = "\u{18}";
const ESC: &str = "\u{1b}";

/// Everything the menu reaches except the doors, with the key that gets there.
async fn walk_screens(term: &mut Term, staff: bool, post: bool) {
    let menu = term.press("x").await;
    assert!(menu.contains("Main menu"), "{menu}");
    term.check(staff, "menu");

    if post {
        // A board and a thread, so the lists and the reader have something to show.
        term.press("f").await;
        term.check(staff, "forums, no boards");
        term.press("n").await;
        term.line("General").await;
        term.line("Talk about anything").await;
        term.press("n").await;
        term.press("1").await;
        term.check(staff, "forums, threads");
        term.press("w").await;
        term.line("Welcome").await;
        term.check(staff, "forums, editor");
        term.press("Hello everyone.\rSecond line.").await;
        let reader = term.press(SEND).await;
        assert!(reader.contains("Post 1 of 1"), "{reader}");
        term.check(staff, "forums, reader");
        term.press(ESC).await;
        term.press(ESC).await;
        term.check(staff, "forums, boards");
        term.press(ESC).await;
        let menu = term.press(ESC).await;
        assert!(menu.contains("Main menu"), "{menu}");
    }

    term.press("m").await;
    term.check(staff, "mail, inbox");
    term.press("c").await;
    term.check(staff, "mail, compose");
    term.press(ESC).await;
    term.press(ESC).await;
    term.press(ESC).await;

    let chat = term.press("c").await;
    assert!(chat.contains("Chat rooms"), "{chat}");
    term.check(staff, "chat, rooms");
    let room = term.press("\r").await;
    assert!(room.contains("#lobby"), "{room}");
    term.check(staff, "chat, room");
    term.press(ESC).await;
    term.press(ESC).await;

    for (key, name) in [
        ("u", "members"),
        ("y", "community"),
        ("i", "invites"),
        ("s", "settings"),
        ("o", "sysop tools"),
        ("d", "door list"),
    ] {
        term.press(key).await;
        term.check(staff, name);
        term.press(ESC).await;
        let menu = term.press(ESC).await;
        assert!(
            menu.contains("Main menu") || menu.contains("Doors"),
            "{name}: {menu}"
        );
        // Back at the menu whatever the screen was.
        if !menu.contains("Main menu") {
            term.press(ESC).await;
        }
    }
}

/// A running door of each of the six games, each in a session of its own.
async fn walk_doors(rig: &Rig, key: &PrivateKey, staff: bool, cols: u32, rows: u32) {
    for door in 1..=6 {
        let mut term = Term::open(rig, key, cols, rows).await;
        term.press("x").await;
        term.press("d").await;
        let playing = term.press(&door.to_string()).await;
        assert!(
            !playing.contains("Pick a door"),
            "door {door} did not open:\n{playing}"
        );
        term.check(staff, &format!("door {door}"));
        // A key inside the door redraws it; the marker holds.
        term.press("1").await;
        term.check(staff, &format!("door {door} after a key"));
    }
}

#[tokio::test]
async fn a_staff_session_over_ssh_shows_the_marker_on_every_screen() {
    let rig = Rig::start().await;
    let key = rig.account("opal", Slug::SYSOP).await;
    for (cols, rows) in [(80, 24), (132, 43)] {
        let mut term = Term::open(&rig, &key, cols, rows).await;
        // The board and thread are made once, at the first size.
        walk_screens(&mut term, true, cols == 80).await;
        for (screen, count) in &term.seen {
            eprintln!("{cols}x{rows} {screen}: {count} reserved cells");
        }
        walk_doors(&rig, &key, true, cols, rows).await;
    }
}

#[tokio::test]
async fn a_plain_member_over_ssh_never_sees_the_marker() {
    let rig = Rig::start().await;
    let key = rig.account("alice", Slug::MEMBER).await;
    let mut term = Term::open(&rig, &key, 80, 24).await;
    walk_screens(&mut term, false, false).await;
    walk_doors(&rig, &key, false, 80, 24).await;
}
