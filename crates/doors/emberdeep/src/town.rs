//! Cindermere: the places a hero spends coin and salvage in. Market, forge, temple, guild hall,
//! tavern, board, library and the town works. All of it is rules on [`Game`]: an action goes in,
//! state and events come out, and every roll is the hero's own generator. The screen only maps
//! keys to these actions.
//!
//! **The RPG rule.** What a place offers depends on the hero (a calling's recipes, a runesmith's
//! cheaper work, a cutthroat's loaded dice, an ember-priest's cheaper rites, the board's postings
//! by capability) and on the shared town (a finished project changes the options of everyone).

use russh_bbs_door::DoorRng;

use crate::content::{Attr, Content};
use crate::content_town::{
    Affix, Contract, ContractKind, Learn, Makes, ProjectEffect, RumourEffect, Slot,
};
use crate::game::{Event, Game, Refused, Tone, say};
use crate::hero::{Item, Kit, Taken, modifier};
use crate::rules::{
    CORES_MAX, DEBT_LIMIT, FOUND_MAX, PACK_MAX, TRAIN_LEVEL_MAX, gear_level, xp_for_level,
};
use crate::world::{Change, Pledge};

/// Which worn piece an action is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Worn {
    Weapon,
    Armour,
}

impl Worn {
    pub(crate) fn slot(self) -> Slot {
        match self {
            Self::Weapon => Slot::Weapon,
            Self::Armour => Slot::Armour,
        }
    }

    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Weapon => "weapon",
            Self::Armour => "armour",
        }
    }
}

/// Something to do in the town.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TownAct {
    /// Buy stock number `n` at the market.
    BuyStock(usize),
    /// Sell spare gear number `n`.
    Sell(usize),
    /// Sell ten salvage from the store.
    SellSalvage,
    /// Wield spare gear number `n`.
    Equip(usize),
    /// Repair what is worn.
    Repair(Worn),
    /// Follow a recipe.
    Craft(String),
    /// Slot loose core number `n` into what is worn.
    Slot(usize, Worn),
    /// Lift a core out whole (a runesmith's work).
    Unslot(Worn),
    /// Be tended at the temple.
    Heal,
    /// Have wound number `n` cured at the temple (no scar is left).
    Cure(usize),
    /// Drill at the guild hall.
    Train,
    /// A drink and an ear for the room.
    Drink,
    /// Throw dice for `stake` coin, loaded or not.
    Gamble { stake: u32, cheat: bool },
    /// Pay the dice-crew.
    Repay,
    /// Call on a companion.
    Court(String),
    /// Take posting number `n` from the board.
    Take(usize),
    /// Turn in a finished contract.
    TurnIn,
    /// Give up the contract.
    Drop,
    /// Give coin and salvage to a town project.
    Donate {
        project: String,
        coin: u32,
        salvage: u32,
    },
    /// Have the library read a relic.
    Identify,
}

/// One thing for sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wares {
    Gear(Item),
    Core(String),
}

/// A market stall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stock {
    pub(crate) wares: Wares,
    pub(crate) price: u32,
}

/// What a base is listed at. A base with no price (a starting kit or a find) is worth a
/// band-scaled amount so it can still be sold.
pub(crate) fn base_price(content: &Content, base: &str) -> u32 {
    let (price, band) = if let Some(w) = content.weapon(base) {
        (w.price, w.band)
    } else if let Some(a) = content.armour(base) {
        (a.price, a.band)
    } else {
        return 0;
    };
    if price > 0 { price } else { 12 * band * band }
}

/// The band of a base.
pub(crate) fn base_band(content: &Content, base: &str) -> u32 {
    content
        .weapon(base)
        .map(|w| w.band)
        .or_else(|| content.armour(base).map(|a| a.band))
        .unwrap_or(1)
}

/// The shown name of a piece of gear: its affixes, then its base.
pub(crate) fn item_name(content: &Content, base: &str, kit: &Kit) -> String {
    let mut name = String::new();
    for id in &kit.affixes {
        if let Some(a) = content.affix(id) {
            name.push_str(&a.name);
            name.push(' ');
        }
    }
    let base_name = content
        .weapon(base)
        .map(|w| w.name.as_str())
        .or_else(|| content.armour(base).map(|a| a.name.as_str()))
        .unwrap_or(base);
    name.push_str(base_name);
    name
}

/// What a piece is worth at the counter, before the haggling: price, 40% more for each affix,
/// less for wear.
pub(crate) fn item_value(content: &Content, item: &Item) -> u32 {
    let base = base_price(content, &item.base);
    let n = u32::try_from(item.kit.affixes.len()).unwrap_or(0);
    let gross = base * (100 + 40 * n) / 100;
    gross * (100 - u32::from(item.kit.wear) / 2) / 100
}

/// Coin for repairing `wear` on a piece of `band`, at a forge of `forge` level.
pub(crate) fn repair_cost(wear: u8, band: u32, forge: u32, smith: bool) -> u32 {
    if wear == 0 {
        return 0;
    }
    let rate = match forge {
        0 | 1 => 4,
        2 => 3,
        _ => 2,
    };
    let mut cost = (u32::from(wear) * rate * band).div_ceil(10).max(1);
    if smith {
        cost = cost.div_ceil(2).max(1);
    }
    cost
}

fn pct(amount: u32, loot: i32) -> u32 {
    let factor = u32::try_from((100 + loot).max(0)).unwrap_or(100);
    amount * factor / 100
}

