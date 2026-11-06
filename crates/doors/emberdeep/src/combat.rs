//! Turn-based combat: one to four enemies, three stances, two rows, and every enemy announcing
//! what it will do next round. The announcement is binding: the engine executes exactly the
//! intent it showed, and a test checks that over many seeded fights. Reading the intents (guard
//! against a strike, evade a heavy blow, interrupt a wind-up, do not hit a bracing enemy) is the
//! skill; a quick-resolve fight skips the reading and costs less time.

use std::fmt::Write as _;

use crate::content::{Content, DamageKind, Intent, MoveKind, Reach, Row};
use crate::dungeon;
use crate::game::{Event, Game, Place, Refused, Tone};
use crate::hero::{Derived, fatigue_steps};
use crate::rules::{FIGHT_TURNS, QUICK_FIGHT_TURNS, QUICK_ROUND_CAP};

/// How the hero stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stance {
    /// Hit harder, get hit harder.
    Aggressive,
    /// Soak blows, hit softer.
    Guarded,
    /// Slip strikes and never be caught by a heavy blow; hit a little softer.
    Evasive,
}

impl Stance {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Aggressive => "aggressive",
            Self::Guarded => "guarded",
            Self::Evasive => "evasive",
        }
    }
}

/// What the hero does in a round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FightAct {
    /// A basic attack on enemy `n`.
    Attack(usize),
    /// A move on enemy `n`.
    Use(String, usize),
    /// Change stance. Free: it takes no round.
    Stance(Stance),
    /// Step to the other row.
    Step,
    /// Try to break off.
    Flee,
    /// Let the fight play itself out (only at the start, never against a guardian).
    Quick,
}

/// One enemy in a fight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Enemy {
    pub(crate) id: String,
    pub(crate) hp: i32,
    pub(crate) row: Row,
    pub(crate) intent: Intent,
    pub(crate) engaged: bool,
    pub(crate) mark_pct: u8,
    pub(crate) sunder: u8,
    pub(crate) bleed: u8,
    pub(crate) rallied: bool,
    pub(crate) interrupted: bool,
}

impl Enemy {
    pub(crate) const fn alive(&self) -> bool {
        self.hp > 0
    }
}

/// A fight in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fight {
    pub(crate) enemies: Vec<Enemy>,
    pub(crate) stance: Stance,
    pub(crate) row: Row,
    pub(crate) focus: u8,
    pub(crate) ward: i32,
    pub(crate) round: u8,
    pub(crate) guardian: bool,
    pub(crate) paid: bool,
}

impl Fight {
    pub(crate) fn alive(&self) -> impl Iterator<Item = (usize, &Enemy)> {
        self.enemies.iter().enumerate().filter(|(_, e)| e.alive())
    }

    fn front_alive(&self) -> bool {
        self.alive().any(|(_, e)| e.row == Row::Front)
    }
}

fn say(ev: &mut Vec<Event>, tone: Tone, text: impl Into<String>) {
    ev.push(Event::Say(tone, text.into()));
}

/// The chance (percent) to avoid an announced blow. The evasive stance beats a heavy blow
/// outright and slips strikes; any stance gets a little of the hero's evasion against anything
/// but a heavy blow.
fn dodge_chance(stance: Stance, intent: Intent, d: &Derived) -> i32 {
    let finesse = d.attr(crate::content::Attr::Finesse);
    let strike_dodge = (35 + d.evade + finesse * 2).clamp(5, 85);
    match (stance, intent) {
        (Stance::Evasive, Intent::Heavy) => 100,
        (Stance::Evasive, Intent::Strike) => strike_dodge,
        (Stance::Evasive, _) => strike_dodge / 2,
        (_, Intent::Heavy) => 0,
        _ => d.evade.clamp(0, 60) / 2,
    }
}

