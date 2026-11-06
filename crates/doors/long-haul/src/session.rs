//! The playable screen: a thin text front end over the engine. Keys map to [`Action`]s; the
//! screen holds no rules. Every key that changes the captain asks for a save at once, so a
//! disconnect loses at most that key and no roll can be re-rolled by dropping the line.
//!
//! This is the minimal screen of slice L1. The chart, the planner and the combat panel are
//! the UI slice (L3); the screen reads the same lines for the look-around view.

mod screens;
mod society;
mod ui;

use std::sync::Arc;

use russh_bbs_door::{Canvas, Ctx, DoorEnv, DoorKey, DoorRng, DoorSession, DoorStyle, Flow};

use crate::combat::Maneuver;
use crate::content::{Content, Faction};
use crate::engine::{Action, Env, RepairTarget};
use crate::events::Event;
use crate::frontier::{Frontier, Role};
use crate::player::Player;
use crate::ship::Ship;
use crate::world::World;

/// How long the market feed stays trustworthy once the door is open. The world is read when the
/// door opens (the framework gives a door no fresher view), so after this long the screen asks
/// the captain to come back in rather than trade on old stock.
pub(crate) const FEED_FRESH_SECS: i64 = 900;
/// Log lines kept.
const LOG_LINES: usize = 4;
/// Log lines kept for the fight log and the footer.
const LOG_KEEP: usize = 24;

/// How a session starts.
pub(crate) enum Start {
    /// No captain yet.
    New,
    /// A saved captain.
    Captain(Box<Player>),
    /// A save this build cannot read; nothing is written over it.
    Unreadable,
    /// No captain yet this season, but an earlier season's captain left a legacy.
    Successor {
        /// Earlier seasons, the last one included.
        legacy: Vec<crate::socialplay::Legacy>,
        /// The module handed down (255 for none).
        heirloom: u8,
    },
}

enum Mode {
    Pick(usize),
    Unreadable,
    Bridge,
    Docks(usize),
    Market(usize),
    Chart,
    Route,
    Standing,
    Yard(usize),
    Loan,
    News,
    Crew(usize),
    Jobs(usize),
    Society(society::Tab, usize, Option<String>),
}

pub(crate) struct Session {
    content: Arc<Content>,
    /// The generated map, without outposts (what a stored world is keyed to).
    base: Arc<Frontier>,
    frontier: Arc<Frontier>,
    world: World,
    player: Option<Player>,
    handle: String,
    mode: Mode,
    log: Vec<(String, DoorStyle)>,
    opened_at: Option<i64>,
    /// The manoeuvre the next combat round will use.
    maneuver: Maneuver,
    chart: ui::chart::ChartState,
    plan: Option<ui::route::Plan>,
    reactor: ui::combat::Reactor,
    /// World changes made while the door was opening, sent with the first key.
    pending: Vec<crate::world::Delta>,
    /// What an earlier season left the captain who is about to sign on.
    inheritance: Option<(Vec<crate::socialplay::Legacy>, u8)>,
    /// Opening the door changed the captain (a new day, collected raids and dividends); the
    /// first key writes it, even if that key leaves.
    opened_dirty: bool,
}

