//! The market, the fuel pump, the repair dock, the shipyard and the Guild: everything a captain
//! does while docked. Prices come from [`crate::market`]; every credit moves through the
//! player's ledger.

use crate::content::{Content, Faction, GoodClass, Perk, SlotKind};
use crate::engine::{Env, RepairTarget, TURNOVER_PER_STANDING, can_pay};
use crate::events::{Event, Refusal};
use crate::frontier::{Frontier, Role, Station};
use crate::market::{Fill, depth_at, fill_buy_within, fill_sell, spread_pm, step_mid};
use crate::player::{Cause, Player};
use crate::ship::{Installed, Lot, MAX_LOTS, Ship, WEAR_MAX, wear_gain};
use crate::skills::Skill;
use crate::world::{Delta, World};

/// A line of the market screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The slot in the station's goods list.
    pub slot: u8,
    /// The good (index into the content's goods).
    pub good: u8,
    /// What the station does with it.
    pub role: Role,
    /// Tons in stock.
    pub stock: i64,
    /// The middle price now.
    pub mid: i64,
    /// What the next ton costs to buy (0 if none in stock).
    pub buy: i64,
    /// What the next ton fetches (0 if the warehouse is full).
    pub sell: i64,
    /// Tons of it in the hold.
    pub held: u16,
    /// Whether the captain may buy it here.
    pub can_buy: bool,
}

impl Player {
    /// The spread per side this captain gets at a station.
    #[must_use]
    pub fn spread_at(&self, content: &Content, station: &Station) -> i64 {
        let faction = content.economies[usize::from(station.economy)].faction;
        spread_pm(self.level(Skill::Trade), self.standing[faction.index()])
    }

    /// Whether this captain may buy a good in a slot here (contraband only from a haven that
    /// trusts them).
    pub(crate) fn may_buy(
        &self,
        content: &Content,
        station: &Station,
        slot: usize,
    ) -> Result<(), Refusal> {
        let Some(ms) = station.goods.get(slot) else {
            return Err(Refusal::NoSuchThing);
        };
        let good = &content.goods[usize::from(ms.good)];
        if good.class == GoodClass::Contraband {
            let eco = &content.economies[usize::from(station.economy)];
            if eco.faction != Faction::Free {
                return Err(Refusal::NotForSale);
            }
            if self.standing[Faction::Free.index()] < 0 && self.perk(content) != Some(Perk::Fence) {
                return Err(Refusal::NotTrusted);
            }
        }
        Ok(())
    }

    /// The market screen's rows for the station the captain is docked at.
    #[must_use]
    pub fn market_rows(&self, content: &Content, frontier: &Frontier, world: &World) -> Vec<Row> {
        let Some(station) = self.docked.and_then(|s| frontier.station(s)) else {
            return Vec::new();
        };
        let spread = self.spread_at(content, station);
        station
            .goods
            .iter()
            .enumerate()
            .map(|(slot, ms)| {
                let good = &content.goods[usize::from(ms.good)];
                let depth = depth_at(&content.goods, frontier, station.id, slot);
                let stock = world.stock_of(station.id, slot);
                let mid = step_mid(good.base, depth, stock);
                let buy_fill = crate::market::fill_buy(good.base, depth, stock, 1, spread);
                let sell_fill = fill_sell(good.base, depth, stock, 1, spread);
                Row {
                    slot: u8::try_from(slot).unwrap_or(0),
                    good: ms.good,
                    role: ms.role,
                    stock,
                    mid,
                    buy: buy_fill.total,
                    sell: sell_fill.total,
                    held: self.ship.tons_of(ms.good),
                    can_buy: self.may_buy(content, station, slot).is_ok(),
                }
            })
            .collect()
    }

