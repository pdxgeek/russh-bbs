//! The chat screen's state machine: keys in, requests out, answers and room events back in.
//! It draws nothing and touches no store or hub; the server runs its requests and
//! `crate::chat` in the server crate draws its [`ChatPage`].
//!
//! Two pages: the **room list** (the rooms the member may join; staff also get "new room" and
//! "close room") and a **room** (transcript, who is here, a one-line input). Everything shown
//! from other people was sanitized before it got into an [`Event`]; everything else is a fixed
//! string from [`strings`].
//!
//! Typing: `/w name text` whispers, `/who` lists who is here, `/leave` or Esc goes back to the
//! list, `/help` lists these. Anything else is a public line. The line being typed lives only
//! in this struct, so a connection that ends mid-line sends nothing.

use std::collections::VecDeque;

use super::event::{Event, Line};
use super::room::Room;
use super::room::RoomId;
use super::state::Entered;
use super::text::{MAX_LINE_CHARS, clean_line};
use crate::access::Slug;
use crate::sanitize::{Sanitized, Username};
use crate::waiting_room::NoticeKind;

/// Fixed texts of the chat screen (chrome). Content never appears here.
pub mod strings {
    /// Heading of the room list.
    pub const HEADING_ROOMS: &str = "Chat rooms";
    /// Shown when no room is joinable.
    pub const NO_ROOMS: &str = "There are no rooms you can join.";
    /// Footer of the room list.
    pub const FOOTER_ROOMS: &str = "Up/Down choose, Enter join, Esc back";
    /// Extra footer words for staff.
    pub const FOOTER_STAFF: &str = "n new room, x close room";
    /// Footer of a room.
    pub const FOOTER_ROOM: &str = "Enter send, /w name text whisper, /who, /leave, /help";
    /// The new-room prompt.
    pub const PROMPT_NEW: &str = "New room name (a-z, 0-9, -), Enter to make it, Esc to cancel:";
    /// The close confirmation.
    pub const CONFIRM_CLOSE: &str =
        "Close this room and put everyone out? y to close, any other key cancels:";
    /// Label before the roster.
    pub const HERE: &str = "Here";
    /// Said to a member who came in.
    pub const YOU_ARE_IN: &str = "You are in";
    /// The command list, one row each.
    pub const HELP: [&str; 4] = [
        "/w name text  whisper to someone in this room",
        "/who          list who is here",
        "/leave        back to the room list (Esc does too)",
        "/help         this list",
    ];
    /// A whisper's label.
    pub const WHISPER_FROM: &str = "whispers";
    /// The echo of a whisper sent.
    pub const WHISPER_TO: &str = "you whisper to";
}

/// A key the chat screen understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKey {
    /// A printable character.
    Char(char),
    /// Enter.
    Enter,
    /// Backspace.
    Backspace,
    /// Esc.
    Escape,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
}

/// What the screen needs the server to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatRequest {
    /// Fetch the joinable rooms.
    List,
    /// Enter a room.
    Join(RoomId),
    /// Say a line in the current room.
    Say(Sanitized),
    /// Whisper to a handle in the current room.
    Whisper {
        /// The handle as typed.
        to: String,
        /// The text.
        text: Sanitized,
    },
    /// Who is in the current room.
    Who,
    /// Leave the current room and show the list.
    Leave,
    /// Open a room (staff).
    Create(Slug),
    /// Close a room (staff).
    Close(RoomId),
}

/// What a key did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEffect {
    /// Redraw.
    Stay,
    /// Leave the chat screen (back to the menu).
    Exit,
    /// Run this, then give the answer to [`ChatScreen::deliver`].
    Request(ChatRequest),
}

/// Why something did not happen, as the screen words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatNotice {
    /// No such room, or not one the member may join.
    NotFound,
    /// The permission is not held (any more).
    NotPermitted,
    /// The room is at its member cap.
    RoomFull,
    /// Chat is at its session cap.
    ChatFull,
    /// Too fast.
    SlowDown,
    /// The whisper target is not in this room.
    NotHere,
    /// The store or hub failed; nothing changed.
    Failed,
    /// A room name that is not valid.
    BadName,
    /// An open room has that name.
    NameTaken,
    /// The room cap is reached.
    TooManyRooms,
    /// A room was opened.
    Created,
    /// A room was closed.
    Closed,
}

