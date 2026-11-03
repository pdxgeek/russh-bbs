//! **TEST-ONLY.** A throwaway SSH server that puts the M3-B message editor on a real PTY so it
//! can be driven with an ordinary `ssh` client (and `pexpect` + `pyte`, as in `docs/qa/m2.md`),
//! before any feature screen calls the editor. It is not part of the board:
//!
//! * it is an *example*, so `cargo build` and the Docker image never contain it;
//! * it accepts **every** public key, so it must only ever listen on loopback (it refuses any
//!   other address);
//! * it has no waiting room, access check, limits or audit: one built-in account, one scenario
//!   per SSH user name, and a store of its own.
//!
//! ```text
//! cargo run -p russh-bbs-server --example editor_demo -- --listen 127.0.0.1:2290
//! ssh -tt -p 2290 -l reply   127.0.0.1   # context "demo:reply", starts with a quote
//! ssh -tt -p 2290 -l plain   127.0.0.1   # no context key: no drafts
//! ssh -tt -p 2290 -l tiny    127.0.0.1   # 120 characters, 4 lines
//! ssh -tt -p 2290 -l <other> 127.0.0.1   # context "demo:<other>", empty
//! ```
//!
//! The editor runs in the alternate screen; when it finishes, the result is printed on the main
//! screen as `RESULT: submitted|cancelled|saved-draft|logged-off` followed by the text, so a
//! script can read it. A submitted text is discarded from the draft store afterwards, the way a
//! real caller does after its own write succeeded.

// A test tool: printing its listen address is its output.
#![allow(clippy::print_stdout)]

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{
    Auth, Config, Handler, Msg, PublicKeyOffer, Server, Session, VerifiedPublicKey,
};
use russh::{Channel, ChannelId, ChannelOpenFailure, MethodKind, MethodSet, Pty, SshId};
use russh_bbs_core::access::Slug;
use russh_bbs_core::drafts::{BodyLimits, ContextKey, QuoteLimits, discard_draft, quote};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::sanitize::{MultiLine, SingleLine, sanitize_username};
use russh_bbs_core::store::{
    AccountId, AccountState, AccountStore, NewAccount, StoreError, UnixSeconds,
};
use russh_bbs_core::terminal_settings::TerminalSettings;
use russh_bbs_server::ssh::terminal::{ESC_TIMEOUT, KeyParser, PtySize};
use russh_bbs_server::ui::Ui;
use russh_bbs_server::ui::editor::{Editor, EditorEffect, EditorRequest, EditorView, Outcome};
use russh_bbs_store::SqliteStore;
use tokio::sync::Mutex;

struct Shared {
    store: SqliteStore,
    account: AccountId,
    ui: Ui,
}

struct Run {
    parser: KeyParser,
    editor: Option<Editor>,
    size: PtySize,
    scenario: String,
}

struct Demo {
    shared: Arc<Shared>,
    run: Arc<Mutex<Run>>,
    channel: Option<ChannelId>,
    user: String,
}

struct DemoServer(Arc<Shared>);

impl Server for DemoServer {
    type Handler = Demo;

    fn new_client(&mut self, _peer: Option<SocketAddr>) -> Demo {
        Demo {
            shared: Arc::clone(&self.0),
            run: Arc::new(Mutex::new(Run {
                parser: KeyParser::new(),
                editor: None,
                size: PtySize::from_request(80, 24),
                scenario: String::new(),
            })),
            channel: None,
            user: String::new(),
        }
    }
}

impl Handler for Demo {
    type Error = russh::Error;

    // The trait method is async; this test tool has nothing to await.
    #[allow(clippy::unused_async_trait_impl)]
    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        _offer: PublicKeyOffer<'_>,
    ) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }

    #[allow(clippy::unused_async_trait_impl)]
    async fn auth_publickey(
        &mut self,
        user: &str,
        _key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        user.clone_into(&mut self.user);
        Ok(Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.channel.is_some() {
            reply.reject(ChannelOpenFailure::ResourceShortage).await;
            return Ok(());
        }
        self.channel = Some(channel.id());
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.run.lock().await.size = PtySize::from_request(col_width, row_height);
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        let (editor, scenario) = open_editor(&self.shared, &self.user).await;
        let mut run = self.run.lock().await;
        run.scenario = scenario;
        run.editor = Some(editor);
        let mut bytes = b"\x1b[?1049h\x1b[?25l\x1b[2J".to_vec();
        bytes.extend(paint(&self.shared, &run));
        drop(run);
        session.handle().data(channel, bytes).await.ok();
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let mut run = self.run.lock().await;
        run.size = PtySize::from_request(col_width, row_height);
        let mut bytes = b"\x1b[2J".to_vec();
        bytes.extend(paint(&self.shared, &run));
        drop(run);
        session.handle().data(channel, bytes).await.ok();
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let keys = self
            .run
            .lock()
            .await
            .parser
            .feed(data, std::time::Instant::now());
        pump(&self.shared, &self.run, session.handle(), channel, keys).await;
        // A lone Esc becomes the Escape key after a short wait with no more input.
        let (shared, run, handle) = (
            Arc::clone(&self.shared),
            Arc::clone(&self.run),
            session.handle(),
        );
        tokio::spawn(async move {
            tokio::time::sleep(ESC_TIMEOUT + Duration::from_millis(50)).await;
            let keys = {
                let mut guard = run.lock().await;
                let due = guard
                    .parser
                    .deadline()
                    .is_some_and(|at| at <= std::time::Instant::now());
                if due {
                    guard.parser.expire()
                } else {
                    Vec::new()
                }
            };
            pump(&shared, &run, handle, channel, keys).await;
        });
        Ok(())
    }
}

