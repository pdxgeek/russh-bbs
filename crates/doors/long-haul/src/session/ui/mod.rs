//! The Long Haul screens (slice L3): the bridge of a worn freighter drawn with ratatui.
//!
//! Every screen is drawn into the door's own buffer through `DoorFrame`; the board cleans and
//! degrades every cell. A window smaller than [`MIN_COLS`] by [`MIN_ROWS`] gets the plain
//! text screens of L1 and L2 instead (they are also the look-around view), so the game stays
//! playable on anything. Nothing in this module holds a rule: it reads the engine and shows it.

#![allow(
    clippy::many_single_char_names,
    clippy::unused_self,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::items_after_statements,
    clippy::format_push_string,
    clippy::uninlined_format_args,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::default_trait_access,
    clippy::explicit_counter_loop,
    clippy::if_not_else,
    clippy::manual_checked_ops,
    reason = "drawing code: short geometry names, one function per panel, `&self` kept so every \
              screen reads the same way"
)]

pub(in crate::session) mod boarding;
mod bridge;
pub(in crate::session) mod chart;
pub(in crate::session) mod combat;
mod crew;
mod intro;
pub(in crate::session) mod kit;
pub(in crate::session) mod look;
mod market;
pub(in crate::session) mod route;
mod scene;
mod standing;

use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::Paragraph;
use russh_bbs_door::{Canvas, DoorEnv, DoorFrame, Tier};

use self::kit::{Pal, credits, key_lines, pips, trunc};
use super::{Mode, Session};
use crate::scene::Scene;

/// The narrowest window that gets the full screens.
pub(in crate::session) const MIN_COLS: u16 = 60;
/// The shortest window that gets the full screens.
pub(in crate::session) const MIN_ROWS: u16 = 15;

/// Which screen is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Intro,
    Event,
    Combat,
    Boarding,
    Bridge,
    Chart,
    Route,
    Docks,
    Market,
    Yard,
    Loan,
    News,
    Crew,
    Jobs,
    Standing,
}

impl Session {
    fn screen(&self) -> Screen {
        let Some(player) = &self.player else {
            return Screen::Intro;
        };
        match &player.scene {
            Some(Scene::Event(_)) => return Screen::Event,
            Some(Scene::Combat(_)) => return Screen::Combat,
            Some(Scene::Boarding(_)) => return Screen::Boarding,
            None => {}
        }
        match self.mode {
            Mode::Pick(_) | Mode::Unreadable => Screen::Intro,
            Mode::Bridge => Screen::Bridge,
            Mode::Chart => Screen::Chart,
            Mode::Route => Screen::Route,
            Mode::Docks(_) => Screen::Docks,
            Mode::Market(_) => Screen::Market,
            Mode::Yard(_) => Screen::Yard,
            Mode::Loan => Screen::Loan,
            Mode::News => Screen::News,
            Mode::Crew(_) => Screen::Crew,
            Mode::Jobs(_) => Screen::Jobs,
            Mode::Standing => Screen::Standing,
        }
    }
}

/// Draw the full screen. Returns false when the window is too small, and the caller draws the
/// text screens instead.
pub(super) fn draw(s: &Session, canvas: &mut Canvas<'_>, env: &DoorEnv) -> bool {
    if canvas.width() < MIN_COLS || canvas.height() < MIN_ROWS {
        return false;
    }
    let mut frame = canvas.frame();
    let pal = Pal::new(&frame, env.tier == Tier::Ascii);
    let area = frame.area();
    let screen = s.screen();
    if screen == Screen::Intro {
        s.draw_intro(&mut frame, area, &pal);
        return true;
    }
    let keys = s.screen_keys(screen, &pal, area.width);
    let toast = u16::from(!s.log.is_empty());
    let foot_h = toast + u16::try_from(keys.len()).unwrap_or(2);
    let [status, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(foot_h),
    ])
    .areas(area);
    s.draw_status(&mut frame, status, &pal);
    match screen {
        Screen::Bridge => s.draw_bridge(&mut frame, body, &pal),
        Screen::Chart => s.draw_chart(&mut frame, body, &pal),
        Screen::Route => s.draw_route(&mut frame, body, &pal),
        Screen::Event => s.draw_event(&mut frame, body, &pal),
        Screen::Combat => s.draw_combat(&mut frame, body, &pal),
        Screen::Boarding => s.draw_boarding(&mut frame, body, &pal),
        Screen::Market => s.draw_market(&mut frame, body, &pal),
        Screen::Jobs => s.draw_jobs(&mut frame, body, &pal),
        Screen::Crew => s.draw_crew(&mut frame, body, &pal),
        Screen::Standing => s.draw_standing(&mut frame, body, &pal),
        Screen::Docks => s.draw_docks(&mut frame, body, &pal),
        Screen::Yard => s.draw_yard(&mut frame, body, &pal),
        Screen::Loan => s.draw_loan(&mut frame, body, &pal),
        Screen::News => s.draw_news(&mut frame, body, &pal),
        Screen::Intro => {}
    }
    // The newest message, then the keys.
    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some((text, style)) = s.log.last() {
        lines.push(Line::styled(
            trunc(text, usize::from(area.width)),
            pal.role(*style),
        ));
    }
    lines.extend(keys);
    frame.render_widget(Paragraph::new(lines), foot);
    true
}