impl ChatNotice {
    /// The words, and whether they are an error.
    #[must_use]
    pub const fn text(self) -> (NoticeKind, &'static str) {
        match self {
            Self::NotFound => (NoticeKind::Error, "No such room."),
            Self::NotPermitted => (NoticeKind::Error, "You may not do that."),
            Self::RoomFull => (NoticeKind::Error, "That room is full."),
            Self::ChatFull => (NoticeKind::Error, "Chat is full. Try again soon."),
            Self::SlowDown => (NoticeKind::Error, "Slow down. Nothing was sent."),
            Self::NotHere => (NoticeKind::Error, "Not here."),
            Self::Failed => (NoticeKind::Error, "That did not work. Nothing changed."),
            Self::BadName => (
                NoticeKind::Error,
                "Room names use a-z, 0-9 and -, up to 32.",
            ),
            Self::NameTaken => (NoticeKind::Error, "A room with that name is open."),
            Self::TooManyRooms => (NoticeKind::Error, "The board has the most rooms it allows."),
            Self::Created => (NoticeKind::Info, "Room opened."),
            Self::Closed => (NoticeKind::Info, "Room closed."),
        }
    }
}

/// The server's answer to a [`ChatRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatResponse {
    /// The joinable rooms (also the answer to a leave, to a staff action, and to a request
    /// made after the member lost the room), with a notice to show, if any.
    Rooms {
        /// The rooms.
        rooms: Vec<Room>,
        /// What to say about the request that led here.
        notice: Option<ChatNotice>,
    },
    /// The request worked and there is nothing to show beyond what the room sends.
    Done,
    /// The member is in the room.
    Joined {
        /// The room.
        room: Room,
        /// Who is here and what was said lately.
        entered: Entered,
    },
    /// Who is here.
    Roster(Vec<Username>),
    /// The request did not happen.
    Refused(ChatNotice),
}

/// One row of a room's transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A public line.
    Say(Line),
    /// Someone came in.
    Joined(Username),
    /// Someone left.
    Left(Username),
    /// A whisper to the member.
    Whisper {
        /// From.
        from: Username,
        /// Text.
        text: Sanitized,
    },
    /// A whisper the member sent.
    WhisperSent {
        /// To.
        to: Username,
        /// Text.
        text: Sanitized,
    },
    /// The member's queue overflowed: this many events were not shown.
    Missed(u32),
    /// Who is here.
    Here(Vec<Username>),
    /// The member came into this room: drawn as "You are in #name." (QA M3 L6).
    Entered(Slug),
    /// A fixed line from [`strings`].
    Info(&'static str),
}

/// Rows a room page keeps. Older ones scroll away for good.
pub const TRANSCRIPT_ROWS: usize = 200;
/// Characters the input line takes before it ignores more.
pub const INPUT_MAX_CHARS: usize = MAX_LINE_CHARS + 40;

/// A room page's contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomPage {
    /// The room.
    pub room: Room,
    /// What has happened, oldest first.
    pub rows: VecDeque<Row>,
    /// The line being typed.
    pub input: String,
    /// The rooms the member may join, as the list last showed them (the room list pane).
    pub rooms: Vec<Room>,
    /// Who is in the room now, in the order they came (the who's-online pane). Handles are
    /// validated usernames from the hub.
    pub roster: Vec<Username>,
}

/// The room list page's contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListPage {
    /// Joinable rooms.
    pub rooms: Vec<Room>,
    /// Selected row.
    pub cursor: usize,
    /// The new-room name being typed, while that prompt is open.
    pub naming: Option<String>,
    /// The room staff is being asked to confirm closing.
    pub closing: Option<RoomId>,
}

#[derive(Debug)]
enum Mode {
    List(ListPage),
    Room(RoomPage),
}