/// What a move does to an enemy that survived it.
fn apply_move_effect(enemy: &mut Enemy, kind: MoveKind, amount: i32) {
    match kind {
        MoveKind::Interrupt => enemy.interrupted = true,
        MoveKind::Mark => enemy.mark_pct = u8::try_from(amount).unwrap_or(30),
        MoveKind::Sunder => {
            enemy.sunder = enemy
                .sunder
                .saturating_add(u8::try_from(amount).unwrap_or(1))
                .min(4);
        }
        MoveKind::Bleed => enemy.bleed = u8::try_from(amount).unwrap_or(2),
        MoveKind::Shove => {
            enemy.row = Row::Back;
            enemy.engaged = false;
        }
        MoveKind::Strike | MoveKind::Mend | MoveKind::Ward => {}
    }
}

fn scale(value: i32, percent: i32) -> i32 {
    value * percent / 100
}

impl Game {
    /// Begin a fight with the monsters named by `ids`.
    pub(crate) fn start_fight(
        &mut self,
        content: &Content,
        ids: &[String],
        guardian: bool,
        ev: &mut Vec<Event>,
    ) {
        let d = self.derived(content);
        let enemies: Vec<Enemy> = ids
            .iter()
            .filter_map(|id| content.monster(id))
            .take(4)
            .map(|m| Enemy {
                id: m.id.clone(),
                hp: i32::try_from(m.hp).unwrap_or(10),
                row: m.row,
                intent: Intent::Strike,
                engaged: false,
                mark_pct: 0,
                sunder: 0,
                bleed: 0,
                rallied: false,
                interrupted: false,
            })
            .collect();
        let mut enemies = enemies;
        if enemies.is_empty() {
            return;
        }
        // A back row with no front row in front of it is simply the front.
        if enemies.iter().all(|e| e.row == Row::Back) {
            for e in &mut enemies {
                e.row = Row::Front;
            }
        }
        let names: Vec<&str> = enemies
            .iter()
            .filter_map(|e| content.monster(&e.id))
            .map(|m| m.name.as_str())
            .collect();
        say(ev, Tone::Bad, format!("A fight! {}.", names.join(", ")));
        self.fight = Some(Fight {
            enemies,
            stance: Stance::Aggressive,
            row: Row::Front,
            focus: u8::try_from(d.start_focus.clamp(0, 5)).unwrap_or(1),
            ward: 0,
            round: 0,
            guardian,
            paid: false,
        });
        self.choose_all_intents(content, ev);
    }

    fn choose_all_intents(&mut self, content: &Content, ev: &mut Vec<Event>) {
        let count = self.fight.as_ref().map_or(0, |f| f.enemies.len());
        for i in 0..count {
            let alive = self.fight.as_ref().is_some_and(|f| f.enemies[i].alive());
            if alive {
                let intent = self.pick_intent(content, i);
                if let Some(f) = self.fight.as_mut() {
                    f.enemies[i].intent = intent;
                    f.enemies[i].interrupted = false;
                }
                ev.push(Event::Announced { enemy: i, intent });
            }
        }
    }

    fn pick_intent(&mut self, content: &Content, i: usize) -> Intent {
        let Some(fight) = self.fight.as_ref() else {
            return Intent::Strike;
        };
        let enemy = &fight.enemies[i];
        let Some(m) = content.monster(&enemy.id) else {
            return Intent::Strike;
        };
        if m.reach == Reach::Melee && fight.row == Row::Back && !enemy.engaged {
            return Intent::Advance;
        }
        let others = fight.alive().any(|(j, _)| j != i);
        let weak = enemy.hp * 100 < i32::try_from(m.hp).unwrap_or(1) * 30;
        let weights: Vec<(Intent, u32)> = m
            .intents
            .iter()
            .map(|w| {
                let mut weight = w.weight;
                if w.intent == Intent::Rally && !others {
                    weight = 0;
                }
                if w.intent == Intent::Brace && weak {
                    weight *= 3;
                }
                (w.intent, weight)
            })
            .collect();
        let total: u32 = weights.iter().map(|(_, w)| *w).sum();
        let mut roll = self.rng.below(total.max(1));
        for (intent, weight) in weights {
            if roll < weight {
                return intent;
            }
            roll -= weight;
        }
        Intent::Strike
    }

    fn can_reach(row: Row, target: Row, reach: Reach) -> bool {
        match reach {
            Reach::Ranged => true,
            Reach::Melee => row == Row::Front && target == Row::Front,
        }
    }

