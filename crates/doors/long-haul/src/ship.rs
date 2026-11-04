//! The ship: a hull with module slots, wear, fuel and a hold.
//!
//! Wear is kept in tenths of a percent (0 to 1000). A worn module works worse: its effect is
//! scaled by [`condition_pct`], from 100 % new down to 50 % at full wear, and a drive past 60 %
//! wear can fault mid-jump. Cargo is kept as lots with the credits paid, so the screen can show
//! a profit and a sale can be priced against what the cargo cost.

use crate::content::{Content, Module, SlotKind};

/// Wear when a module is finished.
pub const WEAR_MAX: u16 = 1000;
/// The most cargo lots a hold keeps (one per good).
pub const MAX_LOTS: usize = 32;

/// A module in a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Installed {
    /// Index into the content's modules.
    pub module: u8,
    /// Wear, tenths of a percent.
    pub wear: u16,
}

/// A load of one good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lot {
    /// Index into the content's goods.
    pub good: u8,
    /// Tons.
    pub tons: u16,
    /// Credits paid for the whole lot.
    pub paid: i64,
}

/// A captain's ship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ship {
    /// Index into the content's hulls.
    pub hull: u8,
    /// Structural points left.
    pub hull_hp: u16,
    /// Fuel on board.
    pub fuel: u16,
    /// Installed modules.
    pub modules: Vec<Installed>,
    /// The hold.
    pub cargo: Vec<Lot>,
}

/// What a ship can do right now, from its hull, modules and wear, and its captain's skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Cargo capacity, tons.
    pub cargo_cap: u16,
    /// Fuel capacity.
    pub fuel_cap: u16,
    /// Speed in tenths of a span per drive-day (at least 1).
    pub speed_x10: u32,
    /// Fuel per hundred spans.
    pub burn: u32,
    /// Best working scanner grade, 0 if none.
    pub scan: u8,
    /// Hazard damage absorbed, percent.
    pub shield_pct: u8,
    /// Extra survey value, per-mille.
    pub lab_pm: u32,
    /// Extra skim yield, per-mille.
    pub refine_pm: u32,
    /// Berths (for crew, a later slice).
    pub berths: u8,
    /// Total weapon power (for combat, a later slice).
    pub power: u8,
}

/// How well a module with `wear` works, percent.
#[must_use]
pub fn condition_pct(wear: u16) -> u32 {
    100 - u32::from(wear.min(WEAR_MAX)) / 20
}

/// Wear gained by a use that would add `base` tenths to a new engineer, scaled down 4 % a level
/// of Engineering (never below one tenth).
#[must_use]
pub fn wear_gain(base: u32, engineer_level: u8) -> u16 {
    let scaled = base * (100 - 4 * u32::from(engineer_level.min(10))) / 100;
    u16::try_from(scaled.max(1)).unwrap_or(u16::MAX)
}

impl Ship {
    /// The starter freighter every captain flies away from the yard in.
    #[must_use]
    pub fn starter(content: &Content, with_scanner: bool) -> Self {
        let hull = content.hull_ix("freighter").unwrap_or(0);
        let mut ids = vec!["ion1", "pod1", "tank1"];
        if with_scanner {
            ids.push("scan1");
        }
        let modules = ids
            .into_iter()
            .filter_map(|id| content.module_ix(id))
            .map(|m| Installed {
                module: u8::try_from(m).unwrap_or(0),
                wear: 0,
            })
            .collect();
        let mut ship = Self {
            hull: u8::try_from(hull).unwrap_or(0),
            hull_hp: content.hulls.get(hull).map_or(1, |h| h.hull_points),
            fuel: 0,
            modules,
            cargo: Vec::new(),
        };
        ship.fuel = ship.stats(content, 0, 0).fuel_cap;
        ship
    }

    /// The bare courier a lender hands back after a repossession.
    #[must_use]
    pub fn hand_me_down(content: &Content) -> Self {
        let hull = content.hull_ix("courier").unwrap_or(0);
        let drive = content.module_ix("ion1").unwrap_or(0);
        let mut ship = Self {
            hull: u8::try_from(hull).unwrap_or(0),
            hull_hp: content.hulls.get(hull).map_or(1, |h| h.hull_points),
            fuel: 0,
            modules: vec![Installed {
                module: u8::try_from(drive).unwrap_or(0),
                wear: 400,
            }],
            cargo: Vec::new(),
        };
        ship.fuel = ship.stats(content, 0, 0).fuel_cap / 2;
        ship
    }

    /// Tons in the hold.
    #[must_use]
    pub fn cargo_tons(&self) -> u32 {
        self.cargo.iter().map(|l| u32::from(l.tons)).sum()
    }

    /// Tons of one good in the hold.
    #[must_use]
    pub fn tons_of(&self, good: u8) -> u16 {
        self.cargo
            .iter()
            .find(|l| l.good == good)
            .map_or(0, |l| l.tons)
    }

