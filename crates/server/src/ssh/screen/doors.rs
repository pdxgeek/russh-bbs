//! The door screens' part of the session screen (M4-D0, D98): moving the member into the door
//! list and back, feeding it keys, giving it the store's answers and drawing it. Kept in a file
//! of its own so `screen.rs` only names the one `Content` arm.

use std::io;

use russh_bbs_core::terminal_settings::{MemberInput, TerminalSettings};
use russh_bbs_door::{DoorEnv, TransportKind};

use super::{Content, KeyEffects, Screen};
use crate::doors::{self, DoorEffect, DoorsScreen};
use crate::settings::Look;
use crate::ssh::terminal::Key;
use crate::ui::layout;

/// A member in the doors: the screens, and the flow that is given back at the main menu.
pub(crate) struct DoorsContent {
    pub(super) flow: Box<russh_bbs_core::terminal_settings::MemberFlow>,
    pub(super) screen: DoorsScreen,
}

/// What a batch of keys did to the door screens' place in the session.
#[derive(Clone, Copy)]
pub(super) enum DoorMove {
    /// Nothing: the keys stayed where they were.
    Stay,
    /// The menu chose Doors at this key.
    Open(usize),
    /// The doors asked to go back to the main menu.
    Leave,
    /// The member logged off (Ctrl-C, Ctrl-D).
    LogOff,
}

/// Feed `keys` to the door screens. A key that needs the store stops the batch: the rest wait in
/// `held_back` until the answer is in.
pub(super) fn keys(screen: &mut DoorsScreen, keys: &[Key], effects: &mut KeyEffects) -> DoorMove {
    for (index, key) in keys.iter().copied().enumerate() {
        match screen.input(key) {
            DoorEffect::Stay => {}
            DoorEffect::Exit => return DoorMove::Leave,
            DoorEffect::LogOff => return DoorMove::LogOff,
            DoorEffect::Request(request) => {
                effects.door_request = Some(request);
                effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                break;
            }
        }
    }
    DoorMove::Stay
}

impl Screen {
    /// What a door is told about the terminal, as it is right now.
    fn door_env(&self, settings: &TerminalSettings, handle: &str) -> DoorEnv {
        DoorEnv {
            transport: if self.transport.footer.is_some() {
                TransportKind::Telnet
            } else {
                TransportKind::Ssh
            },
            settings: *settings,
            tier: settings.icon_tier(),
            layout: layout::effective(settings.layout(), self.viewport),
            handle: handle.to_owned(),
        }
    }

    /// Carry out a move into or out of the door screens that the keys asked for.
    pub(super) fn door_transitions(
        &mut self,
        effects: &mut KeyEffects,
        keys: &[Key],
        moved: DoorMove,
    ) {
        match moved {
            DoorMove::Stay => {}
            DoorMove::Open(index) => {
                self.open_doors();
                let later = self.apply_keys(keys.get(index + 1..).unwrap_or_default());
                effects.absorb(later);
            }
            DoorMove::Leave => self.close_doors(),
            DoorMove::LogOff => {
                self.close_doors();
                if let Content::Member(flow) = &mut self.content {
                    // Ctrl-C and Ctrl-D end the session from anywhere (D54): the flow shows its
                    // goodbye. A door's writes were flushed after every key, so none are lost.
                    let _ = flow.input(MemberInput::Quit);
                }
                effects.disconnect = true;
            }
        }
    }

    /// Move the member into the door list.
    fn open_doors(&mut self) {
        let placeholder = Content::Limited(Vec::new());
        match std::mem::replace(&mut self.content, placeholder) {
            Content::Member(flow) => {
                let env = self.door_env(flow.settings(), flow.handle());
                let screen = DoorsScreen::open(self.ui.doors().all(), env);
                self.content = Content::Doors(Box::new(DoorsContent { flow, screen }));
                self.rebuild = true;
            }
            other => self.content = other,
        }
    }

    /// Give the member back to the main menu.
    fn close_doors(&mut self) {
        let placeholder = Content::Limited(Vec::new());
        match std::mem::replace(&mut self.content, placeholder) {
            Content::Doors(session) => {
                self.content = Content::Member(session.flow);
                self.rebuild = true;
            }
            other => self.content = other,
        }
    }

    /// The answer to the door request taken from [`KeyEffects`].
    pub(in crate::ssh) fn deliver_door(&mut self, response: doors::Response) {
        self.dirty = true;
        if let Content::Doors(session) = &mut self.content {
            session.screen.deliver(response);
        }
    }

    /// The board's doors clock, for running door requests.
    pub(in crate::ssh) fn door_clock(&self) -> std::sync::Arc<dyn russh_bbs_core::limits::Clock> {
        std::sync::Arc::clone(self.ui.doors().clock())
    }

    /// Draw the door screens if that is what the session shows. `None` for anything else.
    pub(super) fn draw_doors(&mut self) -> io::Result<Option<Vec<u8>>> {
        let Content::Doors(session) = &self.content else {
            return Ok(None);
        };
        let settings = *session.flow.settings();
        let theme = self.ui.theme(settings.theme());
        let layout = layout::effective(settings.layout(), self.viewport);
        let env = self.door_env(&settings, session.flow.handle());
        let ctx = doors::DrawCtx {
            strings: theme.strings(),
            look: Look::themed(&settings, layout, theme.palette()),
            tier: settings.icon_tier(),
        };
        let tier = ctx.tier;
        // The door sees the terminal as it is now (a resize or a theme switch), not as it was
        // when the list opened.
        if let Content::Doors(session) = &mut self.content {
            session.screen.set_env(env);
        }
        let Content::Doors(session) = &self.content else {
            return Ok(None);
        };
        self.terminal
            .draw(|frame| doors::render(frame, &session.screen, &ctx))?;
        Ok(Some(self.finish_frame(tier)))
    }
}
