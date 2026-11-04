//! A minimal text screen over the engine. It exists so the door can be opened, replayed and
//! driven by simulated players; the real screens (map, plot grid, event scenes) are the UI
//! slice. Every line is plain text, so the same lines are both the drawing and the
//! look-around view, and a bot can read them.

use std::sync::Arc;

use russh_bbs_door::{Canvas, Ctx, DoorEnv, DoorKey, DoorSession, DoorStyle, Flow};

use crate::clan::{Clan, NAME_MAX, Settings};
use crate::content::Catalog;
use crate::engine::{self, Action, Env};
use crate::event::Event;
use crate::hold::PLOTS;
use crate::map::Map;
use crate::tide::{DayTable, Tide, Warning};
use crate::types::{Resource, Track, Trade};
use crate::world::World;

/// The most menu entries (one key each, `a` to `z`).
const MENU_MAX: usize = 26;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuId {
    Main,
    Hold,
    Crew,
    Market,
    Tides,
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
}

enum Act {
    Go(MenuId),
    Do(Action),
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
}

impl Session {
    pub(crate) fn new(setup: Setup) -> Self {
        let Setup {
            cat,
            settings,
            world,
            world_stored,
            mut clan,
            today,
            unreadable,
            default_name,
        } = setup;
        let map = Map::generate(world.seed, &cat.pois);
        let tide = Tide::new(world.seed);
        let mut report = Vec::new();
        let mut dirty = false;
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
            let events = engine::catch_up(&env, c, today);
            dirty |= !events.is_empty();
            report = events
                .iter()
                .filter(|e| !matches!(e, Event::Night { .. }))
                .map(ToString::to_string)
                .collect();
        }
        let mode = if unreadable {
            Mode::Unreadable
        } else if clan.is_none() {
            Mode::Naming(default_name)
        } else if report.is_empty() {
            Mode::Menu(MenuId::Main)
        } else {
            Mode::Report
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
        }
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
        let who = self.clan.as_ref().map(|c| (c.name.clone(), c.site));
        ctx.edit_world(move |old| {
            let mut w = old
                .and_then(World::decode)
                .filter(|w| w.season == season)
                .unwrap_or_else(|| World::new(season, seed, day));
            if let Some((name, site)) = &who {
                w.register(name, *site);
            }
            Some(w.encode())
        });
    }

    fn save(&self, ctx: &mut Ctx<'_>) {
        if let Some(c) = &self.clan {
            ctx.request_save(c.encode());
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
                self.report = events
                    .iter()
                    .filter(|e| !matches!(e, Event::Night { .. }))
                    .map(ToString::to_string)
                    .collect();
                self.dirty = true;
                self.clan = Some(clan);
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
                    "go:tides",
                    "Today's tide table",
                    Act::Go(MenuId::Tides),
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
            MenuId::Tides => {}
        }
        out.truncate(MENU_MAX);
        out
    }

    fn tide_lines(&self, clan: &Clan) -> Vec<(String, DoorStyle)> {
        let env = self.env();
        let minute = env.minute(clan);
        let level = self.tide.level(minute);
        let (wait, high) = self.tide.next_turn(minute);
        let table: DayTable = self.tide.table(self.today);
        let warn = match self.tide.warning(level, self.today) {
            Warning::Calm => ("", DoorStyle::Plain),
            Warning::High => (" HIGH WATER", DoorStyle::Strong),
            Warning::Flood => (" FLOOD", DoorStyle::Bad),
            Warning::Storm => (" STORM SURGE", DoorStyle::Bad),
        };
        let mut out = vec![(
            format!(
                "Tide: water {level}, next {} in {}h{:02}m, {} tides{}{}",
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
            Mode::Menu(menu) => {
                if key == DoorKey::Escape {
                    if menu == MenuId::Main {
                        return Flow::Exit;
                    }
                    self.mode = Mode::Menu(MenuId::Main);
                    self.notes.clear();
                    return Flow::Stay;
                }
                if menu == MenuId::Tides {
                    self.mode = Mode::Menu(MenuId::Main);
                    return Flow::Stay;
                }
                let DoorKey::Char(c) = key else {
                    return Flow::Stay;
                };
                let Some(index) = u32::from(c.to_ascii_lowercase()).checked_sub(u32::from('a'))
                else {
                    return Flow::Stay;
                };
                let mut entries = self.entries();
                let index = usize::try_from(index).unwrap_or(usize::MAX);
                if index >= entries.len() {
                    return Flow::Stay;
                }
                match entries.swap_remove(index).act {
                    Act::Go(next) => {
                        self.mode = Mode::Menu(next);
                        self.notes.clear();
                    }
                    Act::Leave => return Flow::Exit,
                    Act::Do(action) => self.act(action, ctx),
                }
            }
        }
        Flow::Stay
    }

    fn render(&self, canvas: &mut Canvas<'_>, _env: &DoorEnv) {
        canvas.clear();
        for (y, (text, style)) in self.lines().iter().enumerate() {
            let Ok(y) = u16::try_from(y) else { break };
            canvas.text(1, y, text, *style);
        }
    }

    fn look_around(&self, _env: &DoorEnv) -> Vec<String> {
        self.lines().into_iter().map(|(text, _)| text).collect()
    }
}

impl Session {
    fn act(&mut self, action: Action, ctx: &mut Ctx<'_>) {
        let Some(mut clan) = self.clan.take() else {
            return;
        };
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
                self.notes = events
                    .iter()
                    .map(|e| {
                        let style = if matches!(
                            e,
                            Event::Injured { .. } | Event::Pocketed { .. } | Event::Starved { .. }
                        ) {
                            DoorStyle::Bad
                        } else {
                            DoorStyle::Good
                        };
                        (e.to_string(), style)
                    })
                    .collect();
                self.save(ctx);
            }
            Err(e) => self.notes = vec![(e.to_string(), DoorStyle::Bad)],
        }
    }
}
