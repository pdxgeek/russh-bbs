//! Lessons: each subject's minigame with its own small picture. The puzzle is the engine's
//! (`Game::lesson_view`); the screen is a blackboard in ink with the subject's emblem, the
//! question in chalk, and the answers as rows you choose from. Answers the practised pupil has
//! already ruled out are shown struck, because knowing what you know is part of the lesson.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Modifier;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Block, BorderType, Paragraph};

use super::{Size, Ui, centred, fit, wrap};

/// The emblem of a lesson: a title glyph and a three-line picture, in a rich and an ASCII form.
struct Emblem {
    glyph: &'static str,
    plain: &'static str,
    rich: [&'static str; 3],
    ascii: [&'static str; 3],
}

fn emblem(kind: &str) -> Emblem {
    match kind {
        "sequence" => Emblem {
            glyph: "Σ",
            plain: "S",
            rich: ["  ▁ ▂ ▃ ▅ ▇  ", "  2 4 8 ? ?  ", " + × ÷ − = π "],
            ascii: ["  . : - = #  ", "  2 4 8 ? ?  ", " + * / - = pi"],
        },
        "balance" => Emblem {
            glyph: "⚗",
            plain: "%",
            rich: ["  ╲▁▁▁▁▁▁╱   ", "  ◖g◗ ┴ ◖g◗  ", " ───────────  "],
            ascii: ["  \\______/   ", "  (g) ^ (g)  ", " ----------- "],
        },
        "vocab" => Emblem {
            glyph: "Æ",
            plain: "A",
            rich: [" ┌──┐ ┌──┐ ┌──┐", " │Aa│ │Bb│ │Cc│", " └──┘ └──┘ └──┘"],
            ascii: [" +--+ +--+ +--+", " |Aa| |Bb| |Cc|", " +--+ +--+ +--+"],
        },
        "timeline" => Emblem {
            glyph: "⌛",
            plain: "H",
            rich: ["             ", " ├──●───●──●─┤", " 1100  1500 1800"],
            ascii: ["             ", " |--o---o--o-|", " 1100  1500 1800"],
        },
        "pattern" => Emblem {
            glyph: "✎",
            plain: "~",
            rich: [" ▚▞▚▞▚▞▚▞▚▞▚▞", " ▞▚▞▚▞▚▞▚▞▚▞▚", " ▚▞▚▞▚▞▚▞▚▞▚▞"],
            ascii: [
                " /\\/\\/\\/\\/\\/\\",
                " \\/\\/\\/\\/\\/\\/",
                " /\\/\\/\\/\\/\\/\\",
            ],
        },
        "relay" => Emblem {
            glyph: "⚑",
            plain: "!",
            rich: [" ═══◂═══════▸═ ", " ══════◂══▸════ ", " ═▸══════◂═════ "],
            ascii: [" ===<=======>== ", " ======<==>==== ", " =>======<===== "],
        },
        _ => Emblem {
            glyph: "?",
            plain: "?",
            rich: ["", "", ""],
            ascii: ["", "", ""],
        },
    }
}

/// The lesson screen.
pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let Some(view) = g.lesson_view() else {
        return;
    };
    let Some(lesson) = g.player.life.lesson.as_ref() else {
        return;
    };
    let kind = g
        .content
        .lesson_for(&view.subject)
        .map_or("", |l| l.kind.as_str());
    let em = emblem(kind);
    let subject = g
        .content
        .subject(&view.subject)
        .map_or(view.subject.as_str(), |s| s.name.as_str());
    let teacher = g
        .content
        .subject(&view.subject)
        .and_then(|s| g.content.npc(&s.teacher))
        .map(|n| n.name.clone());
    let answers_h = u16::try_from(view.round.options.len()).unwrap_or(4) + 3;
    let [head, board, answers] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(answers_h + 1),
    ])
    .areas(area);
    // ---- the strip: subject, puzzle number, how you are doing.
    let rounds = view.rounds;
    let taken = usize::from(lesson.round);
    let mut spans = vec![
        Span::styled(format!(" {} ", ui.look.g(em.glyph, em.plain)), pal.lamp()),
        Span::styled(subject.to_owned(), pal.chalk()),
        Span::styled(
            format!("  puzzle {} of {rounds}  ", u32::from(view.number) + 1),
            pal.slate(),
        ),
    ];
    spans.extend(ui.pips(taken, rounds, pal.fg_or(super::hue::INK, pal.accent)));
    spans.push(Span::styled(
        format!("  {} right", lesson.right),
        pal.moss(),
    ));
    if let Some(t) = teacher
        && usize::from(head.width) > 70
    {
        spans.push(Span::styled(format!("  {}", fit(&t, 24)), pal.slate()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), head);
    // ---- the blackboard.
    let block = Block::bordered()
        .border_type(if ui.look.ascii {
            BorderType::Plain
        } else {
            BorderType::Double
        })
        .border_style(pal.brass())
        .style(pal.on(super::hue::CHALK, super::hue::INK_DEEP, pal.plain));
    let inner = block.inner(board);
    f.render_widget(block, board);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let chalk = pal
        .on(super::hue::CHALK, super::hue::INK_DEEP, pal.strong)
        .add_modifier(Modifier::BOLD);
    let faint = pal.on(super::hue::INK, super::hue::INK_DEEP, pal.dim);
    let pic = if ui.look.ascii { em.ascii } else { em.rich };
    let tall = inner.height >= 8 || ui.look.size >= Size::Roomy;
    if tall {
        for row in pic {
            lines.push(Line::styled(row.to_owned(), faint).centered());
        }
        lines.push(Line::default());
    } else if let Some(row) = pic.get(1).filter(|r| !r.trim().is_empty()) {
        lines.push(Line::styled((*row).to_owned(), faint).centered());
    }
    for (i, l) in view.round.prompt.iter().enumerate() {
        for w in wrap(l, usize::from(inner.width).saturating_sub(4)) {
            lines.push(Line::styled(w, if i == 0 { faint } else { chalk }).centered());
        }
    }
    let r = centred(inner, inner.width, u16::try_from(lines.len()).unwrap_or(1));
    f.render_widget(Paragraph::new(lines), r);
    // ---- the answers.
    let frame = ui.framed(f, answers, "Your answer");
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut item_at = 0usize;
    let cursor = ui.cursor();
    for (i, option) in view.round.options.iter().enumerate() {
        let open = view.open.get(i).copied().unwrap_or(false);
        let key = char::from(b'1' + u8::try_from(i).unwrap_or(0));
        if open {
            let on = item_at == cursor;
            item_at += 1;
            let room = usize::from(frame.width).saturating_sub(6);
            let text = fit(option, room);
            if on {
                let pad = usize::from(frame.width).saturating_sub(5 + text.chars().count());
                rows.push(Line::styled(
                    format!(
                        " {key} {} {text}{}",
                        ui.look.g("▸", ">"),
                        " ".repeat(pad.saturating_sub(2))
                    ),
                    pal.bar_on(),
                ));
            } else {
                rows.push(Line::from(vec![
                    Span::styled(format!(" {key}   "), pal.brass()),
                    Span::styled(text, pal.chalk_soft()),
                ]));
            }
        } else {
            rows.push(Line::from(vec![
                Span::styled(format!(" {}   ", ui.look.g("╳", "x")), pal.slate()),
                Span::styled(
                    format!(
                        "{}  (ruled out)",
                        fit(option, usize::from(frame.width).saturating_sub(18))
                    ),
                    pal.slate(),
                ),
            ]));
        }
    }
    let leave_on = item_at == cursor;
    rows.push(if leave_on {
        Line::styled(
            format!(
                " x {} walk out of the lesson (it counts as a skipped class){}",
                ui.look.g("▸", ">"),
                ""
            ),
            pal.bar_on(),
        )
    } else {
        Line::from(vec![
            Span::styled(" x   ", pal.brass()),
            Span::styled("walk out of the lesson", pal.slate()),
        ])
    });
    f.render_widget(Paragraph::new(rows), frame);
}