    /// One action in a fight.
    pub(crate) fn fight_action(
        &mut self,
        content: &Content,
        act: &FightAct,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let d = self.derived(content);
        let Some(fight) = self.fight.as_mut() else {
            return Err(Refused("You are not in a fight.".to_owned()));
        };
        match act {
            FightAct::Stance(s) => {
                fight.stance = *s;
                say(ev, Tone::Plain, format!("You take a {} stance.", s.label()));
                return Ok(());
            }
            FightAct::Quick => {
                if fight.round != 0 || fight.paid || fight.guardian {
                    return Err(Refused("You cannot leave this one to chance.".to_owned()));
                }
                fight.paid = true;
                self.hero.turns = self.hero.turns.saturating_sub(QUICK_FIGHT_TURNS);
                say(ev, Tone::Dim, "You fight it out without stopping to think.");
                self.auto_resolve(content, ev);
                return Ok(());
            }
            FightAct::Attack(t) => self.validate_target(content, *t, None, &d)?,
            FightAct::Use(id, t) => self.validate_target(content, *t, Some(id), &d)?,
            FightAct::Step | FightAct::Flee => {}
        }
        self.pay_for_fight();
        self.hero_round(content, act, &d, ev);
        if self.fight.is_some() {
            self.enemy_phase(content, &d, ev);
        }
        if self.fight.is_some() {
            self.end_round(content, ev);
        }
        Ok(())
    }

    fn validate_target(
        &self,
        content: &Content,
        t: usize,
        mv: Option<&String>,
        d: &Derived,
    ) -> Result<(), Refused> {
        let Some(fight) = self.fight.as_ref() else {
            return Ok(());
        };
        let reach;
        let self_only;
        if let Some(id) = mv {
            let Some(def) = content.mv(id).filter(|_| d.moves.contains(id)) else {
                return Err(Refused("You do not know that move.".to_owned()));
            };
            if u32::from(fight.focus) < def.cost {
                return Err(Refused("You do not have the focus for that.".to_owned()));
            }
            reach = def.reach;
            self_only = matches!(def.kind, MoveKind::Mend | MoveKind::Ward);
        } else {
            reach = self.weapon(content).map_or(Reach::Melee, |w| w.reach);
            self_only = false;
        }
        if self_only {
            return Ok(());
        }
        let Some(enemy) = fight.enemies.get(t).filter(|e| e.alive()) else {
            return Err(Refused("There is no such enemy.".to_owned()));
        };
        if !Self::can_reach(fight.row, enemy.row, reach) {
            return Err(Refused(
                if fight.row == Row::Back && reach == Reach::Melee {
                    "You are too far back for that. Step forward, or use something with reach."
                } else {
                    "That one is behind the others. You need reach, or to clear the front."
                }
                .to_owned(),
            ));
        }
        Ok(())
    }

    fn pay_for_fight(&mut self) {
        if let Some(f) = self.fight.as_mut()
            && !f.paid
        {
            f.paid = true;
            self.hero.turns = self.hero.turns.saturating_sub(FIGHT_TURNS);
        }
    }

    fn hero_round(&mut self, content: &Content, act: &FightAct, d: &Derived, ev: &mut Vec<Event>) {
        match act {
            FightAct::Attack(t) => self.hero_attack(content, *t, None, d, ev),
            FightAct::Use(id, t) => self.hero_attack(content, *t, Some(id), d, ev),
            FightAct::Step => {
                if let Some(f) = self.fight.as_mut() {
                    f.row = if f.row == Row::Front {
                        Row::Back
                    } else {
                        Row::Front
                    };
                    let word = if f.row == Row::Front {
                        "forward"
                    } else {
                        "back"
                    };
                    say(ev, Tone::Plain, format!("You step {word}."));
                }
            }
            FightAct::Flee => self.try_flee(d, ev),
            FightAct::Stance(_) | FightAct::Quick => {}
        }
    }