/// What to draw.
#[derive(Debug, Clone, Copy)]
pub enum ChatPage<'a> {
    /// The room list.
    List(&'a ListPage),
    /// A room.
    Room(&'a RoomPage),
}

/// The chat screen.
#[derive(Debug)]
pub struct ChatScreen {
    mode: Mode,
    /// Whether the member may open and close rooms (`chat.manage_rooms`). Set by the server
    /// from the snapshot loaded for each action.
    manage: bool,
    notice: Option<ChatNotice>,
}

impl ChatScreen {
    /// A screen showing an empty list; the server asks for the real one with
    /// [`ChatRequest::List`] straight away.
    #[must_use]
    pub fn open() -> Self {
        Self {
            mode: Mode::List(ListPage {
                rooms: Vec::new(),
                cursor: 0,
                naming: None,
                closing: None,
            }),
            manage: false,
            notice: None,
        }
    }

    /// Tell the screen whether the member may manage rooms right now.
    pub fn set_manage(&mut self, manage: bool) {
        self.manage = manage;
        if !manage && let Mode::List(list) = &mut self.mode {
            list.naming = None;
            list.closing = None;
        }
    }

    /// Whether the room-management keys are offered.
    #[must_use]
    pub const fn can_manage(&self) -> bool {
        self.manage
    }

    /// What to draw.
    #[must_use]
    pub fn page(&self) -> ChatPage<'_> {
        match &self.mode {
            Mode::List(list) => ChatPage::List(list),
            Mode::Room(room) => ChatPage::Room(room),
        }
    }

    /// The last notice, until the next key.
    #[must_use]
    pub const fn notice(&self) -> Option<ChatNotice> {
        self.notice
    }

    /// The room the screen shows, while it is in one.
    #[must_use]
    pub const fn room_id(&self) -> Option<RoomId> {
        match &self.mode {
            Mode::Room(page) => Some(page.room.id),
            Mode::List(_) => None,
        }
    }

    /// Whether the member is in a room.
    #[must_use]
    pub const fn in_room(&self) -> bool {
        matches!(self.mode, Mode::Room(_))
    }

    /// Handle one key.
    pub fn input(&mut self, key: ChatKey) -> ChatEffect {
        self.notice = None;
        match &mut self.mode {
            Mode::List(list) => list_input(list, key, self.manage),
            Mode::Room(room) => room_input(room, key),
        }
    }

    /// Show the answer to a request.
    pub fn deliver(&mut self, response: ChatResponse) {
        match response {
            ChatResponse::Done => {}
            ChatResponse::Rooms { rooms, notice } => {
                self.notice = notice;
                let (naming, closing, cursor) = match &self.mode {
                    Mode::List(old) => (None::<String>, None::<RoomId>, old.cursor),
                    Mode::Room(_) => (None, None, 0),
                };
                let cursor = cursor.min(rooms.len().saturating_sub(1));
                self.mode = Mode::List(ListPage {
                    rooms,
                    cursor,
                    naming,
                    closing,
                });
            }
            ChatResponse::Joined { room, entered } => {
                let mut rows = VecDeque::new();
                rows.extend(entered.history.into_iter().map(Row::Say));
                rows.push_back(Row::Entered(room.name.clone()));
                let roster = entered.roster.clone();
                rows.push_back(Row::Here(entered.roster));
                // The room list the member just chose from stays beside the conversation.
                let rooms = match &mut self.mode {
                    Mode::List(list) => std::mem::take(&mut list.rooms),
                    Mode::Room(page) => std::mem::take(&mut page.rooms),
                };
                self.mode = Mode::Room(RoomPage {
                    room,
                    rows,
                    input: String::new(),
                    rooms,
                    roster,
                });
            }
            ChatResponse::Roster(names) => {
                if let Mode::Room(room) = &mut self.mode {
                    room.roster.clone_from(&names);
                    push_row(room, Row::Here(names));
                }
            }
            ChatResponse::Refused(notice) => self.notice = Some(notice),
        }
    }

    /// Show what the room sent. Returns a request when the screen needs one (the room was
    /// closed, so the list must be fetched).
    pub fn receive(&mut self, events: Vec<Event>) -> Option<ChatRequest> {
        let Mode::Room(room) = &mut self.mode else {
            return None;
        };
        let mut closed = false;
        for event in events {
            let row = match event {
                Event::Line(line) => Row::Say(line),
                Event::Joined(who) => {
                    if !room.roster.contains(&who) {
                        room.roster.push(who.clone());
                    }
                    Row::Joined(who)
                }
                Event::Left(who) => {
                    room.roster.retain(|name| name != &who);
                    Row::Left(who)
                }
                Event::Whisper { from, text } => Row::Whisper { from, text },
                Event::WhisperSent { to, text } => Row::WhisperSent { to, text },
                Event::Missed(n) => Row::Missed(n),
                Event::RoomClosed => {
                    closed = true;
                    continue;
                }
            };
            push_row(room, row);
        }
        closed.then_some(ChatRequest::List)
    }
}