    pub(crate) fn buy(
        &mut self,
        env: &mut Env<'_>,
        slot: u8,
        tons: u16,
    ) -> Result<Vec<Event>, Refusal> {
        let content = env.content;
        let station = self.docked_at(env)?;
        let slot_ix = usize::from(slot);
        self.may_buy(content, station, slot_ix)?;
        let ms = station.goods[slot_ix];
        let good = &content.goods[usize::from(ms.good)];
        if tons == 0 {
            return Err(Refusal::NothingToDo);
        }
        let stats = self.stats(content);
        let free = u32::from(stats.cargo_cap).saturating_sub(self.ship.cargo_tons());
        if free == 0 {
            return Err(Refusal::HoldFull);
        }
        if self.ship.tons_of(ms.good) == 0 && self.ship.cargo.len() >= MAX_LOTS {
            return Err(Refusal::HoldFull);
        }
        let depth = depth_at(&content.goods, env.frontier, station.id, slot_ix);
        let stock = env.world.stock_of(station.id, slot_ix);
        if stock < 1 {
            return Err(Refusal::NoStock);
        }
        let spread = self.spread_at(content, station);
        let want = u32::from(tons).min(free);
        let fill = fill_buy_within(good.base, depth, stock, want, spread, self.cash.max(0));
        if fill.units == 0 {
            let first = crate::market::fill_buy(good.base, depth, stock, 1, spread).total;
            return Err(Refusal::NoCash { need: first });
        }
        self.debit(Cause::Trade, fill.total);
        let units = u16::try_from(fill.units).unwrap_or(u16::MAX);
        match self.ship.cargo.iter_mut().find(|l| l.good == ms.good) {
            Some(lot) => {
                lot.tons = lot.tons.saturating_add(units);
                lot.paid += fill.total;
            }
            None => self.ship.cargo.push(Lot {
                good: ms.good,
                tons: units,
                paid: fill.total,
            }),
        }
        let station_id = station.id;
        env.push(Delta::Trade {
            station: station_id,
            slot,
            tons: -i32::from(units),
        });
        self.stats.tons_traded = self.stats.tons_traded.saturating_add(fill.units);
        let mut events = vec![Event::Bought {
            good: ms.good,
            tons: fill.units,
            total: fill.total,
        }];
        let xp = u32::try_from(fill.total / 100).unwrap_or(u32::MAX).max(1);
        self.gain(Skill::Trade, xp, &mut events);
        Ok(events)
    }

    pub(crate) fn sell(
        &mut self,
        env: &mut Env<'_>,
        slot: u8,
        tons: u16,
    ) -> Result<Vec<Event>, Refusal> {
        let content = env.content;
        let station = self.docked_at(env)?;
        let slot_ix = usize::from(slot);
        let ms = *station.goods.get(slot_ix).ok_or(Refusal::NoSuchThing)?;
        let good = &content.goods[usize::from(ms.good)];
        let have = self.ship.tons_of(ms.good);
        if have == 0 {
            return Err(Refusal::NothingToSell);
        }
        if tons == 0 {
            return Err(Refusal::NothingToDo);
        }
        let depth = depth_at(&content.goods, env.frontier, station.id, slot_ix);
        let stock = env.world.stock_of(station.id, slot_ix);
        let eco = &content.economies[usize::from(station.economy)];
        let spread = self.spread_at(content, station);
        let want = u32::from(tons.min(have));
        let mut fill: Fill = fill_sell(good.base, depth, stock, want, spread);
        if fill.units == 0 {
            return Err(Refusal::WarehouseFull);
        }
        let contraband = good.class == GoodClass::Contraband;
        if contraband && eco.faction == Faction::Free && self.perk(content) == Some(Perk::Fence) {
            fill.total = fill.total * 11 / 10;
        }
        let Some(lot_ix) = self.ship.cargo.iter().position(|l| l.good == ms.good) else {
            return Err(Refusal::NothingToSell);
        };
        let lot = self.ship.cargo[lot_ix];
        let basis = lot.paid * i64::from(fill.units) / i64::from(lot.tons.max(1));
        let units = u16::try_from(fill.units).unwrap_or(u16::MAX);
        if units >= lot.tons {
            self.ship.cargo.remove(lot_ix);
        } else {
            self.ship.cargo[lot_ix].tons -= units;
            self.ship.cargo[lot_ix].paid -= basis;
        }
        self.credit(Cause::Trade, fill.total);
        let station_id = station.id;
        let faction = eco.faction;
        env.push(Delta::Trade {
            station: station_id,
            slot,
            tons: i32::from(units),
        });
        self.stats.tons_traded = self.stats.tons_traded.saturating_add(fill.units);
        let mut events = vec![Event::Sold {
            good: ms.good,
            tons: fill.units,
            total: fill.total,
            profit: fill.total - basis,
        }];
        let xp = u32::try_from(fill.total / 100).unwrap_or(u32::MAX).max(1);
        self.gain(Skill::Trade, xp, &mut events);
        self.sale_standing(faction, fill.total, contraband, fill.units, &mut events);
        Ok(events)
    }

