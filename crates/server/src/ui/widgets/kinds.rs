//! The launch set: one [`Widget`] per [`WidgetId`] (UI-10).
//!
//! Each draws from the [`WidgetData`] it is handed, with the theme's palette roles through
//! [`Pen`]. Member text goes through [`Cx::member`]. Glyph choices follow `cx.ascii`.

use std::time::Duration;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use russh_bbs_core::widgets::WidgetId;

use super::gfx::{self, Unit};
use super::{Cx, Pen, Widget};

/// Every chrome sentence the widgets use, as string-table keys (checked at boot with the
/// framework's keys, so a missing one is an error naming it, never a blank).
pub(crate) const TEXT_KEYS: &[&str] = &[
    "ui.widget.title.identity",
    "ui.widget.title.clock",
    "ui.widget.title.online",
    "ui.widget.title.callers",
    "ui.widget.title.new_posts",
    "ui.widget.title.mail",
    "ui.widget.title.chat",
    "ui.widget.title.activity",
    "ui.widget.title.credits",
    "ui.widget.title.oneliner",
    "ui.widget.title.wall",
    "ui.widget.title.poll",
    "ui.widget.title.news",
    "ui.widget.title.motd",
    "ui.widget.title.quote",
    "ui.widget.title.doors",
    "ui.widget.title.status",
    "ui.widget.online_count",
    "ui.widget.ago_now",
    "ui.widget.ago_min",
    "ui.widget.ago_hour",
    "ui.widget.ago_day",
    "ui.widget.posts_some",
    "ui.widget.posts_none",
    "ui.widget.mail_unread",
    "ui.widget.mail_none",
    "ui.widget.mail_total",
    "ui.widget.activity_total",
    "ui.widget.activity_from",
    "ui.widget.activity_to",
    "ui.widget.credits_now",
    "ui.widget.credits_change",
    "ui.widget.poll_hidden",
    "ui.widget.doors_season",
    "ui.widget.since",
    "ui.widget.status_sessions",
    "ui.widget.status_members",
    "ui.widget.status_sent",
    "ui.widget.status_up",
];

macro_rules! meta {
    ($id:ident, $title:literal, min $mw:literal x $mh:literal, pref $pw:literal x $ph:literal, every $secs:literal) => {
        fn id(&self) -> WidgetId {
            WidgetId::$id
        }
        fn title_key(&self) -> &'static str {
            concat!("ui.widget.title.", $title)
        }
        fn min_size(&self) -> (u16, u16) {
            ($mw, $mh)
        }
        fn preferred(&self) -> (u16, u16) {
            ($pw, $ph)
        }
        fn budget(&self) -> Duration {
            Duration::from_secs($secs)
        }
    };
}

/// The registry: the widget for `id`.
pub(super) fn widget(id: WidgetId) -> &'static dyn Widget {
    match id {
        WidgetId::Identity => &Identity,
        WidgetId::Clock => &Clock,
        WidgetId::Online => &Online,
        WidgetId::Callers => &Callers,
        WidgetId::NewPosts => &NewPosts,
        WidgetId::Mail => &Mail,
        WidgetId::Chat => &Chat,
        WidgetId::Activity => &Activity,
        WidgetId::Credits => &Credits,
        WidgetId::Oneliner => &Oneliner,
        WidgetId::Wall => &Wall,
        WidgetId::Poll => &Poll,
        WidgetId::News => &News,
        WidgetId::Motd => &Motd,
        WidgetId::Quote => &Quote,
        WidgetId::Doors => &Doors,
        WidgetId::Status => &Status,
    }
}

// ---- small helpers ----------------------------------------------------------------------

fn line(text: impl Into<String>, style: Style) -> Line<'static> {
    Line::styled(text.into(), style)
}

