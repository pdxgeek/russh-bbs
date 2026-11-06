//! The society screen (M4-L4): convoys, piracy, outposts and the hall, as tabs. The lines are
//! plain text so the same view is the look-around and the small-window fallback; the full
//! screen draws them in a panel. Nothing here holds a rule: keys become [`Action`]s.

use russh_bbs_door::{Ctx, DoorKey, DoorStyle};

use super::{Mode, Session, cr};
use crate::engine::Action;
use crate::social::{self, CONVOY_MAX, OUTPOST_MIN_DAY, Social};
use crate::socialplay::Say;

/// The tabs of the society screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Convoy,
    Piracy,
    Outposts,
    Hall,
    Journal,
}

impl Tab {
    pub(crate) const ALL: [Self; 5] = [
        Self::Convoy,
        Self::Piracy,
        Self::Outposts,
        Self::Hall,
        Self::Journal,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Convoy => "Convoy",
            Self::Piracy => "Piracy",
            Self::Outposts => "Outposts",
            Self::Hall => "Hall",
            Self::Journal => "Journal",
        }
    }

    fn step(self, by: isize) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        let n = isize::try_from(Self::ALL.len()).unwrap_or(1);
        let next = (isize::try_from(i).unwrap_or(0) + by).rem_euclid(n);
        Self::ALL[usize::try_from(next).unwrap_or(0)]
    }
}

type Lines = Vec<(String, DoorStyle)>;

fn line(text: impl Into<String>, style: DoorStyle) -> (String, DoorStyle) {
    (text.into(), style)
}