    fn sale_standing(
        &mut self,
        faction: Faction,
        total: i64,
        contraband: bool,
        tons: u32,
        events: &mut Vec<Event>,
    ) {
        if contraband {
            self.stats.contraband_sold = self.stats.contraband_sold.saturating_add(tons);
            self.shift_standing(Faction::Free, 2, events);
            return;
        }
        let slot = &mut self.turnover[faction.index()];
        *slot += total;
        let steps = *slot / TURNOVER_PER_STANDING;
        if steps > 0 {
            *slot -= steps * TURNOVER_PER_STANDING;
            self.shift_standing(faction, i16::try_from(steps.min(10)).unwrap_or(10), events);
        }
    }

    pub(crate) fn refuel(&mut self, env: &Env<'_>, units: u16) -> Result<Vec<Event>, Refusal> {
        let content = env.content;
        let station = self.docked_at(env)?;
        let eco = &content.economies[usize::from(station.economy)];
        let stats = self.stats(content);
        let room = stats.fuel_cap.saturating_sub(self.ship.fuel);
        if room == 0 {
            return Err(Refusal::TankFull);
        }
        let mut price = crate::market::fuel_price(eco.fuel_pm);
        if eco.faction == Faction::League && self.perk(content) == Some(Perk::KnownFace) {
            price = (price * 9 / 10).max(1);
        }
        let want = units.min(room);
        let affordable =
            u16::try_from((self.cash.max(0) / price).min(i64::from(u16::MAX))).unwrap_or(0);
        let take = want.min(affordable);
        if take == 0 {
            return Err(Refusal::NoCash { need: price });
        }
        let cost = price * i64::from(take);
        self.debit(Cause::Fuel, cost);
        self.ship.fuel += take;
        Ok(vec![Event::Refueled { units: take, cost }])
    }

    /// What it would cost to repair `target`, and how many drive-days it takes.
    #[must_use]
    pub fn repair_quote(&self, env: &Env<'_>, target: RepairTarget) -> (i64, u32) {
        let content = env.content;
        let eng = i64::from(self.level(Skill::Engineer).min(10));
        let discount = 100 - 5 * eng;
        let mut cost = 0i64;
        let mut work = 0u32;
        let mut module_cost = |inst: &Installed| {
            let price = content
                .modules
                .get(usize::from(inst.module))
                .map_or(0, |m| i64::from(m.price));
            cost += price * i64::from(inst.wear) / 1000 * 35 / 100;
            work += u32::from(inst.wear);
        };
        match target {
            RepairTarget::All | RepairTarget::Hull => {}
            RepairTarget::Module(i) => {
                if let Some(inst) = self.ship.modules.get(i) {
                    module_cost(inst);
                }
            }
        }
        if target == RepairTarget::All {
            for inst in &self.ship.modules {
                module_cost(inst);
            }
        }
        if matches!(target, RepairTarget::All | RepairTarget::Hull) {
            let hull = content.hulls.get(usize::from(self.ship.hull));
            let max = hull.map_or(0, |h| h.hull_points);
            let missing = u32::from(max.saturating_sub(self.ship.hull_hp));
            // Thirty percent of the hull's price over the whole range of structural points. A
            // hull trades in at half price whatever its state, so repairing it never adds to
            // net worth (the invariant test relies on that).
            if let Some(h) = hull {
                cost += i64::from(missing) * i64::from(h.price) * 30
                    / (i64::from(h.hull_points.max(1)) * 100);
            }
            work += missing * 12;
        }
        let cost = cost * discount / 100;
        let days = if work == 0 {
            0
        } else {
            work.div_ceil(300).max(1)
        };
        (cost, days)
    }