    fn try_flee(&mut self, d: &Derived, ev: &mut Vec<Event>) {
        let Some(fight) = self.fight.as_ref() else {
            return;
        };
        let foes = i32::try_from(fight.alive().count()).unwrap_or(4);
        let mut chance = 45 + d.attr(crate::content::Attr::Finesse) * 2 + d.evade / 2 - 8 * foes;
        if fight.guardian {
            chance -= 25;
        }
        let chance = chance.clamp(10, 90);
        if i32::try_from(self.rng.below(100)).unwrap_or(99) < chance {
            self.fight = None;
            if let Place::Delve { depth, .. } = self.place {
                self.place = Place::Delve {
                    depth,
                    room: self.prev_room,
                };
            }
            say(ev, Tone::Plain, "You break off and run, and get away.");
        } else {
            say(ev, Tone::Bad, "You try to run, but they are on you.");
        }
    }

    fn hero_attack(
        &mut self,
        content: &Content,
        target: usize,
        mv: Option<&String>,
        d: &Derived,
        ev: &mut Vec<Event>,
    ) {
        let Some(weapon) = self.weapon(content) else {
            return;
        };
        let def = mv.and_then(|id| content.mv(id));
        let (power, kind, amount, cost) = def.map_or((100, MoveKind::Strike, 0, 0), |m| {
            (
                i32::try_from(m.power).unwrap_or(100),
                m.kind,
                i32::try_from(m.amount).unwrap_or(0),
                m.cost,
            )
        });
        if let Some(f) = self.fight.as_mut() {
            f.focus = f.focus.saturating_sub(u8::try_from(cost).unwrap_or(0));
        }
        let name = def.map_or("attack", |m| m.name.as_str()).to_lowercase();
        match kind {
            MoveKind::Mend => {
                self.heal(amount, d);
                say(ev, Tone::Good, format!("You {name}: +{amount} hit points."));
                return;
            }
            MoveKind::Ward => {
                if let Some(f) = self.fight.as_mut() {
                    f.ward += amount;
                }
                say(
                    ev,
                    Tone::Good,
                    format!("You {name}: the next {amount} damage is absorbed."),
                );
                return;
            }
            _ => {}
        }
        let roll = i32::try_from(self.rng.range(weapon.damage[0], weapon.damage[1])).unwrap_or(5);
        let attr_bonus = ((d.attr(weapon.attr) - 8) / 2).max(0);
        let mut dmg = scale(
            roll + attr_bonus + d.damage - fatigue_steps(self.hero.fatigue),
            power,
        )
        .max(1);
        let crit = i32::try_from(self.rng.below(100)).unwrap_or(99) < d.crit;
        if crit {
            dmg *= 2;
        }
        let Some(fight) = self.fight.as_mut() else {
            return;
        };
        dmg = scale(
            dmg,
            match fight.stance {
                Stance::Aggressive => 125,
                Stance::Guarded => 75,
                Stance::Evasive => 90,
            },
        );
        let Some(enemy) = fight.enemies.get_mut(target) else {
            return;
        };
        let Some(m) = content.monster(&enemy.id) else {
            return;
        };
        dmg = scale(dmg, 100 + i32::from(enemy.mark_pct));
        let braced = enemy.intent == Intent::Brace;
        if braced {
            dmg /= 2;
        }
        let defense = i32::try_from(m.defense).unwrap_or(0) - i32::from(enemy.sunder);
        dmg = (dmg - defense.max(0)).max(1);
        enemy.hp -= dmg;
        let mut line = format!("You {name} the {}: {dmg}", m.name.to_lowercase());
        if crit {
            line.push_str(" (a critical hit)");
        }
        if braced {
            line.push_str(" (it was braced)");
        }
        say(ev, Tone::Good, line);
        if enemy.alive() {
            apply_move_effect(enemy, kind, amount);
        } else {
            say(
                ev,
                Tone::Good,
                format!("The {} falls.", m.name.to_lowercase()),
            );
        }
        if self
            .fight
            .as_ref()
            .is_some_and(|f| f.alive().next().is_none())
        {
            self.win_fight(content, ev);
        }
    }

