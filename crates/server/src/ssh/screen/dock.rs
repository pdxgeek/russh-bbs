//! The dock on a session's screen (UI-W): live chat in a second pane beside any framed screen.
//!
//! The geometry, the bounds and the frame hook are `ui::dock`; the chat is the ordinary
//! `ChatScreen` and the shell's ordinary chat session, so joining, the permission checks on
//! every request, the bounded queues and the push path are the ones the full-screen chat uses.
//! What this module adds is who has the keys and what the dock keys do:
//!
//! | Key | Does |
//! |---|---|
//! | `Ctrl-T` | open chat beside the screen (the pane has the keys); close it again |
//! | `Ctrl-W` | move the keys between the screen and the pane |
//! | `<` `>` / `{` `}` | widen or narrow the pane by 1 or 8 columns (the screen has the keys and no text field is open) |
//! | mouse | drag the divider (the same resize); click the side without the keys to move them |
//!
//! Rules: the dock is offered only to a member who holds `chat.join`; a screen the frame does
//! not carve (the editor, the settings screens, doors) closes it rather than hide a joined
//! chat; the pane and divider live inside the body, so the status bar and its marker cell are
//! never covered (`ui::dock` tests); an unfocused pane shows no blinking cursor.

use ratatui::Frame;
use ratatui::layout::Rect;
use russh_bbs_core::access::{Permission, PermissionCheck};
use russh_bbs_core::chat::{ChatRequest, ChatScreen};

use super::{Content, KeyEffects, Screen};
use crate::chat::{self, ChatCtx};
use crate::ssh::terminal::Key;
use crate::ui::dock::{self, Geometry};
use crate::ui::input::{self, DockHit};
use crate::ui::layout;
use crate::ui::panes;
use crate::ui::toast::{Kind as ToastKind, Text as ToastText};
use russh_bbs_core::terminal_settings::LayoutId;

/// The dock's state. The chat itself is `Screen::chat`, which is `Some` exactly while `open`.
#[derive(Debug, Default)]
pub(super) struct DockState {
    /// Chat is beside the screen.
    pub(super) open: bool,
    /// The pane has the keys.
    pub(super) chat_focus: bool,
    /// The pane width the member chose; `None` until they move the divider.
    pub(super) width: Option<u16>,
}

/// What the pane needs to draw, built where the frame is drawn.
pub(super) struct Pane<'a> {
    pub(super) chat: &'a ChatScreen,
    pub(super) ctx: ChatCtx<'a>,
    pub(super) focused: bool,
}

impl Pane<'_> {
    /// Draw the pane and the divider into what the frame reserved, and record where they are
    /// for the mouse. Nothing is drawn when no dock was reserved (a screen the frame does not
    /// carve, or a body too narrow).
    pub(super) fn draw(&self, frame: &mut Frame<'_>, reserved: Option<Geometry>) {
        let Some(geometry) = reserved else {
            return;
        };
        chat::render_pane(frame, geometry.pane, self.chat, &self.ctx, self.focused);
        panes::draw_divider(frame, geometry.divider, self.ctx.look);
        let body = Rect {
            x: geometry.main.x,
            y: geometry.main.y,
            width: geometry.main.width + geometry.divider.width + geometry.pane.width,
            height: geometry.main.height,
        };
        input::dock(DockHit {
            body,
            main: geometry.main,
            divider: geometry.divider,
            pane: geometry.pane,
            chat_focused: self.focused,
        });
    }
}

impl Screen {
    /// The chat is the whole screen (opened from the menu), not a pane.
    pub(super) fn chat_fullscreen(&self) -> bool {
        self.chat.is_some() && !self.dock.open
    }

    /// Chat is beside the screen and the terminal is wide enough to show it.
    pub(super) fn dock_active(&self) -> bool {
        self.dock.open && self.chat.is_some() && dock::bounds(self.viewport.width).is_some()
    }

    /// The pane has the keys.
    pub(super) fn dock_has_keys(&self) -> bool {
        self.dock_active() && self.dock.chat_focus
    }