/// `left` and `right` on one row of `width`, the right one flush to the edge. The left is cut
/// to leave the right and a space.
fn row(
    cx: &Cx<'_>,
    left: Vec<Span<'static>>,
    right: Option<(String, Style)>,
    width: usize,
) -> Line<'static> {
    let right_len = right.as_ref().map_or(0, |(t, _)| t.chars().count());
    let room = width.saturating_sub(right_len + usize::from(right_len > 0));
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for span in left {
        let left_over = room.saturating_sub(used);
        if left_over == 0 {
            break;
        }
        let text = gfx::clip(span.content.as_ref(), left_over, cx.ascii);
        used += text.chars().count();
        spans.push(Span::styled(text, span.style));
    }
    if let Some((text, style)) = right {
        let gap = width.saturating_sub(used + right_len);
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

fn ago(cx: &Cx<'_>, at: i64) -> String {
    match gfx::ago(cx.now, at) {
        None => cx.strings.text("ui.widget.ago_now").to_owned(),
        Some((unit, n)) => cx.said(
            match unit {
                Unit::Minute => "ui.widget.ago_min",
                Unit::Hour => "ui.widget.ago_hour",
                Unit::Day => "ui.widget.ago_day",
            },
            n,
        ),
    }
}

fn dot(cx: &Cx<'_>) -> &'static str {
    if cx.ascii { "*" } else { "\u{25cf}" }
}

fn pointer(cx: &Cx<'_>) -> &'static str {
    if cx.ascii { ">" } else { "\u{25b8}" }
}

fn fit(mut lines: Vec<Line<'static>>, height: u16) -> Vec<Line<'static>> {
    lines.truncate(usize::from(height));
    lines
}

/// "1,234" for 1234.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ---- identity and time ------------------------------------------------------------------

struct Identity;

impl Widget for Identity {
    meta!(Identity, "identity", min 12 x 2, pref 30 x 4, every 3600);

    fn has_content(&self, _cx: &Cx<'_>) -> bool {
        true
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let board = cx.board;
        let width = usize::from(width);
        let mut lines = vec![line(
            gfx::clip(cx.strings.text("ui.title"), width, cx.ascii),
            pen.accent(),
        )];
        if !board.tagline().is_empty() {
            lines.push(line(
                gfx::clip(board.tagline().as_str(), width, cx.ascii),
                pen.text(),
            ));
        }
        let since = (!board.since().is_empty())
            .then(|| cx.strings.filled("ui.widget.since", &[("n", board.since().as_str())]));
        let dot = if cx.ascii { "-" } else { "\u{b7}" };
        let place = match (board.location().is_empty(), since) {
            (false, Some(since)) => {
                let both = format!("{} {dot} {since}", board.location().as_str());
                Some(if both.chars().count() <= width {
                    both
                } else {
                    board.location().as_str().to_owned()
                })
            }
            (false, None) => Some(board.location().as_str().to_owned()),
            (true, Some(since)) => Some(since),
            (true, None) => None,
        };
        if let Some(place) = place {
            lines.push(line(gfx::clip(&place, width, cx.ascii), pen.muted()));
        }
        fit(lines, height)
    }
}

struct Clock;

impl Widget for Clock {
    meta!(Clock, "clock", min 10 x 1, pref 24 x 12, every 60);

    fn has_content(&self, _cx: &Cx<'_>) -> bool {
        true
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let date = gfx::date_of(cx.now);
        let secs = cx.now.rem_euclid(86_400);
        let hhmm = format!("{:02}:{:02}", secs / 3_600, secs % 3_600 / 60);
        let day = format!(
            "{} {} {} {}",
            gfx::weekday_name(date.weekday),
            date.day,
            gfx::month_name(date.month),
            date.year
        );
        let width = usize::from(width);
        if height < 5 || width < 12 {
            return fit(
                vec![
                    line(format!("{hhmm} UTC"), pen.accent()),
                    line(gfx::clip(&day, width, cx.ascii), pen.muted()),
                ],
                height,
            );
        }
        let mut lines: Vec<Line<'static>> = gfx::big_digits(&hhmm, cx.ascii)
            .into_iter()
            .map(|row| line(row, pen.accent()))
            .collect();
        lines.push(Line::from(vec![
            Span::styled(day, pen.text()),
            Span::styled(" UTC", pen.muted()),
        ]));
        // A month calendar when the cell is tall enough: heads, then up to six weeks.
        let weeks = gfx::month_weeks(date);
        if width >= 21 && usize::from(height) >= lines.len() + 1 + weeks.len() {
            lines.push(line(
                gfx::WEEKDAYS
                    .iter()
                    .map(|d| format!("{d} "))
                    .collect::<String>()
                    .trim_end()
                    .to_owned(),
                pen.heading(),
            ));
            for week in weeks {
                let mut spans = Vec::new();
                for day in week {
                    spans.push(match day {
                        Some(d) if d == date.day => {
                            let mark = if cx.ascii { '*' } else { ' ' };
                            Span::styled(format!("{d:>2}{mark}"), pen.today())
                        }
                        Some(d) => Span::styled(format!("{d:>2} "), pen.text()),
                        None => Span::raw("   "),
                    });
                }
                lines.push(Line::from(spans));
            }
        }
        fit(lines, height)
    }
}

// ---- people -----------------------------------------------------------------------------

struct Online;

impl Widget for Online {
    meta!(Online, "online", min 10 x 2, pref 22 x 8, every 10);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.online.as_ref().is_some_and(|v| !v.is_empty())
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(names) = &cx.data.online else {
            return Vec::new();
        };
        let width = usize::from(width);
        let mut lines = vec![Line::from(vec![
            Span::styled(format!("{} ", dot(cx)), pen.good()),
            Span::styled(cx.said("ui.widget.online_count", names.len()), pen.heading()),
        ])];
        // Two columns when the cell is wide enough, so a busy board still shows many names.
        let columns = if width >= 26 { 2 } else { 1 };
        let each = (width / columns).max(1);
        let room = usize::from(height).saturating_sub(1);
        let shown: Vec<&String> = names.iter().take(room * columns).collect();
        for chunk in shown.chunks(columns) {
            let mut spans = Vec::new();
            for name in chunk {
                let name = cx.member(name, each.saturating_sub(2));
                let text = format!("{} {name}", pointer(cx));
                let pad = each.saturating_sub(text.chars().count());
                spans.push(Span::styled(text, pen.text()));
                spans.push(Span::raw(" ".repeat(pad)));
            }
            lines.push(Line::from(spans));
        }
        fit(lines, height)
    }
}

struct Callers;

impl Widget for Callers {
    meta!(Callers, "callers", min 14 x 2, pref 24 x 8, every 60);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.callers.as_ref().is_some_and(|v| !v.is_empty())
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(callers) = &cx.data.callers else {
            return Vec::new();
        };
        let lines = callers
            .iter()
            .map(|caller| {
                let name = cx.member(&caller.handle, 32);
                row(
                    cx,
                    vec![Span::styled(name, pen.text())],
                    Some((ago(cx, caller.at.as_raw()), pen.stamp())),
                    usize::from(width),
                )
            })
            .collect();
        fit(lines, height)
    }
}

struct Chat;

impl Widget for Chat {
    meta!(Chat, "chat", min 12 x 2, pref 24 x 8, every 10);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.chat.as_ref().is_some_and(|v| !v.is_empty())
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(rooms) = &cx.data.chat else {
            return Vec::new();
        };
        let lines = rooms
            .iter()
            .map(|room| {
                let heads = room.members.min(8) as usize;
                let people = dot(cx).repeat(heads);
                let name = cx.member(&room.name, 32);
                row(
                    cx,
                    vec![
                        Span::styled("#", pen.muted()),
                        Span::styled(name, pen.key()),
                    ],
                    Some((
                        format!("{people} {}", room.members),
                        if room.members > 0 { pen.good() } else { pen.muted() },
                    )),
                    usize::from(width),
                )
            })
            .collect();
        fit(lines, height)
    }
}

// ---- counts and charts ------------------------------------------------------------------

struct NewPosts;

impl Widget for NewPosts {
    meta!(NewPosts, "new_posts", min 14 x 2, pref 26 x 7, every 60);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.new_posts.is_some()
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(posts) = &cx.data.new_posts else {
            return Vec::new();
        };
        let width = usize::from(width);
        if posts.threads == 0 {
            return fit(
                vec![line(cx.strings.text("ui.widget.posts_none"), pen.muted())],
                height,
            );
        }
        let mut lines = vec![line(
            gfx::clip(&cx.said("ui.widget.posts_some", posts.threads), width, cx.ascii),
            pen.good(),
        )];
        let most = posts.boards.iter().map(|(_, n)| *n).max().unwrap_or(1).max(1);
        let name_w = (width / 3).clamp(4, 14);
        let count_w = 4;
        let bar_w = width.saturating_sub(name_w + count_w + 2);
        for (name, n) in &posts.boards {
            let name = gfx::clip(&cx.member(name, 40), name_w, cx.ascii);
            let (filled, rest) = gfx::bar(*n as f64 / most as f64, bar_w, cx.ascii);
            lines.push(Line::from(vec![
                Span::styled(format!("{name:<name_w$} "), pen.text()),
                Span::styled(filled, pen.accent2()),
                Span::styled(rest, pen.muted()),
                Span::styled(format!(" {n:>3}"), pen.heading()),
            ]));
        }
        fit(lines, height)
    }
}

