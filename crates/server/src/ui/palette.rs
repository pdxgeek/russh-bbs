//! The **command palette** (UI-9): `Ctrl-K` opens a search box over everything the member may
//! reach; typing narrows it with a fuzzy match, Enter runs the choice.
//!
//! **It cannot list what the member cannot reach.** The entries are built from
//! `russh_bbs_core::access` and nothing else:
//!
//! - *areas*: every item of the menu tree that [`Item::visible`] says this snapshot may see
//!   (the same rule the menu itself draws with), by its path of hotkeys;
//! - *settings pages*: only when the member holds the settings permission;
//! - *doors*: only when the member holds `doors.run`;
//! - *boards*: the board names the forum service already filtered for this snapshot.
//!
//! Members are not searched yet (a person has no key path that opens their profile directly;
//! that arrives with the UI-4 screens).
//!
//! **It does not run anything itself.** An entry is a *key path*: the keys that reach the same
//! place from the main menu (`f` for Forums, `d` `Enter` for a door ...). The screen feeds
//! those keys through the ordinary path, so the menu, the permission check and the limits all
//! run again exactly as for a typed key: a permission taken away since the palette was built
//! simply makes the path do nothing. The palette is offered at the main menu and its
//! submenus only; the waiting room and the claim prompt have none.
//!
//! Text from the board (door titles, board names) is sanitized when the entry is made, and the
//! palette is drawn as an overlay clear of the header, the status bar and the marker cell.

use std::collections::VecDeque;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use russh_bbs_core::access::{Permission, PermissionCheck};
use russh_bbs_core::menu::{Action, MenuTree, Target};
use russh_bbs_core::sanitize::SingleLine;

use super::input::{self, Click};
use super::strings::StringTable;
use super::theme::Element;
use crate::settings::Look;
use crate::ssh::terminal::Key;

/// Most results shown at once.
const SHOWN: usize = 8;
/// Longest query kept, in characters.
pub(crate) const MAX_QUERY: usize = 40;
/// Most entries one palette holds (boards are capped so a huge board list cannot grow it).
const MAX_ENTRIES: usize = 400;
/// How many recent choices are remembered for the session.
pub(crate) const MAX_RECENT: usize = 6;
/// Longest label kept, in characters.
const MAX_LABEL: usize = 48;

/// The string-table keys the palette draws with.
pub(crate) const TEXT_KEYS: [&str; 15] = [
    "palette.title",
    "palette.prompt",
    "palette.empty",
    "palette.hint",
    "palette.group.area",
    "palette.group.settings",
    "palette.group.door",
    "palette.group.board",
    "palette.recent",
    "palette.settings.width",
    "palette.settings.height",
    "palette.settings.mouse",
    "palette.settings.motion",
    "palette.settings.layout",
    "palette.settings.theme",
];

/// What kind of place an entry leads to (drawn as a word, not only a colour).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    Area,
    Settings,
    Door,
    Board,
}

impl Group {
    const fn key(self) -> &'static str {
        match self {
            Self::Area => "palette.group.area",
            Self::Settings => "palette.group.settings",
            Self::Door => "palette.group.door",
            Self::Board => "palette.group.board",
        }
    }
}

/// One thing the palette can go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// Sanitized text to show and to search.
    label: String,
    group: Group,
    /// Stable identity, for the recent list.
    id: String,
    /// The keys from the main menu's root that reach it.
    keys: Vec<Key>,
}

impl Entry {
    /// The label.
    #[cfg(test)]
    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    /// The group.
    #[cfg(test)]
    pub(crate) fn group(&self) -> Group {
        self.group
    }
}

/// A board the viewer may list, as the forum service returned it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoardName(pub(crate) String);

/// What the entries are built from. Everything here has already been through `core::access`
/// except the tree, which the entries filter themselves.
pub(crate) struct Sources<'a> {
    pub(crate) tree: &'static MenuTree,
    pub(crate) access: &'a dyn PermissionCheck,
    pub(crate) strings: &'a StringTable,
    /// Door titles, in the order the door list shows them.
    pub(crate) doors: Vec<String>,
    pub(crate) boards: Vec<BoardName>,
}

