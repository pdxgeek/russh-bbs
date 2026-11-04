//! The L2 screens: an open scene (event, fight, boarding), the crew list and the jobs board.
//! Minimal text, like the rest of the L1 screen; the full UI is slice L3. Each screen is a list
//! of lines (also the look-around view) and a set of numbered or single-key actions that map
//! straight to engine [`Action`]s. The screen holds no rules.

use russh_bbs_door::{Ctx, DoorKey, DoorStyle, Flow};

use super::{Mode, Session, WRAP, cr, wrap};
use crate::boarding::Drill;
use crate::combat::{Maneuver, Orders, Phase, Range};
use crate::engine::Action;
use crate::player::Player;
use crate::scene::Scene;
use crate::transit::Where;

type Line = (String, DoorStyle);

fn plain(text: impl Into<String>) -> Line {
    (text.into(), DoorStyle::Plain)
}

pub(super) fn band(range: Range) -> String {
    let mark = |r: Range| {
        if r == range {
            format!("[{}]", r.name().to_uppercase())
        } else {
            format!(" {} ", r.name())
        }
    };
    format!(
        "{}  --  {}  --  {}",
        mark(Range::Far),
        mark(Range::Mid),
        mark(Range::Close)
    )
}

/// The reactor split for a preset, as weapons, shields, engines.
pub(super) fn preset(budget: u8, which: usize) -> (u8, u8, u8) {
    let b = budget;
    match which {
        0 => (b, 0, 0),
        1 => {
            let w = (b * 3).div_ceil(5);
            (w, b - w, 0)
        }
        2 => {
            let third = b / 3;
            (third, third, b - 2 * third)
        }
        _ => (1.min(b), 0, b.saturating_sub(1)),
    }
}

const PRESETS: [&str; 4] = [
    "Full guns",
    "Guns and shields",
    "Balanced",
    "Evade (engines up)",
];