    /// Whether the hold has any contraband.
    #[must_use]
    pub fn contraband_tons(&self, content: &Content) -> u32 {
        self.cargo
            .iter()
            .filter(|l| {
                content
                    .goods
                    .get(usize::from(l.good))
                    .is_some_and(|g| g.class == crate::content::GoodClass::Contraband)
            })
            .map(|l| u32::from(l.tons))
            .sum()
    }

    /// Modules of one slot kind, with their definitions.
    pub fn of_kind<'a>(
        &'a self,
        content: &'a Content,
        kind: SlotKind,
    ) -> impl Iterator<Item = (usize, &'a Installed, &'a Module)> {
        self.modules
            .iter()
            .enumerate()
            .filter_map(move |(i, inst)| {
                let def = content.modules.get(usize::from(inst.module))?;
                (def.slot == kind).then_some((i, inst, def))
            })
    }

    /// Slots of a kind in use.
    #[must_use]
    pub fn used_slots(&self, content: &Content, kind: SlotKind) -> usize {
        self.of_kind(content, kind).count()
    }

    /// What the ship can do, for a captain with the given Piloting and Engineering levels.
    #[must_use]
    pub fn stats(&self, content: &Content, pilot: u8, _engineer: u8) -> Stats {
        let Some(hull) = content.hulls.get(usize::from(self.hull)) else {
            return Stats::default();
        };
        let mut s = Stats {
            cargo_cap: hull.cargo,
            fuel_cap: hull.fuel,
            speed_x10: 10,
            burn: 30,
            ..Stats::default()
        };
        let mut shield = 0u32;
        let pilot = u32::from(pilot.min(10));
        for inst in &self.modules {
            let Some(m) = content.modules.get(usize::from(inst.module)) else {
                continue;
            };
            let cond = condition_pct(inst.wear);
            match m.slot {
                SlotKind::Drive => {
                    let speed = u64::from(m.speed) * 10 * u64::from(hull.speed_pm) / 1000;
                    let speed = speed * u64::from(cond) / 100 * u64::from(100 + 2 * pilot) / 100;
                    s.speed_x10 = u32::try_from(speed).unwrap_or(u32::MAX).max(1);
                    s.burn = (m.burn * (100 - 3 * pilot) / 100).max(1);
                }
                SlotKind::Cargo => s.cargo_cap = s.cargo_cap.saturating_add(m.cargo),
                SlotKind::Tank => s.fuel_cap = s.fuel_cap.saturating_add(m.fuel),
                SlotKind::Scanner => {
                    let working = u8::try_from((u32::from(m.scan) * cond + 50) / 100).unwrap_or(0);
                    s.scan = s.scan.max(working);
                }
                SlotKind::Weapon => s.power = s.power.saturating_add(m.power),
                SlotKind::Shield => shield += u32::from(m.shield) * cond / 100,
                SlotKind::Cabin => s.berths = s.berths.saturating_add(m.berths),
                SlotKind::Refinery => s.refine_pm += m.refine_pm * cond / 100,
                SlotKind::Lab => s.lab_pm += m.lab_pm * cond / 100,
            }
        }
        s.shield_pct = u8::try_from(shield.min(90)).unwrap_or(90);
        s
    }

    /// Fuel for a lane of `dist` spans.
    #[must_use]
    pub fn fuel_for(stats: &Stats, dist: u16) -> u16 {
        u16::try_from((u32::from(dist) * stats.burn).div_ceil(100).max(1)).unwrap_or(u16::MAX)
    }

    /// Drive-days for a lane of `dist` spans.
    #[must_use]
    pub fn days_for(stats: &Stats, dist: u16) -> u32 {
        (u32::from(dist) * 10)
            .div_ceil(stats.speed_x10.max(1))
            .max(1)
    }

    /// Credits a module is worth at a yard (half its price, less wear).
    #[must_use]
    pub fn module_resale(inst: &Installed, content: &Content) -> i64 {
        content
            .modules
            .get(usize::from(inst.module))
            .map_or(0, |m| {
                i64::from(m.price) / 2 * (2000 - i64::from(inst.wear.min(WEAR_MAX))) / 2000
            })
    }

    /// Credits the hull is worth at a yard (half its price, scaled by structure).
    #[must_use]
    pub fn hull_resale(&self, content: &Content) -> i64 {
        content.hulls.get(usize::from(self.hull)).map_or(0, |h| {
            i64::from(h.price) / 2 * i64::from(self.hull_hp) / i64::from(h.hull_points.max(1))
        })
    }

    /// Whether the structure is too weak to jump (under a fifth).
    #[must_use]
    pub fn hull_too_weak(&self, content: &Content) -> bool {
        content
            .hulls
            .get(usize::from(self.hull))
            .is_none_or(|h| u32::from(self.hull_hp) * 5 < u32::from(h.hull_points))
    }
}