    fn enemy_phase(&mut self, content: &Content, d: &Derived, ev: &mut Vec<Event>) {
        let count = self.fight.as_ref().map_or(0, |f| f.enemies.len());
        let mut rally_next = Vec::new();
        for i in 0..count {
            let Some(fight) = self.fight.as_mut() else {
                return;
            };
            let enemy = &mut fight.enemies[i];
            if !enemy.alive() {
                continue;
            }
            let intent = enemy.intent;
            if enemy.interrupted {
                enemy.interrupted = false;
                ev.push(Event::Interrupted { enemy: i });
                let name = content.monster(&enemy.id).map_or("It", |m| m.name.as_str());
                say(ev, Tone::Good, format!("{name} loses its wind-up."));
                continue;
            }
            match intent {
                Intent::Brace => {
                    ev.push(Event::Acted {
                        enemy: i,
                        intent,
                        damage: 0,
                        dodged: false,
                    });
                }
                Intent::Rally => {
                    rally_next.push(i);
                    ev.push(Event::Acted {
                        enemy: i,
                        intent,
                        damage: 0,
                        dodged: false,
                    });
                    say(
                        ev,
                        Tone::Bad,
                        "A rallying cry: its allies hit harder next round.",
                    );
                }
                Intent::Advance => {
                    enemy.engaged = true;
                    ev.push(Event::Acted {
                        enemy: i,
                        intent,
                        damage: 0,
                        dodged: false,
                    });
                    say(ev, Tone::Dim, "It closes in.");
                }
                Intent::Strike | Intent::Heavy | Intent::Shoot | Intent::Breathe => {
                    self.enemy_attack(content, i, d, ev);
                    if self.hero.hp <= 0 {
                        self.hero_down(content, ev);
                        return;
                    }
                }
            }
        }
        if let Some(fight) = self.fight.as_mut() {
            for e in &mut fight.enemies {
                e.rallied = false;
            }
            for i in rally_next {
                for (j, e) in fight.enemies.iter_mut().enumerate() {
                    if j != i && e.alive() {
                        e.rallied = true;
                    }
                }
            }
        }
    }

    fn enemy_attack(&mut self, content: &Content, i: usize, d: &Derived, ev: &mut Vec<Event>) {
        let Some(fight) = self.fight.as_ref() else {
            return;
        };
        let enemy = &fight.enemies[i];
        let intent = enemy.intent;
        let Some(m) = content.monster(&enemy.id) else {
            return;
        };
        let reaches = matches!(intent, Intent::Shoot | Intent::Breathe)
            || m.reach == Reach::Ranged
            || fight.row == Row::Front
            || enemy.engaged;
        if !reaches {
            ev.push(Event::Acted {
                enemy: i,
                intent,
                damage: 0,
                dodged: false,
            });
            say(
                ev,
                Tone::Good,
                format!("The {} swings at where you were.", m.name.to_lowercase()),
            );
            return;
        }
        let stance = fight.stance;
        let rallied = enemy.rallied;
        let attack = i32::try_from(m.attack).unwrap_or(5);
        let spread = i32::try_from(self.rng.range(80, 120)).unwrap_or(100);
        let mut dmg = scale(attack, spread);
        dmg = scale(
            dmg,
            match intent {
                Intent::Heavy => 200,
                Intent::Shoot => 80,
                Intent::Breathe => 150,
                _ => 100,
            },
        );
        if rallied {
            dmg = scale(dmg, 125);
        }
        let name = m.name.to_lowercase();
        let kind = m.kind;
        let chance = dodge_chance(stance, intent, d);
        let dodged = chance >= 100 || i32::try_from(self.rng.below(100)).unwrap_or(99) < chance;
        if dodged {
            ev.push(Event::Acted {
                enemy: i,
                intent,
                damage: 0,
                dodged: true,
            });
            say(
                ev,
                Tone::Good,
                format!(
                    "The {name} {}, and you are not there.",
                    intent
                        .describe()
                        .replace("readies", "swings")
                        .replace("winds up", "brings down")
                ),
            );
            return;
        }
        match stance {
            Stance::Aggressive => dmg = scale(dmg, 125),
            Stance::Guarded => dmg /= 2,
            Stance::Evasive => {}
        }
        let Some(fight) = self.fight.as_mut() else {
            return;
        };
        let absorbed = dmg.min(fight.ward);
        fight.ward -= absorbed;
        dmg -= absorbed;
        if dmg > 0 {
            dmg = (dmg - d.armor).max(1);
        }
        self.hero.hp -= dmg;
        ev.push(Event::Acted {
            enemy: i,
            intent,
            damage: dmg,
            dodged: false,
        });
        let mut line = format!("The {name} hits you for {dmg}.");
        if absorbed > 0 {
            let _ = write!(line, " ({absorbed} absorbed)");
        }
        say(ev, Tone::Bad, line);
        if dmg * 100 >= d.max_hp * 35 && self.rng.chance(7, 10) {
            self.grievous_wound(content, kind, ev);
        }
    }

