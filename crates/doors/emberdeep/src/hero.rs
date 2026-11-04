//! The hero: attributes, calling, traits, scars, wounds, skill nodes, and everything derived from
//! them. Every number a rule reads comes out of [`derive`], which folds the calling, taken nodes,
//! traits, scars and live wounds into one [`Derived`]; that is why no node or trait is decoration.

use std::collections::BTreeSet;

use crate::content::{Attr, Content, Effect, Need};
use crate::rules::{
    ATTR_MAX, CREATION_POINTS, CREATION_TRAITS, LEVEL_MAX, SCARS_MAX, SKILL_POINT_LEVEL_MAX,
    xp_for_level,
};

/// A hero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hero {
    pub(crate) name: String,
    pub(crate) calling: String,
    /// Might, finesse, wits, spirit, before any effect.
    pub(crate) attrs: [u8; 4],
    pub(crate) level: u8,
    pub(crate) xp: u32,
    pub(crate) skill_points: u8,
    pub(crate) attr_points: u8,
    pub(crate) hp: i32,
    pub(crate) fatigue: u8,
    pub(crate) turns: u32,
    pub(crate) coin: u32,
    pub(crate) salvage: u32,
    /// Salvage banked in the town store (the town slice crafts from it).
    pub(crate) stash: u32,
    pub(crate) relics: u16,
    pub(crate) nodes: Vec<String>,
    pub(crate) traits: Vec<String>,
    pub(crate) scars: Vec<String>,
    /// Wound id and days left.
    pub(crate) wounds: Vec<(String, u8)>,
    pub(crate) weapon: String,
    pub(crate) armour: String,
    pub(crate) deaths: u16,
    pub(crate) best_depth: u8,
    pub(crate) kills: u32,
}

/// Everything the rules read about a hero, computed from the hero and the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Derived {
    pub(crate) attrs: [i32; 4],
    pub(crate) max_hp: i32,
    pub(crate) armor: i32,
    pub(crate) damage: i32,
    pub(crate) crit: i32,
    pub(crate) evade: i32,
    pub(crate) death_save: i32,
    pub(crate) carry: i32,
    pub(crate) climb_ease: i32,
    pub(crate) start_focus: i32,
    pub(crate) loot: i32,
    pub(crate) trap_sense: i32,
    pub(crate) fatigue_guard: i32,
    pub(crate) tags: BTreeSet<String>,
    pub(crate) moves: BTreeSet<String>,
}

impl Derived {
    pub(crate) fn attr(&self, attr: Attr) -> i32 {
        self.attrs[attr.index()]
    }

    pub(crate) fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(tag)
    }
}

/// Why creating a hero was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreateError {
    UnknownCalling,
    UnknownTrait,
    TraitCount,
    DuplicateTrait,
    Points,
}

/// The d20 modifier of an attribute value.
pub(crate) fn modifier(value: i32) -> i32 {
    (value - 10).div_euclid(2)
}

/// Fatigue steps (0 to 4): each costs a point on rolls and on damage.
pub(crate) fn fatigue_steps(fatigue: u8) -> i32 {
    i32::from(fatigue / 25)
}

fn apply_effect(d: &mut Derived, effect: &Effect) {
    match effect {
        Effect::Attr { attr, amount } => d.attrs[attr.index()] += amount,
        Effect::MaxHp { amount } => d.max_hp += amount,
        Effect::Armor { amount } => d.armor += amount,
        Effect::Damage { amount } => d.damage += amount,
        Effect::CritPct { amount } => d.crit += amount,
        Effect::EvadePct { amount } => d.evade += amount,
        Effect::DeathSave { amount } => d.death_save += amount,
        Effect::Carry { amount } => d.carry += amount,
        Effect::ClimbEase { amount } => d.climb_ease += amount,
        Effect::StartFocus { amount } => d.start_focus += amount,
        Effect::LootPct { amount } => d.loot += amount,
        Effect::TrapSense { amount } => d.trap_sense += amount,
        Effect::FatigueGuard { amount } => d.fatigue_guard += amount,
        Effect::Tag { tag } => {
            d.tags.insert(tag.clone());
        }
        Effect::Move { id } => {
            d.moves.insert(id.clone());
        }
    }
}

