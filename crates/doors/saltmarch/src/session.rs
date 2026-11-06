//! A minimal text screen over the engine. It exists so the door can be opened, replayed and
//! driven by simulated players; the real screens (map, plot grid, event scenes) are the UI
//! slice. Every line is plain text, so the same lines are both the drawing and the
//! look-around view, and a bot can read them.

use std::sync::{Arc, Mutex};

use russh_bbs_door::{Canvas, Ctx, DoorEnv, DoorKey, DoorSession, DoorStyle, Flow};

use crate::clan::{Clan, NAME_MAX, Settings};
use crate::content::Catalog;
use crate::engine::{self, Action, Env};
use crate::event::Event;
use crate::hold::PLOTS;
use crate::map::Map;
use crate::social::dispatch;
use crate::social::ledger::Acks;
use crate::social::{Posted, renown};
use crate::tide::{DayTable, Tide, Warning};
use crate::types::{Resource, Track, Trade};
use crate::world::World;

mod social;
mod trip_ui;
mod ui;

use social::{Soc, SocialUi};
use trip_ui::{Edit, Plan};

use crate::trip::{Reach, ReachKey};

/// The most menu entries (one key each, `a` to `z`).
const MENU_MAX: usize = 26;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuId {
    Main,
    Hold,
    Crew,
    Market,
    Tides,
    Plan,
    Trip,
    /// The marsh chart, seen from the hold.
    Map,
    /// The Tidecourt: the town square.
    Court,
    /// The post.
    Letters,
    /// The league table.
    League,
    /// The Hall of Tides.
    Hall,
    /// The exchange: the player market.
    Exchange,
    /// The gate: raids.
    Raid,
    /// Pacts and diplomacy.
    Pacts,
    /// Writing a letter.
    Compose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    /// The save or the world could not be read; nothing is written.
    Unreadable,
    /// Choosing a clan name.
    Naming(String),
    /// Reading the morning report.
    Report,
    /// In a menu.
    Menu(MenuId),
    /// Typing the line of a letter.
    Line(String),
}

impl Mode {
    /// Whether this mode takes typed text, so the board passes every key (a `?` included) on.
    const fn takes_text(&self) -> bool {
        matches!(self, Self::Naming(_) | Self::Line(_))
    }
}

#[cfg(test)]
mod mode_tests {
    use super::{MenuId, Mode};

    #[test]
    fn only_the_typing_modes_take_text() {
        assert!(Mode::Naming(String::new()).takes_text());
        assert!(Mode::Line(String::new()).takes_text());
        assert!(!Mode::Report.takes_text());
        assert!(!Mode::Unreadable.takes_text());
        assert!(!Mode::Menu(MenuId::Main).takes_text());
    }
}

enum Act {
    Go(MenuId),
    Do(Action),
    Edit(Edit),
    Soc(Soc),
    Leave,
}

struct Entry {
    tag: String,
    text: String,
    act: Act,
}

fn entry(tag: impl Into<String>, text: impl Into<String>, act: Act) -> Entry {
    Entry {
        tag: tag.into(),
        text: text.into(),
        act,
    }
}

/// What `open` worked out; see `Saltmarch::open`.
pub(crate) struct Setup {
    pub(crate) cat: Arc<Catalog>,
    pub(crate) settings: Settings,
    pub(crate) world: World,
    pub(crate) world_stored: bool,
    pub(crate) clan: Option<Clan>,
    pub(crate) today: i64,
    pub(crate) unreadable: bool,
    /// The clan was carried into a new season by `open`, and nothing is saved yet.
    pub(crate) rolled: bool,
    pub(crate) default_name: String,
}

pub(crate) struct Session {
    cat: Arc<Catalog>,
    settings: Settings,
    world: World,
    /// Whether the stored world is this one (else the first key publishes it).
    world_stored: bool,
    clan: Option<Clan>,
    map: Map,
    tide: Tide,
    today: i64,
    mode: Mode,
    notes: Vec<(String, DoorStyle)>,
    report: Vec<String>,
    dirty: bool,
    plan: Plan,
    /// The lightbar: an index into the menu's entries.
    cursor: usize,
    /// What the marsh screen was last showing (free, a scene, a fight), so the lightbar starts
    /// at the top of a new kind of screen.
    marsh_phase: u8,
    /// The crew member on the roster's highlight.
    crew_sel: usize,
    /// How far the marsh chart is panned from the hold, in tiles.
    pan: (i16, i16),
    /// What the nights brought, for the morning report's dashboard.
    ledger: Vec<Event>,
    /// The renown last written to the league table.
    scored: u32,
    /// What the clan last showed the marsh (its posted defence), as the world has it.
    posted: Posted,
    /// What to tell the world it may now forget, once the fold is saved.
    acks: Acks,
    /// The social screens' state.
    soc: SocialUi,
    /// The last search of the marsh, kept while the party stands still.
    reach: Mutex<Option<(ReachKey, Arc<Reach>)>>,
}