impl Session {
    /// The numbered choices of an open scene: label, action, and why it is closed (if it is).
    #[allow(
        clippy::too_many_lines,
        reason = "one flat match over the three scenes"
    )]
    pub(super) fn scene_menu(&self, player: &Player) -> Vec<(String, Action, Option<String>)> {
        let content = &self.content;
        let mut out = Vec::new();
        match &player.scene {
            None => {}
            Some(Scene::Event(ix)) => {
                let def = &content.transit.event[usize::from(*ix)];
                let at = Where {
                    content,
                    frontier: &self.frontier,
                    system: player.at,
                    fresh: false,
                };
                for (i, o) in def.options.iter().enumerate() {
                    out.push((
                        o.label.clone(),
                        Action::Choose(u8::try_from(i).unwrap_or(0)),
                        player.option_closed(&at, o),
                    ));
                }
            }
            Some(Scene::Combat(fight)) => {
                if fight.phase == Phase::Won {
                    out.push((
                        "Board the wreck".to_owned(),
                        Action::Wreck { board: true },
                        None,
                    ));
                    out.push(("Leave it".to_owned(), Action::Wreck { board: false }, None));
                } else {
                    let budget = player.gear(content).budget;
                    let armed = [Range::Far, Range::Mid, Range::Close]
                        .iter()
                        .any(|r| player.firepower(content, *r) > 0);
                    for (i, name) in PRESETS.iter().enumerate() {
                        let (guns, shields, engines) = preset(budget, i);
                        let closed =
                            (!armed && guns > 0 && i == 0).then(|| "no weapons aboard".to_owned());
                        out.push((
                            format!("{name} (guns {guns}, shields {shields}, engines {engines})"),
                            Action::Fight(Orders {
                                weapons: guns,
                                shields,
                                engines,
                                maneuver: self.maneuver,
                            }),
                            closed,
                        ));
                    }
                    out.push(("Run".to_owned(), Action::Run, None));
                }
            }
            Some(Scene::Boarding(party)) => {
                let room = party.room(content);
                let def = party.def(content);
                let foes = party.foes_here(content);
                if foes > 0 {
                    out.push(("Fight".to_owned(), Action::Skirmish(Drill::Fight), None));
                    out.push((
                        "Patch the most hurt".to_owned(),
                        Action::Skirmish(Drill::Patch),
                        None,
                    ));
                } else {
                    for &to in &room.exits {
                        let target = &def.rooms[usize::from(to)];
                        let open = target.lock == 0 || party.opened & (1u16 << u32::from(to)) != 0;
                        if open {
                            out.push((
                                format!("Go to {}", target.name),
                                Action::Skirmish(Drill::Move(to)),
                                None,
                            ));
                        } else {
                            out.push((
                                format!(
                                    "Cut open the door to {} (lock {})",
                                    target.name, target.lock
                                ),
                                Action::Skirmish(Drill::Breach(to)),
                                None,
                            ));
                        }
                    }
                    if room.cache > 0 && party.read & (1u16 << u32::from(party.at)) == 0 {
                        out.push((
                            "Search for a data cache".to_owned(),
                            Action::Skirmish(Drill::Search),
                            None,
                        ));
                    }
                    if party.looted & (1u16 << u32::from(party.at)) == 0
                        && (room.credits[1] > 0 || room.tons > 0 || room.parts > 0)
                    {
                        out.push((
                            "Take what is here".to_owned(),
                            Action::Skirmish(Drill::Take),
                            None,
                        ));
                    }
                    if party.party.iter().any(|m| m.up() && m.hp < m.max) {
                        out.push((
                            "Patch the most hurt".to_owned(),
                            Action::Skirmish(Drill::Patch),
                            None,
                        ));
                    }
                }
                out.push((
                    "Pull out with what you have".to_owned(),
                    Action::Skirmish(Drill::Withdraw),
                    None,
                ));
            }
        }
        out
    }

    pub(super) fn scene_lines(&self, p: &Player) -> Vec<Line> {
        let content = &self.content;
        let mut out = vec![plain("")];
        match &p.scene {
            None => {}
            Some(Scene::Event(ix)) => {
                let def = &content.transit.event[usize::from(*ix)];
                out.push((def.title.to_uppercase(), DoorStyle::Accent));
                out.extend(wrap(&def.text, WRAP).into_iter().map(plain));
            }
            Some(Scene::Combat(c)) => {
                let def = c.def(content);
                let max_hull = content
                    .hulls
                    .get(usize::from(p.ship.hull))
                    .map_or(1, |h| h.hull_points);
                out.push((
                    format!(
                        "COMBAT: {} ({})   round {}",
                        def.name,
                        def.faction.name(),
                        c.round
                    ),
                    DoorStyle::Accent,
                ));
                out.push(plain(format!(
                    "Enemy hull {}/{} shield {}   You hull {}/{}",
                    c.e_hull.max(0),
                    def.hull,
                    c.e_shield.max(0),
                    p.ship.hull_hp,
                    max_hull
                )));
                out.push(plain(format!("Range: {}", band(c.range))));
                if c.phase == Phase::Won {
                    out.push(("The enemy is a wreck, venting.".to_owned(), DoorStyle::Good));
                } else {
                    out.push((format!("It is {}.", c.shown.telegraph()), DoorStyle::Strong));
                    let gear = p.gear(content);
                    out.push(plain(format!(
                        "Reactor {} points.  Manoeuvre: {}  ([c]lose [o]pen [h]old)",
                        gear.budget,
                        match self.maneuver {
                            Maneuver::Hold => "hold",
                            Maneuver::Close => "close",
                            Maneuver::Open => "open",
                        }
                    )));
                }
            }
            Some(Scene::Boarding(b)) => {
                let def = b.def(content);
                let room = b.room(content);
                out.push((format!("BOARDING: {}", def.name), DoorStyle::Accent));
                out.push(("Room: ".to_owned() + &room.name, DoorStyle::Strong));
                out.extend(wrap(&room.text, WRAP).into_iter().map(plain));
                let foes = b.foes_here(content);
                if foes > 0 {
                    out.push((format!("{} {} here.", foes, def.foe_name), DoorStyle::Bad));
                }
                let party: Vec<String> = b
                    .party
                    .iter()
                    .map(|m| {
                        let name = m
                            .crew
                            .and_then(|i| p.crew.get(usize::from(i)))
                            .map_or("Captain", |c| c.name.split(' ').next().unwrap_or("?"));
                        format!("{name} {}/{}", m.hp.max(0), m.max)
                    })
                    .collect();
                out.push(plain(format!("Party: {}", party.join(", "))));
                out.push(plain(format!(
                    "Taken so far: {} cr, {} parts, {} survey",
                    b.haul.credits, b.haul.parts, b.haul.survey
                )));
            }
        }
        out.push(plain(""));
        for (i, (label, _, closed)) in self.scene_menu(p).iter().enumerate() {
            match closed {
                None => out.push((format!("[{}] {label}", i + 1), DoorStyle::Strong)),
                Some(why) => out.push((
                    format!("[{}] {label}  (closed: {why})", i + 1),
                    DoorStyle::Dim,
                )),
            }
        }
        out
    }

    pub(super) fn scene_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        if self.combat_key(key, ctx) {
            return Flow::Stay;
        }
        let DoorKey::Char(c) = key else {
            return Flow::Stay;
        };
        match c.to_ascii_lowercase() {
            'q' => return Flow::Exit,
            'c' => self.maneuver = Maneuver::Close,
            'o' => self.maneuver = Maneuver::Open,
            'h' => self.maneuver = Maneuver::Hold,
            d @ '1'..='9' => {
                let n = usize::from(d as u8 - b'1');
                let Some(player) = &self.player else {
                    return Flow::Stay;
                };
                if let Some((_, action, _)) = self.scene_menu(player).get(n).cloned() {
                    self.act(ctx, &action);
                }
            }
            'r' => {
                if matches!(
                    self.player.as_ref().and_then(|p| p.scene.as_ref()),
                    Some(Scene::Combat(_))
                ) {
                    self.act(ctx, &Action::Run);
                }
            }
            _ => {}
        }
        Flow::Stay
    }

    // ------------------------------------------------------------------ crew

    #[allow(clippy::too_many_lines, reason = "one list of crew lines")]
    pub(super) fn crew_lines(&self, p: &Player, cursor: usize) -> Vec<Line> {
        let content = &self.content;
        let mut out = vec![plain("")];
        out.push((
            format!(
                "CREW  {}/{} berths   wages {} cr a week   rations {}",
                p.berths_used(),
                p.berths(content),
                cr(p.weekly_wages()),
                if p.rations_until >= p.last_day {
                    format!("to day {}", p.rations_until - p.last_day)
                } else {
                    "none".to_owned()
                }
            ),
            DoorStyle::Accent,
        ));
        if p.crew.is_empty() {
            out.push(plain("No crew. You fly alone."));
        }
        for (i, c) in p.crew.iter().enumerate() {
            let flag = if c.sworn {
                "sworn"
            } else if c.restless() {
                "RESTLESS"
            } else {
                ""
            };
            let hurt = if c.fit() {
                String::new()
            } else {
                format!(" hurt {}d", c.hurt_days)
            };
            let text = format!(
                "{} {} {} sk{} {} loy{} {}{}",
                if i == cursor { '>' } else { ' ' },
                c.name,
                c.role.name(),
                c.effective(),
                c.mood_word(),
                c.loyalty,
                flag,
                hurt
            );
            out.push((
                text,
                if i == cursor {
                    DoorStyle::Selected
                } else if c.restless() {
                    DoorStyle::Bad
                } else {
                    DoorStyle::Plain
                },
            ));
            if i == cursor {
                out.push((
                    format!(
                        "    Goal: {} ({})",
                        c.goal_text(content, &self.frontier),
                        c.goal_progress()
                    ),
                    DoorStyle::Dim,
                ));
            }
        }
        if let Some(station) = p.docked {
            out.push(plain(""));
            let list = p.recruit_list(content, &self.frontier, station, p.last_day);
            if list.is_empty() {
                out.push((
                    "No one is looking for a berth here today.".to_owned(),
                    DoorStyle::Dim,
                ));
            }
            for (j, (_, r)) in list.iter().enumerate() {
                let row = p.crew.len() + j;
                out.push((
                    format!(
                        "{} For hire: {} {} sk{} {} cr/wk (fee {})",
                        if row == cursor { '>' } else { ' ' },
                        r.name,
                        r.role.name(),
                        r.skill,
                        r.wage,
                        u32::from(r.wage) * 2
                    ),
                    if row == cursor {
                        DoorStyle::Selected
                    } else {
                        DoorStyle::Plain
                    },
                ));
                if row == cursor {
                    out.push((
                        format!("    Wants: {}", r.goal_text(content, &self.frontier)),
                        DoorStyle::Dim,
                    ));
                }
            }
        }
        out.push((
            "[Up/Down] choose  [Enter] hire  [x] pay off  [p] stock galley  [Esc] back".to_owned(),
            DoorStyle::Strong,
        ));
        out
    }

    pub(super) fn crew_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let Mode::Crew(cursor) = &mut self.mode else {
            return;
        };
        let Some(p) = &self.player else { return };
        let list = p
            .docked
            .map(|s| p.recruit_list(&self.content, &self.frontier, s, p.last_day))
            .unwrap_or_default();
        let total = p.crew.len() + list.len();
        if Self::list_cursor(key, cursor, total) {
            return;
        }
        let at = *cursor;
        let crew_len = p.crew.len();
        match key {
            DoorKey::Escape | DoorKey::Char('q') => self.mode = Mode::Bridge,
            DoorKey::Enter if at >= crew_len => {
                if let Some((slot, _)) = list.get(at - crew_len) {
                    let action = Action::Hire(*slot);
                    self.act(ctx, &action);
                }
            }
            DoorKey::Char('x') if at < crew_len => {
                self.act(ctx, &Action::Dismiss(u8::try_from(at).unwrap_or(0)));
                if let Mode::Crew(c) = &mut self.mode {
                    *c = 0;
                }
            }
            DoorKey::Char('p') => {
                self.act(ctx, &Action::Provision);
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ jobs

    pub(super) fn jobs_lines(&self, p: &Player, cursor: usize) -> Vec<Line> {
        let content = &self.content;
        let mut out = vec![plain("")];
        out.push((
            format!("JOBS  {} of 4 carried", p.contracts.len()),
            DoorStyle::Accent,
        ));
        for c in &p.contracts {
            let to = self.frontier.station(c.to).map_or("?", |s| s.name.as_str());
            out.push(plain(format!(
                "  {} for the {} to {}, {} cr, due day {}",
                c.kind.name(),
                c.faction.name(),
                to,
                cr(i64::from(c.reward)),
                c.due - p.last_day
            )));
        }
        out.push(plain(""));
        let offers = self.offers(p);
        if offers.is_empty() {
            out.push(("Nothing posted for you here.".to_owned(), DoorStyle::Dim));
        }
        for (i, o) in offers.iter().enumerate() {
            let c = &o.contract;
            let to = self.frontier.station(c.to).map_or("?", |s| s.name.as_str());
            let what = match c.kind {
                crate::contracts::Kind::Delivery => {
                    format!("{} t {}", c.tons, content.goods[usize::from(c.good)].name)
                }
                crate::contracts::Kind::Passenger => format!(
                    "{} {}",
                    c.count,
                    content.jobs.passenger[usize::from(c.pax)].name
                ),
                crate::contracts::Kind::Bounty => {
                    format!("wreck a {}", content.enemies.enemy[usize::from(c.pax)].name)
                }
                crate::contracts::Kind::Escort => "escort".to_owned(),
                crate::contracts::Kind::Survey => format!(
                    "survey {}",
                    self.frontier
                        .system(c.target)
                        .map_or("?", |s| s.name.as_str())
                ),
            };
            let lock = o
                .locked
                .map_or(String::new(), |need| format!("  (needs standing {need:+})"));
            out.push((
                format!(
                    "{} {} {}: {} -> {} {} cr{}",
                    if i == cursor { '>' } else { ' ' },
                    c.faction.name(),
                    c.kind.name(),
                    what,
                    to,
                    cr(i64::from(c.reward)),
                    lock
                ),
                if i == cursor {
                    DoorStyle::Selected
                } else if o.locked.is_some() {
                    DoorStyle::Dim
                } else {
                    DoorStyle::Plain
                },
            ));
            if i == cursor {
                for line in wrap(&o.line.replace("{to}", to), WRAP) {
                    out.push((format!("    {line}"), DoorStyle::Dim));
                }
            }
        }
        out.push((
            "[Up/Down] choose  [Enter] take  [a] give up the first job  [Esc] back".to_owned(),
            DoorStyle::Strong,
        ));
        out
    }

    pub(super) fn offers(&self, p: &Player) -> Vec<crate::contracts::Offer> {
        let Some(station) = p.docked else {
            return Vec::new();
        };
        crate::contracts::board(
            &self.content,
            &self.frontier,
            station,
            p.last_day,
            &p.standing,
        )
        .into_iter()
        .filter(|o| !p.taken.contains(&o.key))
        .collect()
    }

    pub(super) fn jobs_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let Mode::Jobs(mut at) = self.mode else {
            return;
        };
        let Some(p) = &self.player else { return };
        let n = self.offers(p).len();
        if Self::list_cursor(key, &mut at, n) {
            self.mode = Mode::Jobs(at);
            return;
        }
        match key {
            DoorKey::Escape | DoorKey::Char('q') => self.mode = Mode::Bridge,
            DoorKey::Enter => {
                if let Some(offer) = self.offers(p).get(at) {
                    let key = offer.key;
                    self.act(ctx, &Action::Accept(key));
                }
            }
            DoorKey::Char('a') => {
                self.act(ctx, &Action::Abandon(0));
            }
            _ => {}
        }
    }
}