/// Compute a hero's numbers. Unknown ids (content that was removed after a save was written) are
/// skipped rather than failing the hero.
pub(crate) fn derive(hero: &Hero, content: &Content) -> Derived {
    let mut d = Derived {
        attrs: hero.attrs.map(i32::from),
        max_hp: 0,
        armor: 0,
        damage: 0,
        crit: 0,
        evade: 0,
        death_save: 0,
        carry: crate::rules::BASE_CARRY,
        climb_ease: 0,
        start_focus: 1,
        loot: 0,
        trap_sense: 0,
        fatigue_guard: 0,
        tags: BTreeSet::new(),
        moves: BTreeSet::new(),
    };
    let mut base_hp = 20;
    if let Some(calling) = content.calling(&hero.calling) {
        base_hp = i32::try_from(calling.hp).unwrap_or(30);
        d.tags.extend(calling.tags.iter().cloned());
        d.moves.extend(calling.moves.iter().cloned());
    }
    if let Some(armour) = content.armour(&hero.armour) {
        d.armor += i32::try_from(armour.armor).unwrap_or(0);
    }
    let mut effects: Vec<&Effect> = Vec::new();
    effects.extend(
        hero.nodes
            .iter()
            .filter_map(|id| content.node(id))
            .flat_map(|n| n.effects.iter()),
    );
    effects.extend(
        hero.traits
            .iter()
            .filter_map(|id| content.trait_def(id))
            .flat_map(|t| t.effects.iter()),
    );
    effects.extend(
        hero.scars
            .iter()
            .filter_map(|id| content.scar(id))
            .flat_map(|t| t.effects.iter()),
    );
    effects.extend(
        hero.wounds
            .iter()
            .filter_map(|(id, _)| content.wound(id))
            .flat_map(|w| w.effects.iter()),
    );
    for effect in effects {
        apply_effect(&mut d, effect);
    }
    for a in &mut d.attrs {
        *a = (*a).max(1);
    }
    d.max_hp += base_hp + d.attr(Attr::Might) + i32::from(hero.level) * 3;
    d.max_hp = d.max_hp.max(10);
    d.armor = d.armor.max(0);
    d.fatigue_guard = d.fatigue_guard.clamp(-50, 60);
    d.start_focus = d.start_focus.clamp(0, 4);
    d
}

impl Hero {
    /// A new hero, or why not.
    pub(crate) fn create(
        content: &Content,
        name: &str,
        calling: &str,
        traits: &[String],
        points: [u8; 4],
    ) -> Result<Self, CreateError> {
        let def = content
            .calling(calling)
            .ok_or(CreateError::UnknownCalling)?;
        if traits.len() != CREATION_TRAITS {
            return Err(CreateError::TraitCount);
        }
        for (n, id) in traits.iter().enumerate() {
            if content.trait_def(id).is_none() {
                return Err(CreateError::UnknownTrait);
            }
            if traits[..n].contains(id) {
                return Err(CreateError::DuplicateTrait);
            }
        }
        let spent: u8 = points.iter().sum();
        if spent != CREATION_POINTS {
            return Err(CreateError::Points);
        }
        let mut attrs = [0u8; 4];
        for (i, slot) in attrs.iter_mut().enumerate() {
            let base = u8::try_from(def.attrs[i]).unwrap_or(8);
            *slot = base + points[i];
            if *slot > ATTR_MAX {
                return Err(CreateError::Points);
            }
        }
        let mut hero = Self {
            name: name.to_owned(),
            calling: def.id.clone(),
            attrs,
            level: 1,
            xp: 0,
            skill_points: 1,
            attr_points: 0,
            hp: 1,
            fatigue: 0,
            turns: crate::rules::TURNS_PER_DAY,
            coin: 20,
            salvage: 0,
            stash: 0,
            relics: 0,
            nodes: Vec::new(),
            traits: traits.to_vec(),
            scars: Vec::new(),
            wounds: Vec::new(),
            weapon: def.weapon.clone(),
            armour: def.armour.clone(),
            deaths: 0,
            best_depth: 0,
            kills: 0,
        };
        hero.hp = derive(&hero, content).max_hp;
        Ok(hero)
    }

