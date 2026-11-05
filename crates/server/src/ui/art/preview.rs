//! The sysop's theme preview (UI-3), under Sysop tools: a style, a layout, a size and an icon
//! tier drawn on a sample screen before the sysop sets a default.
//!
//! **What it shows.** The sample (the main menu with its header art, or the login screen with
//! its banner) is drawn off-screen at the chosen size, in the chosen style, layout, tier and
//! colour mode, by the same code the members' screens use, then copied into the preview's body.
//! If the sysop's terminal is smaller than the chosen size, the top-left part is shown and the
//! screen says so. Above it: what was chosen, which art piece is in use and any art files that
//! were refused, with their reasons.
//!
//! **Markers.** The sample is drawn with no security markers, and the copy strips the reverse
//! attribute from every cell, so nothing in the preview can wear the reserved style. The real
//! marker cell is the preview screen's own status bar, drawn by the frame as on every screen.

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph};
use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::menu::{MenuSession, main_tree};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::terminal_settings::{
    LayoutId, MemberFlow, PreviewSample, TerminalSettings, ThemePreview,
};

use super::draw::ArtCtx;
use super::piece::Slot;
use crate::settings::{self, Look};
use crate::ui::classic::{self, MenuScreen};
use crate::ui::frame::{ChromeText, FrameSpec, Hint};
use crate::ui::strings::StringTable;
use crate::ui::{Ui, backdrop, keys, layout};

/// What the preview screen is drawn with: the sysop's own terminal.
pub(crate) struct PreviewCtx<'a> {
    pub(crate) ui: &'a Ui,
    pub(crate) strings: &'a StringTable,
    /// The sysop's look: the frame around the preview is drawn with it.
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    pub(crate) markers: &'a [SecurityMarker],
    pub(crate) clock: Option<&'a str>,
}