struct Mail;

impl Widget for Mail {
    meta!(Mail, "mail", min 10 x 2, pref 22 x 7, every 30);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.mail.is_some()
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(mail) = cx.data.mail else {
            return Vec::new();
        };
        let width = usize::from(width);
        let mut lines = Vec::new();
        if mail.unread == 0 {
            lines.push(line(cx.strings.text("ui.widget.mail_none"), pen.muted()));
        } else {
            if height >= 6 && width >= 12 {
                let shown = mail.unread.min(99).to_string();
                for digits in gfx::big_digits(&shown, cx.ascii) {
                    lines.push(line(digits, pen.good()));
                }
            }
            lines.push(line(
                gfx::clip(&cx.said("ui.widget.mail_unread", mail.unread), width, cx.ascii),
                pen.good(),
            ));
            let (filled, rest) = gfx::bar(
                mail.unread as f64 / mail.total.max(1) as f64,
                width,
                cx.ascii,
            );
            lines.push(Line::from(vec![
                Span::styled(filled, pen.good()),
                Span::styled(rest, pen.muted()),
            ]));
        }
        lines.push(line(
            gfx::clip(&cx.said("ui.widget.mail_total", mail.total), width, cx.ascii),
            pen.muted(),
        ));
        fit(lines, height)
    }
}