fn push_row(room: &mut RoomPage, row: Row) {
    room.rows.push_back(row);
    while room.rows.len() > TRANSCRIPT_ROWS {
        room.rows.pop_front();
    }
}

fn list_input(list: &mut ListPage, key: ChatKey, manage: bool) -> ChatEffect {
    if let Some(name) = &mut list.naming {
        return match key {
            ChatKey::Char(c) if name.chars().count() < 32 => {
                name.push(c.to_ascii_lowercase());
                ChatEffect::Stay
            }
            ChatKey::Backspace => {
                name.pop();
                ChatEffect::Stay
            }
            ChatKey::Enter => {
                let typed = std::mem::take(name);
                list.naming = None;
                match Slug::parse(&typed) {
                    Ok(slug) => ChatEffect::Request(ChatRequest::Create(slug)),
                    Err(_) => ChatEffect::Request(ChatRequest::List),
                }
            }
            ChatKey::Escape => {
                list.naming = None;
                ChatEffect::Stay
            }
            ChatKey::Char(_) | ChatKey::Up | ChatKey::Down => ChatEffect::Stay,
        };
    }
    if let Some(id) = list.closing.take() {
        return match key {
            ChatKey::Char('y' | 'Y') => ChatEffect::Request(ChatRequest::Close(id)),
            _ => ChatEffect::Stay,
        };
    }
    match key {
        ChatKey::Up => {
            list.cursor = list.cursor.saturating_sub(1);
            ChatEffect::Stay
        }
        ChatKey::Down => {
            list.cursor = (list.cursor + 1).min(list.rooms.len().saturating_sub(1));
            ChatEffect::Stay
        }
        ChatKey::Enter => match list.rooms.get(list.cursor) {
            Some(room) => ChatEffect::Request(ChatRequest::Join(room.id)),
            None => ChatEffect::Stay,
        },
        ChatKey::Escape => ChatEffect::Exit,
        ChatKey::Char('n' | 'N') if manage => {
            list.naming = Some(String::new());
            ChatEffect::Stay
        }
        ChatKey::Char('x' | 'X') if manage => {
            list.closing = list.rooms.get(list.cursor).map(|room| room.id);
            ChatEffect::Stay
        }
        ChatKey::Char(_) | ChatKey::Backspace => ChatEffect::Stay,
    }
}

fn room_input(room: &mut RoomPage, key: ChatKey) -> ChatEffect {
    match key {
        ChatKey::Char(c) => {
            if room.input.chars().count() < INPUT_MAX_CHARS {
                room.input.push(c);
            }
            ChatEffect::Stay
        }
        ChatKey::Backspace => {
            room.input.pop();
            ChatEffect::Stay
        }
        ChatKey::Escape => ChatEffect::Request(ChatRequest::Leave),
        ChatKey::Up | ChatKey::Down => ChatEffect::Stay,
        ChatKey::Enter => {
            let typed = std::mem::take(&mut room.input);
            submit(room, &typed)
        }
    }
}

fn submit(room: &mut RoomPage, typed: &str) -> ChatEffect {
    let typed = typed.trim();
    if typed.is_empty() {
        return ChatEffect::Stay;
    }
    let Some(command) = typed.strip_prefix('/') else {
        return clean_line(typed).map_or(ChatEffect::Stay, |text| {
            ChatEffect::Request(ChatRequest::Say(text))
        });
    };
    let (word, rest) = command
        .split_once(char::is_whitespace)
        .unwrap_or((command, ""));
    match word.to_ascii_lowercase().as_str() {
        "w" | "whisper" => {
            let rest = rest.trim_start();
            let (to, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if let (false, Some(text)) = (to.is_empty(), clean_line(text)) {
                ChatEffect::Request(ChatRequest::Whisper {
                    to: to.to_owned(),
                    text,
                })
            } else {
                push_row(room, Row::Info("Usage: /w name text"));
                ChatEffect::Stay
            }
        }
        "who" => ChatEffect::Request(ChatRequest::Who),
        "leave" | "q" => ChatEffect::Request(ChatRequest::Leave),
        "help" | "?" => {
            for line in strings::HELP {
                push_row(room, Row::Info(line));
            }
            ChatEffect::Stay
        }
        _ => {
            push_row(room, Row::Info("Unknown command. /help lists them."));
            ChatEffect::Stay
        }
    }
}
