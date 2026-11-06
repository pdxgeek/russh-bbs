//! One hero's game: the pure rules engine. State plus an [`Action`] gives new state plus
//! [`Event`]s. Time comes in as a calendar day number and every roll comes from the hero's own
//! generator, which is kept in the save, so a recorded run replays exactly and reopening a save
//! cannot re-roll an outcome.

use russh_bbs_door::DoorRng;

use crate::combat::{Fight, FightAct};
use crate::company::SocialAct;
use crate::content::{Attr, Content, Gain, Intent, RoomKind, RoomOption, Weapon};
use crate::dungeon::{self, Dir, Level};
use crate::hero::{Derived, Hero, derive, fatigue_steps, modifier};
use crate::rules::{
    BASE_CARRY, DEPTH_MAX, NIGHT_BELOW_TURNS, RECOVERY_DAYS, RESCUE_TURNS, REST_TURNS,
    TURNS_PER_DAY,
};
use crate::social::SocialChange;
use crate::town::TownAct;
use crate::world::{Change, Fallen, World};

/// How a line of the log is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Plain,
    Good,
    Bad,
    Dim,
}

/// What happened, for the screen and for tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    Say(Tone, String),
    /// An enemy's next action was shown to the hero.
    Announced {
        enemy: usize,
        intent: Intent,
    },
    /// An enemy did what it had announced.
    Acted {
        enemy: usize,
        intent: Intent,
        damage: i32,
        dodged: bool,
    },
    /// The hero spoiled an enemy's announced action.
    Interrupted {
        enemy: usize,
    },
}

/// Why an action was not taken. Nothing changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refused(pub(crate) String);

impl Refused {
    pub(crate) fn new(text: &str) -> Self {
        Self(text.to_owned())
    }
}

/// Where the hero is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Place {
    Town,
    Delve {
        depth: u8,
        room: u8,
    },
    /// Died below; may be carried home until `since + RECOVERY_DAYS`.
    Dead {
        depth: u8,
        room: u8,
        since: i64,
    },
}

/// What has been done today. Cleared each morning: the dungeon shifts in the night.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Today {
    /// Depth, room, option index (in the template) already done.
    pub(crate) spent: Vec<(u8, u8, u8)>,
    /// Depth, room: the pack there no longer bars the way.
    pub(crate) cleared: Vec<(u8, u8)>,
    /// Depth, room, state: 1 spotted, 2 sprung or disarmed.
    pub(crate) traps: Vec<(u8, u8, u8)>,
    /// Drinks taken at the tavern.
    pub(crate) drinks: u8,
    /// Games of dice played.
    pub(crate) gambles: u8,
    /// Drills at the guild hall.
    pub(crate) trained: u8,
    /// Whether the hero has called on a companion.
    pub(crate) courted: bool,
    /// Market stalls already bought from, by index.
    pub(crate) bought: Vec<u8>,
    /// Levels a rumour has opened the map of for the next descent.
    pub(crate) tips: Vec<u8>,
    /// Thrown out of the dice game for the night.
    pub(crate) shut_out: bool,
    /// Challenges made in the duel ring today.
    pub(crate) bouts: u8,
    /// Whom the hero has already fought today.
    pub(crate) sparred: Vec<String>,
}

impl Today {
    pub(crate) fn is_cleared(&self, depth: u8, room: u8) -> bool {
        self.cleared.contains(&(depth, room))
    }

    pub(crate) fn trap_state(&self, depth: u8, room: u8) -> u8 {
        self.traps
            .iter()
            .find(|(d, r, _)| *d == depth && *r == room)
            .map_or(0, |(_, _, s)| *s)
    }

    fn set_trap(&mut self, depth: u8, room: u8, state: u8) {
        if let Some(slot) = self
            .traps
            .iter_mut()
            .find(|(d, r, _)| *d == depth && *r == room)
        {
            slot.2 = state;
        } else {
            self.traps.push((depth, room, state));
        }
    }
}

/// Something the hero can do in the room they stand in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OptionView {
    pub(crate) label: String,
    pub(crate) cost: u32,
    /// The roll it asks for, if any.
    pub(crate) roll: Option<(Attr, u32)>,
    pub(crate) what: OptionWhat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OptionWhat {
    /// The room template's option, by index.
    Template(usize),
    /// Disarm a spotted trap.
    Disarm,
    /// Carry a fallen hero home.
    Rescue(String),
    /// The dungeon event's option, by index.
    Event(usize),
}

/// A thing the hero can try to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    /// From town: to level 1, or by a shortcut to its level.
    Descend(u8),
    Move(Dir),
    /// Down the stairs, from the way-down room.
    Stairs,
    /// Entry `n` of the room's current option list.
    Option(usize),
    Rest,
    /// Back up to town.
    Climb,
    /// Give up waiting to be rescued.
    GiveUp,
    Buy(String),
    Raise(Attr),
    Fight(FightAct),
    /// Something in the town's places.
    Town(TownAct),
    /// Something about other heroes, a questline or the Furnace's verdict.
    Social(SocialAct),
}