struct Activity;

impl Widget for Activity {
    meta!(Activity, "activity", min 14 x 2, pref 28 x 8, every 300);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.activity.is_some()
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(series) = &cx.data.activity else {
            return Vec::new();
        };
        let w = usize::from(width);
        let total = series.iter().fold(0_u64, |a, b| a.saturating_add(*b));
        let mut lines = vec![line(
            gfx::clip(&cx.said("ui.widget.activity_total", total), w, cx.ascii),
            pen.heading(),
        )];
        let chart_rows = usize::from(height).saturating_sub(2);
        if chart_rows >= 3 {
            for chart in gfx::area_chart(series, w, chart_rows, cx.ascii) {
                lines.push(line(chart, pen.accent2()));
            }
        } else {
            lines.push(line(gfx::spark(series, w, cx.ascii), pen.accent2()));
        }
        let from = cx.strings.text("ui.widget.activity_from");
        let to = cx.strings.text("ui.widget.activity_to");
        if height >= 3 {
            lines.push(row(
                cx,
                vec![Span::styled(from.to_owned(), pen.muted())],
                Some((to.to_owned(), pen.muted())),
                w,
            ));
        }
        fit(lines, height)
    }
}

struct Credits;

impl Widget for Credits {
    meta!(Credits, "credits", min 12 x 2, pref 24 x 7, every 60);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.credits.is_some()
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(credits) = &cx.data.credits else {
            return Vec::new();
        };
        let w = usize::from(width);
        let mut lines = vec![line(
            gfx::clip(
                &cx.said("ui.widget.credits_now", grouped(credits.balance)),
                w,
                cx.ascii,
            ),
            pen.warn(),
        )];
        let first = credits.series.first().copied().unwrap_or(credits.balance);
        let change = i128::from(credits.balance) - i128::from(first);
        let (text, style) = match change.cmp(&0) {
            std::cmp::Ordering::Greater => (format!("+{change}"), pen.good()),
            std::cmp::Ordering::Less => (format!("{change}"), pen.bad()),
            std::cmp::Ordering::Equal => ("0".to_owned(), pen.muted()),
        };
        lines.push(line(
            gfx::clip(&cx.said("ui.widget.credits_change", text), w, cx.ascii),
            style,
        ));
        let chart_rows = usize::from(height).saturating_sub(2);
        if chart_rows >= 2 {
            for chart in gfx::area_chart(&credits.series, w, chart_rows, cx.ascii) {
                lines.push(line(chart, pen.accent2()));
            }
        }
        fit(lines, height)
    }
}

struct Poll;