/// The editor for the scenario the SSH user name picks.
async fn open_editor(shared: &Shared, user: &str) -> (Editor, String) {
    let title = SingleLine::new(40).sanitize(&format!("Demo: {user}"));
    let mut request = EditorRequest::new(title);
    let scenario = user.to_owned();
    match user {
        "plain" => {}
        "tiny" => {
            request = request
                .context(ContextKey::parse("demo:tiny").unwrap_or_else(|_| unreachable_key()))
                .limits(BodyLimits::new(120, 4));
        }
        "reply" => {
            let who = SingleLine::new(40).sanitize("bob wrote:");
            let post = MultiLine::new(2_000).sanitize("I think the new board is great.\n\nThe editor should keep a long thought like this one safe.");
            request = request
                .context(ContextKey::parse("demo:reply").unwrap_or_else(|_| unreachable_key()))
                .initial(quote(Some(&who), &post, QuoteLimits::default()));
        }
        other => {
            let key = format!(
                "demo:{}",
                other
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .take(40)
                    .collect::<String>()
            );
            request =
                request.context(ContextKey::parse(&key).unwrap_or_else(|_| unreachable_key()));
        }
    }
    match Editor::open_with_store(&shared.store, shared.account, request.clone()).await {
        Ok(editor) => (editor, scenario),
        // A saved draft that no longer fits (the demo's own limits changed): start clean.
        Err(_) => (
            Editor::open(request, None).unwrap_or_else(|_| unreachable_editor()),
            scenario,
        ),
    }
}

fn unreachable_key() -> ContextKey {
    std::process::exit(2)
}

fn unreachable_editor() -> Editor {
    std::process::exit(2)
}

/// Feed `keys` to the editor and send what changed; finish the session on a result.
async fn pump(
    shared: &Shared,
    run: &Mutex<Run>,
    handle: russh::server::Handle,
    channel: ChannelId,
    keys: Vec<russh_bbs_server::ssh::terminal::Key>,
) {
    if keys.is_empty() {
        return;
    }
    let mut guard = run.lock().await;
    let mut result: Option<String> = None;
    for key in keys {
        let Some(editor) = guard.editor.as_mut() else {
            return;
        };
        let effect = editor.input(key);
        match effect {
            EditorEffect::Stay => {}
            EditorEffect::Done(Outcome::Submitted(body)) => {
                let text = body.trimmed().to_owned();
                if let Some(context) = editor.context().cloned() {
                    // The "write" succeeded; now the draft is no longer needed.
                    let _ = discard_draft(&shared.store, shared.account, &context).await;
                }
                result = Some(format!("RESULT: submitted\r\n{}", crlf(&text)));
            }
            EditorEffect::Done(Outcome::Cancelled) => result = Some("RESULT: cancelled".into()),
            EditorEffect::Done(Outcome::SavedDraft) => result = Some("RESULT: saved-draft".into()),
            EditorEffect::SaveDraft(save) => {
                let saved = save.persist(&shared.store, shared.account, now()).await;
                if let EditorEffect::Done(Outcome::SavedDraft) = editor.draft_saved(saved) {
                    result = Some(format!("RESULT: saved-draft\r\n{}", crlf(save.text())));
                }
            }
            EditorEffect::LogOff(save) => {
                let note = match save {
                    Some(save) => match save.persist(&shared.store, shared.account, now()).await {
                        Ok(_) => "draft saved",
                        Err(_) => "draft not saved",
                    },
                    None => "no draft",
                };
                result = Some(format!("RESULT: logged-off ({note})"));
            }
        }
        if result.is_some() {
            break;
        }
    }
    let bytes = match result {
        Some(text) => {
            guard.editor = None;
            let mut bytes = b"\x1b[?25h\x1b[?1049l".to_vec();
            bytes.extend(format!("{text}\r\n").into_bytes());
            bytes
        }
        None => paint(shared, &guard),
    };
    drop(guard);
    let finished = bytes.starts_with(b"\x1b[?25h");
    handle.data(channel, bytes).await.ok();
    if finished {
        let _ = handle.eof(channel).await;
        let _ = handle.close(channel).await;
    }
}