    /// The pane width to ask the frame for, when the dock is up.
    pub(super) fn dock_request(&self) -> Option<u16> {
        self.dock_active().then(|| {
            self.dock
                .width
                .unwrap_or_else(|| dock::default_width(self.viewport.width))
        })
    }

    /// The area the screen beside the dock is laid out in: the viewport less the dock's share.
    /// Layout choice, the look and the paging keys use this, so what is drawn and what the keys
    /// assume agree.
    pub(super) fn layout_area(&self) -> Rect {
        match self.dock_request() {
            Some(wanted) => Rect {
                width: dock::main_width(self.viewport.width, wanted),
                ..self.viewport
            },
            None => self.viewport,
        }
    }

    /// The layout in force for the member's menu at the area the screen is drawn in.
    pub(super) fn menu_layout(&self) -> LayoutId {
        let area = self.layout_area();
        if self.dock_active() {
            self.content.flow().map_or(LayoutId::Classic, |flow| {
                layout::effective_docked(self.ui.layout_for(flow.settings()), area)
            })
        } else {
            self.content.layout(&self.ui, area)
        }
    }

    /// Whether the screen can have the dock beside it: a framed screen of a signed-in member,
    /// the forums or the mail, with the chat permission and room for both sides.
    fn dock_capable(&self) -> bool {
        let framed = match &self.content {
            Content::Member(_) => {
                self.content.menu_page().is_some()
                    || self.content.members_view().is_some()
                    || self.content.community_view().is_some()
                    || self.content.queue_view().is_some()
                    || self.content.is_framed_screen()
            }
            Content::Forums(_) | Content::Mail(_) => true,
            Content::Room(_) | Content::Claim(_) | Content::Doors(_) | Content::Limited(_) => false,
        };
        framed
            && self.editor.is_none()
            && self.widgets_page.is_none()
            && dock::bounds(self.viewport.width).is_some()
    }

    fn may_chat(&self) -> bool {
        self.access()
            .is_some_and(|access| access.allows(Permission::ChatJoin))
    }

    /// The screen beside the pane has a text field open, so `<` and `>` are text.
    pub(super) fn main_has_input(&self) -> bool {
        match &self.content {
            Content::Forums(session) => session.screen.has_input(),
            Content::Mail(session) => session.screen.has_input(),
            _ => self.main_member_input(),
        }
    }

    /// Leave the chat (the pane, or the full screen): the session leaves the hub.
    pub(super) fn drop_chat(&mut self) {
        self.chat = None;
        self.dock.open = false;
        self.dock.chat_focus = false;
    }

    /// Whether `keys` hold anything the dock acts on.
    pub(super) fn dock_keys_in(&self, keys: &[Key]) -> bool {
        keys.iter().any(|key| match key {
            Key::Ctrl('t') => true,
            Key::Ctrl('w') => self.dock_active(),
            key if dock::is_resize_key(*key) => self.dock_active(),
            _ => false,
        })
    }

    /// Apply a batch that may hold dock keys: split it at each of them, give each run of other
    /// keys to whichever side has the keys at that moment, and act on the dock key between.
    /// A run that asks for something to be answered first (a chat or forum request) holds the
    /// rest back, as everywhere else, and the shell applies it afterwards.
    pub(super) fn apply_keys_docked(&mut self, keys: &[Key]) -> KeyEffects {
        let mut effects = KeyEffects::default();
        let mut run: Vec<Key> = Vec::new();
        for (index, key) in keys.iter().copied().enumerate() {
            let acts = match key {
                Key::Ctrl('t') => true,
                Key::Ctrl('w') => self.dock_active(),
                key => {
                    dock::is_resize_key(key)
                        && self.dock_active()
                        && !self.dock.chat_focus
                        && !self.main_has_input()
                }
            };
            if !acts {
                run.push(key);
                continue;
            }
            self.flush_run(&mut run, &mut effects);
            if Self::must_wait(&effects) {
                let rest = keys.get(index..).unwrap_or_default();
                effects.held_back.extend_from_slice(rest);
                return effects;
            }
            self.dock_key(key, &mut effects);
            if Self::must_wait(&effects) {
                let rest = keys.get(index + 1..).unwrap_or_default();
                effects.held_back.extend_from_slice(rest);
                return effects;
            }
        }
        self.flush_run(&mut run, &mut effects);
        effects
    }