/// Draw the preview screen.
pub(crate) fn render(frame: &mut Frame<'_>, preview: &ThemePreview, ctx: &PreviewCtx<'_>) {
    let strings = ctx.strings;
    let key = |k: &'static str| ChromeText::fixed(k);
    let word = |k: &'static str| ChromeText::table(strings, k);
    let hints = [
        Hint::new(key("t"), word(keys::PREVIEW_HINT_STYLE)),
        Hint::new(key("l"), word(keys::PREVIEW_HINT_LAYOUT)),
        Hint::new(key("z"), word(keys::PREVIEW_HINT_SIZE)),
        Hint::new(key("i"), word(keys::PREVIEW_HINT_TIER)),
        Hint::new(key("c"), word(keys::PREVIEW_HINT_COLOUR)),
        Hint::new(key("s"), word(keys::PREVIEW_HINT_SAMPLE)),
        Hint::new(key("a"), word(keys::PREVIEW_HINT_ART)),
        Hint::new(key("b"), word(keys::PREVIEW_HINT_BACK)),
    ];
    let body = FrameSpec::new(strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(word(keys::PREVIEW_TITLE))
        .clock(ctx.clock)
        .hints(&hints)
        .draw(frame);
    let sample = draw_sample(ctx.ui, preview, ctx.look);
    let (want_cols, want_rows) = preview.size();
    let notes = notes(ctx, preview, body, (want_cols, want_rows));
    let note_rows = u16::try_from(notes.len())
        .unwrap_or(u16::MAX)
        .min(body.height);
    let notes_area = Rect {
        height: note_rows,
        ..body
    };
    frame.render_widget(Clear, notes_area);
    frame.render_widget(
        Paragraph::new(notes.into_iter().map(Line::raw).collect::<Vec<_>>()),
        Rect {
            height: note_rows,
            ..body
        },
    );
    let region = Rect {
        y: body.y + note_rows,
        height: body.height - note_rows,
        ..body
    };
    blit(frame, region, &sample);
}

/// The lines above the sample.
fn notes(
    ctx: &PreviewCtx<'_>,
    preview: &ThemePreview,
    body: Rect,
    (want_cols, want_rows): (u16, u16),
) -> Vec<String> {
    let strings = ctx.strings;
    let ui = ctx.ui;
    let on_off = |on: bool| {
        strings.text(if on {
            keys::PREVIEW_ON
        } else {
            keys::PREVIEW_OFF
        })
    };
    let layout_name = strings.text(keys::LAYOUT_NAMES[layout_index(preview.layout())]);
    let mut out = vec![strings.filled(
        keys::PREVIEW_STATE,
        &[
            ("style", preview.style().as_str()),
            ("layout", layout_name),
            ("cols", &want_cols.to_string()),
            ("rows", &want_rows.to_string()),
            (
                "tier",
                strings.text(if preview.ascii() {
                    keys::PREVIEW_TIER_ASCII
                } else {
                    keys::PREVIEW_TIER_UNICODE
                }),
            ),
            (
                "colour",
                strings.text(if preview.mono() {
                    keys::PREVIEW_MONO
                } else {
                    keys::PREVIEW_COLOUR
                }),
            ),
            (
                "sample",
                strings.text(match preview.sample() {
                    PreviewSample::Menu => keys::PREVIEW_SAMPLE_MENU,
                    PreviewSample::Login => keys::PREVIEW_SAMPLE_LOGIN,
                }),
            ),
            ("art", on_off(preview.art() && ui.board().art())),
        ],
    )];
    let slot = match preview.sample() {
        PreviewSample::Menu => Slot::Menu,
        PreviewSample::Login => Slot::Login,
    };
    let art = ui.art_ctx(preview.style());
    let ascii = if preview.ascii() {
        Tier::Ascii
    } else {
        Tier::Unicode
    };
    let used = (preview.art() && art.enabled)
        .then(|| {
            // The smallest room the sample leaves art is not known here; report the piece the
            // sample's own choice would be: the best that fits the whole sample.
            art.set
                .pick(slot, preview.style(), want_cols, want_rows)
                .filter(|piece| ascii != Tier::Ascii || piece.ascii_safe())
        })
        .flatten();
    out.push(match used {
        Some(piece) => strings.filled(
            keys::PREVIEW_ART,
            &[("set", piece.set()), ("credit", piece.credit())],
        ),
        None => strings.text(keys::PREVIEW_ART_NONE).to_owned(),
    });
    let problems = ui.art().problems();
    if let Some(first) = problems.first() {
        out.push(strings.filled(
            keys::PREVIEW_PROBLEMS,
            &[
                ("count", &problems.len().to_string()),
                ("first", &format!("{}: {}", first.file, first.reason)),
            ],
        ));
    }
    if body.width < want_cols || body.height < want_rows {
        out.push(strings.filled(
            keys::PREVIEW_CLIPPED,
            &[
                ("cols", &body.width.min(want_cols).to_string()),
                ("rows", &body.height.min(want_rows).to_string()),
                ("want_cols", &want_cols.to_string()),
                ("want_rows", &want_rows.to_string()),
            ],
        ));
    }
    out
}

const fn layout_index(layout: LayoutId) -> usize {
    match layout {
        LayoutId::Classic => 0,
        LayoutId::Lightbar => 1,
        LayoutId::Compact => 2,
        LayoutId::Dashboard => 3,
        LayoutId::Wide => 4,
        LayoutId::Focus => 5,
        LayoutId::Split => 6,
    }
}

/// What a sample screen is drawn as: a style, a layout, a size, a tier and the sample itself.
/// The sysop preview fills it from its keys; the member gallery from the member's own settings
/// and terminal.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SampleSpec {
    pub(crate) style: russh_bbs_core::terminal_settings::ThemeName,
    pub(crate) layout: LayoutId,
    pub(crate) size: (u16, u16),
    pub(crate) ascii: bool,
    pub(crate) mono: bool,
    pub(crate) sample: PreviewSample,
    pub(crate) art: bool,
}

/// The sample screen, drawn off-screen at the preview's size with no markers.
fn draw_sample(ui: &Ui, preview: &ThemePreview, shown_with: Look) -> Buffer {
    sample_buffer(
        ui,
        &SampleSpec {
            style: preview.style(),
            layout: preview.layout(),
            size: preview.size(),
            ascii: preview.ascii(),
            mono: preview.mono(),
            sample: preview.sample(),
            art: preview.art(),
        },
        shown_with,
    )
}