    pub(crate) fn repair(
        &mut self,
        env: &Env<'_>,
        target: RepairTarget,
    ) -> Result<Vec<Event>, Refusal> {
        let station = self.docked_at(env)?;
        if !station.repair {
            return Err(Refusal::NoRepair);
        }
        if let RepairTarget::Module(i) = target
            && i >= self.ship.modules.len()
        {
            return Err(Refusal::NoSuchThing);
        }
        let (cost, days) = self.repair_quote(env, target);
        if days == 0 {
            return Err(Refusal::NothingToRepair);
        }
        self.require_day()?;
        can_pay(self.cash, cost)?;
        self.debit(Cause::Repair, cost);
        let max = env
            .content
            .hulls
            .get(usize::from(self.ship.hull))
            .map_or(0, |h| h.hull_points);
        match target {
            RepairTarget::All => {
                for inst in &mut self.ship.modules {
                    inst.wear = 0;
                }
                self.ship.hull_hp = max;
            }
            RepairTarget::Hull => self.ship.hull_hp = max,
            RepairTarget::Module(i) => self.ship.modules[i].wear = 0,
        }
        self.spend_days(days);
        let mut events = vec![Event::Repaired { cost, days }];
        let xp = u32::try_from(cost / 100).unwrap_or(0) + 2;
        self.gain(Skill::Engineer, xp, &mut events);
        Ok(events)
    }

    pub(crate) fn buy_module(&mut self, env: &Env<'_>, module: u8) -> Result<Vec<Event>, Refusal> {
        let content = env.content;
        let station = self.docked_at(env)?;
        if !station.shipyard {
            return Err(Refusal::NoShipyard);
        }
        let def = content
            .modules
            .get(usize::from(module))
            .ok_or(Refusal::NoSuchThing)?;
        let hull = &content.hulls[usize::from(self.ship.hull)];
        if self.ship.used_slots(content, def.slot) >= usize::from(hull.slots.of(def.slot)) {
            return Err(Refusal::SlotsFull(def.slot));
        }
        let price = i64::from(def.price);
        can_pay(self.cash, price)?;
        self.debit(Cause::Module, price);
        self.ship.modules.push(Installed { module, wear: 0 });
        Ok(vec![Event::ModuleBought { module, price }])
    }

    pub(crate) fn sell_module(
        &mut self,
        env: &Env<'_>,
        index: usize,
    ) -> Result<Vec<Event>, Refusal> {
        let content = env.content;
        let station = self.docked_at(env)?;
        if !station.shipyard {
            return Err(Refusal::NoShipyard);
        }
        let inst = *self.ship.modules.get(index).ok_or(Refusal::NoSuchThing)?;
        let def = content
            .modules
            .get(usize::from(inst.module))
            .ok_or(Refusal::NoSuchThing)?;
        if def.slot == SlotKind::Drive && self.ship.used_slots(content, SlotKind::Drive) <= 1 {
            return Err(Refusal::LastDrive);
        }
        let price = Ship::module_resale(&inst, content);
        let mut without = self.ship.clone();
        without.modules.remove(index);
        let stats = without.stats(
            content,
            self.level(Skill::Pilot),
            self.level(Skill::Engineer),
        );
        if self.ship.cargo_tons() > u32::from(stats.cargo_cap) {
            return Err(Refusal::HoldWouldOverflow);
        }
        self.ship.modules.remove(index);
        self.ship.fuel = self.ship.fuel.min(stats.fuel_cap);
        self.credit(Cause::Module, price);
        Ok(vec![Event::ModuleSold {
            module: inst.module,
            price,
        }])
    }