impl Widget for Poll {
    meta!(Poll, "poll", min 14 x 3, pref 28 x 8, every 60);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.poll.is_some()
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(poll) = &cx.data.poll else {
            return Vec::new();
        };
        let w = usize::from(width);
        let question = cx.member(&poll.question, 120);
        let q_lines = if height >= 7 { 2 } else { 1 };
        let mut lines: Vec<Line<'static>> = gfx::wrap(&question, w, q_lines, cx.ascii)
            .into_iter()
            .map(|text| line(text, pen.heading()))
            .collect();
        let total = poll
            .options
            .iter()
            .filter_map(|(_, v)| *v)
            .fold(0_u64, u64::saturating_add);
        let hidden = poll.options.iter().any(|(_, v)| v.is_none());
        let label_w = (w / 3).clamp(4, 12);
        let bar_w = w.saturating_sub(label_w + 8);
        for (at, (text, votes)) in poll.options.iter().enumerate() {
            let mine = poll.my_vote == Some(at);
            let mark = if mine { pointer(cx) } else { " " };
            let label = gfx::clip(&cx.member(text, 60), label_w, cx.ascii);
            let mut spans = vec![Span::styled(
                format!("{mark}{label:<label_w$} "),
                if mine { pen.key() } else { pen.text() },
            )];
            if let Some(votes) = votes {
                let (filled, rest) = gfx::bar(
                    *votes as f64 / total.max(1) as f64,
                    bar_w,
                    cx.ascii,
                );
                let pct = if total == 0 { 0 } else { votes * 100 / total };
                spans.push(Span::styled(filled, pen.accent2()));
                spans.push(Span::styled(rest, pen.muted()));
                spans.push(Span::styled(format!(" {pct:>3}%"), pen.heading()));
            }
            lines.push(Line::from(spans));
        }
        if hidden {
            lines.push(line(
                gfx::clip(cx.strings.text("ui.widget.poll_hidden"), w, cx.ascii),
                pen.muted(),
            ));
        }
        fit(lines, height)
    }
}

// ---- text lists -------------------------------------------------------------------------

/// A list of "author: text" lines (one-liners, the wall), newest first.
fn said_lines(cx: &Cx<'_>, entries: &[russh_bbs_core::widgets::Line], width: u16, height: u16) -> Vec<Line<'static>> {
    let pen = Pen(cx.look);
    let w = usize::from(width);
    let mut lines = Vec::new();
    for entry in entries {
        let author = cx.member(&entry.author, 24);
        let text = cx.member(&entry.text, 160);
        let head = format!("{author}: ");
        let head_w = head.chars().count();
        let mut parts = gfx::wrap(&text, w.saturating_sub(head_w).max(1), 2, cx.ascii).into_iter();
        let first = parts.next().unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(head, pen.key()),
            Span::styled(first, pen.text()),
        ]));
        for rest in parts {
            lines.push(line(format!("{}{rest}", " ".repeat(head_w.min(w / 2))), pen.text()));
        }
        if lines.len() >= usize::from(height) {
            break;
        }
    }
    fit(lines, height)
}

struct Oneliner;

impl Widget for Oneliner {
    meta!(Oneliner, "oneliner", min 14 x 2, pref 28 x 7, every 60);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.oneliners.as_ref().is_some_and(|v| !v.is_empty())
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        cx.data
            .oneliners
            .as_ref()
            .map_or_else(Vec::new, |e| said_lines(cx, e, width, height))
    }
}

struct Wall;

impl Widget for Wall {
    meta!(Wall, "wall", min 14 x 2, pref 28 x 7, every 60);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.wall.as_ref().is_some_and(|v| !v.is_empty())
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        cx.data
            .wall
            .as_ref()
            .map_or_else(Vec::new, |e| said_lines(cx, e, width, height))
    }
}

struct News;

impl Widget for News {
    meta!(News, "news", min 14 x 2, pref 30 x 8, every 120);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.news.as_ref().is_some_and(|v| !v.is_empty())
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(news) = &cx.data.news else {
            return Vec::new();
        };
        let w = usize::from(width);
        let bullet = if cx.ascii { "-" } else { "\u{25aa}" };
        let mut lines = Vec::new();
        for item in news {
            let subject = cx.member(&item.subject, 120);
            lines.push(row(
                cx,
                vec![
                    Span::styled(format!("{bullet} "), pen.accent()),
                    Span::styled(subject, pen.text()),
                ],
                Some((ago(cx, item.at.as_raw()), pen.stamp())),
                w,
            ));
            if usize::from(height) > lines.len() + 1 && height >= 8 {
                lines.push(line(
                    format!("  {}", gfx::clip(&cx.member(&item.board, 40), w.saturating_sub(2), cx.ascii)),
                    pen.muted(),
                ));
            }
        }
        fit(lines, height)
    }
}