impl Session {
    pub(super) fn society_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let Mode::Society(tab, cursor, input) = &mut self.mode else {
            return;
        };
        // Typing a line to the convoy takes every key until Enter or Escape.
        if let Some(buf) = input {
            match key {
                DoorKey::Escape => *input = None,
                DoorKey::Backspace => {
                    buf.pop();
                }
                DoorKey::Char(c) if buf.chars().count() < 140 => buf.push(c),
                DoorKey::Enter => {
                    let raw = std::mem::take(buf);
                    *input = None;
                    match ctx.player_text(&raw) {
                        Ok(clean) => {
                            self.act(ctx, &Action::ConvoySay(Say::from_sanitized(clean.as_str())));
                        }
                        Err(e) => self.push(e.to_string(), DoorStyle::Bad),
                    }
                }
                _ => {}
            }
            return;
        }
        match key {
            DoorKey::Escape | DoorKey::Char('q') => {
                self.mode = Mode::Bridge;
                return;
            }
            DoorKey::Left => *tab = tab.step(-1),
            DoorKey::Right | DoorKey::Char('\t') => *tab = tab.step(1),
            DoorKey::Up => *cursor = cursor.saturating_sub(1),
            DoorKey::Down => *cursor += 1,
            _ => {}
        }
        let (tab, cursor) = (*tab, *cursor);
        let Some(player) = &self.player else { return };
        let me = player.social.handle.clone();
        match (tab, key) {
            (Tab::Convoy, DoorKey::Char('n')) => {
                self.act(ctx, &Action::ConvoyForm);
            }
            (Tab::Convoy, DoorKey::Enter) if player.social.convoy == 0 => {
                let open: Vec<u32> = self
                    .world
                    .social
                    .convoys
                    .iter()
                    .filter(|c| c.members.len() < CONVOY_MAX)
                    .map(|c| c.id)
                    .collect();
                if let Some(id) = open.get(cursor.min(open.len().saturating_sub(1))) {
                    self.act(ctx, &Action::ConvoyJoin(*id));
                }
            }
            (Tab::Convoy, DoorKey::Char('x')) => {
                self.act(ctx, &Action::ConvoyLeave);
            }
            (Tab::Convoy, DoorKey::Char('t')) if player.social.convoy != 0 => {
                self.mode = Mode::Society(tab, cursor, Some(String::new()));
            }
            (Tab::Convoy, DoorKey::Char(d @ '1'..='8')) => {
                let n = u8::try_from(u32::from(d) - u32::from('1')).unwrap_or(0);
                self.act(ctx, &Action::ConvoyNote(n));
            }
            (Tab::Piracy, DoorKey::Char('f')) => {
                let on = !player.social.flagged;
                self.act(ctx, &Action::Flag(on));
            }
            (Tab::Piracy, DoorKey::Enter) => {
                let others: Vec<String> = self
                    .world
                    .social
                    .roster
                    .iter()
                    .filter(|p| p.handle != me)
                    .map(|p| p.handle.clone())
                    .collect();
                if let Some(h) = others.get(cursor.min(others.len().saturating_sub(1))) {
                    self.act(ctx, &Action::Intercept(h.clone()));
                }
            }
            (Tab::Outposts, DoorKey::Char(c @ ('p' | 'P'))) => {
                let amount = if c == 'p' { 500 } else { 5_000 };
                let system = player.at;
                self.act(ctx, &Action::Pledge { system, amount });
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn society_lines(
        &self,
        tab: Tab,
        cursor: usize,
        input: Option<&str>,
        height: usize,
    ) -> Lines {
        let Some(p) = &self.player else {
            return vec![line("No captain.", DoorStyle::Plain)];
        };
        let mut out: Lines = Vec::new();
        let tabs: Vec<String> = Tab::ALL
            .iter()
            .map(|t| {
                if *t == tab {
                    format!("[{}]", t.name())
                } else {
                    format!(" {} ", t.name())
                }
            })
            .collect();
        out.push(line(tabs.join(" "), DoorStyle::Accent));
        out.push(line(
            "The server can read all of this. Other captains see only what is marked shared.",
            DoorStyle::Dim,
        ));
        let soc = &self.world.social;
        let today = self.world.day;
        match tab {
            Tab::Convoy => {
                if let Some(c) = soc.convoy(p.social.convoy) {
                    out.push(line(
                        format!(
                            "{} convoy, {} of {} captains, guard {}%, pool {} cr",
                            c.name,
                            c.members.len(),
                            CONVOY_MAX,
                            c.guard_for(&p.social.handle, today),
                            cr(c.pool)
                        ),
                        DoorStyle::Strong,
                    ));
                    for m in &c.members {
                        let fresh = today - m.seen <= social::ACTIVE_DAYS;
                        out.push(line(
                            format!(
                                "  {} {}  guns {}  earned {} cr",
                                if fresh { "*" } else { "." },
                                m.name,
                                m.power,
                                cr(m.earned)
                            ),
                            if fresh {
                                DoorStyle::Plain
                            } else {
                                DoorStyle::Dim
                            },
                        ));
                    }
                    out.push(line(
                        format!(
                            "{}% of each profit goes to the pool; the night shares it out.",
                            social::POOL_PCT
                        ),
                        DoorStyle::Dim,
                    ));
                    for ev in c.events.iter().rev().take(2) {
                        if let Some(def) =
                            self.content.social.convoy_event.get(usize::from(ev.kind))
                        {
                            out.push(line(format!("Night: {}", def.title), DoorStyle::Plain));
                        }
                    }
                    out.push(line("Log:", DoorStyle::Dim));
                    for n in &c.notes {
                        out.push(line(format!("  {}: {}", n.who, n.text), DoorStyle::Plain));
                    }
                    if c.notes.is_empty() {
                        out.push(line("  (nothing yet)", DoorStyle::Dim));
                    }
                    if let Some(buf) = input {
                        out.push(line(format!("Say: {buf}_"), DoorStyle::Strong));
                    } else {
                        out.push(line("Quick notes:", DoorStyle::Dim));
                        for (i, t) in self.content.social.notes.iter().enumerate().take(8) {
                            out.push(line(format!("  {} {t}", i + 1), DoorStyle::Plain));
                        }
                    }
                } else {
                    out.push(line(
                        format!(
                            "Convoys: up to {CONVOY_MAX} captains who share risk and reward. Your \
                             ships need not be in the same place: the guns of the others cut your \
                             raider odds and the pool pays out while you sleep."
                        ),
                        DoorStyle::Plain,
                    ));
                    let open: Vec<_> = soc
                        .convoys
                        .iter()
                        .filter(|c| c.members.len() < CONVOY_MAX)
                        .collect();
                    if open.is_empty() {
                        out.push(line("No convoy has room. [n] forms one.", DoorStyle::Dim));
                    }
                    for (i, c) in open.iter().enumerate() {
                        let names: Vec<&str> = c.members.iter().map(|m| m.name.as_str()).collect();
                        out.push(line(
                            format!(
                                "{} {} ({}/{CONVOY_MAX}): {}",
                                if i == cursor.min(open.len() - 1) {
                                    ">"
                                } else {
                                    " "
                                },
                                c.name,
                                c.members.len(),
                                names.join(", ")
                            ),
                            if i == cursor.min(open.len() - 1) {
                                DoorStyle::Selected
                            } else {
                                DoorStyle::Plain
                            },
                        ));
                    }
                }
            }
            Tab::Piracy => {
                out.push(line(
                    if p.social.flagged {
                        format!(
                            "Your colours are up since season day {}. Infamy {}, renown {}.",
                            p.social.flagged_since - soc.born,
                            p.social.infamy,
                            p.social.renown
                        )
                    } else {
                        "Your colours are down. Nobody can hunt you, and you hunt nobody."
                            .to_owned()
                    },
                    if p.social.flagged {
                        DoorStyle::Bad
                    } else {
                        DoorStyle::Good
                    },
                ));
                out.push(line(
                    format!(
                        "Only flagged captains meet. Caps: {} hunts a day, {} hits on one target a \
                         day, {} days before the same pair meet again, prizes at most {} cr and {}% \
                         of the purse. An unflagged hauler is never touched, whatever it carries.",
                        social::INTERCEPTS_PER_DAY,
                        social::TARGET_PER_DAY,
                        social::PAIR_COOLDOWN_DAYS,
                        social::LOOT_CAP,
                        social::LOOT_PCT
                    ),
                    DoorStyle::Dim,
                ));
                out.push(line(
                    format!(
                        "Raids won {}, driven off {}, robbed {}, fought off {}.",
                        p.social.tally[0], p.social.tally[1], p.social.tally[2], p.social.tally[3]
                    ),
                    DoorStyle::Plain,
                ));
                let others: Vec<_> = soc
                    .roster
                    .iter()
                    .filter(|x| x.handle != p.social.handle)
                    .collect();
                out.push(line("Flagged captains:", DoorStyle::Dim));
                if others.is_empty() {
                    out.push(line("  none in the lanes", DoorStyle::Dim));
                }
                for (i, x) in others.iter().enumerate() {
                    let sel = i == cursor.min(others.len() - 1);
                    out.push(line(
                        format!(
                            "{} {}  strength {}  worth {}/4  infamy {}  seen {}d ago",
                            if sel { ">" } else { " " },
                            x.name,
                            x.strength(),
                            x.worth,
                            x.infamy,
                            today - x.seen
                        ),
                        if sel {
                            DoorStyle::Selected
                        } else {
                            DoorStyle::Plain
                        },
                    ));
                }
            }
            Tab::Outposts => {
                let day = soc.season_day(today);
                out.push(line(
                    if day >= OUTPOST_MIN_DAY {
                        format!(
                            "Players can fund new stations in charted systems with none. {} cr opens \
                             one; {} and {} raise it to level 2 and 3. Pledges are spent for good.",
                            cr(Social::cost_of(1)),
                            cr(Social::cost_of(2)),
                            cr(Social::cost_of(3))
                        )
                    } else {
                        format!(
                            "Outposts open on day {OUTPOST_MIN_DAY} of the season (this is day {day})."
                        )
                    },
                    DoorStyle::Plain,
                ));
                for o in &soc.outposts {
                    let sys = self
                        .frontier
                        .system(o.system)
                        .map_or("?", |s| s.name.as_str());
                    out.push(line(
                        format!(
                            "  {} ({sys}) level {} - {} cr pledged, {} t traded, founded by {}",
                            o.name,
                            o.level,
                            cr(o.fund),
                            o.traded,
                            o.founder
                        ),
                        DoorStyle::Good,
                    ));
                }
                for s in &soc.sites {
                    let sys = self
                        .frontier
                        .system(s.system)
                        .map_or("?", |s| s.name.as_str());
                    out.push(line(
                        format!(
                            "  site at {sys}: {} of {} cr, {} backers",
                            cr(s.fund),
                            cr(Social::cost_of(1)),
                            s.backers.len()
                        ),
                        DoorStyle::Accent,
                    ));
                }
                if soc.outposts.is_empty() && soc.sites.is_empty() {
                    out.push(line("  none yet", DoorStyle::Dim));
                }
                let here = self
                    .frontier
                    .system(p.at)
                    .map_or("here", |s| s.name.as_str());
                out.push(line(
                    format!("You are in {here}. [p] pledge 500 cr  [P] pledge 5,000 cr toward a station here."),
                    DoorStyle::Strong,
                ));
            }
            Tab::Journal => {
                out.push(line(
                    "Stories you have begun, and how they ended.",
                    DoorStyle::Dim,
                ));
                for (i, arc) in self.content.arcs.arc.iter().enumerate() {
                    let state = p.social.arcs[i];
                    let total = self
                        .content
                        .arcs
                        .event
                        .iter()
                        .filter(|e| e.arc == arc.id)
                        .count();
                    if let Some((title, text)) = state
                        .finished()
                        .then(|| self.content.ending_text(state.ending))
                        .flatten()
                    {
                        out.push(line(
                            format!("{} - finished: {title}", arc.title),
                            DoorStyle::Good,
                        ));
                        out.push(line(format!("  {text}"), DoorStyle::Dim));
                    } else if state.step == 0 {
                        out.push(line(format!("? {}", arc.hint), DoorStyle::Dim));
                    } else {
                        out.push(line(
                            format!("{} - step {} of {total}", arc.title, state.step + 1),
                            DoorStyle::Strong,
                        ));
                        out.push(line(format!("  {}", arc.blurb), DoorStyle::Plain));
                    }
                }
                for l in &p.social.legacy {
                    let title = self
                        .content
                        .ending_text(l.ending)
                        .map_or("unknown", |(t, _)| t);
                    out.push(line(
                        format!("Season {}: {title}, worth {} cr", l.season, cr(l.worth)),
                        DoorStyle::Accent,
                    ));
                }
            }
            Tab::Hall => {
                for (cat, title) in [
                    (0u8, "Richest captains"),
                    (1, "Best surveyors"),
                    (2, "Most feared"),
                ] {
                    out.push(line(title, DoorStyle::Dim));
                    let rows = soc.hall(cat);
                    if rows.is_empty() {
                        out.push(line("  nobody yet", DoorStyle::Dim));
                    }
                    for (i, r) in rows.iter().enumerate() {
                        out.push(line(
                            format!("  {}. {}  {}", i + 1, r.name, cr(r.value)),
                            DoorStyle::Plain,
                        ));
                    }
                }
                if let Some(g) = &soc.gates {
                    out.push(line(
                        format!(
                            "The gates were opened on day {} by {}.",
                            g.day - soc.born,
                            g.by
                        ),
                        DoorStyle::Good,
                    ));
                }
                for past in soc.past.iter().rev() {
                    let best = past
                        .records
                        .iter()
                        .filter(|r| r.category == 0)
                        .max_by_key(|r| r.value);
                    out.push(line(
                        format!(
                            "Season {}: richest {}{}",
                            past.season,
                            best.map_or("unknown", |r| r.name.as_str()),
                            past.gates
                                .as_ref()
                                .map_or(String::new(), |g| format!(", gates opened by {g}"))
                        ),
                        DoorStyle::Dim,
                    ));
                }
            }
        }
        out.truncate(height.max(4));
        out
    }
}