/// The entries `sources` allows. Pure: no store, no clock.
pub(crate) fn entries(sources: &Sources<'_>) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut settings_path: Option<Vec<Key>> = None;
    let mut doors_path: Option<Vec<Key>> = None;
    let mut forums_path: Option<Vec<Key>> = None;
    walk(
        sources,
        sources.tree.root,
        &mut Vec::new(),
        &mut |path, label, target| {
            let keys: Vec<Key> = path.iter().map(|c| Key::Char(*c)).collect();
            match target {
                Target::Action(Action::TerminalSettings) => settings_path = Some(keys.clone()),
                Target::Action(Action::Doors) => doors_path = Some(keys.clone()),
                Target::Action(Action::Forums) => forums_path = Some(keys.clone()),
                _ => {}
            }
            // Menus are reached through their own items; a placeholder does nothing yet.
            if matches!(target, Target::Action(_)) {
                out.push(Entry {
                    label: clean(label),
                    group: Group::Area,
                    id: format!("area:{}", path.iter().collect::<String>()),
                    keys,
                });
            }
        },
    );
    if let Some(base) = settings_path {
        for (page, key, text) in [
            ("width", 'w', "palette.settings.width"),
            ("height", 'h', "palette.settings.height"),
            ("mouse", 'o', "palette.settings.mouse"),
            ("motion", 'm', "palette.settings.motion"),
            ("layout", 'l', "palette.settings.layout"),
            ("theme", 't', "palette.settings.theme"),
        ] {
            let mut keys = base.clone();
            keys.push(Key::Char(key));
            out.push(Entry {
                label: clean(sources.strings.text(text)),
                group: Group::Settings,
                id: format!("settings:{page}"),
                keys,
            });
        }
    }
    if let Some(base) = doors_path
        && sources.access.allows(Permission::DoorsRun)
    {
        for (index, title) in sources.doors.iter().enumerate() {
            let mut keys = base.clone();
            keys.extend(std::iter::repeat_n(Key::Down, index.min(64)));
            keys.push(Key::Enter);
            out.push(Entry {
                label: clean(title),
                group: Group::Door,
                id: format!("door:{index}"),
                keys,
            });
        }
    }
    if let Some(base) = forums_path
        && sources.access.allows(Permission::ForumList)
    {
        for (index, board) in sources.boards.iter().enumerate() {
            let mut keys = base.clone();
            keys.extend(std::iter::repeat_n(Key::Down, index.min(64)));
            keys.push(Key::Enter);
            out.push(Entry {
                label: clean(&board.0),
                group: Group::Board,
                id: format!("board:{index}"),
                keys,
            });
        }
    }
    out.truncate(MAX_ENTRIES);
    out
}

/// Visit every action or menu item the snapshot may see, depth first, with its hotkey path.
fn walk(
    sources: &Sources<'_>,
    menu: &str,
    path: &mut Vec<char>,
    visit: &mut dyn FnMut(&[char], &str, Target),
) {
    let Some(menu) = sources.tree.menu(menu) else {
        return;
    };
    if path.len() >= russh_bbs_core::menu::MAX_DEPTH {
        return;
    }
    for item in menu.items {
        if !item.visible(sources.access) {
            continue;
        }
        path.push(item.hotkey);
        visit(path, sources.strings.text(item.label), item.target);
        if let Target::Menu(id) = item.target {
            walk(sources, id, path, visit);
        }
        path.pop();
    }
}

fn clean(text: &str) -> String {
    SingleLine::new(MAX_LABEL).sanitize(text).into_string()
}