/// One hero's whole game.
#[derive(Debug, Clone)]
pub(crate) struct Game {
    pub(crate) hero: Hero,
    pub(crate) rng: DoorRng,
    pub(crate) place: Place,
    pub(crate) fight: Option<Fight>,
    /// The calendar day the hero last woke on.
    pub(crate) day: i64,
    pub(crate) epoch: u16,
    pub(crate) season: u32,
    /// Per level: a bitmask of rooms the hero has been in or seen.
    pub(crate) explored: Vec<u32>,
    pub(crate) today: Today,
    pub(crate) prev_room: u8,
    /// The shared world as the hero last saw it (not saved).
    pub(crate) world: World,
    /// World changes not yet handed to the board (not saved).
    pub(crate) changes: Vec<Change>,
}

/// Event options are remembered as spent at this offset in the room's option index.
const EVENT_SPENT: u8 = 100;

pub(crate) fn say(ev: &mut Vec<Event>, tone: Tone, text: impl Into<String>) {
    ev.push(Event::Say(tone, text.into()));
}

fn pct(amount: u32, loot: i32) -> u32 {
    let factor = u32::try_from((100 + loot).max(0)).unwrap_or(100);
    amount * factor / 100
}

impl Game {
    /// A new game for a new hero.
    pub(crate) fn new(hero: Hero, rng: DoorRng, day: i64, season: u32, world: World) -> Self {
        Self {
            hero,
            rng,
            place: Place::Town,
            fight: None,
            day,
            epoch: world.epoch,
            season,
            explored: vec![0; usize::from(DEPTH_MAX)],
            today: Today::default(),
            prev_room: 0,
            world,
            changes: Vec::new(),
        }
    }

    pub(crate) fn derived(&self, content: &Content) -> Derived {
        derive(&self.hero, content)
    }

    pub(crate) fn depth(&self) -> u8 {
        match self.place {
            Place::Delve { depth, .. } | Place::Dead { depth, .. } => depth,
            Place::Town => 0,
        }
    }