    /// The calling's starting kit (what a hero wakes with after losing their gear).
    pub(crate) fn reset_gear(&mut self, content: &Content) {
        if let Some(def) = content.calling(&self.calling) {
            self.weapon.clone_from(&def.weapon);
            self.armour.clone_from(&def.armour);
        }
    }

    /// Add experience, taking any levels it earns. Returns how many levels were gained.
    pub(crate) fn gain_xp(&mut self, amount: u32) -> u8 {
        self.xp = self.xp.saturating_add(amount);
        let mut gained = 0;
        while self.level < LEVEL_MAX && self.xp >= xp_for_level(self.level + 1) {
            self.level += 1;
            gained += 1;
            if self.level <= SKILL_POINT_LEVEL_MAX {
                self.skill_points += 1;
            }
            if self.level.is_multiple_of(4) {
                self.attr_points += 1;
            }
        }
        gained
    }

    /// Why a node is out of reach for a reason other than points, if it is: another calling's,
    /// already taken, closed by an earlier choice, too high a tier, or an earlier node missing.
    pub(crate) fn node_locked(&self, content: &Content, id: &str) -> Option<&'static str> {
        let Some(node) = content.node(id) else {
            return Some("There is no such skill.");
        };
        if node.calling != self.calling {
            return Some("That skill belongs to another calling.");
        }
        if self.nodes.iter().any(|n| n == id) {
            return Some("You already have that skill.");
        }
        let closed = self
            .nodes
            .iter()
            .filter_map(|n| content.node(n))
            .any(|taken| taken.excludes.iter().any(|e| e == id));
        if closed {
            return Some("An earlier choice closed that path for good.");
        }
        if u32::from(self.level) < 2 * node.tier - 1 {
            return Some("You are not experienced enough for that skill.");
        }
        if !node.requires.is_empty() && !node.requires.iter().any(|r| self.nodes.contains(r)) {
            return Some("You need an earlier skill first.");
        }
        None
    }

    /// Why a node cannot be taken now, if it cannot.
    pub(crate) fn node_refusal(&self, content: &Content, id: &str) -> Option<&'static str> {
        self.node_locked(content, id).or({
            if self.skill_points == 0 {
                Some("You have no skill points to spend.")
            } else {
                None
            }
        })
    }

    /// Take a node.
    pub(crate) fn take_node(&mut self, content: &Content, id: &str) -> Result<(), &'static str> {
        if let Some(why) = self.node_refusal(content, id) {
            return Err(why);
        }
        self.skill_points -= 1;
        self.nodes.push(id.to_owned());
        Ok(())
    }

    /// Spend an attribute point.
    pub(crate) fn raise(&mut self, attr: Attr) -> Result<(), &'static str> {
        if self.attr_points == 0 {
            return Err("You have no attribute points to spend.");
        }
        let slot = &mut self.attrs[attr.index()];
        if *slot >= ATTR_MAX {
            return Err("That attribute cannot go higher.");
        }
        *slot += 1;
        self.attr_points -= 1;
        Ok(())
    }

    /// Add or refresh a wound.
    pub(crate) fn inflict(&mut self, content: &Content, id: &str) {
        let Some(def) = content.wound(id) else {
            return;
        };
        let days = u8::try_from(def.days).unwrap_or(u8::MAX);
        if let Some(slot) = self.wounds.iter_mut().find(|(w, _)| w == id) {
            slot.1 = days;
        } else {
            self.wounds.push((id.to_owned(), days));
        }
    }

    /// Whether a scar can still be added.
    pub(crate) fn can_scar(&self, id: &str) -> bool {
        self.scars.len() < SCARS_MAX && !self.scars.iter().any(|s| s == id)
    }

    /// Whether the hero satisfies a room option's condition.
    pub(crate) fn meets(&self, d: &Derived, need: &Need) -> bool {
        match need {
            Need::Tag { tag } => d.has_tag(tag),
            Need::NoTag { tag } => !d.has_tag(tag),
            Need::Attr { attr, min } => d.attr(*attr) >= i32::try_from(*min).unwrap_or(i32::MAX),
            Need::Calling { id } => self.calling == *id,
            Need::Trait { id } => self.traits.contains(id) || self.scars.contains(id),
            Need::Node { id } => self.nodes.contains(id),
            Need::Level { min } => u32::from(self.level) >= *min,
        }
    }
}
