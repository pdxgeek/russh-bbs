//! The invite screens as state: what the member sees after creating a code, listing their
//! invites or opening the tree. The store calls happen in the front end (it has the store and
//! the randomness); this module turns their results into a [`View`].
//!
//! Everything shown is fixed chrome, the code itself (shown once, then dropped), or a handle
//! that has been through the shared sanitizer again here.

use super::service::CreatedInvite;
use super::store::{InviteStatus, InviteSummary};
use super::strings;
use super::tree::TreeLine;
use crate::sanitize::{SingleLine, USERNAME_MAX_CHARS};
use crate::store::UnixSeconds;
use crate::terminal_settings::format_utc;
use crate::terminal_settings::strings::fill;
use crate::waiting_room::{Notice, NoticeKind, View};

/// Tree lines per page. With the heading, count, page line and footer this fits an 80x24
/// terminal inside the framed layout and its status row.
pub const TREE_PAGE_LINES: usize = 12;

/// What the member asked for. The front end carries it out and reports an [`InviteOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteRequest {
    /// Make a new code.
    Create,
    /// List my codes.
    List,
    /// Open the invite tree.
    Tree,
}

/// What came of a request.
#[derive(Debug)]
pub enum InviteOutcome {
    /// A code was made.
    Created(CreatedInvite),
    /// The member's codes.
    List(Vec<InviteSummary>),
    /// The tree, in reading order.
    Tree(Vec<TreeLine>),
    /// The quota is used up.
    QuotaReached {
        /// The quota.
        limit: u32,
        /// When the count restarts.
        resets_at: UnixSeconds,
    },
    /// The session does not hold the permission.
    Denied,
    /// The store failed; nothing was changed.
    Failed,
}

/// An invite screen being shown.
#[derive(Debug)]
pub struct InviteScreen {
    outcome: InviteOutcome,
    page: usize,
}

impl InviteScreen {
    /// Show `outcome`.
    #[must_use]
    pub const fn new(outcome: InviteOutcome) -> Self {
        Self { outcome, page: 0 }
    }

    /// How many tree pages there are (1 for every other screen).
    fn pages(&self) -> usize {
        match &self.outcome {
            InviteOutcome::Tree(lines) => lines.len().div_ceil(TREE_PAGE_LINES).max(1),
            _ => 1,
        }
    }

    /// Move to the next or previous page. Returns whether the key was a paging key; any other
    /// key closes the screen.
    pub fn page_key(&mut self, key: char) -> bool {
        if !matches!(self.outcome, InviteOutcome::Tree(_)) {
            return false;
        }
        match key.to_ascii_lowercase() {
            'n' | ' ' => {
                self.page = (self.page + 1).min(self.pages() - 1);
                true
            }
            'p' | 'b' => {
                self.page = self.page.saturating_sub(1);
                true
            }
            _ => false,
        }
    }

    /// The screen as a [`View`].
    #[must_use]
    pub fn view(&self) -> View {
        let mut view = View {
            title: crate::terminal_settings::strings::TITLE,
            heading: "",
            banner: Vec::new(),
            body: Vec::new(),
            note: None,
            menu: Vec::new(),
            fields: Vec::new(),
            notice: None,
            footer: "",
        };
        match &self.outcome {
            InviteOutcome::Created(made) => {
                view.heading = strings::CREATE_HEADING;
                view.body = vec![
                    strings::CREATED_INTRO.to_owned(),
                    String::new(),
                    fill(strings::CODE_LINE, &[("code", made.code.reveal())]),
                    fill(strings::EXPIRES, &[("when", &format_utc(made.expires_at))]),
                    String::new(),
                    strings::SHOWN_ONCE.to_owned(),
                    strings::BOUND.to_owned(),
                    match made.remaining {
                        Some(n) => fill(strings::REMAINING, &[("n", &n.to_string())]),
                        None => strings::UNLIMITED.to_owned(),
                    },
                ];
                view.footer = strings::FOOTER_CODE;
            }
            InviteOutcome::List(items) => {
                view.heading = strings::LIST_HEADING;
                view.body = list_lines(items);
                view.note = Some(strings::LIST_NOTE);
                view.footer = strings::FOOTER_LIST;
            }
            InviteOutcome::Tree(lines) => self.tree_view(&mut view, lines),
            InviteOutcome::QuotaReached { limit, resets_at } => {
                view.heading = strings::CREATE_HEADING;
                view.body = vec![fill(
                    strings::QUOTA_REACHED,
                    &[
                        ("limit", &limit.to_string()),
                        ("when", &format_utc(*resets_at)),
                    ],
                )];
                view.notice = Some(error(strings::NOTICE_QUOTA));
                view.footer = strings::FOOTER_LIST;
            }
            InviteOutcome::Denied => {
                view.heading = strings::CREATE_HEADING;
                view.notice = Some(error(strings::NOTICE_DENIED));
                view.footer = strings::FOOTER_LIST;
            }
            InviteOutcome::Failed => {
                view.heading = strings::CREATE_HEADING;
                view.notice = Some(error(strings::NOTICE_FAILED));
                view.footer = strings::FOOTER_LIST;
            }
        }
        view
    }

    fn tree_view(&self, view: &mut View, lines: &[TreeLine]) {
        view.heading = strings::TREE_HEADING;
        if lines.is_empty() {
            view.body = vec![strings::TREE_EMPTY.to_owned()];
            view.footer = strings::FOOTER_TREE;
            return;
        }
        view.body = vec![fill(
            strings::TREE_COUNT,
            &[("n", &lines.len().to_string())],
        )];
        view.body.extend(
            lines
                .iter()
                .skip(self.page * TREE_PAGE_LINES)
                .take(TREE_PAGE_LINES)
                .map(|line| {
                    format!(
                        "{}{}",
                        "  ".repeat(usize::from(line.depth)),
                        clean_handle(line.handle.as_str())
                    )
                }),
        );
        let pages = self.pages();
        if pages > 1 {
            view.body.push(fill(
                strings::TREE_PAGE,
                &[
                    ("page", &(self.page + 1).to_string()),
                    ("pages", &pages.to_string()),
                ],
            ));
            view.footer = strings::FOOTER_TREE_PAGED;
        } else {
            view.footer = strings::FOOTER_TREE;
        }
    }
}

const fn error(text: &'static str) -> Notice {
    Notice {
        kind: NoticeKind::Error,
        text,
    }
}

fn clean_handle(handle: &str) -> String {
    SingleLine::new(USERNAME_MAX_CHARS)
        .sanitize(handle)
        .into_string()
}

fn list_lines(items: &[InviteSummary]) -> Vec<String> {
    if items.is_empty() {
        return vec![strings::LIST_EMPTY.to_owned()];
    }
    items
        .iter()
        .map(|item| {
            let status = match &item.status {
                InviteStatus::Open => fill(
                    strings::STATUS_OPEN,
                    &[("when", &format_utc(item.expires_at))],
                ),
                InviteStatus::Redeemed(Some(handle)) => fill(
                    strings::STATUS_USED_BY,
                    &[("handle", &clean_handle(handle.as_str()))],
                ),
                InviteStatus::Redeemed(None) => strings::STATUS_USED.to_owned(),
                InviteStatus::Expired => fill(
                    strings::STATUS_EXPIRED,
                    &[("when", &format_utc(item.expires_at))],
                ),
            };
            fill(
                strings::LIST_LINE,
                &[
                    ("id", &item.id.to_string()),
                    ("made", &format_utc(item.created_at)),
                    ("status", &status),
                ],
            )
        })
        .collect()
}