/// How well `query` matches `label`: `None` when the letters do not appear in order. A higher
/// score is a better match: a run of consecutive letters and a match at the start of a word
/// both count for more. Case does not matter.
pub(crate) fn score(query: &str, label: &str) -> Option<u32> {
    let mut points = 0_u32;
    let mut wanted = query.chars().filter(|c| !c.is_whitespace()).peekable();
    let mut previous_matched = false;
    let mut previous: Option<char> = None;
    for c in label.chars() {
        let Some(next) = wanted.peek().copied() else {
            break;
        };
        if c.eq_ignore_ascii_case(&next) || c.to_lowercase().eq(next.to_lowercase()) {
            points += 1;
            if previous_matched {
                points += 3;
            }
            if previous.is_none_or(|p| !p.is_alphanumeric()) {
                points += 5;
            }
            previous_matched = true;
            wanted.next();
        } else {
            previous_matched = false;
        }
        previous = Some(c);
    }
    wanted.peek().is_none().then_some(points)
}

/// What a key did to the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Nothing for the caller to do (the palette redraws).
    Stay,
    /// Close it.
    Close,
    /// Close it and press these keys, from the main menu's root.
    Run(Vec<Key>),
}

/// The open palette.
#[derive(Debug, Clone)]
pub(crate) struct Palette {
    entries: Vec<Entry>,
    query: String,
    /// Indexes into `entries`, best first.
    shown: Vec<usize>,
    cursor: usize,
    recent: VecDeque<String>,
}

impl Palette {
    /// Open over `entries`, putting `recent` choices first.
    pub(crate) fn new(entries: Vec<Entry>, recent: &VecDeque<String>) -> Self {
        let mut palette = Self {
            entries,
            query: String::new(),
            shown: Vec::new(),
            cursor: 0,
            recent: recent.clone(),
        };
        palette.refilter();
        palette
    }

    /// The labels now shown, best first.
    #[cfg(test)]
    pub(crate) fn shown_labels(&self) -> Vec<&str> {
        self.shown
            .iter()
            .filter_map(|at| self.entries.get(*at))
            .map(Entry::label)
            .collect()
    }

    /// The query typed so far.
    pub(crate) fn typed(&self) -> &str {
        &self.query
    }

    /// Type `text` as if key by key (used when the entries are rebuilt under an open palette).
    pub(crate) fn retype(&mut self, text: &str) {
        self.query = text.chars().take(MAX_QUERY).collect();
        self.refilter();
    }

    /// The id of the entry Enter would run, if any (the caller remembers it as a recent).
    pub(crate) fn chosen_id(&self) -> Option<&str> {
        self.entries
            .get(*self.shown.get(self.cursor)?)
            .map(|entry| entry.id.as_str())
    }