impl Game {
    /// Today's market stock: the same for every hero, by season, epoch and day. The shelves
    /// follow the shared town: the deeper the deepest hero has been, and the bigger the market,
    /// the better the goods.
    pub(crate) fn stock(&self, content: &Content) -> Vec<Stock> {
        let fx = self.world.town(content);
        let deepest = u32::from(self.world.deepest.first().map_or(0, |d| d.1));
        let cap = (1 + deepest / 3 + u32::from(fx.market >= 2)).min(4);
        let mut rng =
            DoorRng::from_label(&format!("market-{}-{}", self.season, self.epoch), self.day);
        let bases: Vec<(&str, Slot)> = content
            .gear
            .weapon
            .iter()
            .filter(|w| w.price > 0 && w.band <= cap)
            .map(|w| (w.id.as_str(), Slot::Weapon))
            .chain(
                content
                    .gear
                    .armour
                    .iter()
                    .filter(|a| a.price > 0 && a.band <= cap)
                    .map(|a| (a.id.as_str(), Slot::Armour)),
            )
            .collect();
        let mut out: Vec<Stock> = Vec::new();
        let slots = 4 + 2 * fx.market as usize;
        for _ in 0..slots {
            if bases.is_empty() {
                break;
            }
            let mut pick = bases[rng.below(u32::try_from(bases.len()).unwrap_or(1)) as usize];
            for _ in 0..12 {
                if !out
                    .iter()
                    .any(|s| matches!(&s.wares, Wares::Gear(i) if i.base == pick.0))
                {
                    break;
                }
                pick = bases[rng.below(u32::try_from(bases.len()).unwrap_or(1)) as usize];
            }
            let mut kit = Kit::default();
            if rng.chance(30 + 15 * fx.market, 100) {
                let fits: Vec<&Affix> = content
                    .affixes
                    .affix
                    .iter()
                    .filter(|a| a.band <= cap && a.slot.fits(pick.1))
                    .collect();
                if !fits.is_empty() {
                    let a = fits[rng.below(u32::try_from(fits.len()).unwrap_or(1)) as usize];
                    kit.affixes.push(a.id.clone());
                }
            }
            let item = Item {
                base: pick.0.to_owned(),
                kit,
            };
            let price = item_value(content, &item);
            out.push(Stock {
                wares: Wares::Gear(item),
                price,
            });
        }
        let cores: Vec<&crate::content_town::Core> = content
            .affixes
            .core
            .iter()
            .filter(|c| c.price > 0 && c.band <= cap)
            .collect();
        for _ in 0..fx.market {
            if cores.is_empty() {
                break;
            }
            let c = cores[rng.below(u32::try_from(cores.len()).unwrap_or(1)) as usize];
            if !out
                .iter()
                .any(|s| matches!(&s.wares, Wares::Core(id) if *id == c.id))
            {
                out.push(Stock {
                    wares: Wares::Core(c.id.clone()),
                    price: c.price,
                });
            }
        }
        out
    }

    /// The town the shared world has built.
    pub(crate) fn town_fx(&self, content: &Content) -> crate::world::TownEffects {
        self.world.town(content)
    }

    /// Every way down from the town, from the shared world.
    pub(crate) fn ways_down(&self, content: &Content) -> Vec<(u8, crate::content::ShortcutKind)> {
        self.world.ways_down(content)
    }

    /// The coin a hero gets for something, with their luck at the counter (`LootPct`).
    pub(crate) fn sell_price(&self, content: &Content, item: &Item) -> u32 {
        let d = self.derived(content);
        pct(item_value(content, item) / 2, d.loot / 2).max(1)
    }