    /// A request is waiting for its answer, so later keys have to wait too.
    fn must_wait(effects: &KeyEffects) -> bool {
        !effects.held_back.is_empty() || effects.chat_request.is_some()
    }

    fn flush_run(&mut self, run: &mut Vec<Key>, effects: &mut KeyEffects) {
        if run.is_empty() {
            return;
        }
        let keys = std::mem::take(run);
        let later = if self.dock_has_keys() {
            self.apply_chat_keys(&keys)
        } else {
            self.apply_keys_main(&keys)
        };
        effects.absorb(later);
    }

    fn dock_key(&mut self, key: Key, effects: &mut KeyEffects) {
        self.dirty = true;
        match key {
            Key::Ctrl('t') => {
                if self.dock.open {
                    self.close_dock(effects);
                } else {
                    self.open_dock(effects);
                }
            }
            Key::Ctrl('w') => self.dock.chat_focus = !self.dock.chat_focus,
            other => {
                let current = self
                    .dock
                    .width
                    .unwrap_or_else(|| dock::default_width(self.viewport.width));
                if let Some(width) = dock::resized(self.viewport.width, current, other) {
                    self.dock.width = Some(width);
                }
            }
        }
    }

    fn open_dock(&mut self, effects: &mut KeyEffects) {
        if self.chat.is_some() || !self.may_chat() || !self.dock_capable() {
            if self.chat.is_none() && self.may_chat() && self.dock_capable_but_narrow() {
                self.toast(ToastKind::Notice, ToastText::Key("toast.dock_narrow"));
            }
            return;
        }
        let mut chat = ChatScreen::open();
        chat.set_manage(
            self.access()
                .is_some_and(|access| access.allows(Permission::ChatManageRooms)),
        );
        self.chat = Some(chat);
        self.dock.open = true;
        self.dock.chat_focus = true;
        effects.chat_open = true;
        effects.chat_request = Some(ChatRequest::List);
        self.toast(ToastKind::Notice, ToastText::Key("toast.dock_open"));
    }

    /// The dock was asked for on a framed screen whose terminal is too narrow for it.
    fn dock_capable_but_narrow(&self) -> bool {
        dock::bounds(self.viewport.width).is_none() && self.content.menu_page().is_some()
    }

    pub(super) fn close_dock(&mut self, effects: &mut KeyEffects) {
        self.drop_chat();
        if std::mem::take(&mut effects.chat_open) {
            // Opened and closed in the same batch: the session never joined.
            effects.chat_request = None;
        } else {
            effects.chat_exit = true;
        }
    }

    /// After a batch: a dock that the screen can no longer carry (the member went into the
    /// editor, the settings or a door, or the terminal shrank) is closed, so a chat is never
    /// left joined behind a pane nobody can see.
    pub(super) fn close_dock_if_stranded(&mut self, effects: &mut KeyEffects) {
        if self.dock.open && !self.dock_capable() {
            self.close_dock(effects);
            self.dirty = true;
        }
    }

    /// Drag the divider to terminal column `column` (the same resize the keys make).
    pub(super) fn dock_drag(&mut self, column: u16, body: Rect) {
        if !self.dock_active() {
            return;
        }
        if let Some(width) = dock::drag_target(body, column) {
            if self.dock.width != Some(width) {
                self.dock.width = Some(width);
                self.dirty = true;
            }
        }
    }
}

impl<'a> Pane<'a> {
    /// The pane for `chat`, drawn with the same strings, look, tier, markers and clock as the
    /// screen beside it.
    pub(super) fn new(chat: &'a ChatScreen, ctx: ChatCtx<'a>, focused: bool) -> Self {
        Self { chat, ctx, focused }
    }
}
