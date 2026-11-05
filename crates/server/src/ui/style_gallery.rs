//! The member's style gallery and the sysop's Theme packs screen (UI-7).
//!
//! **Gallery.** A strip of the styles the board allows, a line about the one under the cursor,
//! and below them a live sample of it: the main menu (or the login screen) drawn off-screen by
//! the same code the real screens use, in that style, with **the member's own layout, tier,
//! colour mode, colour depth and the size of the room the gallery has**, then copied in. The
//! styles are exactly the board's allowed list (one entry is a lock: the gallery says so and
//! offers no pick). The sample is drawn with no markers and copied with [`blit`], which strips
//! the reverse attribute, and the copy goes into the body only: the status bar's left cell (the
//! marker cell) is the frame's, outside the body, so a sample can never cover it (D39, D109).
//!
//! **Packs.** Two lists, installed styles and pack files, with a line for the last outcome.
//! Every name is a validated word; a file's refusal reason may quote the file, so it is
//! sanitized again here before it is drawn.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::terminal_settings::{Gallery, PackScreen, TerminalSettings};

use crate::settings::Look;
use crate::ui::art::preview::{SampleSpec, blit, sample_buffer};
use crate::ui::frame::{ChromeText, FrameSpec, Hint};
use crate::ui::strings::StringTable;
use crate::ui::{Ui, keys};

/// What both screens are drawn with.
pub(crate) struct GalleryCtx<'a> {
    pub(crate) ui: &'a Ui,
    pub(crate) strings: &'a StringTable,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    pub(crate) markers: &'a [SecurityMarker],
    pub(crate) clock: Option<&'a str>,
    /// The member's own settings: the sample follows their layout, icon tier and colour.
    pub(crate) settings: &'a TerminalSettings,
}

/// The cursor keys' label in a hint.
const fn arrows(tier: Tier) -> &'static str {
    match tier {
        Tier::Ascii => "Up/Dn",
        _ => "\u{2191}\u{2193}",
    }
}