fn cr(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// Columns a line may use inside the board's box at 80 columns.
const WRAP: usize = 68;

/// Break `text` at spaces into lines of at most `width` characters; a continuation is indented.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split(' ').filter(|w| !w.is_empty()) {
        let limit = if lines.is_empty() { width } else { width - 2 };
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > limit {
            lines.push(std::mem::take(&mut line));
        }
        if line.is_empty() && !lines.is_empty() {
            line.push_str("  ");
        }
        if !line.trim().is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Lay `items` out in lines of at most `width` characters, two spaces apart, never splitting
/// an item (a key hint stays whole).
fn wrap_items(items: &[String], width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for item in items {
        if !line.is_empty() && line.chars().count() + 2 + item.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push_str("  ");
        }
        line.push_str(item);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn clip(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

impl Session {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        content: Arc<Content>,
        base: Arc<Frontier>,
        frontier: Arc<Frontier>,
        world: World,
        start: Start,
        handle: &str,
        today: i64,
        rng: &mut DoorRng,
    ) -> Self {
        let mut inheritance = None;
        let (player, mode) = match start {
            Start::New => (None, Mode::Pick(0)),
            Start::Successor { legacy, heirloom } => {
                inheritance = Some((legacy, heirloom));
                (None, Mode::Pick(0))
            }
            Start::Unreadable => (None, Mode::Unreadable),
            Start::Captain(player) => (Some(*player), Mode::Bridge),
        };
        let mut world = world;
        let mut player = player;
        let mut pending = Vec::new();
        if let Some(p) = player.as_mut()
            && p.social.handle != handle
        {
            // Bookkeeping, not a change worth a save of its own.
            handle.clone_into(&mut p.social.handle);
        }
        let loaded = player.clone();
        let events = player.as_mut().map_or_else(Vec::new, |p| {
            let mut env = Env::new(&content, &frontier, &mut world, rng, today);
            let mut events = p.settle(&mut env);
            events.extend(p.claim_social(&mut env));
            pending = std::mem::take(&mut env.deltas);
            events
        });
        let mut session = Self {
            content,
            base,
            frontier,
            world,
            player,
            handle: handle.to_owned(),
            mode,
            log: Vec::new(),
            opened_at: None,
            maneuver: Maneuver::Hold,
            chart: ui::chart::ChartState::new(0),
            plan: None,
            reactor: ui::combat::Reactor::new(),
            inheritance,
            pending,
            opened_dirty: false,
        };
        session.opened_dirty = session.player != loaded || !session.pending.is_empty();
        if let Some(p) = &session.player {
            session.chart = ui::chart::ChartState::new(p.at);
        }
        // Settling is deterministic from the save and the day, so doing it at open is safe even
        // if the captain leaves without another key; the first key saves it.
        session.note_events(&events);
        session
    }

    /// Write what opening the door changed, once, with the first key.
    fn flush_open(&mut self, ctx: &mut Ctx<'_>) {
        if !self.opened_dirty {
            return;
        }
        self.opened_dirty = false;
        if let Some(p) = &self.player {
            ctx.request_save(p.encode());
        }
        let deltas = std::mem::take(&mut self.pending);
        if !deltas.is_empty() {
            let (content, base) = (Arc::clone(&self.content), Arc::clone(&self.base));
            let today = ctx.today().as_raw();
            ctx.edit_world(move |old| {
                Some(World::apply_stored(old, &content, &base, today, &deltas))
            });
        }
    }

    fn push(&mut self, text: String, style: DoorStyle) {
        self.log.push((text, style));
        if self.log.len() > LOG_KEEP {
            self.log.remove(0);
        }
    }

    fn note_events(&mut self, events: &[Event]) {
        for event in events {
            let style = match event {
                Event::Sold { profit, .. } if *profit >= 0 => DoorStyle::Good,
                Event::Sold { .. }
                | Event::Seized { .. }
                | Event::DueMissed { .. }
                | Event::Repossessed { .. }
                | Event::DriveFault { .. }
                | Event::Hazard { avoided: false, .. }
                | Event::YouWereHit { .. }
                | Event::Beaten { .. }
                | Event::Boarded { .. }
                | Event::CrewHurt { .. }
                | Event::CrewLost { .. }
                | Event::Deserted { .. }
                | Event::Mutiny { .. }
                | Event::WagesMissed { .. }
                | Event::ContractFailed { .. }
                | Event::Downed { .. }
                | Event::Standing { change: ..=-1, .. } => DoorStyle::Bad,
                Event::Charted { .. }
                | Event::SkillUp { .. }
                | Event::CrewSkill { .. }
                | Event::YouHit { .. }
                | Event::Wrecked { .. }
                | Event::GoalMet { .. }
                | Event::ContractDone { .. }
                | Event::Standing { .. } => DoorStyle::Good,
                _ => DoorStyle::Plain,
            };
            let text = event.text(&self.content, &self.frontier);
            self.push(text, style);
        }
    }

    /// Run one engine action: settle the day, apply, send world changes, save.
    fn act(&mut self, ctx: &mut Ctx<'_>, action: &Action) -> bool {
        let Some(mut player) = self.player.take() else {
            return false;
        };
        let now = ctx.now().as_raw();
        let opened = *self.opened_at.get_or_insert(now);
        let stale = now - opened > FEED_FRESH_SECS;
        let touches_world = matches!(
            action,
            Action::Buy { .. } | Action::Sell { .. } | Action::Jump(_)
        );
        if stale && touches_world {
            self.push(
                "The market feed has gone stale. Leave the door and come back in to refresh it."
                    .to_owned(),
                DoorStyle::Bad,
            );
            self.player = Some(player);
            return false;
        }
        let today = ctx.today().as_raw();
        let before = player.clone();
        let carried = std::mem::take(&mut self.pending);
        let (settled, result, mut deltas) = {
            let mut env = Env::new(
                &self.content,
                &self.frontier,
                &mut self.world,
                ctx.rng(),
                today,
            );
            let settled = player.settle(&mut env);
            let result = player.apply(&mut env, action);
            (settled, result, env.deltas)
        };
        deltas.splice(0..0, carried);
        self.note_events(&settled);
        let ok = match result {
            Ok(events) => {
                self.note_events(&events);
                true
            }
            Err(refusal) => {
                self.push(refusal.text(), DoorStyle::Bad);
                false
            }
        };
        if ok || player != before {
            ctx.request_save(player.encode());
        }
        if !deltas.is_empty() {
            let (content, base) = (Arc::clone(&self.content), Arc::clone(&self.base));
            ctx.edit_world(move |old| {
                Some(World::apply_stored(old, &content, &base, today, &deltas))
            });
        }
        self.player = Some(player);
        ok
    }

    fn rows(&self) -> Vec<crate::trade::Row> {
        self.player.as_ref().map_or_else(Vec::new, |p| {
            p.market_rows(&self.content, &self.frontier, &self.world)
        })
    }

    fn pick_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        let Mode::Pick(cursor) = &mut self.mode else {
            return Flow::Stay;
        };
        let n = self.content.backgrounds.len();
        match key {
            DoorKey::Up => *cursor = (*cursor + n - 1) % n,
            DoorKey::Down => *cursor = (*cursor + 1) % n,
            DoorKey::Escape | DoorKey::Char('q' | 'Q') => return Flow::Exit,
            DoorKey::Enter => {
                let background = *cursor;
                let today = ctx.today().as_raw();
                let captain = ctx
                    .player_name(&self.handle)
                    .map_or_else(|_| self.handle.clone(), |n| n.as_str().to_owned());
                let mut player =
                    Player::new(&self.content, &self.frontier, &captain, background, today);
                player.social.handle.clone_from(&self.handle);
                if let Some((legacy, heirloom)) = self.inheritance.take() {
                    if let Some(last) = legacy.last()
                        && let Some((title, text)) = self.content.ending_text(last.ending)
                    {
                        self.push(
                            format!("Last season ended as {title}: {text}"),
                            DoorStyle::Accent,
                        );
                    }
                    if let Some(name) = player.inherit(&self.content, legacy, heirloom) {
                        self.push(
                            format!("A {name} comes down to you from the last captain."),
                            DoorStyle::Good,
                        );
                    }
                }
                ctx.request_save(player.encode());
                self.push(
                    "You sign the loan. The ship is yours, and so is the debt.".to_owned(),
                    DoorStyle::Good,
                );
                self.player = Some(player);
                self.mode = Mode::Bridge;
            }
            _ => {}
        }
        Flow::Stay
    }

    fn bridge_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        let DoorKey::Char(c) = key else {
            return if key == DoorKey::Escape {
                Flow::Exit
            } else {
                Flow::Stay
            };
        };
        let Some(player) = self.player.clone() else {
            return Flow::Stay;
        };
        let system = self.frontier.system(player.at);
        match c.to_ascii_lowercase() {
            'q' => return Flow::Exit,
            'd' => {
                let here: Vec<u16> = system.map(|s| s.stations.clone()).unwrap_or_default();
                match here.as_slice() {
                    [] => self.push(
                        "There is no station in this system.".to_owned(),
                        DoorStyle::Bad,
                    ),
                    [only] => {
                        self.act(ctx, &Action::Dock(*only));
                    }
                    _ => self.mode = Mode::Docks(0),
                }
            }
            'u' => {
                self.act(ctx, &Action::Undock);
            }
            'm' if player.docked.is_some() => self.mode = Mode::Market(0),
            'j' | 'v' => {
                self.chart.cursor = self.plan.map_or(player.at, |p| p.target);
                self.mode = if self.plan.is_some() && c.eq_ignore_ascii_case(&'j') {
                    Mode::Route
                } else {
                    Mode::Chart
                };
            }
            'a' => self.mode = Mode::Standing,
            'o' => self.mode = Mode::Society(society::Tab::Convoy, 0, None),
            'y' if player.docked.is_some() => self.mode = Mode::Yard(0),
            'l' if player.docked.is_some() => self.mode = Mode::Loan,
            'n' => self.mode = Mode::News,
            'c' => self.mode = Mode::Crew(0),
            'b' if player.docked.is_some() => self.mode = Mode::Jobs(0),
            'p' if player.docked.is_some() => {
                self.act(ctx, &Action::Provision);
            }
            'f' => {
                self.act(ctx, &Action::Refuel(u16::MAX));
            }
            'r' => {
                self.act(ctx, &Action::Repair(RepairTarget::All));
            }
            's' => {
                self.act(ctx, &Action::Skim);
            }
            'x' => {
                self.act(ctx, &Action::DeepScan);
            }
            'g' => {
                self.act(ctx, &Action::SellSurvey);
            }
            't' => {
                self.act(ctx, &Action::CallTow);
            }
            _ => {}
        }
        Flow::Stay
    }

    fn list_cursor(key: DoorKey, cursor: &mut usize, len: usize) -> bool {
        match key {
            DoorKey::Up if len > 0 => *cursor = (*cursor + len - 1) % len,
            DoorKey::Down if len > 0 => *cursor = (*cursor + 1) % len,
            _ => return false,
        }
        true
    }

    fn docks_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let Mode::Docks(cursor) = &mut self.mode else {
            return;
        };
        let Some(player) = &self.player else { return };
        let here: Vec<u16> = self
            .frontier
            .system(player.at)
            .map(|s| s.stations.clone())
            .unwrap_or_default();
        if Self::list_cursor(key, cursor, here.len()) {
            return;
        }
        match key {
            DoorKey::Escape | DoorKey::Char('q') => self.mode = Mode::Bridge,
            DoorKey::Enter => {
                let pick = here.get(*cursor).copied();
                self.mode = Mode::Bridge;
                if let Some(station) = pick {
                    self.act(ctx, &Action::Dock(station));
                }
            }
            _ => {}
        }
    }

    fn market_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let rows = self.rows();
        let Mode::Market(cursor) = &mut self.mode else {
            return;
        };
        if Self::list_cursor(key, cursor, rows.len()) {
            return;
        }
        let slot = rows.get(*cursor).map(|r| r.slot);
        let held = rows.get(*cursor).map_or(0, |r| r.held);
        let action = match (key, slot) {
            (DoorKey::Escape | DoorKey::Char('q'), _) => {
                self.mode = Mode::Bridge;
                return;
            }
            (DoorKey::Char('b'), Some(slot)) => Action::Buy { slot, tons: 1 },
            (DoorKey::Char('B'), Some(slot)) => Action::Buy { slot, tons: 10 },
            (DoorKey::Char('a'), Some(slot)) => Action::Buy { slot, tons: 999 },
            (DoorKey::Char('s'), Some(slot)) => Action::Sell { slot, tons: 1 },
            (DoorKey::Char('S'), Some(slot)) => Action::Sell { slot, tons: 10 },
            (DoorKey::Char('z'), Some(slot)) => Action::Sell {
                slot,
                tons: held.max(1),
            },
            _ => return,
        };
        self.act(ctx, &action);
    }

    /// The yard's menu: modules that fit, hulls, and the ship's own modules to sell.
    fn yard_items(&self) -> Vec<(Action, String, i64)> {
        let Some(player) = &self.player else {
            return Vec::new();
        };
        let content = &self.content;
        let hull = &content.hulls[usize::from(player.ship.hull)];
        let mut items = Vec::new();
        for (i, m) in content.modules.iter().enumerate() {
            if player.ship.used_slots(content, m.slot) < usize::from(hull.slots.of(m.slot)) {
                items.push((
                    Action::BuyModule(u8::try_from(i).unwrap_or(0)),
                    format!("Buy  {} ({})", m.name, m.slot.name()),
                    -i64::from(m.price),
                ));
            }
        }
        for (i, h) in content.hulls.iter().enumerate() {
            if i != usize::from(player.ship.hull) {
                let net = i64::from(h.price) - player.ship.hull_resale(content);
                items.push((
                    Action::BuyHull(u8::try_from(i).unwrap_or(0)),
                    format!("Hull {}", h.name),
                    -net,
                ));
            }
        }
        for (i, inst) in player.ship.modules.iter().enumerate() {
            if let Some(m) = content.modules.get(usize::from(inst.module)) {
                items.push((
                    Action::SellModule(i),
                    format!("Sell {} (wear {}%)", m.name, inst.wear / 10),
                    Ship::module_resale(inst, content),
                ));
            }
        }
        items
    }

    fn yard_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let items = self.yard_items();
        let Mode::Yard(cursor) = &mut self.mode else {
            return;
        };
        if Self::list_cursor(key, cursor, items.len()) {
            return;
        }
        match key {
            DoorKey::Escape | DoorKey::Char('q') => self.mode = Mode::Bridge,
            DoorKey::Enter => {
                if let Some((action, _, _)) = items.get(*cursor) {
                    let action = action.clone();
                    self.act(ctx, &action);
                }
            }
            _ => {}
        }
    }

    fn loan_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let action = match key {
            DoorKey::Escape | DoorKey::Char('q') => {
                self.mode = Mode::Bridge;
                return;
            }
            DoorKey::Char('b') => Action::Borrow(1_000),
            DoorKey::Char('B') => Action::Borrow(5_000),
            DoorKey::Char('p') => Action::Repay(500),
            DoorKey::Char('P') => Action::Repay(5_000),
            DoorKey::Char('a') => Action::Repay(
                self.player
                    .as_ref()
                    .map_or(0, |p| p.cash.min(p.debt.principal)),
            ),
            _ => return,
        };
        self.act(ctx, &action);
    }

    // ---- drawing -------------------------------------------------------------------------

    fn header(&self, player: &Player) -> Vec<(String, DoorStyle)> {
        let content = &self.content;
        let stats = player.stats(content);
        let hull = content.hulls.get(usize::from(player.ship.hull));
        let max_hp = hull.map_or(0, |h| h.hull_points);
        let place = match (
            self.frontier.system(player.at),
            player.docked.and_then(|s| self.frontier.station(s)),
        ) {
            (Some(sys), Some(st)) => format!(
                "Docked at {} [{}], {} system",
                st.name,
                content.economies[usize::from(st.economy)].name,
                sys.name
            ),
            (Some(sys), None) => format!(
                "In {} ({}, {})",
                sys.name,
                sys.star.name(),
                sys.hazard.name()
            ),
            _ => "Adrift".to_owned(),
        };
        let weekly = player.weekly_payment();
        let due_in = player.debt.next_due - player.last_day;
        vec![
            (
                format!(
                    "LONG HAUL  Capt. {}   Drive-days left: {}   Voyage day {}",
                    player.captain, player.days_left, player.voyage_days
                ),
                DoorStyle::Accent,
            ),
            (
                format!(
                    "Cash {} cr   Debt {} cr   Pay {} cr in {} d{}",
                    cr(player.cash),
                    cr(player.debt.principal),
                    cr(weekly),
                    due_in.max(0),
                    if player.debt.missed > 0 {
                        format!("   MISSED {}", player.debt.missed)
                    } else {
                        String::new()
                    }
                ),
                if player.debt.missed > 0 {
                    DoorStyle::Bad
                } else {
                    DoorStyle::Plain
                },
            ),
            (
                format!(
                    "{}  Hull {}/{}  Fuel {}/{}  Hold {}/{} t",
                    hull.map_or("ship", |h| h.name.as_str()),
                    player.ship.hull_hp,
                    max_hp,
                    player.ship.fuel,
                    stats.fuel_cap,
                    player.hold_used(),
                    stats.cargo_cap
                ),
                DoorStyle::Plain,
            ),
            (place, DoorStyle::Strong),
        ]
    }

    /// The bridge: the menu, the hold and what the station deals in, then whatever else fits in
    /// `budget` rows (most useful first).
    #[allow(clippy::too_many_lines, reason = "one list of bridge lines")]
    fn bridge_lines(&self, player: &Player, budget: usize) -> Vec<(String, DoorStyle)> {
        let mut out = vec![(String::new(), DoorStyle::Plain)];
        let docked = player.docked.is_some();
        let system = self.frontier.system(player.at);
        let mut keys = vec![
            "[j] jump".to_owned(),
            "[n] news".to_owned(),
            "[c] crew".to_owned(),
        ];
        if docked {
            keys.splice(
                0..0,
                [
                    "[m] market".to_owned(),
                    "[f] refuel".to_owned(),
                    "[r] repair".to_owned(),
                    "[y] yard".to_owned(),
                    "[l] loan".to_owned(),
                    "[g] guild".to_owned(),
                    "[b] jobs".to_owned(),
                    "[p] rations".to_owned(),
                    "[u] undock".to_owned(),
                ],
            );
        } else {
            keys.insert(0, "[d] dock".to_owned());
        }
        if system.is_some_and(|s| s.gas_giant) {
            keys.push("[s] skim".to_owned());
        }
        if player.stats(&self.content).scan > 0 {
            keys.push("[x] scan".to_owned());
        }
        keys.push("[t] tow".to_owned());
        keys.push("[q] leave".to_owned());
        for line in wrap_items(&keys, WRAP) {
            out.push((line, DoorStyle::Strong));
        }
        out.push((String::new(), DoorStyle::Plain));
        let cargo: Vec<String> = player
            .ship
            .cargo
            .iter()
            .map(|l| {
                format!(
                    "{} t {}",
                    l.tons,
                    self.content.goods[usize::from(l.good)].name
                )
            })
            .collect();
        let hold = if cargo.is_empty() {
            "Hold: empty.".to_owned()
        } else {
            format!("Hold: {}", cargo.join(", "))
        };
        for line in wrap(&hold, WRAP) {
            out.push((line, DoorStyle::Plain));
        }
        let mut optional: Vec<(String, DoorStyle)> = Vec::new();
        if let Some(st) = player.docked.and_then(|s| self.frontier.station(s)) {
            let eco = &self.content.economies[usize::from(st.economy)];
            let names = |ids: &[String]| -> String {
                ids.iter()
                    .filter_map(|id| self.content.good_ix(id))
                    .map(|i| self.content.goods[i].name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let guide = format!(
                "{}s make {}. They want {}.",
                eco.name,
                names(&eco.produces),
                names(&eco.needs)
            );
            for line in wrap(&guide, WRAP) {
                optional.push((line, DoorStyle::Dim));
            }
        }
        if !player.survey.is_empty() {
            let total: i64 = player.survey.iter().map(|s| i64::from(s.value)).sum();
            optional.push((
                format!(
                    "Survey data: {} records, about {} cr at a Guild office.",
                    player.survey.len(),
                    cr(total)
                ),
                DoorStyle::Dim,
            ));
        }
        let standing: Vec<String> = Faction::ALL
            .iter()
            .map(|f| format!("{} {:+}", f.short(), player.standing[f.index()]))
            .collect();
        optional.push((format!("Standing: {}", standing.join(", ")), DoorStyle::Dim));
        let skills: Vec<String> = crate::skills::Skill::ALL
            .iter()
            .map(|&s| format!("{} {}", s.name(), player.level(s)))
            .collect();
        optional.push((skills.join("  "), DoorStyle::Dim));
        let room = budget.saturating_sub(out.len());
        out.extend(optional.into_iter().take(room));
        out
    }

    fn market_lines(&self, height: usize, cursor: usize) -> Vec<(String, DoorStyle)> {
        let rows = self.rows();
        let mut out = vec![(
            "   Good                 Stock   Mid   Buy  Sell  Held  Role".to_owned(),
            DoorStyle::Dim,
        )];
        let room = height.saturating_sub(10).max(3);
        let first = cursor
            .saturating_sub(room - 1)
            .min(rows.len().saturating_sub(room));
        for (i, row) in rows.iter().enumerate().skip(first).take(room) {
            let good = &self.content.goods[usize::from(row.good)];
            let role = match row.role {
                Role::Produces => "makes",
                Role::Needs => "needs",
                Role::Neutral => "",
            };
            let mark = if good.class == crate::content::GoodClass::Contraband {
                '*'
            } else {
                ' '
            };
            let text = format!(
                "{} {}{:<20}{:>6}{:>6}{:>6}{:>6}{:>6}  {}",
                if i == cursor { '>' } else { ' ' },
                mark,
                clip(&good.name, 20),
                row.stock,
                row.mid,
                if row.can_buy {
                    row.buy.to_string()
                } else {
                    "-".to_owned()
                },
                row.sell,
                row.held,
                role
            );
            out.push((
                text,
                if i == cursor {
                    DoorStyle::Selected
                } else {
                    DoorStyle::Plain
                },
            ));
        }
        let keys = [
            "[b/B] buy 1/10",
            "[a] fill hold",
            "[s/S] sell 1/10",
            "[z] sell all",
            "[Esc] back",
            "(* contraband)",
        ]
        .map(str::to_owned);
        out.extend(
            wrap_items(&keys, WRAP)
                .into_iter()
                .map(|l| (l, DoorStyle::Strong)),
        );
        out
    }

    fn yard_lines(&self, height: usize, cursor: usize) -> Vec<(String, DoorStyle)> {
        let items = self.yard_items();
        let room = height.saturating_sub(10).max(3);
        let first = cursor
            .saturating_sub(room - 1)
            .min(items.len().saturating_sub(room));
        let mut out = vec![(
            "Shipyard (prices in credits; negative is what you pay)".to_owned(),
            DoorStyle::Dim,
        )];
        for (i, (_, label, money)) in items.iter().enumerate().skip(first).take(room) {
            let text = format!(
                "{} {:<44}{:>9}",
                if i == cursor { '>' } else { ' ' },
                clip(label, 44),
                cr(*money)
            );
            out.push((
                text,
                if i == cursor {
                    DoorStyle::Selected
                } else {
                    DoorStyle::Plain
                },
            ));
        }
        out.push((
            "[Up/Down] choose  [Enter] do it  [Esc] back".to_owned(),
            DoorStyle::Strong,
        ));
        out
    }

    fn news_lines(&self, player: &Player) -> Vec<(String, DoorStyle)> {
        let news = self
            .world
            .news_at(&self.content, &self.frontier, player.at, self.world.day);
        let mut out = vec![(
            "News that has reached this system (it travels one hop a night):".to_owned(),
            DoorStyle::Dim,
        )];
        if news.is_empty() {
            out.push(("Nothing yet.".to_owned(), DoorStyle::Plain));
        }
        for (age, text) in news.into_iter().take(8) {
            out.push((format!("{age}d ago: {text}"), DoorStyle::Plain));
        }
        out.push(("[Esc] back".to_owned(), DoorStyle::Strong));
        out
    }

    fn loan_lines(player: &Player) -> Vec<(String, DoorStyle)> {
        let mut out = vec![
            (
                format!(
                    "The lender holds {} cr. Interest runs {} per mille a day.",
                    cr(player.debt.principal),
                    crate::engine::INTEREST_PM
                ),
                DoorStyle::Plain,
            ),
            (
                format!(
                    "Next payment {} cr in {} days. The lender's limit is {} cr.",
                    cr(player.weekly_payment()),
                    (player.debt.next_due - player.last_day).max(0),
                    cr(player.credit_limit())
                ),
                DoorStyle::Plain,
            ),
            (
                format!(
                    "Three missed payments in a row: repossession. So far: {}.",
                    player.repossessions
                ),
                DoorStyle::Dim,
            ),
            (String::new(), DoorStyle::Plain),
        ];
        let keys = [
            "[b] borrow 1,000",
            "[B] borrow 5,000",
            "[p] pay 500",
            "[P] pay 5,000",
            "[a] pay all",
            "[Esc] back",
        ]
        .map(str::to_owned);
        out.extend(
            wrap_items(&keys, WRAP)
                .into_iter()
                .map(|l| (l, DoorStyle::Strong)),
        );
        out
    }

    /// The screens before there is a captain: the choice of background, or the unreadable-save
    /// notice.
    fn intro_lines(&self) -> Vec<(String, DoorStyle)> {
        match &self.mode {
            Mode::Unreadable => vec![
                ("LONG HAUL".to_owned(), DoorStyle::Accent),
                (
                    "Your saved captain was written by a version this board cannot read."
                        .to_owned(),
                    DoorStyle::Bad,
                ),
                (
                    "Nothing has been changed. Ask the sysop; press any key to leave.".to_owned(),
                    DoorStyle::Plain,
                ),
            ],
            Mode::Pick(cursor) => {
                let mut lines = vec![
                    (
                        "LONG HAUL: the gates failed long ago, and you owe for your ship."
                            .to_owned(),
                        DoorStyle::Accent,
                    ),
                    (
                        "Who were you before the loan?  (Up/Down, Enter to sign, Esc to leave)"
                            .to_owned(),
                        DoorStyle::Plain,
                    ),
                    (String::new(), DoorStyle::Plain),
                ];
                for (i, b) in self.content.backgrounds.iter().enumerate() {
                    let style = if i == *cursor {
                        DoorStyle::Selected
                    } else {
                        DoorStyle::Plain
                    };
                    lines.push((
                        format!(
                            "{} {:<20} owes {} cr, starts with {} cr",
                            if i == *cursor { '>' } else { ' ' },
                            b.name,
                            cr(i64::from(b.loan)),
                            cr(i64::from(b.cash))
                        ),
                        style,
                    ));
                }
                if let Some(b) = self.content.backgrounds.get(*cursor) {
                    lines.push((String::new(), DoorStyle::Plain));
                    lines.push((b.blurb.clone(), DoorStyle::Dim));
                    lines.push((format!("Perk: {}", b.perk_text), DoorStyle::Good));
                }
                lines
            }
            _ => vec![("No captain.".to_owned(), DoorStyle::Plain)],
        }
    }

    fn lines(&self, height: usize) -> Vec<(String, DoorStyle)> {
        let mut out = Vec::new();
        let Some(player) = &self.player else {
            return fit_width(self.intro_lines());
        };
        out.extend(self.header(player));
        if player.scene.is_some() {
            out.extend(self.scene_lines(player));
            out.extend(self.scene_extra_lines());
            out.push((String::new(), DoorStyle::Plain));
            for (text, style) in self
                .log
                .iter()
                .skip(self.log.len().saturating_sub(LOG_LINES))
            {
                out.push((text.clone(), *style));
            }
            return fit_width(out);
        }
        match &self.mode {
            Mode::Bridge | Mode::Pick(_) | Mode::Unreadable => {
                let budget = height.saturating_sub(out.len() + self.log.len().min(LOG_LINES) + 1);
                out.extend(self.bridge_lines(player, budget));
            }
            Mode::Docks(cursor) => {
                out.push((String::new(), DoorStyle::Plain));
                out.push(("Stations in this system:".to_owned(), DoorStyle::Dim));
                for (i, id) in self
                    .frontier
                    .system(player.at)
                    .map(|s| s.stations.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .enumerate()
                {
                    if let Some(st) = self.frontier.station(id) {
                        let eco = &self.content.economies[usize::from(st.economy)];
                        out.push((
                            format!(
                                "{} {} - {} (size {})",
                                if i == *cursor { '>' } else { ' ' },
                                st.name,
                                eco.name,
                                st.size
                            ),
                            if i == *cursor {
                                DoorStyle::Selected
                            } else {
                                DoorStyle::Plain
                            },
                        ));
                    }
                }
                out.push((
                    "[Up/Down] choose  [Enter] dock (1 drive-day)  [Esc] back".to_owned(),
                    DoorStyle::Strong,
                ));
            }
            Mode::Market(cursor) => out.extend(self.market_lines(height, *cursor)),
            Mode::Chart => out.extend(self.chart_lines()),
            Mode::Route => out.extend(self.route_lines()),
            Mode::Standing => out.extend(self.standing_lines()),
            Mode::Yard(cursor) => out.extend(self.yard_lines(height, *cursor)),
            Mode::Loan => out.extend(Self::loan_lines(player)),
            Mode::News => out.extend(self.news_lines(player)),
            Mode::Crew(cursor) => out.extend(self.crew_lines(player, *cursor)),
            Mode::Jobs(cursor) => out.extend(self.jobs_lines(player, *cursor)),
            Mode::Society(tab, cursor, input) => {
                out.extend(self.society_lines(
                    *tab,
                    *cursor,
                    input.as_deref(),
                    height.saturating_sub(8).max(10),
                ));
            }
        }
        out.push((String::new(), DoorStyle::Plain));
        for (text, style) in self
            .log
            .iter()
            .skip(self.log.len().saturating_sub(LOG_LINES))
        {
            out.push((text.clone(), *style));
        }
        fit_width(out)
    }
}

/// Wrap any line wider than the board's box at a space, keeping its style.
fn fit_width(lines: Vec<(String, DoorStyle)>) -> Vec<(String, DoorStyle)> {
    lines
        .into_iter()
        .flat_map(|(text, style)| {
            if text.chars().count() <= WRAP + 2 {
                vec![(text, style)]
            } else {
                wrap(&text, WRAP + 2)
                    .into_iter()
                    .map(|l| (l, style))
                    .collect()
            }
        })
        .collect()
}

impl DoorSession for Session {
    fn key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        self.flush_open(ctx);
        if self.player.as_ref().is_some_and(|p| p.scene.is_some()) {
            return self.scene_key(key, ctx);
        }
        match self.mode {
            Mode::Unreadable => Flow::Exit,
            Mode::Pick(_) => self.pick_key(key, ctx),
            Mode::Bridge => self.bridge_key(key, ctx),
            Mode::Docks(_) => {
                self.docks_key(key, ctx);
                Flow::Stay
            }
            Mode::Market(_) => {
                self.market_key(key, ctx);
                Flow::Stay
            }
            Mode::Chart => {
                self.chart_key(key, ctx);
                Flow::Stay
            }
            Mode::Route => {
                self.route_key(key, ctx);
                Flow::Stay
            }
            Mode::Standing => {
                if matches!(key, DoorKey::Escape | DoorKey::Char('q')) {
                    self.mode = Mode::Bridge;
                }
                Flow::Stay
            }
            Mode::Yard(_) => {
                self.yard_key(key, ctx);
                Flow::Stay
            }
            Mode::Loan => {
                self.loan_key(key, ctx);
                Flow::Stay
            }
            Mode::News => {
                if matches!(key, DoorKey::Escape | DoorKey::Char('q')) {
                    self.mode = Mode::Bridge;
                }
                Flow::Stay
            }
            Mode::Crew(_) => {
                self.crew_key(key, ctx);
                Flow::Stay
            }
            Mode::Jobs(_) => {
                self.jobs_key(key, ctx);
                Flow::Stay
            }
            Mode::Society(..) => {
                self.society_key(key, ctx);
                Flow::Stay
            }
        }
    }

    fn render(&self, canvas: &mut Canvas<'_>, env: &DoorEnv) {
        canvas.clear();
        if ui::draw(self, canvas, env) {
            return;
        }
        let height = usize::from(canvas.height());
        let lines = self.lines(height);
        // Keep the newest log lines visible when the screen is short.
        let skip = lines.len().saturating_sub(height);
        let skip = if matches!(
            self.mode,
            Mode::Market(_) | Mode::Yard(_) | Mode::Crew(_) | Mode::Jobs(_)
        ) {
            0
        } else {
            skip
        };
        for (y, (text, style)) in lines.iter().skip(skip).enumerate() {
            let Ok(y) = u16::try_from(y) else { break };
            canvas.text(1, y, text, *style);
        }
    }

    fn captures_text(&self) -> bool {
        // The convoy line takes every key, a `?` included.
        matches!(self.mode, Mode::Society(_, _, Some(_)))
    }

    fn look_around(&self, _env: &DoorEnv) -> Vec<String> {
        let mut text: Vec<String> = self.lines(40).into_iter().map(|(t, _)| t).collect();
        if let Some(player) = &self.player {
            text.push(format!(
                "(Standing as {}; wear worst {}%.)",
                player.captain,
                player.worst_wear_pct()
            ));
        }
        text
    }
}