/// A sample screen drawn off-screen by the same code the members' screens use, with no security
/// markers (the callers copy it with [`blit`], which strips the reverse attribute).
pub(crate) fn sample_buffer(ui: &Ui, spec: &SampleSpec, shown_with: Look) -> Buffer {
    let (cols, rows) = spec.size;
    let area = Rect::new(0, 0, cols, rows);
    let tier = if spec.ascii {
        Tier::Ascii
    } else {
        Tier::Unicode
    };
    let settings = TerminalSettings::default()
        .with_tier(tier)
        .with_color(!spec.mono)
        .with_theme(spec.style)
        .with_layout(spec.layout);
    let theme = ui.theme(spec.style);
    let strings = theme.strings();
    // The viewer's terminal limits the colours the sample can honestly show.
    let look = ui.look(&settings, area, shown_with.depth());
    let art = ArtCtx {
        set: ui.art(),
        style: spec.style,
        board: ui.board().name(),
        enabled: spec.art && ui.board().art(),
    };
    let mut terminal =
        Terminal::new(TestBackend::new(cols, rows)).unwrap_or_else(|never| match never {});
    // The sample shows the layout's cells empty: widgets are the member's own, not the board's.
    let widget_data = russh_bbs_core::widgets::WidgetData::default();
    let widget_settings = crate::ui::widgets::WidgetSettings::default();
    let drawn = terminal.draw(|frame| match spec.sample {
        PreviewSample::Menu => {
            let access = Snapshot::from_role(
                None,
                &Slug::SYSOP,
                100,
                &Via::Account,
                Permission::ALL.iter().copied(),
            );
            let page = MenuSession::new(main_tree()).page(&access);
            let body = [strings.text(keys::PREVIEW_SAMPLE_BODY).to_owned()];
            layout::render_menu(
                frame,
                look.layout(),
                &MenuScreen {
                    strings,
                    page: &page,
                    body: &body,
                    notice: None,
                    look,
                    tier,
                    markers: &[],
                    clock: Some("UTC 12:00"),
                    mail_over_ssh_only: false,
                    board: ui.board(),
                    wanted: spec.layout,
                    popover: false,
                    art: Some(&art),
                    widgets: crate::ui::widgets::WidgetView {
                        shown: &[],
                        data: &widget_data,
                        settings: &widget_settings,
                        now: 0,
                    },
                },
            );
            backdrop::finish(frame, look);
        }
        PreviewSample::Login => {
            let Some(flow) = sample_flow(settings) else {
                return;
            };
            let view = flow.view(strings);
            let hints = classic::content_hints(strings);
            settings::render_framed(
                frame,
                &view,
                &settings::Framed {
                    strings,
                    look,
                    tier,
                    markers: &[],
                    hints: &hints,
                    art: Some((&art, Slot::Login)),
                },
            );
            backdrop::finish(frame, look);
        }
    });
    if drawn.is_err() {
        return Buffer::empty(area);
    }
    terminal.backend().buffer().clone()
}

/// A member session to draw the login sample from. The handle is a fixed, valid word.
fn sample_flow(settings: TerminalSettings) -> Option<MemberFlow> {
    let handle = sanitize_username("sample").ok()?;
    Some(MemberFlow::new(
        &handle,
        Snapshot::from_role(None, &Slug::MEMBER, 10, &Via::Account, std::iter::empty()),
        None,
        settings,
    ))
}

/// Copy the top-left of `sample` into `region`, clearing it first. The reverse attribute is
/// stripped from every copied cell: the preview shows how a screen looks, never a marker.
pub(crate) fn blit(frame: &mut Frame<'_>, region: Rect, sample: &Buffer) {
    frame.render_widget(Clear, region);
    let width = region.width.min(sample.area.width);
    let height = region.height.min(sample.area.height);
    let target = frame.buffer_mut();
    for dy in 0..height {
        for dx in 0..width {
            let mut cell = sample[(dx, dy)].clone();
            cell.modifier.remove(Modifier::REVERSED);
            target[(region.x + dx, region.y + dy)] = cell;
        }
    }
}