    fn grievous_wound(&mut self, content: &Content, kind: DamageKind, ev: &mut Vec<Event>) {
        let pool: Vec<&str> = content
            .wounds
            .wound
            .iter()
            .filter(|w| w.source == Some(kind))
            .map(|w| w.id.as_str())
            .collect();
        if pool.is_empty() {
            return;
        }
        let at = self.rng.below(u32::try_from(pool.len()).unwrap_or(1)) as usize;
        let id = pool[at].to_owned();
        self.hero.inflict(content, &id);
        let label = content
            .wound(&id)
            .map_or(id.as_str(), |w| w.name.as_str())
            .to_lowercase();
        say(
            ev,
            Tone::Bad,
            format!("A grievous blow: you are wounded ({label})."),
        );
    }

    fn end_round(&mut self, content: &Content, ev: &mut Vec<Event>) {
        let Some(fight) = self.fight.as_mut() else {
            return;
        };
        for e in &mut fight.enemies {
            if e.alive() && e.bleed > 0 {
                e.hp -= i32::from(e.bleed);
            }
        }
        if fight.alive().next().is_none() {
            say(ev, Tone::Good, "The last of them bleeds out.");
            self.win_fight(content, ev);
            return;
        }
        if !fight.front_alive() {
            for e in &mut fight.enemies {
                if e.alive() {
                    e.row = Row::Front;
                }
            }
        }
        fight.focus = (fight.focus + 1).min(5);
        fight.round = fight.round.saturating_add(1);
        self.choose_all_intents(content, ev);
    }

    fn win_fight(&mut self, content: &Content, ev: &mut Vec<Event>) {
        let Some(fight) = self.fight.take() else {
            return;
        };
        let d = self.derived(content);
        let (mut xp, mut coin, mut salvage) = (0u32, 0u32, 0u32);
        let mut guardian_name = None;
        for e in &fight.enemies {
            if let Some(m) = content.monster(&e.id) {
                xp += m.xp;
                coin += self.rng.range(m.coin[0], m.coin[1]);
                salvage += self.rng.range(m.salvage[0], m.salvage[1]);
                if m.guardian {
                    guardian_name = Some(m.name.clone());
                }
            }
        }
        let factor = u32::try_from((100 + d.loot).max(0)).unwrap_or(100);
        coin = coin * factor / 100;
        salvage = salvage * factor / 100;
        self.hero.coin = self.hero.coin.saturating_add(coin);
        self.hero.salvage = self.hero.salvage.saturating_add(salvage);
        self.hero.kills = self
            .hero
            .kills
            .saturating_add(u32::try_from(fight.enemies.len()).unwrap_or(0));
        for e in &fight.enemies {
            self.note_kill(&e.id);
        }
        say(
            ev,
            Tone::Good,
            format!("Victory! +{xp} xp, +{coin} coin, +{salvage} salvage."),
        );
        self.gain_xp(xp, ev);
        self.add_fatigue(3 + i32::try_from(fight.enemies.len()).unwrap_or(1), &d);
        self.wear_after_fight(content, fight.guardian, ev);
        if let Place::Delve { depth, .. } = self.place {
            let ids: Vec<String> = fight.enemies.iter().map(|e| e.id.clone()).collect();
            self.contract_beast(content, &ids, depth, ev);
            self.drops_after_fight(content, depth, &fight, ev);
        }
        if let Place::Delve { depth, room } = self.place {
            self.today.cleared.push((depth, room));
            self.share_clear(depth, room);
            if let Some(name) = guardian_name {
                let band = dungeon::band_of(depth);
                self.hero.relics = self.hero.relics.saturating_add(1);
                say(
                    ev,
                    Tone::Good,
                    format!("{name} is slain. The way down is open for everyone."),
                );
                let by = self.hero.name.clone();
                self.hero.story.guardians |= 1 << (band - 1);
                if band >= 5 {
                    self.hero.story.verdict_due = true;
                }
                self.emit(crate::world::Change::Slain(band, by));
            }
        }
    }