fn crlf(text: &str) -> String {
    text.replace('\n', "\r\n")
}

fn now() -> UnixSeconds {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    UnixSeconds::from_raw(i64::try_from(secs).unwrap_or(0))
}

/// Draw the editor into a buffer and turn the whole buffer into terminal bytes (full repaint).
fn paint(shared: &Shared, run: &Run) -> Vec<u8> {
    let Some(editor) = run.editor.as_ref() else {
        return Vec::new();
    };
    let (cols, rows) = (run.size.cols(), run.size.rows());
    // The test backend cannot fail, so the error type is uninhabited.
    let Ok(mut terminal) = Terminal::new(TestBackend::new(cols, rows));
    let settings = TerminalSettings::default()
        .with_color(true)
        .with_tier(Tier::Unicode);
    let markers = [SecurityMarker::ServerReadable];
    let drawn = terminal.draw(|frame| {
        editor.render(
            frame,
            &EditorView {
                ui: &shared.ui,
                settings: &settings,
                markers: &markers,
            },
        );
    });
    if drawn.is_err() {
        return Vec::new();
    }
    serialize(terminal.backend().buffer())
}

fn serialize(buffer: &Buffer) -> Vec<u8> {
    let mut out = String::new();
    for y in 0..buffer.area.height {
        let _ = write!(out, "\x1b[{};1H", y + 1);
        let mut skip = 0usize;
        for x in 0..buffer.area.width {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let cell = &buffer[(x, y)];
            out.push_str("\x1b[0m");
            let mut sgr = Vec::new();
            for (modifier, code) in [
                (Modifier::BOLD, "1"),
                (Modifier::DIM, "2"),
                (Modifier::UNDERLINED, "4"),
                (Modifier::REVERSED, "7"),
            ] {
                if cell.modifier.contains(modifier) {
                    sgr.push(code);
                }
            }
            match cell.fg {
                Color::Red => sgr.push("31"),
                Color::Green => sgr.push("32"),
                Color::Yellow => sgr.push("33"),
                Color::Cyan => sgr.push("36"),
                _ => {}
            }
            if !sgr.is_empty() {
                let _ = write!(out, "\x1b[{}m", sgr.join(";"));
            }
            let symbol = cell.symbol();
            out.push_str(if symbol.is_empty() { " " } else { symbol });
            // A wide character covers the next cell too.
            skip = symbol_width(symbol).saturating_sub(1);
        }
        out.push_str("\x1b[0m");
    }
    out.into_bytes()
}

fn symbol_width(symbol: &str) -> usize {
    ratatui::text::Line::raw(symbol.to_owned()).width()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut listen: SocketAddr = "127.0.0.1:2290".parse()?;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => listen = args.next().ok_or("--listen needs an address")?.parse()?,
            other => return Err(format!("unknown argument {other}").into()),
        }
    }
    if !listen.ip().is_loopback() {
        return Err("this demo accepts every key; it only listens on loopback".into());
    }
    let dir = tempfile::tempdir()?;
    let store = SqliteStore::open(&dir.path().join("demo.db")).await?;
    let account = match store
        .create_account(NewAccount {
            username: sanitize_username("tester").map_err(|e| format!("{e:?}"))?,
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
    {
        Ok(account) => account.id,
        Err(StoreError::UsernameTaken) => AccountId::from_raw(1),
        Err(error) => return Err(error.into()),
    };
    let shared = Arc::new(Shared {
        store,
        account,
        ui: Ui::standard()?,
    });
    let config = Config {
        server_id: SshId::Standard("SSH-2.0-editor-demo".into()),
        methods: MethodSet::from(&[MethodKind::PublicKey][..]),
        keys: vec![PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?],
        auth_rejection_time: Duration::from_millis(10),
        auth_rejection_time_initial: Some(Duration::ZERO),
        inactivity_timeout: Some(Duration::from_secs(300)),
        nodelay: true,
        ..Config::default()
    };
    println!("editor_demo listening on {listen} (test-only: every key is accepted)");
    DemoServer(shared)
        .run_on_address(Arc::new(config), listen)
        .await?;
    drop(dir);
    Ok(())
}