    pub(crate) fn buy_hull(&mut self, env: &Env<'_>, hull: u8) -> Result<Vec<Event>, Refusal> {
        let content = env.content;
        let station = self.docked_at(env)?;
        if !station.shipyard {
            return Err(Refusal::NoShipyard);
        }
        let new = content
            .hulls
            .get(usize::from(hull))
            .ok_or(Refusal::NoSuchThing)?;
        if hull == self.ship.hull {
            return Err(Refusal::NothingToDo);
        }
        let trade_in = self.ship.hull_resale(content);
        let mut kept: Vec<Installed> = Vec::new();
        let mut sold = 0i64;
        let mut modules = self.ship.modules.clone();
        modules.sort_by_key(|m| {
            std::cmp::Reverse(
                content
                    .modules
                    .get(usize::from(m.module))
                    .map_or(0, |d| d.price),
            )
        });
        for inst in modules {
            let Some(def) = content.modules.get(usize::from(inst.module)) else {
                continue;
            };
            let used = kept
                .iter()
                .filter(|k| {
                    content
                        .modules
                        .get(usize::from(k.module))
                        .is_some_and(|d| d.slot == def.slot)
                })
                .count();
            if used < usize::from(new.slots.of(def.slot)) {
                kept.push(inst);
            } else {
                sold += Ship::module_resale(&inst, content);
            }
        }
        let trial = Ship {
            hull,
            hull_hp: new.hull_points,
            fuel: 0,
            modules: kept.clone(),
            cargo: Vec::new(),
        };
        let stats = trial.stats(content, 0, 0);
        if self.ship.cargo_tons() > u32::from(stats.cargo_cap) {
            return Err(Refusal::HoldWouldOverflow);
        }
        if !kept.iter().any(|k| {
            content
                .modules
                .get(usize::from(k.module))
                .is_some_and(|d| d.slot == SlotKind::Drive)
        }) {
            return Err(Refusal::NothingToDo);
        }
        let net = i64::from(new.price) - trade_in;
        can_pay(self.cash + sold, net)?;
        self.credit(Cause::Hull, trade_in + sold);
        self.debit(Cause::Hull, i64::from(new.price));
        let fuel = self.ship.fuel;
        let cargo = std::mem::take(&mut self.ship.cargo);
        self.ship = Ship {
            hull,
            hull_hp: new.hull_points,
            fuel: fuel.min(stats.fuel_cap),
            modules: kept,
            cargo,
        };
        Ok(vec![Event::HullBought {
            hull,
            net,
            modules_sold: sold,
        }])
    }

    pub(crate) fn sell_survey(&mut self, env: &Env<'_>) -> Result<Vec<Event>, Refusal> {
        let station = self.docked_at(env)?;
        if env.content.economies[usize::from(station.economy)].id != "research" {
            return Err(Refusal::NoGuild);
        }
        if self.survey.is_empty() {
            return Err(Refusal::NothingToSell);
        }
        let lots = u32::try_from(self.survey.len()).unwrap_or(0);
        let mut total: i64 = self.survey.iter().map(|s| i64::from(s.value)).sum();
        if self.perk(env.content) == Some(Perk::GuildContact) {
            total = total * 125 / 100;
        }
        self.survey.clear();
        self.credit(Cause::Survey, total);
        let mut events = vec![Event::SurveySold { lots, total }];
        let xp = u32::try_from(total / 100).unwrap_or(0).max(1);
        self.gain(Skill::Survey, xp, &mut events);
        Ok(events)
    }

    /// Wear on every module as a percent, for the screen (the worst one).
    #[must_use]
    pub fn worst_wear_pct(&self) -> u16 {
        self.ship
            .modules
            .iter()
            .map(|m| m.wear.min(WEAR_MAX) / 10)
            .max()
            .unwrap_or(0)
    }

    /// Fuel and wear gained by a use, exposed for tests of the Engineering branch.
    #[must_use]
    pub fn wear_for(&self, base: u32) -> u16 {
        wear_gain(base, self.level(Skill::Engineer))
    }
}