    /// The postings on today's board for this hero: three, drawn from the postings the hero's
    /// build and depth fit, so two builds read two boards.
    pub(crate) fn board<'a>(&self, content: &'a Content) -> Vec<&'a Contract> {
        let d = self.derived(content);
        let mut ok: Vec<&Contract> = content
            .town
            .contract
            .iter()
            .filter(|c| {
                c.depth <= u32::from(self.hero.best_depth) + 4
                    && c.need.iter().all(|n| self.hero.meets(&d, n))
            })
            .collect();
        ok.sort_by(|a, b| a.id.cmp(&b.id));
        if ok.is_empty() {
            return ok;
        }
        let mut rng =
            DoorRng::from_label(&format!("board-{}-{}", self.season, self.epoch), self.day);
        let start = rng.below(u32::try_from(ok.len()).unwrap_or(1)) as usize;
        (0..ok.len().min(3))
            .map(|i| ok[(start + i) % ok.len()])
            .collect()
    }

    /// Whether the hero can craft a recipe now, and if not, why.
    pub(crate) fn craft_refusal(&self, content: &Content, id: &str) -> Option<String> {
        let Some(r) = content.recipe(id) else {
            return Some("There is no such recipe.".to_owned());
        };
        if !self.hero.knows(r) {
            return Some("You do not know that recipe.".to_owned());
        }
        let fx = self.town_fx(content);
        if fx.forge.max(1) < r.forge {
            return Some(format!(
                "The forge is not hot enough for that (it needs level {}).",
                r.forge
            ));
        }
        let d = self.derived(content);
        if !r.need.iter().all(|n| self.hero.meets(&d, n)) {
            return Some("You lack the craft for that.".to_owned());
        }
        let (salvage, coin) = Self::craft_cost(r, &d);
        if self.hero.stash < salvage {
            return Some(format!("You need {salvage} salvage in the store."));
        }
        if self.hero.coin < coin {
            return Some(format!("You need {coin} coin."));
        }
        match r.makes {
            Makes::Item { .. } if self.hero.pack.len() >= PACK_MAX => {
                Some("Your spare gear is full. Sell something first.".to_owned())
            }
            Makes::Core { .. } if self.hero.cores.len() >= CORES_MAX => {
                Some("You have no room for another core.".to_owned())
            }
            _ => None,
        }
    }

    /// What a recipe costs this hero: salvage and coin. A runesmith's edge saves a quarter of
    /// the salvage.
    pub(crate) fn craft_cost(
        r: &crate::content_town::Recipe,
        d: &crate::hero::Derived,
    ) -> (u32, u32) {
        let salvage = if d.has_tag("smithcraft") {
            (r.salvage * 3).div_ceil(4)
        } else {
            r.salvage
        };
        (salvage, r.coin)
    }

    // ---- the entry point ----

    /// Do something in the town.
    pub(crate) fn town_act(
        &mut self,
        content: &Content,
        act: TownAct,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        match act {
            TownAct::BuyStock(n) => self.buy_stock(content, n, ev),
            TownAct::Sell(n) => self.sell(content, n, ev),
            TownAct::SellSalvage => self.sell_salvage(ev),
            TownAct::Equip(n) => self.equip(content, n, ev),
            TownAct::Repair(worn) => self.repair(content, worn, ev),
            TownAct::Craft(id) => self.craft(content, &id, ev),
            TownAct::Slot(n, worn) => self.slot_core(content, n, worn, ev),
            TownAct::Unslot(worn) => self.unslot_core(content, worn, ev),
            TownAct::Heal => self.heal_at_temple(content, ev),
            TownAct::Cure(n) => self.cure_at_temple(content, n, ev),
            TownAct::Train => self.train(content, ev),
            TownAct::Drink => self.drink(content, ev),
            TownAct::Gamble { stake, cheat } => self.gamble(content, stake, cheat, ev),
            TownAct::Repay => self.repay(ev),
            TownAct::Court(id) => self.court(content, &id, ev),
            TownAct::Take(n) => self.take_contract(content, n, ev),
            TownAct::TurnIn => self.turn_in(content, ev),
            TownAct::Drop => self.drop_contract(content, ev),
            TownAct::Donate {
                project,
                coin,
                salvage,
            } => self.donate(content, &project, coin, salvage, ev),
            TownAct::Identify => self.identify(content, ev),
        }
    }

    fn spend_coin(&mut self, amount: u32) -> Result<(), Refused> {
        if self.hero.coin < amount {
            return Err(Refused::new(&format!("You need {amount} coin.")));
        }
        self.hero.coin -= amount;
        Ok(())
    }

    // ---- market ----

    fn buy_stock(
        &mut self,
        content: &Content,
        n: usize,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let stock = self.stock(content);
        let Some(item) = stock.get(n) else {
            return Err(Refused::new("There is nothing on that stall."));
        };
        let idx = u8::try_from(n).unwrap_or(u8::MAX);
        if self.today.bought.contains(&idx) {
            return Err(Refused::new("That one is sold."));
        }
        match &item.wares {
            Wares::Gear(g) if self.hero.pack.len() >= PACK_MAX => {
                let _ = g;
                return Err(Refused::new(
                    "Your spare gear is full. Sell something first.",
                ));
            }
            Wares::Core(_) if self.hero.cores.len() >= CORES_MAX => {
                return Err(Refused::new("You have no room for another core."));
            }
            _ => {}
        }
        self.spend_coin(item.price)?;
        self.today.bought.push(idx);
        match &item.wares {
            Wares::Gear(g) => {
                say(
                    ev,
                    Tone::Good,
                    format!(
                        "You buy the {} for {} coin.",
                        item_name(content, &g.base, &g.kit),
                        item.price
                    ),
                );
                self.hero.pack.push(g.clone());
            }
            Wares::Core(id) => {
                let name = content.core(id).map_or("a core", |c| c.name.as_str());
                say(
                    ev,
                    Tone::Good,
                    format!("You buy a {name} for {} coin.", item.price),
                );
                self.hero.cores.push(id.clone());
            }
        }
        Ok(())
    }

    fn sell(&mut self, content: &Content, n: usize, ev: &mut Vec<Event>) -> Result<(), Refused> {
        let Some(item) = self.hero.pack.get(n) else {
            return Err(Refused::new("You have no such spare."));
        };
        if item.kit.core.is_some() {
            return Err(Refused::new(
                "There is a core in it. Slot it elsewhere first; the market will not pay for what is lost.",
            ));
        }
        let price = self.sell_price(content, item);
        let name = item_name(content, &item.base, &item.kit);
        self.hero.pack.remove(n);
        self.hero.coin = self.hero.coin.saturating_add(price);
        say(
            ev,
            Tone::Good,
            format!("You sell the {name} for {price} coin."),
        );
        Ok(())
    }

    fn sell_salvage(&mut self, ev: &mut Vec<Event>) -> Result<(), Refused> {
        if self.hero.stash < 10 {
            return Err(Refused::new(
                "Salvage sells by the ten, and you have fewer.",
            ));
        }
        self.hero.stash -= 10;
        self.hero.coin = self.hero.coin.saturating_add(8);
        say(ev, Tone::Plain, "Ten salvage fetch 8 coin.");
        Ok(())
    }

    /// Wield spare gear number `n`; what was worn goes into its place.
    fn equip(&mut self, content: &Content, n: usize, ev: &mut Vec<Event>) -> Result<(), Refused> {
        let Some(item) = self.hero.pack.get(n).cloned() else {
            return Err(Refused::new("You have no such spare."));
        };
        let Some(slot) = content.base_slot(&item.base) else {
            return Err(Refused::new("You cannot make out what that is."));
        };
        let need = gear_level(base_band(content, &item.base));
        if u32::from(self.hero.level) < need {
            return Err(Refused::new(&format!(
                "You must be level {need} to wield that."
            )));
        }
        let old = match slot {
            Slot::Weapon => Item {
                base: std::mem::replace(&mut self.hero.weapon, item.base.clone()),
                kit: std::mem::replace(&mut self.hero.wkit, item.kit.clone()),
            },
            _ => Item {
                base: std::mem::replace(&mut self.hero.armour, item.base.clone()),
                kit: std::mem::replace(&mut self.hero.akit, item.kit.clone()),
            },
        };
        self.hero.pack[n] = old;
        let d = self.derived(content);
        self.hero.hp = self.hero.hp.min(d.max_hp);
        say(
            ev,
            Tone::Good,
            format!(
                "You take up the {}.",
                item_name(content, &item.base, &item.kit)
            ),
        );
        Ok(())
    }

    // ---- forge ----

    fn kit(&self, worn: Worn) -> (&str, &Kit) {
        match worn {
            Worn::Weapon => (&self.hero.weapon, &self.hero.wkit),
            Worn::Armour => (&self.hero.armour, &self.hero.akit),
        }
    }

    fn kit_mut(&mut self, worn: Worn) -> &mut Kit {
        match worn {
            Worn::Weapon => &mut self.hero.wkit,
            Worn::Armour => &mut self.hero.akit,
        }
    }

    /// Coin to repair the worn piece.
    pub(crate) fn repair_price(&self, content: &Content, worn: Worn) -> u32 {
        let (base, kit) = self.kit(worn);
        let d = self.derived(content);
        repair_cost(
            kit.wear,
            base_band(content, base),
            self.town_fx(content).forge.max(1),
            d.has_tag("smithcraft"),
        )
    }

    fn repair(
        &mut self,
        content: &Content,
        worn: Worn,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let cost = self.repair_price(content, worn);
        if cost == 0 {
            return Err(Refused::new("It needs no repair."));
        }
        self.spend_coin(cost)?;
        self.kit_mut(worn).wear = 0;
        say(
            ev,
            Tone::Good,
            format!("The smith sets your {} right for {cost} coin.", worn.word()),
        );
        Ok(())
    }

    fn craft(&mut self, content: &Content, id: &str, ev: &mut Vec<Event>) -> Result<(), Refused> {
        if let Some(why) = self.craft_refusal(content, id) {
            return Err(Refused(why));
        }
        let Some(r) = content.recipe(id) else {
            return Err(Refused::new("There is no such recipe."));
        };
        let d = self.derived(content);
        let (salvage, coin) = Self::craft_cost(r, &d);
        self.hero.stash -= salvage;
        self.hero.coin -= coin;
        match &r.makes {
            Makes::Item { base, affixes } => {
                let mut kit = Kit {
                    affixes: affixes.clone(),
                    core: None,
                    wear: 0,
                };
                let slot = content.base_slot(base).unwrap_or(Slot::Weapon);
                // A runesmith's edge: a steady eye for a masterwork, the chance rising with wits.
                let mut master = false;
                if d.has_tag("smithcraft")
                    && kit.affixes.len() < 3
                    && self.rng.below(100) < u32::try_from(d.attr(Attr::Wits) * 3).unwrap_or(0)
                {
                    let band = base_band(content, base);
                    let extra: Vec<&Affix> = content
                        .affixes
                        .affix
                        .iter()
                        .filter(|a| {
                            a.band <= band + 1 && a.slot.fits(slot) && !kit.affixes.contains(&a.id)
                        })
                        .collect();
                    if !extra.is_empty() {
                        let a =
                            extra[self.rng.below(u32::try_from(extra.len()).unwrap_or(1)) as usize];
                        kit.affixes.push(a.id.clone());
                        master = true;
                    }
                }
                let name = item_name(content, base, &kit);
                self.hero.pack.push(Item {
                    base: base.clone(),
                    kit,
                });
                say(
                    ev,
                    Tone::Good,
                    if master {
                        format!(
                            "Masterwork! The {name} comes off the anvil better than the pattern."
                        )
                    } else {
                        format!("You forge a {name}.")
                    },
                );
            }
            Makes::Core { id } => {
                let name = content.core(id).map_or("a core", |c| c.name.as_str());
                self.hero.cores.push(id.clone());
                say(
                    ev,
                    Tone::Good,
                    format!("You work a {name} from salvage and fire."),
                );
            }
        }
        Ok(())
    }

    fn slot_core(
        &mut self,
        content: &Content,
        n: usize,
        worn: Worn,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let Some(id) = self.hero.cores.get(n).cloned() else {
            return Err(Refused::new("You have no such core."));
        };
        let Some(core) = content.core(&id) else {
            return Err(Refused::new("That core is not like any the smith knows."));
        };
        if !core.slot.fits(worn.slot()) {
            return Err(Refused::new(&format!(
                "A {} does not take a {}.",
                worn.word(),
                core.name
            )));
        }
        let d = self.derived(content);
        let smith = d.has_tag("smithcraft");
        let old = self.kit(worn).1.core.clone();
        if old.is_some() && smith && self.hero.cores.len() > CORES_MAX {
            return Err(Refused::new("You have no room to keep the old core."));
        }
        self.spend_coin(8)?;
        self.hero.cores.remove(n);
        self.kit_mut(worn).core = Some(id);
        match old {
            Some(old_id) if smith => {
                self.hero.cores.push(old_id);
                say(
                    ev,
                    Tone::Good,
                    format!(
                        "You set the {} in the {}, and lift the old core out whole.",
                        core.name,
                        worn.word()
                    ),
                );
            }
            Some(_) => say(
                ev,
                Tone::Bad,
                format!(
                    "You set the {} in the {}. The old core cracks and is gone.",
                    core.name,
                    worn.word()
                ),
            ),
            None => say(
                ev,
                Tone::Good,
                format!("You set the {} in the {}.", core.name, worn.word()),
            ),
        }
        let d = self.derived(content);
        self.hero.hp = self.hero.hp.min(d.max_hp);
        Ok(())
    }

    fn unslot_core(
        &mut self,
        content: &Content,
        worn: Worn,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let d = self.derived(content);
        if !d.has_tag("smithcraft") {
            return Err(Refused::new(
                "Only a runesmith can lift a core out whole. Anyone else would crack it.",
            ));
        }
        if self.hero.cores.len() >= CORES_MAX {
            return Err(Refused::new("You have no room for another core."));
        }
        let Some(id) = self.kit(worn).1.core.clone() else {
            return Err(Refused::new("There is no core in it."));
        };
        self.spend_coin(4)?;
        self.kit_mut(worn).core = None;
        self.hero.cores.push(id.clone());
        let name = content.core(&id).map_or("core", |c| c.name.as_str());
        say(
            ev,
            Tone::Good,
            format!("You lift the {name} out of the {} whole.", worn.word()),
        );
        let d = self.derived(content);
        self.hero.hp = self.hero.hp.min(d.max_hp);
        Ok(())
    }

    // ---- temple ----

    /// The temple's price list for this hero: (coin for a full tending, turns, rite discount %).
    fn temple_terms(&self, content: &Content) -> (i64, u32) {
        let fx = self.town_fx(content);
        let d = self.derived(content);
        let mut discount = 100;
        if fx.temple >= 2 {
            discount -= 25;
        }
        if d.has_tag("bless") {
            discount -= 25;
        }
        (i64::from(discount), if fx.temple >= 1 { 0 } else { 2 })
    }

    /// Coin for being tended now.
    pub(crate) fn heal_price(&self, content: &Content) -> u32 {
        let d = self.derived(content);
        let missing = i64::from((d.max_hp - self.hero.hp).max(0));
        let tired = i64::from(self.hero.fatigue) / 10;
        let (discount, _) = self.temple_terms(content);
        u32::try_from(((2 * missing + tired) * discount / 100).max(0)).unwrap_or(0)
    }

    /// Turns being tended costs.
    pub(crate) fn heal_turns(&self, content: &Content) -> u32 {
        self.temple_terms(content).1
    }

    fn heal_at_temple(&mut self, content: &Content, ev: &mut Vec<Event>) -> Result<(), Refused> {
        let d = self.derived(content);
        if self.hero.hp >= d.max_hp && self.hero.fatigue < 10 {
            return Err(Refused::new("The sisters find nothing to mend."));
        }
        let price = self.heal_price(content);
        let turns = self.heal_turns(content);
        if self.hero.coin < price {
            return Err(Refused::new(&format!("You need {price} coin.")));
        }
        if self.hero.turns < turns {
            return Err(Refused::new(
                "You do not have the turns to wait to be tended.",
            ));
        }
        self.hero.coin -= price;
        self.hero.turns -= turns;
        self.hero.hp = d.max_hp;
        self.hero.fatigue = 0;
        say(
            ev,
            Tone::Good,
            if turns == 0 {
                format!("The sisters tend you at once. {price} coin.")
            } else {
                format!("You wait your turn, and are tended. {price} coin, {turns} turn(s).")
            },
        );
        Ok(())
    }

    /// Coin to cure wound number `n`, if it is an injury.
    pub(crate) fn cure_price(&self, content: &Content, n: usize) -> Option<u32> {
        let (id, days) = self.hero.wounds.get(n)?;
        if content.wound(id).is_none_or(|w| w.boon) {
            return None;
        }
        let (discount, _) = self.temple_terms(content);
        u32::try_from((i64::from(*days) * 10 * discount / 100).max(1)).ok()
    }

    fn cure_at_temple(
        &mut self,
        content: &Content,
        n: usize,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let Some(price) = self.cure_price(content, n) else {
            return Err(Refused::new("There is nothing there to cure."));
        };
        let turns = u32::from(self.heal_turns(content) > 0);
        if self.hero.turns < turns {
            return Err(Refused::new("You do not have the turn to spare."));
        }
        self.spend_coin(price)?;
        self.hero.turns -= turns;
        let (id, _) = self.hero.wounds.remove(n);
        let name = content.wound(&id).map_or("wound", |w| w.name.as_str());
        say(
            ev,
            Tone::Good,
            format!(
                "The sisters lift your {} away. It leaves no scar. {price} coin.",
                name.to_lowercase()
            ),
        );
        Ok(())
    }

    // ---- guild hall ----

    /// Coin for a drill at the guild hall.
    pub(crate) fn train_price(&self) -> u32 {
        8 * u32::from(self.hero.level)
    }

    fn train(&mut self, content: &Content, ev: &mut Vec<Event>) -> Result<(), Refused> {
        let fx = self.town_fx(content);
        let allowed = 1 + fx.guild;
        if u32::from(self.today.trained) >= allowed {
            return Err(Refused::new(
                "The guild has drilled you enough for one day.",
            ));
        }
        if self.hero.level >= TRAIN_LEVEL_MAX {
            return Err(Refused::new(
                "The guild has nothing left to teach you. The rest is learned below.",
            ));
        }
        if self.hero.turns == 0 {
            return Err(Refused::new("You have no turn to spare for drilling."));
        }
        self.spend_coin(self.train_price())?;
        self.hero.turns -= 1;
        self.today.trained += 1;
        let gap = xp_for_level(self.hero.level + 1).saturating_sub(self.hero.xp);
        let xp = (gap / 4).max(1);
        say(
            ev,
            Tone::Good,
            format!("The guild-master drills you hard. +{xp} xp, and you feel keen."),
        );
        self.hero.inflict(content, "drilled");
        self.gain_xp(xp, ev);
        Ok(())
    }

    // ---- tavern ----

    fn drink(&mut self, content: &Content, ev: &mut Vec<Event>) -> Result<(), Refused> {
        if self.today.drinks >= 3 {
            return Err(Refused::new("The barkeep has cut you off."));
        }
        self.spend_coin(4)?;
        self.today.drinks += 1;
        let n = u32::try_from(content.town.rumour.len()).unwrap_or(1);
        let r = &content.town.rumour[self.rng.below(n) as usize];
        say(ev, Tone::Plain, r.text.clone());
        match &r.effect {
            RumourEffect::Boon { id } => {
                self.hero.inflict(content, id);
                let name = content.wound(id).map_or("a glow", |w| w.name.as_str());
                say(ev, Tone::Good, format!("You feel {}.", name.to_lowercase()));
            }
            RumourEffect::Coin { amount } => {
                self.hero.coin = self.hero.coin.saturating_add(*amount);
                say(ev, Tone::Good, format!("+{amount} coin"));
            }
            RumourEffect::Tip { depth } => {
                if !self.today.tips.contains(depth) {
                    self.today.tips.push(*depth);
                }
                say(
                    ev,
                    Tone::Good,
                    format!("Level {depth} is yours to walk with a map today."),
                );
            }
        }
        if self.today.drinks == 3 {
            self.hero.inflict(content, "hangover");
            say(
                ev,
                Tone::Bad,
                "That third one was a mistake. Your head pounds.",
            );
        }
        Ok(())
    }

    fn gamble(
        &mut self,
        content: &Content,
        stake: u32,
        cheat: bool,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        if self.today.shut_out {
            return Err(Refused::new(
                "The dice-crew has thrown you out for the night.",
            ));
        }
        if self.hero.debt > 0 {
            return Err(Refused::new(
                "The dice-crew will not deal to someone who owes them. Pay up first.",
            ));
        }
        if self.today.gambles >= 3 {
            return Err(Refused::new("Enough dice for one night."));
        }
        if !matches!(stake, 5 | 20 | 50) {
            return Err(Refused::new("The crew plays for 5, 20 or 50."));
        }
        let d = self.derived(content);
        if cheat && !d.has_tag("sneak") {
            return Err(Refused::new("You have not the hands for loaded dice."));
        }
        self.today.gambles += 1;
        let paid = stake.min(self.hero.coin);
        let borrowed = stake - paid;
        self.hero.coin -= paid;
        self.hero.debt += borrowed;
        let roll = i32::try_from(self.rng.range(1, 20)).unwrap_or(10);
        let total = roll + modifier(d.attr(Attr::Wits)) + if cheat { 4 } else { 0 };
        let caught = cheat && roll <= 3;
        let win = !caught && roll != 1 && (roll == 20 || total >= 11);
        if caught {
            let fine = 2 * stake;
            self.hero.debt += fine;
            self.today.shut_out = true;
            self.hero.inflict(content, "roughed-up");
            say(
                ev,
                Tone::Bad,
                format!(
                    "They catch the loaded die. The crew takes your stake, writes {fine} coin in the debt book, and shows you the door the hard way."
                ),
            );
        } else if win {
            let profit = stake * 9 / 10;
            self.hero.coin = self.hero.coin.saturating_add(paid + profit);
            self.hero.debt -= borrowed;
            say(
                ev,
                Tone::Good,
                format!("The dice fall your way. +{profit} coin (the house keeps its tenth)."),
            );
        } else {
            say(
                ev,
                Tone::Bad,
                if borrowed > 0 {
                    format!(
                        "The dice fall wrong. You lose {paid} coin and owe the crew {borrowed}."
                    )
                } else {
                    format!("The dice fall wrong. You lose {paid} coin.")
                },
            );
        }
        Ok(())
    }

    fn repay(&mut self, ev: &mut Vec<Event>) -> Result<(), Refused> {
        if self.hero.debt == 0 {
            return Err(Refused::new("You owe the crew nothing."));
        }
        if self.hero.coin == 0 {
            return Err(Refused::new("You have no coin to give."));
        }
        let pay = self.hero.coin.min(self.hero.debt);
        self.hero.coin -= pay;
        self.hero.debt -= pay;
        say(
            ev,
            Tone::Good,
            if self.hero.debt == 0 {
                format!("You pay the crew {pay} coin. The debt book is closed.")
            } else {
                format!(
                    "You pay the crew {pay} coin. You still owe {}.",
                    self.hero.debt
                )
            },
        );
        Ok(())
    }

    fn court(&mut self, content: &Content, id: &str, ev: &mut Vec<Event>) -> Result<(), Refused> {
        let Some(c) = content.companion(id) else {
            return Err(Refused::new("There is no one by that name."));
        };
        if self.hero.closed.iter().any(|x| x == id) {
            return Err(Refused::new(&format!("{} has moved on.", c.name)));
        }
        if let Some(b) = &self.hero.bond {
            if b.id != id {
                return Err(Refused::new(
                    "You are already keeping company with someone else.",
                ));
            }
            if b.level >= 3 {
                return Err(Refused::new("There is nothing more to win."));
            }
        }
        if self.today.courted {
            return Err(Refused::new("Once a day is as much as anyone should call."));
        }
        self.spend_coin(c.gift)?;
        self.today.courted = true;
        let day = self.day;
        let bond = self.hero.bond.get_or_insert_with(|| crate::hero::Bond {
            id: id.to_owned(),
            level: 0,
            progress: 0,
            last: day,
        });
        bond.last = day;
        let d = self.derived(content);
        if self.roll_check(&d, c.attr, c.dc, 0) {
            let Some(bond) = self.hero.bond.as_mut() else {
                return Ok(());
            };
            bond.progress += 1;
            if bond.progress >= 2 {
                bond.progress = 0;
                bond.level += 1;
                if let Some(step) = c.steps.get(usize::from(bond.level) - 1) {
                    say(
                        ev,
                        Tone::Good,
                        format!("{}: {}. {}", c.name, step.name, step.text),
                    );
                }
            } else {
                say(
                    ev,
                    Tone::Good,
                    format!("{} smiles, and stays a while.", c.name),
                );
            }
        } else {
            say(
                ev,
                Tone::Dim,
                format!(
                    "{} is polite, and busy. The gift is accepted all the same.",
                    c.name
                ),
            );
        }
        Ok(())
    }

    // ---- the board ----

    fn take_contract(
        &mut self,
        content: &Content,
        n: usize,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        if self.hero.contract.is_some() {
            return Err(Refused::new(
                "You already carry a contract. Finish it or drop it.",
            ));
        }
        let board = self.board(content);
        let Some(c) = board.get(n) else {
            return Err(Refused::new("There is no such posting."));
        };
        self.hero.contract = Some(Taken {
            id: c.id.clone(),
            day: self.day,
            done: false,
        });
        say(
            ev,
            Tone::Good,
            format!(
                "You take the posting: {}. {} day(s) to do it.",
                c.name, c.days
            ),
        );
        Ok(())
    }

    fn turn_in(&mut self, content: &Content, ev: &mut Vec<Event>) -> Result<(), Refused> {
        let Some(taken) = self.hero.contract.clone() else {
            return Err(Refused::new("You have no contract."));
        };
        let Some(c) = content.contract(&taken.id) else {
            self.hero.contract = None;
            return Err(Refused::new("That posting has been taken down."));
        };
        if !taken.done {
            return Err(Refused::new("It is not done yet."));
        }
        let d = self.derived(content);
        self.hero.contract = None;
        self.hero.contracts_done = self.hero.contracts_done.saturating_add(1);
        let coin = pct(c.coin, d.loot);
        self.hero.coin = self.hero.coin.saturating_add(coin);
        say(ev, Tone::Good, c.done_text.clone());
        say(ev, Tone::Good, format!("+{coin} coin, +{} xp", c.xp));
        if let Some(r) = &c.recipe {
            self.hero.learn(r);
            let name = content.recipe(r).map_or("a recipe", |x| x.name.as_str());
            say(ev, Tone::Good, format!("You are taught a recipe: {name}."));
        }
        if let Some(core) = &c.core
            && self.hero.cores.len() < CORES_MAX
        {
            self.hero.cores.push(core.clone());
            let name = content.core(core).map_or("a core", |x| x.name.as_str());
            say(ev, Tone::Good, format!("You are given a {name}."));
        }
        self.gain_xp(c.xp, ev);
        let by = self.hero.name.clone();
        self.emit(Change::News(format!(
            "{by} finished the posting: {}.",
            c.name
        )));
        Ok(())
    }

    fn drop_contract(&mut self, content: &Content, ev: &mut Vec<Event>) -> Result<(), Refused> {
        let Some(taken) = self.hero.contract.take() else {
            return Err(Refused::new("You have no contract."));
        };
        let name = content
            .contract(&taken.id)
            .map_or("the posting", |c| c.name.as_str());
        say(ev, Tone::Dim, format!("You take your name off {name}."));
        Ok(())
    }

    /// The hero killed these monsters at `depth`: a beast contract may be done.
    pub(crate) fn contract_beast(
        &mut self,
        content: &Content,
        ids: &[String],
        depth: u8,
        ev: &mut Vec<Event>,
    ) {
        let Some(taken) = self.hero.contract.as_ref().filter(|t| !t.done) else {
            return;
        };
        let Some(c) = content.contract(&taken.id) else {
            return;
        };
        if c.kind == ContractKind::Beast
            && u32::from(depth) >= c.depth
            && c.monster.as_ref().is_some_and(|m| ids.contains(m))
        {
            self.finish_deed(&c.name, ev);
        }
    }

    /// The hero walked into a room of this kind at `depth`: an heirloom or a rescue may be done.
    pub(crate) fn contract_room(
        &mut self,
        content: &Content,
        kind: crate::content::RoomKind,
        depth: u8,
        ev: &mut Vec<Event>,
    ) {
        let Some(taken) = self.hero.contract.as_ref().filter(|t| !t.done) else {
            return;
        };
        let Some(c) = content.contract(&taken.id) else {
            return;
        };
        if matches!(c.kind, ContractKind::Heirloom | ContractKind::Rescue)
            && c.room == Some(kind)
            && u32::from(depth) >= c.depth
        {
            self.finish_deed(&c.name, ev);
        }
    }

    fn finish_deed(&mut self, name: &str, ev: &mut Vec<Event>) {
        if let Some(t) = self.hero.contract.as_mut() {
            t.done = true;
        }
        say(
            ev,
            Tone::Good,
            format!("{name}: it is done. Take it back to the tavern board."),
        );
    }

    /// Whether the hero carries a soul home (the climb costs a turn more).
    pub(crate) fn escorting(&self, content: &Content) -> bool {
        self.hero
            .contract
            .as_ref()
            .filter(|t| t.done)
            .and_then(|t| content.contract(&t.id))
            .is_some_and(|c| c.escort)
    }

    // ---- the town works ----

    fn donate(
        &mut self,
        content: &Content,
        project: &str,
        coin: u32,
        salvage: u32,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let Some(def) = content.project(project) else {
            return Err(Refused::new("There is no such project."));
        };
        if !self.world.is_open(content, project) {
            return Err(Refused::new(
                "The town cannot take gifts for that now. It is finished, or something must come first.",
            ));
        }
        let (have_c, have_s) = self.world.progress(project);
        let c = coin
            .min(self.hero.coin)
            .min(def.coin.saturating_sub(have_c));
        let s = salvage
            .min(self.hero.stash)
            .min(def.salvage.saturating_sub(have_s));
        if c + s == 0 {
            return Err(Refused::new(
                "You have nothing to give, or it needs no more of that.",
            ));
        }
        if self.world.pledges.len() >= crate::world::PLEDGES_MAX
            && !self
                .world
                .pledges
                .iter()
                .any(|p| p.hero == self.hero.name && p.project == project)
        {
            return Err(Refused::new("The works ledger is full."));
        }
        self.hero.coin -= c;
        self.hero.stash -= s;
        let total = if let Some(slot) = self.hero.given.iter_mut().find(|(p, _, _)| p == project) {
            slot.1 = slot.1.saturating_add(c);
            slot.2 = slot.2.saturating_add(s);
            (slot.1, slot.2)
        } else {
            self.hero.given.push((project.to_owned(), c, s));
            (c, s)
        };
        let was_done = self.world.is_done(content, project);
        self.emit(Change::Pledge(Pledge {
            hero: self.hero.name.clone(),
            project: project.to_owned(),
            coin: total.0,
            salvage: total.1,
        }));
        say(
            ev,
            Tone::Good,
            format!("You give {c} coin and {s} salvage toward {}.", def.name),
        );
        if !was_done && self.world.is_done(content, project) {
            let by = self.hero.name.clone();
            self.emit(Change::News(format!(
                "{by} finished a town project: {}.",
                def.name
            )));
            say(ev, Tone::Good, def.done_text.clone());
        }
        Ok(())
    }

    /// Every pledge the hero has made, as world changes (the session sends them again when it
    /// opens, so a lost write is mended; the world keeps the largest totals, so this credits
    /// nothing twice).
    pub(crate) fn pledges_of_hero(&self) -> Vec<Change> {
        self.hero
            .given
            .iter()
            .map(|(project, coin, salvage)| {
                Change::Pledge(Pledge {
                    hero: self.hero.name.clone(),
                    project: project.clone(),
                    coin: *coin,
                    salvage: *salvage,
                })
            })
            .collect()
    }

    // ---- library ----

    fn identify(&mut self, content: &Content, ev: &mut Vec<Event>) -> Result<(), Refused> {
        if !self.town_fx(content).library {
            return Err(Refused::new("The town has no one who can read relics."));
        }
        if self.hero.relics == 0 {
            return Err(Refused::new("You have no relic to read."));
        }
        self.spend_coin(20)?;
        self.hero.relics -= 1;
        let unknown: Vec<&crate::content_town::Recipe> = content
            .recipes
            .recipe
            .iter()
            .filter(|r| matches!(r.learn, Learn::Library) && !self.hero.knows(r))
            .collect();
        let roll = self.rng.below(100);
        if roll < 55 && !unknown.is_empty() {
            let r = unknown[self.rng.below(u32::try_from(unknown.len()).unwrap_or(1)) as usize];
            self.hero.learn(&r.id);
            say(
                ev,
                Tone::Good,
                format!(
                    "The scholars read the relic and copy out a pattern for you: {}.",
                    r.name
                ),
            );
        } else if roll < 80 && self.hero.cores.len() < CORES_MAX {
            let cores: Vec<&crate::content_town::Core> = content
                .affixes
                .core
                .iter()
                .filter(|c| c.band <= 3)
                .collect();
            let c = cores[self.rng.below(u32::try_from(cores.len()).unwrap_or(1)) as usize];
            self.hero.cores.push(c.id.clone());
            say(
                ev,
                Tone::Good,
                format!("The relic holds a live ember. They hand you a {}.", c.name),
            );
        } else {
            let coin = self.rng.range(30, 60);
            self.hero.coin = self.hero.coin.saturating_add(coin);
            say(
                ev,
                Tone::Good,
                format!(
                    "The relic is a maker's strongbox key. The scholars buy it from you: +{coin} coin."
                ),
            );
            self.gain_xp(25, ev);
        }
        Ok(())
    }

    // ---- the days ----

    /// The town's morning for this hero: the debt grows, a companion misses a visit, a posting
    /// lapses.
    pub(crate) fn town_morning(&mut self, content: &Content, today: i64, ev: &mut Vec<Event>) {
        if self.hero.debt > 0 {
            let interest = (self.hero.debt / 10).max(1);
            self.hero.debt = self.hero.debt.saturating_add(interest);
            if self.hero.debt > DEBT_LIMIT {
                let take = self.hero.coin.min(self.hero.debt);
                self.hero.coin -= take;
                self.hero.debt -= take;
                if self.hero.debt > 0 {
                    self.hero.inflict(content, "roughed-up");
                    say(
                        ev,
                        Tone::Bad,
                        "The dice-crew's collectors find you in the night. You are beaten and relieved of your coin.",
                    );
                } else {
                    say(
                        ev,
                        Tone::Bad,
                        "The dice-crew's collectors take what you owe, and a little extra for the trouble.",
                    );
                }
            } else {
                say(
                    ev,
                    Tone::Bad,
                    format!("The crew's debt book says you owe {}.", self.hero.debt),
                );
            }
        }
        if let Some(b) = self.hero.bond.clone()
            && today - b.last > 5
        {
            let name = content
                .companion(&b.id)
                .map_or("Someone", |c| c.name.as_str());
            if b.level == 0 {
                self.hero.bond = None;
                self.hero.closed.push(b.id);
                say(
                    ev,
                    Tone::Dim,
                    format!("{name} has stopped waiting for you, and moved on."),
                );
            } else if let Some(bond) = self.hero.bond.as_mut() {
                bond.level -= 1;
                bond.progress = 0;
                bond.last = today;
                say(
                    ev,
                    Tone::Bad,
                    format!("You have not called on {name} in days. The warmth cools a little."),
                );
            }
        }
        if let Some(t) = self.hero.contract.clone() {
            let days = content.contract(&t.id).map_or(1, |c| i64::from(c.days));
            if today - t.day >= days {
                self.hero.contract = None;
                say(
                    ev,
                    Tone::Bad,
                    format!(
                        "The posting {} has been taken down; someone else was quicker.",
                        content.contract(&t.id).map_or("", |c| c.name.as_str())
                    ),
                );
            }
        }
    }

    /// A drop from a fight: a piece of gear of the band, sometimes with an affix.
    pub(crate) fn roll_drop(&mut self, content: &Content, band: u32) -> Option<Item> {
        let bases: Vec<(&str, Slot)> = content
            .gear
            .weapon
            .iter()
            .filter(|w| w.band == band)
            .map(|w| (w.id.as_str(), Slot::Weapon))
            .chain(
                content
                    .gear
                    .armour
                    .iter()
                    .filter(|a| a.band == band)
                    .map(|a| (a.id.as_str(), Slot::Armour)),
            )
            .collect();
        if bases.is_empty() {
            return None;
        }
        let pick = bases[self.rng.below(u32::try_from(bases.len()).unwrap_or(1)) as usize];
        let mut kit = Kit::default();
        for _ in 0..2 {
            let chance = if kit.affixes.is_empty() { 50 } else { 15 };
            if self.rng.below(100) >= chance {
                break;
            }
            let fits: Vec<&Affix> = content
                .affixes
                .affix
                .iter()
                .filter(|a| a.band <= band && a.slot.fits(pick.1) && !kit.affixes.contains(&a.id))
                .collect();
            if fits.is_empty() {
                break;
            }
            let a = fits[self.rng.below(u32::try_from(fits.len()).unwrap_or(1)) as usize];
            kit.affixes.push(a.id.clone());
        }
        Some(Item {
            base: pick.0.to_owned(),
            kit,
        })
    }

    /// Carry a find; the pack below has room for only a few.
    pub(crate) fn pick_up(&mut self, content: &Content, item: Item, ev: &mut Vec<Event>) {
        let name = item_name(content, &item.base, &item.kit);
        if self.hero.found.len() >= FOUND_MAX {
            say(
                ev,
                Tone::Dim,
                format!("You find a {name}, but you cannot carry it."),
            );
        } else {
            say(ev, Tone::Good, format!("You find a {name}."));
            self.hero.found.push(item);
        }
    }

    /// The effects of a project, as a short description for the works screen.
    pub(crate) fn project_effect_text(effect: ProjectEffect) -> &'static str {
        match effect {
            ProjectEffect::Forge { .. } => "forge",
            ProjectEffect::Temple { .. } => "temple",
            ProjectEffect::Market { .. } => "market",
            ProjectEffect::Guild { .. } => "guild",
            ProjectEffect::Library => "library",
            ProjectEffect::Lift { .. } => "lift",
            ProjectEffect::Reliquary => "reliquary",
        }
    }
}