/// Draw the gallery.
#[allow(clippy::too_many_lines)] // one screen top to bottom: strip, info line, sample
pub(crate) fn render(frame: &mut Frame<'_>, gallery: &Gallery, ctx: &GalleryCtx<'_>) {
    let strings = ctx.strings;
    let key = |k: &'static str| ChromeText::fixed(k);
    let word = |k: &'static str| ChromeText::table(strings, k);
    let hints = [
        Hint::new(key(arrows(ctx.tier)), word(keys::GALLERY_HINT_MOVE)),
        Hint::new(key("Enter"), word(keys::GALLERY_HINT_USE)),
        Hint::new(key("s"), word(keys::GALLERY_HINT_SAMPLE)),
        Hint::new(key("b"), word(keys::GALLERY_HINT_BACK)),
    ];
    let body = FrameSpec::new(strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(word(keys::GALLERY_TITLE))
        .clock(ctx.clock)
        .hints(&hints)
        .draw(frame);
    if body.height < 4 || body.width < 20 {
        return;
    }
    let ui = ctx.ui;
    let palette = ctx.look.palette();
    let selected = gallery.selected();
    let theme = ui.theme(selected);

    // Row 0: the strip of styles, windowed so the one under the cursor is always on screen.
    let names: Vec<String> = gallery
        .choices()
        .iter()
        .map(|name| format!(" {} ", ui.theme(*name).title()))
        .collect();
    let (first, last) = window(
        &names,
        gallery.index(),
        usize::from(body.width).saturating_sub(4),
    );
    // The strip's own marks are chrome: plain ASCII in the ASCII tier.
    let ascii = ctx.tier == Tier::Ascii;
    let (more_left, more_right, dot) = if ascii {
        ("< ", " >", "-")
    } else {
        ("\u{2039} ", " \u{203a}", "\u{b7}")
    };
    let mut strip: Vec<Span<'_>> = Vec::new();
    strip.push(Span::styled(
        if first > 0 { more_left } else { "  " },
        ctx.look.muted(),
    ));
    for (at, text) in names.iter().enumerate().take(last).skip(first) {
        let style = if at == gallery.index() {
            ctx.look
                .fg(palette.selected)
                .patch(ctx.look.bg(palette.select_bg))
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else if gallery.choices()[at] == gallery.current() {
            ctx.look.fg(palette.key)
        } else {
            ctx.look.text()
        };
        strip.push(Span::styled(text.clone(), style));
    }
    if last < names.len() {
        strip.push(Span::styled(more_right, ctx.look.muted()));
    }

    // Row 1: what the style under the cursor is and whether it can be used.
    let state = if gallery.locked() {
        strings.text(keys::GALLERY_LOCKED)
    } else if selected == gallery.current() {
        strings.text(keys::GALLERY_IN_USE)
    } else {
        strings.text(keys::GALLERY_OFFER)
    };
    let credit = if theme.credit().is_empty() {
        String::new()
    } else {
        format!(" {dot} {}", theme.credit())
    };
    let info = Line::from(vec![
        Span::styled(
            theme.title().to_owned(),
            ctx.look.fg(palette.heading).add_modifier(Modifier::BOLD),
        ),
        Span::styled(credit, ctx.look.muted()),
        Span::styled(format!(" {dot} {state}"), ctx.look.fg(palette.key)),
    ]);
    let head = Rect { height: 2, ..body };
    frame.render_widget(Clear, head);
    frame.render_widget(Paragraph::new(vec![Line::from(strip), info]), head);

    // The sample: the room left, the member's own layout, tier and colour.
    let region = Rect {
        y: body.y + 2,
        height: body.height - 2,
        ..body
    };
    let sample = sample_buffer(
        ui,
        &SampleSpec {
            style: selected,
            layout: ui.layout_for(ctx.settings),
            size: (region.width, region.height),
            ascii: ctx.tier == Tier::Ascii,
            mono: !ctx.settings.color(),
            sample: gallery.sample(),
            art: true,
        },
        ctx.look,
    );
    blit(frame, region, &sample);
}

/// The slice `first..last` of `names` that fits `room` columns and holds `at`.
fn window(names: &[String], at: usize, room: usize) -> (usize, usize) {
    let width = |range: std::ops::Range<usize>| -> usize {
        names[range].iter().map(|n| n.chars().count()).sum()
    };
    let (mut first, mut last) = (at, at + 1);
    loop {
        let grew_right = last < names.len() && width(first..last + 1) <= room;
        if grew_right {
            last += 1;
        }
        let grew_left = first > 0 && width(first - 1..last) <= room;
        if grew_left {
            first -= 1;
        }
        if !grew_right && !grew_left {
            return (first, last);
        }
    }
}

/// Draw the Theme packs screen.
#[allow(clippy::too_many_lines)] // one screen top to bottom: two lists and a message line
pub(crate) fn render_packs(frame: &mut Frame<'_>, screen: &PackScreen, ctx: &GalleryCtx<'_>) {
    let strings = ctx.strings;
    let key = |k: &'static str| ChromeText::fixed(k);
    let word = |k: &'static str| ChromeText::table(strings, k);
    let hints = [
        Hint::new(key(arrows(ctx.tier)), word(keys::PACKS_HINT_MOVE)),
        Hint::new(key("e"), word(keys::PACKS_HINT_EXPORT)),
        Hint::new(key("i"), word(keys::PACKS_HINT_IMPORT)),
        Hint::new(key("r"), word(keys::PACKS_HINT_RESCAN)),
        Hint::new(key("b"), word(keys::PACKS_HINT_BACK)),
    ];
    let body = FrameSpec::new(strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(word(keys::PACKS_TITLE))
        .clock(ctx.clock)
        .hints(&hints)
        .draw(frame);
    let palette = ctx.look.palette();
    let heading = ctx.look.fg(palette.heading).add_modifier(Modifier::BOLD);
    let clean = |text: &str| SingleLine::new(160).sanitize(text).into_string();
    let mut lines: Vec<Line<'_>> = Vec::new();
    let mut cursor_line = 0;
    match screen.listing() {
        None => lines.push(Line::styled(
            strings.text(keys::PACKS_LOADING).to_owned(),
            ctx.look.muted(),
        )),
        Some(listing) if listing.unavailable => {
            lines.push(Line::styled(
                strings.text(keys::PACKS_UNAVAILABLE).to_owned(),
                ctx.look.fg(palette.warning),
            ));
        }
        Some(listing) => {
            lines.push(Line::styled(
                strings.text(keys::PACKS_STYLES).to_owned(),
                heading,
            ));
            let row_style = |at: usize| {
                if at == screen.row() {
                    ctx.look
                        .fg(palette.selected)
                        .patch(ctx.look.bg(palette.select_bg))
                        .add_modifier(Modifier::BOLD)
                } else {
                    ctx.look.text()
                }
            };
            for (at, style) in listing.styles.iter().enumerate() {
                if at == screen.row() {
                    cursor_line = lines.len();
                }
                let note = if style.exportable {
                    ""
                } else {
                    strings.text(keys::PACKS_BUILT_IN)
                };
                let pointer = if at == screen.row() { "> " } else { "  " };
                lines.push(Line::styled(
                    format!(
                        "{pointer}{:<24} {}  {note}",
                        style.name.as_str(),
                        clean(&style.title)
                    ),
                    row_style(at),
                ));
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                strings.text(keys::PACKS_FILES).to_owned(),
                heading,
            ));
            if listing.files.is_empty() {
                lines.push(Line::styled(
                    strings.text(keys::PACKS_NO_FILES).to_owned(),
                    ctx.look.muted(),
                ));
            }
            for (i, file) in listing.files.iter().enumerate() {
                let at = listing.styles.len() + i;
                if at == screen.row() {
                    cursor_line = lines.len();
                }
                let pointer = if at == screen.row() { "> " } else { "  " };
                let verdict = match &file.verdict {
                    Ok(name) => strings.filled(keys::PACKS_READY, &[("style", name.as_str())]),
                    Err(reason) => {
                        strings.filled(keys::PACKS_REFUSED, &[("reason", &clean(reason))])
                    }
                };
                lines.push(Line::styled(
                    format!("{pointer}{:<28} {verdict}", clean(&file.file)),
                    row_style(at),
                ));
            }
        }
    }
    // The last outcome sits on the bottom row, and the list scrolls to keep the cursor in view.
    let list_rows = usize::from(body.height).saturating_sub(2);
    let skip = (cursor_line + 1).saturating_sub(list_rows);
    let shown: Vec<Line<'_>> = lines.into_iter().skip(skip).take(list_rows).collect();
    frame.render_widget(Clear, body);
    frame.render_widget(Paragraph::new(shown), body);
    if body.height >= 2 {
        let note = screen.message().unwrap_or("");
        let row = Rect {
            y: body.y + body.height - 1,
            height: 1,
            ..body
        };
        frame.render_widget(
            Paragraph::new(Line::styled(note.to_owned(), ctx.look.fg(palette.info))),
            row,
        );
    }
}