    fn refilter(&mut self) {
        let recent_rank = |id: &str| self.recent.iter().position(|r| r == id);
        let mut scored: Vec<(usize, u32, Option<usize>)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(at, entry)| {
                let points = if self.query.is_empty() {
                    0
                } else {
                    score(&self.query, &entry.label)?
                };
                Some((at, points, recent_rank(&entry.id)))
            })
            .collect();
        // Best match first; among equal matches (everything, when nothing is typed) the recent
        // ones come first, newest first; then the order the entries were made in.
        scored.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| match (a.2, b.2) {
                    (Some(x), Some(y)) => x.cmp(&y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                })
                .then_with(|| a.0.cmp(&b.0))
        });
        self.shown = scored.into_iter().map(|(at, _, _)| at).collect();
        self.cursor = 0;
    }

    /// Apply one key.
    pub(crate) fn input(&mut self, key: Key) -> Outcome {
        match key {
            Key::Escape | Key::Ctrl('k') => return Outcome::Close,
            Key::Enter => {
                return match self
                    .shown
                    .get(self.cursor)
                    .and_then(|at| self.entries.get(*at))
                {
                    Some(entry) => Outcome::Run(entry.keys.clone()),
                    None => Outcome::Stay,
                };
            }
            Key::Up => self.cursor = self.cursor.saturating_sub(1),
            Key::Down => {
                self.cursor = (self.cursor + 1).min(self.shown.len().saturating_sub(1));
            }
            Key::PageUp => self.cursor = self.cursor.saturating_sub(SHOWN),
            Key::PageDown => {
                self.cursor = (self.cursor + SHOWN).min(self.shown.len().saturating_sub(1));
            }
            Key::Home => self.cursor = 0,
            Key::End => self.cursor = self.shown.len().saturating_sub(1),
            Key::Backspace => {
                self.query.pop();
                self.refilter();
            }
            Key::Char(c) if self.query.chars().count() < MAX_QUERY => {
                self.query.push(c);
                self.refilter();
            }
            Key::Char(_)
            | Key::Ctrl(_)
            | Key::Left
            | Key::Right
            | Key::Delete
            | Key::PasteBreak
            | Key::Other => {}
        }
        Outcome::Stay
    }

    /// Draw the palette as an overlay inside `area`, which the caller keeps clear of the header,
    /// the status bar and the marker cell. Records its rows for the mouse.
    pub(crate) fn render(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        look: Look,
        strings: &StringTable,
    ) {
        let width = area.width.min(64);
        let rows = u16::try_from(self.shown.len().min(SHOWN))
            .unwrap_or(0)
            .max(1);
        let height = (rows + 4).min(area.height);
        if width < 20 || height < 5 {
            return;
        }
        let rect = Rect {
            x: area.x + (area.width - width) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 3,
            width,
            height,
        };
        let inner = look.panel(
            frame,
            rect,
            Some(strings.text("palette.title")),
            Element::Popup,
            true,
        );
        // A modal: what is under it no longer answers the mouse.
        input::modal();
        let [prompt_row, list_rows, hint_row] = [
            Rect { height: 1, ..inner },
            Rect {
                y: inner.y + 1,
                height: inner.height.saturating_sub(2),
                ..inner
            },
            Rect {
                y: inner.y + inner.height.saturating_sub(1),
                height: 1,
                ..inner
            },
        ];
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let dim = look.muted();
        let cursor_mark = if look.ascii() { "_" } else { "\u{2581}" };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{} ", strings.text("palette.prompt")), bold),
                Span::raw(self.query.clone()),
                Span::styled(cursor_mark, dim),
            ])),
            prompt_row,
        );
        if self.shown.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(strings.text("palette.empty").to_owned(), dim)),
                list_rows,
            );
        } else {
            let items: Vec<ListItem<'static>> = self
                .shown
                .iter()
                .filter_map(|at| self.entries.get(*at))
                .map(|entry| {
                    let mut spans = vec![Span::raw(entry.label.clone())];
                    spans.push(Span::styled(
                        format!("  ({})", strings.text(entry.group.key())),
                        dim,
                    ));
                    if self.recent.iter().any(|r| *r == entry.id) {
                        spans.push(Span::styled(
                            format!(" {}", strings.text("palette.recent")),
                            dim,
                        ));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let selected = look
                .accent(
                    Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    look.palette().selected,
                )
                .patch(look.bg(look.surfaces().select_bg));
            let list = List::new(items)
                .highlight_style(selected)
                .highlight_symbol(format!("{} ", strings.text(super::keys::POINTER)));
            let mut state = ListState::default();
            state.select(Some(self.cursor));
            frame.render_stateful_widget(list, list_rows, &mut state);
            input::list(
                list_rows,
                1,
                state.offset(),
                self.shown.len(),
                Some(self.cursor),
                Click::Open,
            );
        }
        frame.render_widget(
            Paragraph::new(Line::styled(strings.text("palette.hint").to_owned(), dim)),
            hint_row,
        );
    }
}

/// Remember `id` as the most recent choice.
pub(crate) fn remember(recent: &mut VecDeque<String>, id: &str) {
    recent.retain(|r| r != id);
    recent.push_front(id.to_owned());
    recent.truncate(MAX_RECENT);
}

#[cfg(test)]
mod tests;