    /// Fighting wears the weapon and the armour; a guardian's fight wears them twice as much.
    fn wear_after_fight(&mut self, content: &Content, guardian: bool, ev: &mut Vec<Event>) {
        let amount: u8 = if guardian { 4 } else { 2 };
        for (kit, what) in [
            (&mut self.hero.wkit, "weapon"),
            (&mut self.hero.akit, "armour"),
        ] {
            let before = kit.wear;
            kit.wear = kit.wear.saturating_add(amount).min(100);
            if before < 100 && kit.wear >= 100 {
                say(
                    ev,
                    Tone::Bad,
                    format!(
                        "Your {what} is broken. It will serve only poorly until the smith sees it."
                    ),
                );
            } else if before < 75 && kit.wear >= 75 {
                say(ev, Tone::Dim, format!("Your {what} is badly worn."));
            }
        }
        let _ = content;
    }

    /// What a won fight leaves: a guardian's ember-core, and now and then a piece of gear.
    fn drops_after_fight(
        &mut self,
        content: &Content,
        depth: u8,
        fight: &Fight,
        ev: &mut Vec<Event>,
    ) {
        let band = u32::from(dungeon::band_of(depth));
        for e in &fight.enemies {
            let Some(core) = content.monster(&e.id).and_then(|m| m.core.clone()) else {
                continue;
            };
            if self.hero.cores.len() < crate::rules::CORES_MAX {
                let name = content
                    .core(&core)
                    .map_or("an ember-core", |c| c.name.as_str());
                say(ev, Tone::Good, format!("From its ashes you take a {name}."));
                self.hero.cores.push(core);
            }
        }
        let chance = if fight.guardian { 60 } else { 12 };
        if self.rng.below(100) < chance
            && let Some(item) = self.roll_drop(content, band)
        {
            self.pick_up(content, item, ev);
        }
    }

    /// Play a fight out with a plain policy: no moves, no reading of intents beyond guarding
    /// when hurt. It breaks off after [`QUICK_ROUND_CAP`] rounds.
    pub(crate) fn auto_resolve(&mut self, content: &Content, ev: &mut Vec<Event>) {
        let d = self.derived(content);
        let ranged = self
            .weapon(content)
            .is_some_and(|w| w.reach == Reach::Ranged);
        for _ in 0..QUICK_ROUND_CAP {
            let Some(fight) = self.fight.as_mut() else {
                return;
            };
            fight.paid = true;
            fight.stance = if self.hero.hp * 2 < d.max_hp {
                Stance::Guarded
            } else {
                Stance::Aggressive
            };
            if fight.row == Row::Back && !ranged {
                fight.row = Row::Front;
            }
            let target = fight
                .alive()
                .filter(|(_, e)| {
                    Self::can_reach(
                        fight.row,
                        e.row,
                        if ranged { Reach::Ranged } else { Reach::Melee },
                    )
                })
                .min_by_key(|(i, e)| (e.hp, *i))
                .map(|(i, _)| i);
            let Some(target) = target else {
                // Everything alive is out of reach (all in the back row): wait for them.
                self.enemy_phase(content, &d, ev);
                if self.fight.is_some() {
                    self.end_round(content, ev);
                }
                continue;
            };
            self.hero_attack(content, target, None, &d, ev);
            if self.fight.is_some() {
                self.enemy_phase(content, &d, ev);
            }
            if self.fight.is_some() {
                self.end_round(content, ev);
            }
        }
        if self.fight.take().is_some() {
            if let Place::Delve { depth, .. } = self.place {
                self.place = Place::Delve {
                    depth,
                    room: self.prev_room,
                };
            }
            say(
                ev,
                Tone::Bad,
                "The fight drags on. You break off and fall back.",
            );
        }
    }
}