    pub(crate) fn weapon<'a>(&self, content: &'a Content) -> Option<&'a Weapon> {
        content.weapon(&self.hero.weapon)
    }

    pub(crate) fn emit(&mut self, change: Change) {
        self.world.apply(&change);
        self.changes.push(change);
    }

    /// Take the world changes made since the last call.
    pub(crate) fn take_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }

    // ---- rolls and small effects ----

    pub(crate) fn roll_check(&mut self, d: &Derived, attr: Attr, dc: u32, bonus: i32) -> bool {
        let roll = i32::try_from(self.rng.range(1, 20)).unwrap_or(10);
        if roll == 20 {
            return true;
        }
        if roll == 1 {
            return false;
        }
        let total = roll + modifier(d.attr(attr)) + bonus - fatigue_steps(self.hero.fatigue);
        total >= i32::try_from(dc).unwrap_or(i32::MAX)
    }

    pub(crate) fn add_fatigue(&mut self, amount: i32, d: &Derived) {
        let change = if amount > 0 {
            let scaled = (amount * (100 - d.fatigue_guard) + 99) / 100;
            scaled.max(1)
        } else {
            amount
        };
        let now = i32::from(self.hero.fatigue) + change;
        self.hero.fatigue = u8::try_from(now.clamp(0, 100)).unwrap_or(100);
    }

    pub(crate) fn heal(&mut self, amount: i32, d: &Derived) {
        self.hero.hp = (self.hero.hp + amount).min(d.max_hp);
    }

    pub(crate) fn gain_xp(&mut self, amount: u32, ev: &mut Vec<Event>) {
        let levels = self.hero.gain_xp(amount);
        if levels > 0 {
            say(
                ev,
                Tone::Good,
                format!(
                    "You reach level {}! Skill points to spend: {}.",
                    self.hero.level, self.hero.skill_points
                ),
            );
        }
    }

    pub(crate) fn spend(&mut self, turns: u32) -> Result<(), Refused> {
        if self.hero.turns < turns {
            return Err(Refused::new("You do not have enough torch-turns left."));
        }
        self.hero.turns -= turns;
        Ok(())
    }

    // ---- the morning ----

    /// Start a new day for the hero. Does nothing if `today` is not after the hero's day.
    pub(crate) fn new_day(&mut self, content: &Content, today: i64) -> Vec<Event> {
        let mut ev = Vec::new();
        if today <= self.day {
            return ev;
        }
        let elapsed = today - self.day;
        self.day = today;
        self.today = Today::default();
        self.hero.turns = TURNS_PER_DAY;
        if self.epoch != self.world.epoch {
            self.epoch = self.world.epoch;
            self.explored = vec![0; usize::from(DEPTH_MAX)];
            if matches!(self.place, Place::Delve { .. }) {
                self.place = Place::Town;
                self.fight = None;
                say(
                    &mut ev,
                    Tone::Bad,
                    "The Emberdeep collapsed and reshaped itself in the night. You were thrown out into the town.",
                );
            }
        }
        self.age_wounds(content, elapsed, &mut ev);
        self.town_morning(content, today, &mut ev);
        let d = self.derived(content);
        match self.place.clone() {
            Place::Town => {
                self.hero.hp = d.max_hp;
                self.hero.fatigue = 0;
            }
            Place::Delve { depth, room } => self.night_below(content, depth, room, &d, &mut ev),
            Place::Dead { since, .. } => self.dead_morning(content, today, since, &mut ev),
        }
        ev
    }

    fn age_wounds(&mut self, content: &Content, elapsed: i64, ev: &mut Vec<Event>) {
        let step = u8::try_from(elapsed.clamp(1, 255)).unwrap_or(u8::MAX);
        let mut healed = Vec::new();
        for (id, days) in &mut self.hero.wounds {
            if *days <= step {
                healed.push(id.clone());
            } else {
                *days -= step;
            }
        }
        self.hero.wounds.retain(|(_, days)| *days > 0);
        for id in healed {
            self.hero.wounds.retain(|(w, _)| *w != id);
            let Some(def) = content.wound(&id) else {
                continue;
            };
            let mut line = if def.boon {
                format!("Your {} wears off.", def.name.to_lowercase())
            } else {
                format!("Your {} has healed.", def.name.to_lowercase())
            };
            if let Some(scar) = &def.scar
                && def.scar_pct > 0
                && self.rng.below(100) < def.scar_pct
                && self.hero.can_scar(scar)
                && let Some(scar_def) = content.scar(scar)
            {
                self.hero.scars.push(scar.clone());
                line = format!(
                    "Your {} has healed, leaving a scar: {}.",
                    def.name.to_lowercase(),
                    scar_def.name
                );
            }
            say(ev, Tone::Dim, line);
        }
    }

    fn night_below(
        &mut self,
        content: &Content,
        depth: u8,
        room: u8,
        d: &Derived,
        ev: &mut Vec<Event>,
    ) {
        self.hero.turns = TURNS_PER_DAY.saturating_sub(NIGHT_BELOW_TURNS).max(5);
        self.heal(d.max_hp / 4, d);
        self.hero.fatigue = self.hero.fatigue.saturating_sub(30);
        say(
            ev,
            Tone::Bad,
            "You spent the night below. Your torches burned low keeping watch.",
        );
        if self.rng.chance(2, 5) {
            say(ev, Tone::Bad, "Something found your camp in the dark.");
            let level = dungeon::generate(content, self.season, self.epoch, depth);
            let band = dungeon::band_of(depth);
            let strays = dungeon::pack_at(content, &level, self.season, self.epoch, room, self.day);
            let pack: Vec<String> = strays
                .iter()
                .filter(|m| !m.guardian && m.band == u32::from(band))
                .take(2)
                .map(|m| m.id.clone())
                .collect();
            let pack = if pack.is_empty() {
                vec![
                    content
                        .bestiary
                        .monster
                        .iter()
                        .find(|m| !m.guardian && m.band == u32::from(band))
                        .map(|m| m.id.clone())
                        .unwrap_or_default(),
                ]
            } else {
                pack
            };
            self.start_fight(content, &pack, false, ev);
            self.auto_resolve(content, ev);
        }
    }

    fn dead_morning(&mut self, content: &Content, today: i64, since: i64, ev: &mut Vec<Event>) {
        let carried = self
            .world
            .fallen_of(&self.hero.name, today)
            .and_then(|f| f.rescued_by.clone().map(|by| (by, f.depth)));
        if let Some((by, depth)) = carried {
            self.place = Place::Town;
            self.hero.social.rescued = self.hero.social.rescued.saturating_add(1);
            let family = self.world.commons.companions(&self.hero.name, &by);
            let d = self.derived(content);
            if family {
                // A companion carries you with care: no wound, and you wake on your feet.
                self.hero.hp = d.max_hp * 3 / 4;
                self.hero.fatigue = 30;
                say(
                    ev,
                    Tone::Good,
                    format!(
                        "{by}, of your company, carried you home. Your gear and your pack came with you."
                    ),
                );
            } else {
                self.hero.inflict(content, "dragged-home");
                let d = self.derived(content);
                self.hero.hp = d.max_hp / 2;
                self.hero.fatigue = 60;
                say(
                    ev,
                    Tone::Good,
                    "Someone found you in the dark and carried you home. Your gear and your pack came with you.",
                );
                // The rescued hero thanks the rescuer out of their own purse; the note is
                // paid when the rescuer next opens the door.
                let thanks = (self.hero.coin / 5).min(10 + 3 * u32::from(depth));
                if thanks > 0 {
                    self.hero.coin -= thanks;
                    let note =
                        crate::social::Commons::thanks_note(&self.hero.name, &by, today, thanks);
                    self.emit(Change::Social(SocialChange::Note(note)));
                    say(
                        ev,
                        Tone::Dim,
                        format!("You send {by} {thanks} coin in thanks."),
                    );
                }
            }
        } else if today - since >= RECOVERY_DAYS {
            self.wake_in_temple(content, ev);
        } else {
            let left = RECOVERY_DAYS - (today - since);
            say(
                ev,
                Tone::Dim,
                format!("Your body lies in the dark. {left} day(s) left for someone to find it."),
            );
        }
    }

    fn wake_in_temple(&mut self, content: &Content, ev: &mut Vec<Event>) {
        if let Place::Dead { depth, .. } = self.place {
            self.post_epitaph(depth);
        }
        self.place = Place::Town;
        let kept = self.town_fx(content).reliquary.then(|| {
            (
                self.hero.weapon.clone(),
                std::mem::take(&mut self.hero.wkit),
            )
        });
        self.hero.reset_gear(content);
        if let Some((weapon, kit)) = kept {
            self.hero.weapon = weapon;
            self.hero.wkit = kit;
            say(
                ev,
                Tone::Good,
                "The reliquary kept your weapon safe. Your armour and your pack are lost in the deep.",
            );
        }
        self.hero.found.clear();
        self.hero.coin -= self.hero.coin / 4;
        self.hero.salvage = 0;
        self.hero.inflict(content, "weakened");
        let d = self.derived(content);
        self.hero.hp = d.max_hp;
        self.hero.fatigue = 0;
        say(
            ev,
            Tone::Bad,
            "No one came. You wake in the temple, weakened. Your gear and your pack are lost in the deep.",
        );
    }

    // ---- reading the room ----

    /// Whether a pack bars the way out of the room the hero is in.
    pub(crate) fn is_hostile(&self, content: &Content, level: &Level, room: u8) -> bool {
        let Some(r) = level.room(room) else {
            return false;
        };
        let depth = level.depth;
        match r.kind {
            RoomKind::Lair => !self.pack_cleared(depth, room),
            RoomKind::Guardian => {
                !self.world.is_slain(dungeon::band_of(depth))
                    && !self.pack_cleared(depth, room)
                    && content
                        .guardian(u32::from(dungeon::band_of(depth)))
                        .is_some()
            }
            _ => false,
        }
    }

    /// What the hero may do in this room, by build.
    pub(crate) fn options(&self, content: &Content) -> Vec<OptionView> {
        let Place::Delve { depth, room } = self.place else {
            return Vec::new();
        };
        if self.fight.is_some() {
            return Vec::new();
        }
        let level = dungeon::generate(content, self.season, self.epoch, depth);
        let Some(template) = level.template(content, room) else {
            return Vec::new();
        };
        let d = self.derived(content);
        let mut out = Vec::new();
        for (i, o) in template.option.iter().enumerate() {
            let idx = u8::try_from(i).unwrap_or(u8::MAX);
            if o.once && self.today.spent.contains(&(depth, room, idx)) {
                continue;
            }
            // A cleared room has nothing left to fight, sneak past or stare down.
            let about_the_pack = o.ok.iter().any(|g| matches!(g, Gain::Fight | Gain::Clear));
            if about_the_pack && self.pack_cleared(depth, room) {
                continue;
            }
            if !o.need.iter().all(|n| self.hero.meets(&d, n)) {
                continue;
            }
            out.push(OptionView {
                label: o.label.clone(),
                cost: o.cost,
                roll: o.check.map(|c| (c.attr, c.dc)),
                what: OptionWhat::Template(i),
            });
        }
        if let Some(event) = level.event(content, room) {
            for (i, o) in event.option.iter().enumerate() {
                let idx = EVENT_SPENT + u8::try_from(i).unwrap_or(0);
                if o.once && self.today.spent.contains(&(depth, room, idx)) {
                    continue;
                }
                if !o.need.iter().all(|n| self.hero.meets(&d, n)) {
                    continue;
                }
                out.push(OptionView {
                    label: o.label.clone(),
                    cost: o.cost,
                    roll: o.check.map(|c| (c.attr, c.dc)),
                    what: OptionWhat::Event(i),
                });
            }
        }
        if self.today.trap_state(depth, room) == 1
            && let Some(trap) =
                dungeon::trap_at(content, &level, self.season, self.epoch, room, self.day)
        {
            let roll = (!d.has_tag("disarm")).then_some((Attr::Finesse, trap.dc));
            out.push(OptionView {
                label: format!("Disarm the {}", trap.name.to_lowercase()),
                cost: 1,
                roll,
                what: OptionWhat::Disarm,
            });
        }
        for f in &self.world.fallen {
            if f.depth == depth
                && f.room == room
                && f.rescued_by.is_none()
                && f.name != self.hero.name
                && self.day - f.day <= RECOVERY_DAYS
            {
                let family = self.world.commons.companions(&self.hero.name, &f.name);
                out.push(OptionView {
                    label: format!("Carry {} home", f.name),
                    cost: if family {
                        RESCUE_TURNS - 1
                    } else {
                        RESCUE_TURNS
                    },
                    roll: None,
                    what: OptionWhat::Rescue(f.name.clone()),
                });
            }
        }
        out
    }

    /// Turns the climb back to town costs from where the hero stands.
    pub(crate) fn ascent_cost(&self, content: &Content) -> u32 {
        let depth = self.depth();
        if depth == 0 {
            return 0;
        }
        let d = self.derived(content);
        let mut best = i64::from(depth);
        for (at, kind) in &self.ways_down(content) {
            if *at <= depth {
                best = best.min(i64::from(depth - at) + i64::from(kind.flat_cost()));
            }
        }
        let mut cost = (best - i64::from(d.climb_ease)).max(1);
        let carried = i64::from(self.hero.salvage);
        let allowed = i64::from(d.carry.max(BASE_CARRY / 2));
        if carried > allowed {
            cost += (carried - allowed + 9) / 10;
        }
        if self.hero.fatigue >= 75 {
            cost += 1;
        }
        if self.escorting(content) {
            cost += 1;
        }
        u32::try_from(cost).unwrap_or(u32::MAX)
    }

    // ---- actions ----

    /// Do something. Nothing changes if it is refused.
    pub(crate) fn apply(
        &mut self,
        content: &Content,
        action: Action,
    ) -> Result<Vec<Event>, Refused> {
        let mut ev = self.apply_inner(content, action)?;
        self.story_advance(content, &mut ev);
        Ok(ev)
    }

    fn apply_inner(&mut self, content: &Content, action: Action) -> Result<Vec<Event>, Refused> {
        let mut ev = Vec::new();
        if self.fight.is_some() {
            return match action {
                Action::Fight(act) => {
                    self.fight_action(content, &act, &mut ev)?;
                    Ok(ev)
                }
                _ => Err(Refused::new("You are in a fight.")),
            };
        }
        match (&self.place.clone(), action) {
            (Place::Dead { .. }, Action::GiveUp) => self.wake_in_temple(content, &mut ev),
            (Place::Dead { .. }, _) => {
                return Err(Refused::new("You are dead. Wait to be found, or give up."));
            }
            (Place::Town, Action::Descend(depth)) => self.descend(content, depth, &mut ev)?,
            (Place::Town, Action::Buy(id)) => {
                self.hero.take_node(content, &id).map_err(Refused::new)?;
                let name = content.node(&id).map_or("a skill", |n| n.name.as_str());
                say(&mut ev, Tone::Good, format!("You learn {name}."));
            }
            (Place::Town, Action::Town(act)) => self.town_act(content, act, &mut ev)?,
            (_, Action::Social(act @ (SocialAct::Verdict(_) | SocialAct::Choose { .. })))
                if !matches!(self.place, Place::Dead { .. }) =>
            {
                self.social_act(content, act, &mut ev)?;
            }
            (Place::Town, Action::Social(act)) => self.social_act(content, act, &mut ev)?,
            (Place::Town, Action::Raise(attr)) => {
                self.hero.raise(attr).map_err(Refused::new)?;
                say(
                    &mut ev,
                    Tone::Good,
                    format!("Your {} rises.", attr.label().to_lowercase()),
                );
            }
            (Place::Delve { depth, room }, action) => {
                self.delve_action(content, *depth, *room, &action, &mut ev)?;
            }
            _ => return Err(Refused::new("You cannot do that here.")),
        }
        Ok(ev)
    }

    fn descend(
        &mut self,
        content: &Content,
        depth: u8,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let ways = self.ways_down(content);
        let cost = if depth == 1 {
            1
        } else {
            ways.iter()
                .find(|(d, _)| *d == depth)
                .map(|(_, k)| k.flat_cost())
                .ok_or_else(|| Refused::new("There is no shortcut to that level."))?
        };
        self.spend(cost)?;
        let level = dungeon::generate(content, self.season, self.epoch, depth);
        self.enter_level(&level, level.entry, ev);
        self.prev_room = level.entry;
        if depth == 1 {
            say(
                ev,
                Tone::Plain,
                "You light your torch and go down the Emberdeep's first stair.",
            );
        } else {
            let kind = ways
                .iter()
                .find(|(d, _)| *d == depth)
                .map_or("shortcut", |(_, k)| k.label());
            say(
                ev,
                Tone::Good,
                format!("You ride the {kind} down to level {depth}."),
            );
        }
        Ok(())
    }

    fn enter_level(&mut self, level: &Level, entry: u8, ev: &mut Vec<Event>) {
        let depth = level.depth;
        self.place = Place::Delve { depth, room: entry };
        self.hero.best_depth = self.hero.best_depth.max(depth);
        self.mark(depth, entry);
        if let Some(at) = self.today.tips.iter().position(|d| *d == depth) {
            self.today.tips.remove(at);
            if let Some(mask) = self.explored.get_mut(usize::from(depth) - 1) {
                *mask = level.all_mask();
            }
            say(
                ev,
                Tone::Good,
                "The rumour was good: the whole level is on your map.",
            );
        }
    }

    fn mark(&mut self, depth: u8, room: u8) {
        if let Some(mask) = self.explored.get_mut(usize::from(depth) - 1) {
            *mask |= 1 << room;
        }
    }

    fn delve_action(
        &mut self,
        content: &Content,
        depth: u8,
        room: u8,
        action: &Action,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let level = dungeon::generate(content, self.season, self.epoch, depth);
        if self.hero.turns == 0 && !matches!(action, Action::Climb) {
            return Err(Refused::new(
                "Your torch is out. All you can do is climb in the dark.",
            ));
        }
        match action {
            Action::Move(dir) => self.step(content, &level, room, *dir, ev),
            Action::Stairs => self.stairs(content, &level, room, ev),
            Action::Option(i) => {
                let options = self.options(content);
                let view = options
                    .get(*i)
                    .cloned()
                    .ok_or_else(|| Refused::new("There is no such thing to do."))?;
                self.spend(view.cost)?;
                self.run_option(content, &level, room, &view, ev);
                Ok(())
            }
            Action::Rest => {
                if self.is_hostile(content, &level, room) {
                    return Err(Refused::new("You cannot rest with a pack watching you."));
                }
                self.spend(REST_TURNS)?;
                let d = self.derived(content);
                self.heal(d.max_hp / 5, &d);
                self.add_fatigue(-20, &d);
                say(
                    ev,
                    Tone::Good,
                    "You make a cold camp and rest your legs. You feel steadier.",
                );
                Ok(())
            }
            Action::Climb => {
                self.climb(content, ev);
                Ok(())
            }
            _ => Err(Refused::new("You cannot do that here.")),
        }
    }

    fn step(
        &mut self,
        content: &Content,
        level: &Level,
        room: u8,
        dir: Dir,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let here = level
            .room(room)
            .ok_or_else(|| Refused::new("You are lost."))?;
        let target = here.links[dir.index()]
            .ok_or_else(|| Refused::new("There is no way that direction."))?;
        if self.is_hostile(content, level, room) && target != self.prev_room {
            return Err(Refused::new(
                "The pack bars the way. Fight, deal with it, or go back.",
            ));
        }
        let d = self.derived(content);
        let extra = level
            .template(content, target)
            .map_or(1, |t| t.move_cost.max(1));
        let cost = if extra > 1 && d.has_tag("pathfind") {
            1
        } else {
            extra
        };
        self.spend(cost)?;
        self.prev_room = room;
        self.place = Place::Delve {
            depth: level.depth,
            room: target,
        };
        let fresh = self
            .explored
            .get(usize::from(level.depth) - 1)
            .is_some_and(|m| m & (1 << target) == 0);
        self.mark(level.depth, target);
        if fresh && let Some(t) = level.room(target) {
            self.note_new_room(t.kind);
        }
        self.add_fatigue(1, &d);
        let name = level
            .template(content, target)
            .map_or("a room", |t| t.name.as_str());
        say(ev, Tone::Dim, format!("You go {} to {name}.", dir.word()));
        if self.is_hostile(content, level, target) {
            say(ev, Tone::Bad, "A pack is here, and it has seen you.");
        }
        if let Some(t) = level.room(target) {
            self.contract_room(content, t.kind, level.depth, ev);
        }
        self.check_trap(content, level, target, &d, ev);
        Ok(())
    }

    fn check_trap(
        &mut self,
        content: &Content,
        level: &Level,
        room: u8,
        d: &Derived,
        ev: &mut Vec<Event>,
    ) {
        let depth = level.depth;
        if self.today.trap_state(depth, room) != 0 {
            return;
        }
        let Some(trap) = dungeon::trap_at(content, level, self.season, self.epoch, room, self.day)
        else {
            return;
        };
        let spotted = self.roll_check(d, Attr::Wits, trap.dc, d.trap_sense);
        if spotted {
            self.today.set_trap(depth, room, 1);
            say(
                ev,
                Tone::Good,
                format!(
                    "You spot a {} and step around it.",
                    trap.name.to_lowercase()
                ),
            );
        } else {
            self.today.set_trap(depth, room, 2);
            self.spring(content, trap.damage, trap.wound.as_deref(), &trap.name, ev);
        }
    }

    fn spring(
        &mut self,
        content: &Content,
        damage: [u32; 2],
        wound: Option<&str>,
        name: &str,
        ev: &mut Vec<Event>,
    ) {
        let hit = i32::try_from(self.rng.range(damage[0], damage[1])).unwrap_or(5);
        say(
            ev,
            Tone::Bad,
            format!(
                "The {} springs! You lose {hit} hit points.",
                name.to_lowercase()
            ),
        );
        self.hero.hp -= hit;
        if let Some(w) = wound
            && self.rng.chance(3, 5)
        {
            self.hero.inflict(content, w);
            let label = content
                .wound(w)
                .map_or(w, |def| def.name.as_str())
                .to_lowercase();
            say(ev, Tone::Bad, format!("You are wounded: {label}."));
        }
        if self.hero.hp <= 0 {
            self.hero_down(content, ev);
        }
    }

    fn stairs(
        &mut self,
        content: &Content,
        level: &Level,
        room: u8,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let depth = level.depth;
        if room != level.exit {
            return Err(Refused::new("The stairs down are in another room."));
        }
        if self.is_hostile(content, level, room) {
            return Err(Refused::new("The guardian holds the stair."));
        }
        if depth >= DEPTH_MAX {
            return Err(Refused::new(
                "There is nothing deeper. The Cold Furnace is the bottom of the Emberdeep.",
            ));
        }
        self.spend(1)?;
        let next = dungeon::generate(content, self.season, self.epoch, depth + 1);
        self.enter_level(&next, next.entry, ev);
        self.prev_room = next.entry;
        say(
            ev,
            Tone::Plain,
            format!("You go down to level {}.", depth + 1),
        );
        Ok(())
    }

    fn climb(&mut self, content: &Content, ev: &mut Vec<Event>) {
        let depth = self.depth();
        let cost = self.ascent_cost(content);
        let have = self.hero.turns;
        let short = cost.saturating_sub(have);
        self.hero.turns = have.saturating_sub(cost);
        let d = self.derived(content);
        if short == 0 {
            say(
                ev,
                Tone::Plain,
                format!("You climb back to Cindermere ({cost} turn(s))."),
            );
        } else {
            say(
                ev,
                Tone::Bad,
                format!("Your torch gutters out. You climb the last {short} turn(s) in the dark."),
            );
            let band = i32::from(dungeon::band_of(depth));
            for _ in 0..short {
                let hit = 3 + 2 * band + i32::try_from(self.rng.below(3)).unwrap_or(0);
                self.hero.hp -= hit;
                self.add_fatigue(10, &d);
                say(
                    ev,
                    Tone::Bad,
                    format!("Something in the dark claws at you: {hit} hit points."),
                );
                if self.hero.hp <= 0 {
                    if self.hero_down(content, ev) {
                        return;
                    }
                    self.hero.hp = 1;
                }
            }
        }
        self.arrive_in_town(depth, ev);
    }

    fn arrive_in_town(&mut self, depth: u8, ev: &mut Vec<Event>) {
        self.place = Place::Town;
        self.fight = None;
        self.hero.stash = self.hero.stash.saturating_add(self.hero.salvage);
        self.hero.salvage = 0;
        for item in std::mem::take(&mut self.hero.found) {
            if self.hero.pack.len() < crate::rules::PACK_MAX {
                self.hero.pack.push(item);
            } else {
                // No room at home: the find is broken up for salvage.
                self.hero.stash = self.hero.stash.saturating_add(5);
            }
        }
        let name = self.hero.name.clone();
        if depth > 0 {
            self.emit(Change::Depth(name.clone(), self.hero.best_depth));
            self.emit(Change::Social(SocialChange::PartyDepth {
                who: name,
                depth: self.hero.best_depth,
            }));
        }
        say(
            ev,
            Tone::Good,
            "You are home in Cindermere. The salvage goes into the town store.",
        );
    }

    // ---- options ----

    fn run_option(
        &mut self,
        content: &Content,
        level: &Level,
        room: u8,
        view: &OptionView,
        ev: &mut Vec<Event>,
    ) {
        let depth = level.depth;
        match &view.what {
            OptionWhat::Template(i) => {
                let Some(template) = level.template(content, room) else {
                    return;
                };
                let Some(option) = template.option.get(*i) else {
                    return;
                };
                if option.once {
                    self.today
                        .spent
                        .push((depth, room, u8::try_from(*i).unwrap_or(u8::MAX)));
                }
                self.resolve_option(content, level, room, option, ev);
            }
            OptionWhat::Event(i) => {
                let Some(option) = level.event(content, room).and_then(|e| e.option.get(*i)) else {
                    return;
                };
                if option.once {
                    self.today.spent.push((
                        depth,
                        room,
                        EVENT_SPENT + u8::try_from(*i).unwrap_or(0),
                    ));
                }
                self.resolve_option(content, level, room, option, ev);
            }
            OptionWhat::Disarm => {
                let d = self.derived(content);
                let Some(trap) =
                    dungeon::trap_at(content, level, self.season, self.epoch, room, self.day)
                else {
                    return;
                };
                let ok = d.has_tag("disarm")
                    || self.roll_check(&d, Attr::Finesse, trap.dc, d.trap_sense);
                self.today.set_trap(depth, room, 2);
                if ok {
                    say(
                        ev,
                        Tone::Good,
                        format!("You disarm the {}.", trap.name.to_lowercase()),
                    );
                    self.gain_xp(trap.xp, ev);
                } else {
                    say(ev, Tone::Bad, "Your hands slip.");
                    self.spring(content, trap.damage, trap.wound.as_deref(), &trap.name, ev);
                }
            }
            OptionWhat::Rescue(name) => {
                let d = self.derived(content);
                self.emit(Change::Rescue(name.clone(), self.hero.name.clone()));
                self.hero.social.rescues = self.hero.social.rescues.saturating_add(1);
                let reward = 15 * u32::from(depth);
                self.hero.coin = self.hero.coin.saturating_add(pct(reward, d.loot));
                say(
                    ev,
                    Tone::Good,
                    format!("You haul {name} onto your back. The town will pay you {reward} coin."),
                );
                self.gain_xp(20 + 5 * u32::from(depth), ev);
                self.add_fatigue(8, &d);
            }
        }
    }

    fn resolve_option(
        &mut self,
        content: &Content,
        level: &Level,
        room: u8,
        option: &RoomOption,
        ev: &mut Vec<Event>,
    ) {
        let d = self.derived(content);
        let ok = option
            .check
            .is_none_or(|c| self.roll_check(&d, c.attr, c.dc, 0));
        let (gains, text) = if ok {
            (&option.ok, &option.ok_text)
        } else {
            (&option.fail, &option.fail_text)
        };
        say(ev, if ok { Tone::Good } else { Tone::Bad }, text.clone());
        for gain in gains {
            self.apply_gain(content, level, room, gain, &d, ev);
            if matches!(self.place, Place::Dead { .. }) {
                break;
            }
        }
    }

    fn apply_gain(
        &mut self,
        content: &Content,
        level: &Level,
        room: u8,
        gain: &Gain,
        d: &Derived,
        ev: &mut Vec<Event>,
    ) {
        let depth = level.depth;
        match gain {
            Gain::Coin { amount } => {
                let got = pct(*amount, d.loot);
                self.hero.coin = self.hero.coin.saturating_add(got);
                say(ev, Tone::Plain, format!("+{got} coin"));
            }
            Gain::Salvage { amount } => {
                let got = pct(*amount, d.loot);
                self.hero.salvage = self.hero.salvage.saturating_add(got);
                say(ev, Tone::Plain, format!("+{got} salvage"));
            }
            Gain::Xp { amount } => self.gain_xp(*amount, ev),
            Gain::Heal { amount } => {
                self.heal(i32::try_from(*amount).unwrap_or(0), d);
                say(ev, Tone::Plain, format!("+{amount} hit points"));
            }
            Gain::Hurt { amount } => {
                self.hero.hp -= i32::try_from(*amount).unwrap_or(0);
                say(ev, Tone::Bad, format!("-{amount} hit points"));
                if self.hero.hp <= 0 {
                    self.hero_down(content, ev);
                }
            }
            Gain::Fatigue { amount } => self.add_fatigue(*amount, d),
            Gain::Torch { amount } => {
                let now = i64::from(self.hero.turns) + i64::from(*amount);
                self.hero.turns = u32::try_from(now.clamp(0, 40)).unwrap_or(0);
            }
            Gain::Reveal => {
                if let Some(mask) = self.explored.get_mut(usize::from(depth) - 1) {
                    *mask = level.all_mask();
                }
                say(ev, Tone::Good, "The whole level is on your map now.");
            }
            Gain::Fight => {
                if self.fight.is_none() {
                    let pack: Vec<String> =
                        dungeon::pack_at(content, level, self.season, self.epoch, room, self.day)
                            .iter()
                            .map(|m| m.id.clone())
                            .collect();
                    self.start_fight(
                        content,
                        &pack,
                        level
                            .room(room)
                            .is_some_and(|r| r.kind == RoomKind::Guardian),
                        ev,
                    );
                }
            }
            Gain::Clear => {
                self.today.cleared.push((depth, room));
                self.share_clear(depth, room);
            }
            Gain::Shortcut { shortcut } => {
                let by = self.hero.name.clone();
                self.emit(Change::Shortcut(depth, *shortcut, by));
                say(
                    ev,
                    Tone::Good,
                    format!(
                        "A {} now runs at level {depth}, for everyone.",
                        shortcut.label()
                    ),
                );
            }
            Gain::Wound { id } => {
                self.hero.inflict(content, id);
                let label = content
                    .wound(id)
                    .map_or(id.as_str(), |w| w.name.as_str())
                    .to_lowercase();
                say(ev, Tone::Bad, format!("You are wounded: {label}."));
            }
            Gain::Relic => {
                self.hero.relics = self.hero.relics.saturating_add(1);
                say(ev, Tone::Good, "+1 relic of the old forges");
            }
            Gain::Recipe { id } => self.find_recipe(content, id, ev),
            Gain::Core { id } => self.find_core(content, id, ev),
        }
    }

    pub(crate) fn find_recipe(&mut self, content: &Content, id: &str, ev: &mut Vec<Event>) {
        let name = content.recipe(id).map_or("a recipe", |r| r.name.as_str());
        if content.recipe(id).is_some_and(|r| self.hero.knows(r)) {
            say(
                ev,
                Tone::Dim,
                format!("You already know the pattern for {name}."),
            );
        } else {
            self.hero.learn(id);
            say(ev, Tone::Good, format!("You learn a recipe: {name}."));
        }
    }

    pub(crate) fn find_core(&mut self, content: &Content, id: &str, ev: &mut Vec<Event>) {
        let name = content.core(id).map_or("a core", |c| c.name.as_str());
        if self.hero.cores.len() < crate::rules::CORES_MAX {
            self.hero.cores.push(id.to_owned());
            say(ev, Tone::Good, format!("+1 {name}"));
        } else {
            say(
                ev,
                Tone::Dim,
                format!("You find a {name}, but have no room for it."),
            );
        }
    }

    // ---- death ----

    /// The hero is at zero hit points. Returns `true` if they died, `false` if they survived at
    /// the brink.
    pub(crate) fn hero_down(&mut self, content: &Content, ev: &mut Vec<Event>) -> bool {
        let d = self.derived(content);
        let depth = self.depth();
        let chance = (25 + d.attr(Attr::Spirit) * 2 + d.death_save - i32::from(depth)).clamp(5, 95);
        let roll = i32::try_from(self.rng.below(100)).unwrap_or(99);
        let had_fight = self.fight.take().is_some();
        if roll < chance {
            self.hero.hp = 1;
            self.hero.inflict(content, "brink");
            if had_fight && let Place::Delve { depth, .. } = self.place {
                self.place = Place::Delve {
                    depth,
                    room: self.prev_room,
                };
            }
            say(
                ev,
                Tone::Bad,
                "Everything goes dark. You come to at the brink, alive, and crawl away from the fight.",
            );
            return false;
        }
        let (depth, room) = match self.place {
            Place::Delve { depth, room } | Place::Dead { depth, room, .. } => (depth, room),
            Place::Town => (0, 0),
        };
        self.hero.hp = 0;
        self.hero.deaths = self.hero.deaths.saturating_add(1);
        self.place = Place::Dead {
            depth,
            room,
            since: self.day,
        };
        let name = self.hero.name.clone();
        self.emit(Change::Fall(Fallen {
            name,
            depth,
            room,
            day: self.day,
            rescued_by: None,
        }));
        say(
            ev,
            Tone::Bad,
            format!(
                "You die on level {depth}. Another hero may carry you home within {RECOVERY_DAYS} days."
            ),
        );
        true
    }
}