/// What happened while the player was away, one line each, led by how many nights passed.
fn morning_report(events: &[Event]) -> Vec<String> {
    let nights = events
        .iter()
        .filter(|e| matches!(e, Event::Night { .. }))
        .count();
    let news = events.iter().any(|e| !matches!(e, Event::Night { .. }));
    if nights == 0 && !news {
        return Vec::new();
    }
    let mut lines = vec![if nights == 0 {
        "News from the Tidecourt.".to_owned()
    } else {
        format!(
            "{nights} night{} passed.",
            if nights == 1 { "" } else { "s" }
        )
    }];
    lines.extend(
        events
            .iter()
            .filter(|e| !matches!(e, Event::Night { .. }))
            .map(ToString::to_string),
    );
    lines
}

impl Session {
    pub(crate) fn new(setup: Setup) -> Self {
        let Setup {
            cat,
            settings,
            mut world,
            world_stored,
            mut clan,
            today,
            unreadable,
            rolled,
            default_name,
        } = setup;
        let map = Map::generate(world.seed, &cat.pois);
        let tide = Tide::new(world.seed);
        let mut report = Vec::new();
        let mut ledger = Vec::new();
        let mut acks = Acks::default();
        let mail = clan
            .as_ref()
            .map(|c| dispatch::mail_for(&world, &c.name))
            .unwrap_or_default();
        // The session's own view of the rolls always includes the clan itself.
        if let Some(c) = &clan {
            world.register_founded(&c.name, c.site, c.founded);
        }
        // Nothing is written until the first key; a provisional world or a new season is.
        let mut dirty = rolled || (!world_stored && clan.is_some());
        if let Some(c) = clan.as_mut() {
            if c.seed != world.seed {
                // Another session of this board made the season's world first: join its map.
                c.seed = world.seed;
                c.site = Clan::site_for(&c.name, map.sites.len());
                c.charted = vec![0; c.charted.len()];
                if let Some(&(x, y)) = map.sites.get(usize::from(c.site)) {
                    c.chart_around(usize::from(x), usize::from(y), 3);
                }
                dirty = true;
            }
            let env = Env {
                cat: &cat,
                map: &map,
                tide: &tide,
                prices: &world.prices,
                settings,
                day: today,
            };
            let (events, folded) = engine::catch_up_with(&env, c, today, &mail);
            dirty |= !events.is_empty() || !folded.is_empty();
            acks = folded;
            report = morning_report(&events);
            ledger = events;
        }
        // The session's view of its own hold is today's, not the world's older copy.
        if let Some(c) = clan.as_ref() {
            let rn = renown::of(&cat, c, &world, today).total();
            if let Some(i) = world.ix(&c.name) {
                world.clans[usize::from(i)].score = rn;
            }
        }
        let mode = if unreadable {
            Mode::Unreadable
        } else if clan.is_none() {
            Mode::Naming(default_name)
        } else if !report.is_empty() {
            Mode::Report
        } else if clan.as_ref().is_some_and(|c| c.trip.is_some()) {
            Mode::Menu(MenuId::Trip)
        } else {
            Mode::Menu(MenuId::Main)
        };
        Self {
            cat,
            settings,
            world,
            world_stored,
            clan,
            map,
            tide,
            today,
            mode,
            notes: Vec::new(),
            report,
            dirty,
            plan: Plan::default(),
            cursor: 0,
            marsh_phase: 0,
            crew_sel: 0,
            pan: (0, 0),
            ledger,
            scored: 0,
            posted: Posted::default(),
            acks,
            soc: SocialUi::default(),
            reach: Mutex::new(None),
        }
    }

    /// The clan's renown today: the number the league ranks by.
    fn renown(&self) -> renown::Renown {
        self.clan.as_ref().map_or_else(Default::default, |c| {
            renown::of(&self.cat, c, &self.world, self.today)
        })
    }