struct Motd;

impl Widget for Motd {
    meta!(Motd, "motd", min 12 x 2, pref 30 x 5, every 3600);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        !cx.settings.motd().is_empty()
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let bar = if cx.ascii { "|" } else { "\u{258c}" };
        let w = usize::from(width).saturating_sub(2);
        let lines = gfx::wrap(cx.settings.motd().as_str(), w, usize::from(height), cx.ascii)
            .into_iter()
            .map(|text| {
                Line::from(vec![
                    Span::styled(format!("{bar} "), pen.accent()),
                    Span::styled(text, pen.text()),
                ])
            })
            .collect();
        fit(lines, height)
    }
}

struct Quote;

impl Widget for Quote {
    meta!(Quote, "quote", min 12 x 2, pref 30 x 5, every 3600);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.settings.has_quotes()
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(quote) = cx.settings.quote(cx.now.div_euclid(86_400)) else {
            return Vec::new();
        };
        let (open, close) = if cx.ascii { ("\"", "\"") } else { ("\u{201c}", "\u{201d}") };
        let w = usize::from(width);
        let mut lines: Vec<Line<'static>> =
            gfx::wrap(quote.as_str(), w.saturating_sub(2), usize::from(height), cx.ascii)
                .into_iter()
                .map(|text| line(text, pen.quote()))
                .collect();
        if let Some(first) = lines.first_mut() {
            first.spans.insert(0, Span::styled(open, pen.accent()));
        }
        if let Some(last) = lines.last_mut() {
            last.spans.push(Span::styled(close, pen.accent()));
        }
        fit(lines, height)
    }
}

// ---- doors and status -------------------------------------------------------------------

struct Doors;

impl Widget for Doors {
    meta!(Doors, "doors", min 12 x 2, pref 26 x 7, every 300);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.doors.as_ref().is_some_and(|v| !v.is_empty())
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(doors) = &cx.data.doors else {
            return Vec::new();
        };
        let lines = doors
            .iter()
            .map(|door| {
                row(
                    cx,
                    vec![
                        Span::styled(format!("{} ", pointer(cx)), pen.accent()),
                        Span::styled(cx.member(&door.name, 40), pen.key()),
                    ],
                    Some((cx.said("ui.widget.doors_season", door.season), pen.muted())),
                    usize::from(width),
                )
            })
            .collect();
        fit(lines, height)
    }
}

struct Status;

impl Widget for Status {
    meta!(Status, "status", min 14 x 3, pref 28 x 7, every 5);

    fn has_content(&self, cx: &Cx<'_>) -> bool {
        cx.data.status.is_some()
    }

    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>> {
        let pen = Pen(cx.look);
        let Some(status) = cx.data.status else {
            return Vec::new();
        };
        let w = usize::from(width);
        let label = cx.strings.text("ui.widget.status_sessions");
        let figures = format!("{}/{}", status.sessions, status.capacity);
        let mut lines = vec![row(
            cx,
            vec![Span::styled(label.to_owned(), pen.heading())],
            Some((figures, pen.text())),
            w,
        )];
        let load = f64::from(status.sessions) / f64::from(status.capacity.max(1));
        let (filled, rest) = gfx::bar(load, w, cx.ascii);
        let style = if load > 0.9 {
            pen.bad()
        } else if load > 0.7 {
            pen.warn()
        } else {
            pen.good()
        };
        lines.push(Line::from(vec![
            Span::styled(filled, style),
            Span::styled(rest, pen.muted()),
        ]));
        let pair = |key: &str, value: String| {
            row(
                cx,
                vec![Span::styled(cx.strings.text(key).to_owned(), pen.muted())],
                Some((value, pen.text())),
                w,
            )
        };
        lines.push(pair("ui.widget.status_members", status.members.to_string()));
        lines.push(pair("ui.widget.status_sent", gfx::bytes(status.bytes)));
        lines.push(pair("ui.widget.status_up", gfx::span(status.uptime)));
        fit(lines, height)
    }
}