impl Session {
    /// The top line: who, what they hold, and how much of the day is left.
    fn draw_status(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let Some(p) = &self.player else { return };
        let allowance = u32::try_from(crate::player::ALLOWANCE).unwrap_or(10);
        let left = u32::try_from(p.days_left.max(0))
            .unwrap_or(0)
            .min(allowance);
        let spans = vec![
            Span::styled(if pal.ascii { "* " } else { "▲ " }, pal.amber),
            Span::styled("LONG HAUL", pal.accent),
            Span::styled(format!("  {}", trunc(&p.captain, 14)), pal.strong),
            Span::styled(format!("  {} cr", credits(p.cash)), pal.plain),
            Span::styled(
                format!("  debt {}", credits(p.debt.principal)),
                if p.debt.missed > 0 { pal.bad } else { pal.dim },
            ),
            Span::styled("  days ", pal.dim),
            Span::styled(pips(left, allowance, pal.ascii), pal.amber),
            Span::styled(format!(" {left}"), pal.plain),
        ];
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    /// Key hints for the screen, at most two lines.
    fn screen_keys(&self, screen: Screen, pal: &Pal, width: u16) -> Vec<Line<'static>> {
        let docked = self.player.as_ref().is_some_and(|p| p.docked.is_some());
        let items: Vec<(&str, &str)> = match screen {
            Screen::Bridge => {
                let mut v = vec![("v", " chart"), ("j", " route")];
                if docked {
                    v.extend([
                        ("m", " market"),
                        ("b", " jobs"),
                        ("y", " yard"),
                        ("l", " loan"),
                        ("f", " fuel"),
                        ("r", " repair"),
                        ("u", " undock"),
                    ]);
                } else {
                    v.push(("d", " dock"));
                }
                v.extend([("c", " crew"), ("a", " standing"), ("n", " news")]);
                if self
                    .player
                    .as_ref()
                    .is_some_and(|p| self.frontier.system(p.at).is_some_and(|s| s.gas_giant))
                {
                    v.push(("s", " skim"));
                }
                v.push(("q", " leave"));
                v
            }
            Screen::Chart => return self.chart_keys(pal, width),
            Screen::Route => return self.route_keys(pal, width),
            Screen::Event => return self.event_keys(pal, width),
            Screen::Combat => return self.combat_keys(pal, width),
            Screen::Boarding => return self.boarding_keys(pal, width),
            Screen::Market => vec![
                ("b/B", " buy 1/10"),
                ("a", " fill"),
                ("s/S", " sell 1/10"),
                ("z", " sell all"),
                ("Esc", " back"),
            ],
            Screen::Jobs => vec![
                ("Enter", " take"),
                ("a", " give up first"),
                ("Esc", " back"),
            ],
            Screen::Crew => vec![
                ("Enter", " hire"),
                ("x", " pay off"),
                ("p", " stock galley"),
                ("Esc", " back"),
            ],
            Screen::Docks => vec![("arrows", " choose"), ("Enter", " dock"), ("Esc", " back")],
            Screen::Yard => vec![("arrows", " choose"), ("Enter", " do it"), ("Esc", " back")],
            Screen::Loan => vec![
                ("b/B", " borrow 1k/5k"),
                ("p/P", " pay 500/5k"),
                ("a", " pay all"),
                ("Esc", " back"),
            ],
            Screen::News | Screen::Standing | Screen::Intro => vec![("Esc", " back")],
        };
        key_lines(pal, &items, width, 2)
    }

    /// Where the ship is, in words.
    pub(super) fn place(&self) -> String {
        let Some(p) = &self.player else {
            return String::new();
        };
        match (
            self.frontier.system(p.at),
            p.docked.and_then(|s| self.frontier.station(s)),
        ) {
            (Some(sys), Some(st)) => format!(
                "Docked at {} [{}], {} system",
                st.name,
                self.content.economies[usize::from(st.economy)].name,
                sys.name
            ),
            (Some(sys), None) => format!(
                "In {} ({}, {})",
                sys.name,
                sys.star.name(),
                sys.hazard.name()
            ),
            _ => "Adrift".to_owned(),
        }
    }
}