    fn env(&self) -> Env<'_> {
        Env {
            cat: &self.cat,
            map: &self.map,
            tide: &self.tide,
            prices: &self.world.prices,
            settings: self.settings,
            day: self.today,
        }
    }

    /// Publish the world and the clan's place in it, once, with the first key.
    fn publish(&mut self, ctx: &mut Ctx<'_>) {
        if self.world_stored {
            return;
        }
        self.world_stored = true;
        let (season, seed, day) = (self.world.season, self.world.seed, self.today);
        let who = self
            .clan
            .as_ref()
            .map(|c| (c.name.clone(), c.site, c.founded));
        ctx.edit_world(move |old| {
            let decoded = old.and_then(World::decode);
            let mut w = match decoded {
                Some(w) if w.season == season => w,
                // The first clan of a season makes its world, carrying the book of the last.
                other => World::successor(other.as_ref(), season, seed, day),
            };
            if let Some((name, site, founded)) = &who {
                w.register_founded(name, *site, *founded);
            }
            Some(w.encode())
        });
    }

    fn save(&mut self, ctx: &mut Ctx<'_>) {
        let Some(c) = &self.clan else { return };
        ctx.request_save(c.encode());
        // What the clan has folded the world may now forget (the save above is written first).
        if !self.acks.is_empty() {
            let acks = std::mem::take(&mut self.acks);
            let (season, name) = (self.world.season, c.name.clone());
            ctx.edit_world(move |old| dispatch::ack(old, season, &name, acks.upto, &acks.receipts));
        }
        // The league reads the renown the clan had when it was last saved, and raiders read
        // what it last posted (rounded, so the world is not rewritten for every shift).
        let rn = renown::of(&self.cat, c, &self.world, self.today).total();
        let posted = Posted::of(&self.cat, c, self.today);
        if rn != self.scored || posted != self.posted {
            self.scored = rn;
            self.posted = posted.clone();
            let (season, name) = (self.world.season, c.name.clone());
            if let Some(i) = self.world.ix(&name) {
                self.world.clans[usize::from(i)].score = rn;
                self.world.clans[usize::from(i)].posted = posted.clone();
            }
            ctx.edit_world(move |old| dispatch::post_update(old, season, &name, &posted, Some(rn)));
        }
    }

    fn found(&mut self, name: &str, ctx: &mut Ctx<'_>) {
        let site = Clan::site_for(name, self.map.sites.len());
        let (clan, events) = Clan::found(
            &self.cat,
            &self.map,
            name,
            self.world.season,
            site,
            self.today,
            ctx.rng(),
        );
        self.clan = Some(clan);
        self.world_stored = false;
        self.publish(ctx);
        self.save(ctx);
        self.notes = events
            .iter()
            .map(|e| (e.to_string(), DoorStyle::Good))
            .collect();
        self.mode = Mode::Menu(MenuId::Main);
    }

    fn sync_day(&mut self, ctx: &Ctx<'_>) {
        let today = ctx.today().as_raw();
        if today > self.today {
            self.today = today;
            if let Some(mut clan) = self.clan.take() {
                let events = engine::catch_up(&self.env(), &mut clan, today);
                self.report = morning_report(&events);
                self.ledger = events;
                self.dirty = true;
                self.clan = Some(clan);
                if matches!(self.mode, Mode::Menu(MenuId::Trip | MenuId::Plan)) {
                    self.mode = if self.report.is_empty() {
                        Mode::Menu(MenuId::Main)
                    } else {
                        Mode::Report
                    };
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn entries(&self) -> Vec<Entry> {
        let Mode::Menu(menu) = self.mode else {
            return Vec::new();
        };
        let Some(clan) = &self.clan else {
            return Vec::new();
        };
        let cat = &*self.cat;
        let mut out = Vec::new();
        match menu {
            MenuId::Main => {
                out.push(entry(
                    "go:hold",
                    "The hold: build, work, craft",
                    Act::Go(MenuId::Hold),
                ));
                out.push(entry(
                    "go:crew",
                    "The crew and the gate",
                    Act::Go(MenuId::Crew),
                ));
                out.push(entry(
                    "go:market",
                    "The Tidecourt market",
                    Act::Go(MenuId::Market),
                ));
                out.push(entry(
                    "go:expedition",
                    "Send a party out on the marsh",
                    Act::Go(MenuId::Plan),
                ));
                out.push(entry(
                    "go:tides",
                    "Today's tide table",
                    Act::Go(MenuId::Tides),
                ));
                out.push(entry(
                    "go:map",
                    "The marsh chart: charted ground and the tide",
                    Act::Go(MenuId::Map),
                ));
                out.push(entry(
                    "go:court",
                    "The Tidecourt: league, hall and the post",
                    Act::Go(MenuId::Court),
                ));
                out.push(entry(
                    "survey",
                    "Chart the marsh around the hold (2 turns)",
                    Act::Do(Action::Survey),
                ));
                out.push(entry("leave", "Leave Saltmarch", Act::Leave));
            }
            MenuId::Hold => {
                let free = (0..clan.hold.unlocked(cat)).find(|&p| {
                    clan.hold.plots[p].is_none()
                        && !clan
                            .hold
                            .building
                            .iter()
                            .flatten()
                            .any(|c| usize::from(c.plot) == p)
                });
                if let Some(p) = free {
                    let plot = u8::try_from(p).unwrap_or(0);
                    for def in &cat.buildings {
                        let have = clan.hold.count(&def.id)
                            + clan
                                .hold
                                .building
                                .iter()
                                .flatten()
                                .filter(|c| c.building == def.id)
                                .count();
                        if have < usize::from(def.max)
                            && clan.hold.building[def.track.index()].is_none()
                            && def.requires.iter().all(|r| clan.hold.has(r))
                            && clan.can_pay(&def.cost)
                        {
                            out.push(entry(
                                format!("build:{}@{plot}", def.id),
                                format!(
                                    "Build {} ({}; {} turns of labour)",
                                    def.name,
                                    def.cost.describe(),
                                    def.build_turns
                                ),
                                Act::Do(Action::Build {
                                    plot,
                                    building: def.id.clone(),
                                }),
                            ));
                        }
                    }
                }
                for track in Track::ALL {
                    if let Some(c) = &clan.hold.building[track.index()] {
                        out.push(entry(
                            format!("labour:{}", track.name()),
                            format!("Labour on the {} ({} left)", c.building, c.remaining),
                            Act::Do(Action::Labour { track }),
                        ));
                    }
                }
                for (plot, def) in clan.hold.built(cat) {
                    let plot = u8::try_from(plot).unwrap_or(0);
                    if def.produce.is_empty() {
                        continue;
                    }
                    let best = clan.crew.iter().filter(|c| c.fit(cat)).max_by_key(|c| {
                        (
                            c.power(cat, def.trade.unwrap_or(Trade::Wader)),
                            std::cmp::Reverse(c.id),
                        )
                    });
                    if let Some(c) = best {
                        out.push(entry(
                            format!("shift:{}@{plot}:{}", def.id, c.id),
                            format!("Shift at the {} with {} (2 turns)", def.name, c.name),
                            Act::Do(Action::Shift {
                                plot,
                                crew: vec![c.id],
                            }),
                        ));
                    }
                }
                for (plot, def) in clan.hold.built(cat) {
                    let plot = u8::try_from(plot).unwrap_or(0);
                    let Some(trade) = def.trade else { continue };
                    if def.produce.is_empty() {
                        continue;
                    }
                    let mut fit: Vec<_> = clan.crew.iter().filter(|c| c.fit(cat)).collect();
                    fit.sort_by_key(|c| (std::cmp::Reverse(c.power(cat, trade)), c.id));
                    if let [a, b, ..] = fit[..] {
                        out.push(entry(
                            format!("pair:{}@{plot}:{}+{}", def.id, a.id, b.id),
                            format!(
                                "Shift at the {} with {} and {} (2 turns)",
                                def.name, a.name, b.name
                            ),
                            Act::Do(Action::Shift {
                                plot,
                                crew: vec![a.id, b.id],
                            }),
                        ));
                    }
                }
                if clan.turns_left(&self.settings) >= 3 {
                    out.push(entry(
                        "wait:3",
                        "Wait three turns for the water to move (3 turns)",
                        Act::Do(Action::Wait { turns: 3 }),
                    ));
                }
                for r in &clan.recipes {
                    if let Some(def) = cat.recipe(r)
                        && clan.hold.has(&def.workshop)
                    {
                        out.push(entry(
                            format!("craft:{}", def.id),
                            format!(
                                "{} ({} -> {} {}; {} turns)",
                                def.name,
                                def.inputs.describe(),
                                def.qty,
                                def.output.name(),
                                def.turns
                            ),
                            Act::Do(Action::Craft {
                                recipe: def.id.clone(),
                            }),
                        ));
                    }
                }
                if clan.hold.built(cat).any(|(_, b)| b.is_sluice()) {
                    out.push(entry(
                        "sluice:60",
                        "Set the sluice to flood the approach at water 60",
                        Act::Do(Action::Sluice { level: Some(60) }),
                    ));
                    out.push(entry(
                        "sluice:off",
                        "Leave the sluice open",
                        Act::Do(Action::Sluice { level: None }),
                    ));
                }
            }
            MenuId::Crew => {
                for (i, c) in clan.candidates.iter().enumerate() {
                    out.push(entry(
                        format!("recruit:{i}"),
                        format!(
                            "Take on {}, {} (age {}), {} tallies",
                            c.name,
                            c.trade.name(),
                            c.age,
                            c.price(cat)
                        ),
                        Act::Do(Action::Recruit { candidate: i }),
                    ));
                }
                for c in &clan.crew {
                    if !c.injuries.is_empty() {
                        out.push(entry(
                            format!("tend:{}", c.id),
                            format!("Tend {}", c.name),
                            Act::Do(Action::Tend { crew: c.id }),
                        ));
                    }
                }
                for (plot, def) in clan.hold.built(cat) {
                    let plot = u8::try_from(plot).unwrap_or(0);
                    let Some(trade) = def.trade else { continue };
                    let held = clan.crew.iter().filter(|c| c.post == Some(plot)).count();
                    if def.slots > 0 && held < usize::from(def.slots) {
                        let pick = clan
                            .crew
                            .iter()
                            .filter(|c| c.post.is_none() && c.fit(cat))
                            .max_by_key(|c| (c.power(cat, trade), std::cmp::Reverse(c.id)));
                        if let Some(c) = pick {
                            out.push(entry(
                                format!("post:{}@{plot}", c.id),
                                format!("Post {} at the {}", c.name, def.name),
                                Act::Do(Action::Post {
                                    crew: c.id,
                                    plot: Some(plot),
                                }),
                            ));
                        }
                    }
                }
                for c in &clan.crew {
                    out.push(entry(
                        format!("train:{}:{}", c.id, c.trade.id()),
                        format!(
                            "Train {} in {} (2 turns, 3 tallies)",
                            c.name,
                            c.trade.name()
                        ),
                        Act::Do(Action::Train {
                            crew: c.id,
                            skill: c.trade,
                        }),
                    ));
                }
            }
            MenuId::Market => {
                for r in Resource::ALL {
                    let have = clan.stock.get(r);
                    if have > 0 {
                        out.push(entry(
                            format!("sell:{}:{have}", r.id()),
                            format!(
                                "Sell all {have} {} for {}",
                                r.name(),
                                World::sell_value(&self.world.prices, r, have)
                            ),
                            Act::Do(Action::Sell { what: r, qty: have }),
                        ));
                    }
                }
                for r in [
                    Resource::Food,
                    Resource::Timber,
                    Resource::Reed,
                    Resource::Peat,
                    Resource::Iron,
                    Resource::Salt,
                    Resource::Fish,
                ] {
                    out.push(entry(
                        format!("buy:{}:5", r.id()),
                        format!(
                            "Buy 5 {} for {}",
                            r.name(),
                            World::buy_cost(&self.world.prices, r, 5)
                        ),
                        Act::Do(Action::Buy { what: r, qty: 5 }),
                    ));
                }
            }
            MenuId::Tides | MenuId::Map | MenuId::League | MenuId::Hall => {}
            MenuId::Exchange | MenuId::Raid | MenuId::Pacts | MenuId::Letters | MenuId::Compose => {
                out = self.social_entries(menu, clan);
            }
            MenuId::Court => {
                out.push(entry(
                    "go:market",
                    "The market: prices, sales and purchases",
                    Act::Go(MenuId::Market),
                ));
                out.push(entry(
                    "go:exchange",
                    "The exchange: trade with other clans",
                    Act::Go(MenuId::Exchange),
                ));
                out.push(entry(
                    "go:raid",
                    "The gate: send a raid",
                    Act::Go(MenuId::Raid),
                ));
                out.push(entry(
                    "go:pacts",
                    "Pacts and diplomacy",
                    Act::Go(MenuId::Pacts),
                ));
                out.push(entry(
                    "go:letters",
                    if clan.social.unread() > 0 {
                        format!("The post: {} unread", clan.social.unread())
                    } else {
                        "The post: letters and notices".to_owned()
                    },
                    Act::Go(MenuId::Letters),
                ));
                out.push(entry(
                    "go:league",
                    "The league table",
                    Act::Go(MenuId::League),
                ));
                out.push(entry("go:hall", "The Hall of Tides", Act::Go(MenuId::Hall)));
                out.push(entry(
                    "go:tides",
                    "The quay: the tide table",
                    Act::Go(MenuId::Tides),
                ));
            }
            MenuId::Plan => out = self.plan_entries(clan),
            MenuId::Trip => out = self.trip_entries(clan),
        }
        out.truncate(MENU_MAX);
        out
    }

    fn tide_lines(&self, clan: &Clan) -> Vec<(String, DoorStyle)> {
        let env = self.env();
        let minute = env.minute(clan);
        let level = self.tide.level(minute);
        let (wait, high) = self.tide.next_turn(minute);
        let next_level = self.tide.level(minute + wait);
        let table: DayTable = self.tide.table(self.today);
        let warn = match self.tide.warning(level, self.today) {
            Warning::Calm => ("", DoorStyle::Plain),
            Warning::High => (" HIGH WATER", DoorStyle::Strong),
            Warning::Flood => (" FLOOD", DoorStyle::Bad),
            Warning::Storm => (" STORM SURGE", DoorStyle::Bad),
        };
        let mut out = vec![(
            format!(
                "Tide: water {level}, next {} in {}h{:02}m (water {next_level}), {} tides{}{}",
                if high { "high" } else { "low" },
                wait / 60,
                wait % 60,
                table.phase,
                if table.storm { ", storm today" } else { "" },
                warn.0,
            ),
            warn.1,
        )];
        if self.mode == Mode::Menu(MenuId::Tides) {
            for e in &table.extremes {
                out.push((
                    format!(
                        "  {} at {:02}:{:02}, water {}",
                        if e.high { "High" } else { "Low " },
                        e.minute / 60,
                        e.minute % 60,
                        e.level
                    ),
                    DoorStyle::Plain,
                ));
            }
        }
        out
    }

    #[allow(clippy::too_many_lines)]
    fn lines(&self) -> Vec<(String, DoorStyle)> {
        let mut out = Vec::new();
        match &self.mode {
            Mode::Unreadable => {
                out.push((
                    "Saltmarch cannot read your saved clan or the season's world.".to_owned(),
                    DoorStyle::Bad,
                ));
                out.push((
                    "Nothing was changed. Ask the sysop; Esc leaves.".to_owned(),
                    DoorStyle::Plain,
                ));
                return out;
            }
            Mode::Naming(buffer) => {
                out.push((
                    "Saltmarch - the sea took the old coast. Name your clan.".to_owned(),
                    DoorStyle::Accent,
                ));
                out.push((format!("Clan name: {buffer}_"), DoorStyle::Strong));
                out.push((
                    "Enter to stake your hold, Esc to leave.".to_owned(),
                    DoorStyle::Dim,
                ));
                out.extend(self.notes.iter().cloned());
                return out;
            }
            _ => {}
        }
        let Some(clan) = &self.clan else {
            return out;
        };
        let cat = &*self.cat;
        out.push((
            format!(
                "{} - day {} of season {}",
                clan.name,
                self.today - clan.founded + 1,
                clan.season
            ),
            DoorStyle::Accent,
        ));
        out.extend(self.tide_lines(clan));
        out.push((
            format!(
                "Turns {}/{}  Tallies {}  Morale {}  Crew {}/{}",
                clan.turns_left(&self.settings),
                self.settings.turns_per_day,
                clan.tallies,
                clan.morale,
                clan.crew.len(),
                clan.hold.crew_cap(cat),
            ),
            DoorStyle::Plain,
        ));
        let goods: Vec<String> = Resource::ALL
            .iter()
            .filter(|r| clan.stock.get(**r) > 0)
            .map(|r| format!("{} {}", r.name(), clan.stock.get(*r)))
            .collect();
        out.push((
            format!(
                "Stores: {}",
                if goods.is_empty() {
                    "empty".to_owned()
                } else {
                    goods.join(", ")
                }
            ),
            DoorStyle::Plain,
        ));
        out.push((String::new(), DoorStyle::Plain));
        match &self.mode {
            Mode::Report => {
                out.push(("While you were away:".to_owned(), DoorStyle::Strong));
                out.extend(
                    self.report
                        .iter()
                        .take(14)
                        .map(|r| (format!("  {r}"), DoorStyle::Plain)),
                );
                out.push(("Any key to go on.".to_owned(), DoorStyle::Dim));
            }
            Mode::Menu(MenuId::Crew) => {
                for c in &clan.crew {
                    let hurt = if c.injuries.is_empty() {
                        String::new()
                    } else {
                        " (hurt)".to_owned()
                    };
                    out.push((
                        format!(
                            "  {} - {} {}, {}{}",
                            c.id,
                            c.name,
                            c.trade.name(),
                            c.age,
                            hurt
                        ),
                        DoorStyle::Plain,
                    ));
                }
            }
            Mode::Menu(
                m @ (MenuId::Exchange
                | MenuId::Raid
                | MenuId::Pacts
                | MenuId::Letters
                | MenuId::Compose),
            ) => out.extend(self.social_lines(*m, clan)),
            Mode::Line(buffer) => out.push((format!("Your line: {buffer}_"), DoorStyle::Strong)),
            Mode::Menu(MenuId::Plan) => out.extend(self.plan_lines(clan)),
            Mode::Menu(MenuId::Trip) => out.extend(self.trip_lines(clan)),
            Mode::Menu(MenuId::Hold) => {
                let built: Vec<String> = clan
                    .hold
                    .built(cat)
                    .map(|(p, b)| format!("{p}:{}", b.id))
                    .collect();
                out.push((
                    format!(
                        "Plots {}/{}: {}",
                        clan.hold.unlocked(cat),
                        PLOTS,
                        built.join(" ")
                    ),
                    DoorStyle::Plain,
                ));
            }
            _ => {}
        }
        for (i, e) in self.entries().iter().enumerate() {
            let key = char::from(b'a' + u8::try_from(i).unwrap_or(0));
            out.push((format!("[{key}] {}  {}", e.tag, e.text), DoorStyle::Strong));
        }
        out.extend(self.notes.iter().cloned());
        out
    }
}

impl DoorSession for Session {
    #[allow(clippy::too_many_lines)]
    fn key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        self.sync_day(ctx);
        if self.dirty && self.clan.is_some() && self.mode != Mode::Unreadable {
            self.publish(ctx);
            self.save(ctx);
            self.dirty = false;
        }
        match self.mode.clone() {
            Mode::Unreadable => {
                return if key == DoorKey::Escape {
                    Flow::Exit
                } else {
                    Flow::Stay
                };
            }
            Mode::Naming(mut buffer) => {
                match key {
                    DoorKey::Char(c) if buffer.chars().count() < NAME_MAX => buffer.push(c),
                    DoorKey::Backspace => {
                        buffer.pop();
                    }
                    DoorKey::Escape => return Flow::Exit,
                    DoorKey::Enter => match ctx.player_name(&buffer) {
                        Ok(name) => {
                            self.found(name.as_str(), ctx);
                            return Flow::Stay;
                        }
                        Err(_) => {
                            self.notes =
                                vec![("That name is not allowed here.".to_owned(), DoorStyle::Bad)];
                        }
                    },
                    _ => {}
                }
                self.mode = Mode::Naming(buffer);
                return Flow::Stay;
            }
            Mode::Report => {
                self.mode = Mode::Menu(MenuId::Main);
                self.notes.clear();
                return Flow::Stay;
            }
            Mode::Line(mut buffer) => {
                match key {
                    DoorKey::Char(c)
                        if buffer.chars().count() < crate::social::letters::LINE_MAX =>
                    {
                        buffer.push(c);
                    }
                    DoorKey::Backspace => {
                        buffer.pop();
                    }
                    DoorKey::Escape => self.mode = Mode::Menu(MenuId::Compose),
                    DoorKey::Enter => {
                        self.take_line(&buffer, ctx);
                        return Flow::Stay;
                    }
                    _ => {}
                }
                if matches!(self.mode, Mode::Line(_)) {
                    self.mode = Mode::Line(buffer);
                }
                return Flow::Stay;
            }
            Mode::Menu(menu) => {
                if key == DoorKey::Escape {
                    if menu == MenuId::Main || menu == MenuId::Trip {
                        return Flow::Exit;
                    }
                    self.mode = Mode::Menu(match menu {
                        MenuId::Letters
                        | MenuId::League
                        | MenuId::Hall
                        | MenuId::Exchange
                        | MenuId::Raid
                        | MenuId::Pacts => MenuId::Court,
                        MenuId::Compose => MenuId::Letters,
                        _ => MenuId::Main,
                    });
                    self.cursor = 0;
                    self.notes.clear();
                    return Flow::Stay;
                }
                if menu == MenuId::Tides {
                    self.mode = Mode::Menu(MenuId::Main);
                    return Flow::Stay;
                }
                let entries = self.entries();
                let index = match key {
                    DoorKey::Char(c) => {
                        let Some(i) = u32::from(c.to_ascii_lowercase()).checked_sub(u32::from('a'))
                        else {
                            return Flow::Stay;
                        };
                        usize::try_from(i).unwrap_or(usize::MAX)
                    }
                    DoorKey::Enter => self.cursor,
                    _ => match self.arrow(menu, key, &entries) {
                        Some(walk) => walk,
                        None => return Flow::Stay,
                    },
                };
                let mut entries = entries;
                if index >= entries.len() {
                    return Flow::Stay;
                }
                self.cursor = index;
                match entries.swap_remove(index).act {
                    Act::Go(next) => {
                        if next == MenuId::Plan {
                            self.plan = Plan::default();
                        }
                        self.mode = Mode::Menu(next);
                        self.cursor = 0;
                        self.notes.clear();
                    }
                    Act::Edit(e) => self.edit(e),
                    Act::Soc(s) => self.soc_act(s, ctx),
                    Act::Leave => return Flow::Exit,
                    Act::Do(action) => self.act(action, ctx),
                }
            }
        }
        self.tidy_cursor();
        Flow::Stay
    }

    fn render(&self, canvas: &mut Canvas<'_>, env: &DoorEnv) {
        canvas.clear();
        ui::draw(self, canvas, env);
    }

    fn captures_text(&self) -> bool {
        self.mode.takes_text()
    }

    fn look_around(&self, _env: &DoorEnv) -> Vec<String> {
        let mut out: Vec<String> = self.lines().into_iter().map(|(text, _)| text).collect();
        out.extend(ui::describe(self));
        out
    }
}

impl Session {
    /// Keep the lightbar on an order that exists, and at the top when the marsh screen changes
    /// from walking to a scene or a skirmish (or back).
    fn tidy_cursor(&mut self) {
        let phase = self
            .clan
            .as_ref()
            .and_then(|c| c.trip.as_ref())
            .map_or(0, |t| match t.phase {
                crate::trip::Phase::Free => 1,
                crate::trip::Phase::Scene(_) => 2,
                crate::trip::Phase::Fight(_) => 3,
            });
        if phase != self.marsh_phase {
            self.marsh_phase = phase;
            self.cursor = 0;
        }
        if self.cursor >= self.entries().len() {
            self.cursor = 0;
        }
        // A crew member who left (retired, deserted, lost) leaves the highlight on the last.
        let crew = self.clan.as_ref().map_or(0, |c| c.crew.len());
        self.crew_sel = self.crew_sel.min(crew.saturating_sub(1));
        // A letter on the highlight is a letter read.
        if self.mode == Mode::Menu(MenuId::Letters)
            && let Some(c) = self.clan.as_mut()
        {
            let at = self.soc.letter.min(c.social.inbox.len().saturating_sub(1));
            self.soc.letter = at;
            if let Some(l) = c.social.inbox.get_mut(at)
                && !l.read
            {
                l.read = true;
                // Read is remembered: the next key saves, even if nothing else changed.
                self.dirty = true;
            }
        }
    }

    /// An arrow key: it moves what the screen has to move (the lightbar, the roster, the chart)
    /// or, on the marsh, takes a step.
    fn arrow(&mut self, menu: MenuId, key: DoorKey, entries: &[Entry]) -> Option<usize> {
        let (dx, dy) = match key {
            DoorKey::Up => (0, -1),
            DoorKey::Down => (0, 1),
            DoorKey::Left => (-1, 0),
            DoorKey::Right => (1, 0),
            _ => return None,
        };
        match menu {
            MenuId::Map => {
                let step = |v: i16, d: i16| (v + d * 4).clamp(-40, 40);
                self.pan = (step(self.pan.0, dx), step(self.pan.1, dy));
            }
            MenuId::Exchange | MenuId::Raid | MenuId::Pacts | MenuId::Compose if dx != 0 => {
                // Left and right turn the page: the next good, target or recipient.
                let tag = match (menu, dx > 0) {
                    (MenuId::Exchange, true) => "good:next",
                    (MenuId::Exchange, false) => "good:prev",
                    (MenuId::Raid, true) => "target:next",
                    (MenuId::Raid, false) => "target:prev",
                    (MenuId::Pacts, true) => "ptarget:next",
                    (MenuId::Pacts, false) => "ptarget:prev",
                    (_, true) => "to:next",
                    (_, false) => "to:prev",
                };
                return entries.iter().position(|e| e.tag == tag);
            }
            MenuId::Letters if dy != 0 => {
                let n = self.clan.as_ref().map_or(0, |c| c.social.inbox.len());
                if n > 0 {
                    self.soc.letter =
                        (self.soc.letter + n).wrapping_add_signed(isize::from(dy)) % n;
                    self.cursor = 0;
                    if let Some(c) = self.clan.as_mut()
                        && let Some(l) = c.social.inbox.get_mut(self.soc.letter)
                    {
                        l.read = true;
                    }
                }
            }
            MenuId::Crew => {
                let n = self.clan.as_ref().map_or(0, |c| c.crew.len());
                if dy != 0 && n > 0 {
                    let to = i32::try_from(self.crew_sel).unwrap_or(0) + i32::from(dy);
                    self.crew_sel =
                        usize::try_from(to.rem_euclid(i32::try_from(n).unwrap_or(1))).unwrap_or(0);
                    self.cursor = 0;
                } else if dx != 0 {
                    self.move_cursor(entries, dx);
                }
            }
            _ => {
                let on_marsh = menu == MenuId::Trip
                    && self.clan.as_ref().is_some_and(|c| {
                        c.trip
                            .as_ref()
                            .is_some_and(|t| matches!(t.phase, crate::trip::Phase::Free))
                    });
                if on_marsh {
                    // Arrows walk: one tile, if the marsh lets the party.
                    let letter = match (dx, dy) {
                        (0, -1) => 'n',
                        (1, 0) => 'e',
                        (0, 1) => 's',
                        _ => 'w',
                    };
                    let tag = format!("step:{letter}");
                    return entries.iter().position(|e| e.tag == tag);
                }
                self.move_cursor(entries, if dy != 0 { dy } else { dx });
            }
        }
        None
    }

    /// Move the lightbar along the entries the screen shows.
    fn move_cursor(&mut self, entries: &[Entry], by: i16) {
        let shown = ui::shown(self, entries);
        if shown.is_empty() {
            return;
        }
        let at = shown.iter().position(|&i| i == self.cursor).unwrap_or(0);
        let len = i32::try_from(shown.len()).unwrap_or(1);
        let to = (i32::try_from(at).unwrap_or(0) + i32::from(by)).rem_euclid(len);
        self.cursor = shown[usize::try_from(to).unwrap_or(0)];
    }

    fn act(&mut self, action: Action, ctx: &mut Ctx<'_>) {
        let Some(mut clan) = self.clan.take() else {
            return;
        };
        let on_trip = matches!(action, Action::Depart { .. } | Action::Go(_));
        let result = engine::apply(&self.env(), &mut clan, action, ctx.rng());
        self.clan = Some(clan);
        match result {
            Ok(events) => {
                for e in &events {
                    let (what, delta) = match e {
                        Event::Sold { what, qty, .. } => {
                            (*what, i32::try_from(*qty).unwrap_or(i32::MAX))
                        }
                        Event::Bought { what, qty, .. } => {
                            (*what, -i32::try_from(*qty).unwrap_or(i32::MAX))
                        }
                        _ => continue,
                    };
                    let season = self.world.season;
                    ctx.edit_world(move |old| {
                        let mut w = old.and_then(World::decode).filter(|w| w.season == season)?;
                        w.trade(what, delta);
                        Some(w.encode())
                    });
                }
                let mut notes: Vec<(String, DoorStyle)> = Vec::new();
                for e in &events {
                    let style = match e {
                        Event::Injured { .. }
                        | Event::Pocketed { .. }
                        | Event::Starved { .. }
                        | Event::Died { .. }
                        | Event::Dropped { .. }
                        | Event::LeftBehind { .. } => DoorStyle::Bad,
                        Event::Told(_) | Event::Fight(_) => DoorStyle::Plain,
                        Event::Scene { .. } => continue,
                        _ => DoorStyle::Good,
                    };
                    for line in trip_ui::wrap(&e.to_string(), 76) {
                        notes.push((line, style));
                    }
                }
                let keep = notes.len().saturating_sub(16);
                self.notes = notes.split_off(keep);
                if on_trip {
                    self.mode = if self.clan.as_ref().is_some_and(|c| c.trip.is_some()) {
                        Mode::Menu(MenuId::Trip)
                    } else {
                        Mode::Menu(MenuId::Main)
                    };
                }
                self.save(ctx);
            }
            Err(e) => self.notes = vec![(e.to_string(), DoorStyle::Bad)],
        }
    }
}
