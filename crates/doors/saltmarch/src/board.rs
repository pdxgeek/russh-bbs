//! The faction boards: taking, doing, handing back and losing contracts. The templates are
//! data (`content/contracts.toml`, schema in [`crate::story`]); this is the engine that applies
//! them. Everything here is local to one clan's save: a contract's pay comes from the Tidecourt
//! and the guilds, not from another player, so nothing crosses to the world.

use russh_bbs_door::DoorRng;

use crate::clan::Clan;
use crate::content::Catalog;
use crate::engine::ActionError;
use crate::event::Event;
use crate::map::PoiKind;
use crate::story::{Active, ContractDef, ContractError, ContractKind, board};
use crate::types::Resource;

/// Percent added to a contract's pay for each tier of standing above Stranger.
pub const TIER_BONUS_PCT: u32 = 5;

/// What a contract pays in tallies, with the faction's regard for the clan: 5% more for each
/// tier above Stranger.
fn wage(def: &ContractDef, clan: &Clan, rng: &mut DoorRng) -> u32 {
    let base = rng.range(u32::from(def.pay_lo), u32::from(def.pay_hi));
    let tier = u32::from(clan.story.tier(def.faction).saturating_sub(1));
    base * (100 + TIER_BONUS_PCT * tier) / 100
}

fn finish(def: &ContractDef, clan: &mut Clan, rng: &mut DoorRng, events: &mut Vec<Event>) {
    let pay = wage(def, clan, rng);
    clan.tallies = clan.tallies.saturating_add(pay);
    clan.story.contract = None;
    clan.story.done = clan.story.done.saturating_add(1);
    events.push(Event::ContractDone {
        title: def.title.clone(),
        tallies: pay,
    });
    let (before, after) = clan.story.add_rep(def.faction, i16::from(def.rep));
    events.push(Event::Standing {
        faction: def.faction,
        by: def.rep,
        tier: after,
        changed: before != after,
    });
}

/// The offers on the board today for this clan.
#[must_use]
pub fn offers<'a>(cat: &'a Catalog, clan: &Clan, seed: u64, today: i64) -> Vec<&'a ContractDef> {
    board(&cat.contracts, &clan.story, seed, today)
}

/// Take a contract from today's board.
///
/// # Errors
/// [`ContractError::Busy`] with one in hand, [`ContractError::NotOffered`] if it is not on
/// today's board for this clan.
pub fn take(
    cat: &Catalog,
    clan: &mut Clan,
    seed: u64,
    today: i64,
    id: &str,
    events: &mut Vec<Event>,
) -> Result<(), ContractError> {
    if clan.story.contract.is_some() {
        return Err(ContractError::Busy);
    }
    let def = offers(cat, clan, seed, today)
        .into_iter()
        .find(|c| c.id == id)
        .ok_or(ContractError::NotOffered)?;
    clan.story.contract = Some(Active {
        id: def.id.clone(),
        deadline: today + i64::from(def.days) - 1,
        progress: 0,
    });
    events.push(Event::ContractTaken {
        title: def.title.clone(),
        days: def.days,
    });
    Ok(())
}

/// Hand a contract back (the faction thinks a little worse of the clan).
///
/// # Errors
/// [`ContractError::None`] when nothing is in hand.
pub fn give_back(
    cat: &Catalog,
    clan: &mut Clan,
    events: &mut Vec<Event>,
) -> Result<(), ContractError> {
    let id = clan
        .story
        .contract
        .as_ref()
        .map(|a| a.id.clone())
        .ok_or(ContractError::None)?;
    clan.story.contract = None;
    if let Some(def) = cat.contracts.iter().find(|c| c.id == id) {
        clan.story.failed = clan.story.failed.saturating_add(1);
        let by = -((i16::from(def.fail) + 1) / 2);
        let (before, after) = clan.story.add_rep(def.faction, by);
        events.push(Event::ContractLapsed {
            title: def.title.clone(),
            given_back: true,
        });
        events.push(Event::Standing {
            faction: def.faction,
            by: i8::try_from(by).unwrap_or(-1),
            tier: after,
            changed: before != after,
        });
    }
    Ok(())
}

/// Hand over the goods a `deliver` contract asks for, from the stores.
///
/// # Errors
/// [`ContractError::None`], [`ContractError::WrongKind`] or [`ContractError::Short`].
pub fn deliver(
    cat: &Catalog,
    clan: &mut Clan,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<(), ContractError> {
    let id = clan
        .story
        .contract
        .as_ref()
        .map(|a| a.id.clone())
        .ok_or(ContractError::None)?;
    let def = cat
        .contracts
        .iter()
        .find(|c| c.id == id)
        .ok_or(ContractError::None)?;
    if def.kind != ContractKind::Deliver {
        return Err(ContractError::WrongKind);
    }
    let good = def.resource().ok_or(ContractError::WrongKind)?;
    let qty = u32::from(def.qty);
    if clan.stock.get(good) < qty {
        return Err(ContractError::Short(format!(
            "{} (need {qty}, have {})",
            good.name(),
            clan.stock.get(good)
        )));
    }
    clan.stock.take_up_to(good, qty);
    finish(def, clan, rng, events);
    Ok(())
}

/// A fight was won against these foes (by id): a `clear` contract counts the win if one of
/// them is the foe it names.
pub fn on_fight_won(
    cat: &Catalog,
    clan: &mut Clan,
    foes: &[&str],
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) {
    let Some(a) = clan.story.contract.clone() else {
        return;
    };
    let Some(def) = cat.contracts.iter().find(|c| c.id == a.id) else {
        return;
    };
    if def.kind != ContractKind::Clear || !foes.iter().any(|f| Some(*f) == def.foe.as_deref()) {
        return;
    }
    let progress = a.progress.saturating_add(1);
    if progress >= def.qty {
        finish(def, clan, rng, events);
    } else if let Some(c) = clan.story.contract.as_mut() {
        c.progress = progress;
    }
}

/// An expedition came home with this cargo (already in the stores): a `salvage` contract
/// takes the goods it asks for out of the stores.
pub fn on_return(
    cat: &Catalog,
    clan: &mut Clan,
    cargo: &[(Resource, u32)],
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) {
    let Some(a) = clan.story.contract.clone() else {
        return;
    };
    let Some(def) = cat.contracts.iter().find(|c| c.id == a.id) else {
        return;
    };
    if def.kind != ContractKind::Salvage {
        return;
    }
    let Some(good) = def.resource() else { return };
    let brought: u32 = cargo
        .iter()
        .filter(|(r, _)| *r == good)
        .map(|(_, n)| *n)
        .sum();
    let have = u32::from(a.progress) + brought;
    if have >= u32::from(def.qty) {
        clan.stock.take_up_to(
            good,
            u32::from(def.qty).saturating_sub(u32::from(a.progress)),
        );
        finish(def, clan, rng, events);
    } else if brought > 0
        && let Some(c) = clan.story.contract.as_mut()
    {
        // What was brought is held back for the contract and counted.
        clan.stock.take_up_to(good, brought);
        c.progress = u16::try_from(have).unwrap_or(u16::MAX);
    }
}

/// The party stands at a place of this kind: an `escort` contract for that kind is done.
pub fn on_place(
    cat: &Catalog,
    clan: &mut Clan,
    kind: PoiKind,
    party: usize,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) {
    let Some(a) = clan.story.contract.as_ref() else {
        return;
    };
    let Some(def) = cat.contracts.iter().find(|c| c.id == a.id) else {
        return;
    };
    // An escort needs company: a party of one has no one to see to the place.
    if def.kind == ContractKind::Escort && def.poi == Some(kind) && party >= 2 {
        finish(def, clan, rng, events);
    }
}

/// A new day: a contract past its deadline lapses and the faction remembers.
pub fn expire(cat: &Catalog, clan: &mut Clan, today: i64, events: &mut Vec<Event>) {
    let Some(a) = clan.story.contract.clone() else {
        return;
    };
    if today <= a.deadline {
        return;
    }
    clan.story.contract = None;
    clan.story.failed = clan.story.failed.saturating_add(1);
    if let Some(def) = cat.contracts.iter().find(|c| c.id == a.id) {
        let (before, after) = clan.story.add_rep(def.faction, -i16::from(def.fail));
        events.push(Event::ContractLapsed {
            title: def.title.clone(),
            given_back: false,
        });
        events.push(Event::Standing {
            faction: def.faction,
            by: -i8::try_from(def.fail).unwrap_or(1),
            tier: after,
            changed: before != after,
        });
    }
}

/// Why taking or doing a contract was refused, as an action error for the engine.
impl From<ContractError> for ActionError {
    fn from(e: ContractError) -> Self {
        Self::Missing(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::similar_names,
        clippy::unnecessary_wraps
    )]
    use super::*;
    use crate::clan::tests::fixture;
    use crate::story::Faction;

    const SEED: u64 = 11;
    const DAY: i64 = 20_000;

    /// A clan of this standing everywhere, and the id of a contract of `id` made available by
    /// putting it on today's board (the draw is deterministic, so search the days for one).
    fn offered(id: &str, standing: i16) -> (Catalog, Clan, i64) {
        let (cat, _, mut clan) = fixture();
        clan.story.rep = [standing; 3];
        let day = (DAY..DAY + 400)
            .find(|d| offers(&cat, &clan, SEED, *d).iter().any(|c| c.id == id))
            .expect("a contract of that id is on some board");
        (cat, clan, day)
    }

    fn rng() -> DoorRng {
        DoorRng::from_seed(5)
    }

    #[test]
    fn the_board_is_the_same_for_the_same_standing_and_day_and_closed_to_outcasts() {
        let (cat, _, mut clan) = fixture();
        let a: Vec<_> = offers(&cat, &clan, SEED, DAY)
            .iter()
            .map(|c| &c.id)
            .collect();
        let b: Vec<_> = offers(&cat, &clan, SEED, DAY)
            .iter()
            .map(|c| &c.id)
            .collect();
        assert_eq!(a, b);
        assert!(a.len() <= crate::story::OFFERS && !a.is_empty());
        assert!(
            offers(&cat, &clan, SEED, DAY).iter().all(|c| c.tier <= 1),
            "a Stranger is offered only the first tier"
        );
        clan.story.rep = [70; 3];
        let high =
            (DAY..DAY + 60).any(|d| offers(&cat, &clan, SEED, d).iter().any(|c| c.tier >= 3));
        assert!(high, "the Honoured see the big contracts");
        clan.story.rep = [-30; 3];
        assert!(
            offers(&cat, &clan, SEED, DAY).is_empty(),
            "an outcast is offered nothing"
        );
        clan.story.rep = [0, 0, -30];
        assert!(
            (DAY..DAY + 60).all(|d| offers(&cat, &clan, SEED, d)
                .iter()
                .all(|c| c.faction != Faction::Reed)),
            "one outcast faction closes only its own board"
        );
    }

    #[test]
    fn a_delivery_takes_the_goods_pays_and_moves_standing_with_the_tier_bonus() {
        let (cat, mut clan, day) = offered("rk-salt-sacks", 0);
        let mut ev = Vec::new();
        take(&cat, &mut clan, SEED, day, "rk-salt-sacks", &mut ev).unwrap();
        assert_eq!(
            take(&cat, &mut clan, SEED, day, "rk-salt-sacks", &mut ev),
            Err(ContractError::Busy)
        );
        clan.stock = crate::types::Stock::default();
        assert!(matches!(
            deliver(&cat, &mut clan, &mut rng(), &mut ev),
            Err(ContractError::Short(_))
        ));
        clan.stock.add(Resource::Salt, 20);
        let (t0, rep0) = (clan.tallies, clan.story.rep[Faction::Rakers.index()]);
        deliver(&cat, &mut clan, &mut rng(), &mut ev).unwrap();
        let def = cat
            .contracts
            .iter()
            .find(|c| c.id == "rk-salt-sacks")
            .unwrap();
        assert_eq!(clan.stock.get(Resource::Salt), 20 - u32::from(def.qty));
        let pay = clan.tallies - t0;
        assert!(
            (u32::from(def.pay_lo)..=u32::from(def.pay_hi)).contains(&pay),
            "{pay}"
        );
        assert_eq!(
            clan.story.rep[Faction::Rakers.index()],
            rep0 + i16::from(def.rep)
        );
        assert_eq!((clan.story.done, clan.story.contract.is_none()), (1, true));
        assert!(ev.iter().any(|e| matches!(e, Event::ContractDone { .. })));
        // A clan the guild thinks well of is paid more for the same sack.
        let mut fan = clan.clone();
        fan.story.rep[Faction::Rakers.index()] = 70;
        fan.stock.add(Resource::Salt, 40);
        fan.story.contract = Some(Active {
            id: def.id.clone(),
            deadline: day + 3,
            progress: 0,
        });
        let before = fan.tallies;
        deliver(&cat, &mut fan, &mut DoorRng::from_seed(5), &mut ev).unwrap();
        let mut plain = clan.clone();
        plain.stock.add(Resource::Salt, 40);
        plain.story.contract = fan_contract(def, day);
        let before_p = plain.tallies;
        deliver(&cat, &mut plain, &mut DoorRng::from_seed(5), &mut ev).unwrap();
        assert!(fan.tallies - before > plain.tallies - before_p);
    }

    fn fan_contract(def: &ContractDef, day: i64) -> Option<Active> {
        Some(Active {
            id: def.id.clone(),
            deadline: day + 3,
            progress: 0,
        })
    }

    #[test]
    fn a_clear_counts_wins_over_the_named_foe_only() {
        let (cat, mut clan, day) = offered("rk-hound-bounty", 0);
        let mut ev = Vec::new();
        take(&cat, &mut clan, SEED, day, "rk-hound-bounty", &mut ev).unwrap();
        on_fight_won(&cat, &mut clan, &["mire-eel"], &mut rng(), &mut ev);
        assert_eq!(clan.story.contract.as_ref().unwrap().progress, 0);
        on_fight_won(
            &cat,
            &mut clan,
            &["bog-hound", "bog-hound"],
            &mut rng(),
            &mut ev,
        );
        assert_eq!(clan.story.contract.as_ref().unwrap().progress, 1);
        let t = clan.tallies;
        on_fight_won(&cat, &mut clan, &["bog-hound"], &mut rng(), &mut ev);
        assert!(clan.story.contract.is_none() && clan.tallies > t && clan.story.done == 1);
    }

    #[test]
    fn a_salvage_takes_what_the_trip_brought_home_in_one_or_two_trips() {
        let (cat, mut clan, day) = offered("rk-iron-pans", 20);
        let mut ev = Vec::new();
        take(&cat, &mut clan, SEED, day, "rk-iron-pans", &mut ev).unwrap();
        clan.stock = crate::types::Stock::default();
        clan.stock.add(Resource::Iron, 5);
        on_return(&cat, &mut clan, &[(Resource::Iron, 5)], &mut rng(), &mut ev);
        assert_eq!(clan.story.contract.as_ref().unwrap().progress, 5);
        assert_eq!(clan.stock.get(Resource::Iron), 0, "held back for the guild");
        clan.stock.add(Resource::Iron, 6);
        on_return(&cat, &mut clan, &[(Resource::Iron, 6)], &mut rng(), &mut ev);
        assert!(clan.story.contract.is_none());
        assert_eq!(
            clan.stock.get(Resource::Iron),
            3,
            "only what was asked for was taken"
        );
        // Goods of another kind are not counted.
        let (cat, mut clan, day) = offered("rk-iron-pans", 20);
        take(&cat, &mut clan, SEED, day, "rk-iron-pans", &mut ev).unwrap();
        on_return(
            &cat,
            &mut clan,
            &[(Resource::Salt, 50)],
            &mut rng(),
            &mut ev,
        );
        assert_eq!(clan.story.contract.as_ref().unwrap().progress, 0);
    }

    #[test]
    fn an_escort_needs_company_and_the_right_kind_of_place() {
        let (cat, mut clan, day) = offered("ct-toll-escort", 20);
        let mut ev = Vec::new();
        take(&cat, &mut clan, SEED, day, "ct-toll-escort", &mut ev).unwrap();
        on_place(&cat, &mut clan, PoiKind::Trader, 1, &mut rng(), &mut ev);
        assert!(
            clan.story.contract.is_some(),
            "a party of one escorts nobody"
        );
        on_place(&cat, &mut clan, PoiKind::Camp, 3, &mut rng(), &mut ev);
        assert!(clan.story.contract.is_some(), "the wrong kind of place");
        on_place(&cat, &mut clan, PoiKind::Trader, 2, &mut rng(), &mut ev);
        assert!(clan.story.contract.is_none() && clan.story.done == 1);
    }

    #[test]
    fn a_contract_that_runs_out_or_is_given_back_costs_standing() {
        let (cat, mut clan, day) = offered("rk-salt-sacks", 20);
        let mut ev = Vec::new();
        take(&cat, &mut clan, SEED, day, "rk-salt-sacks", &mut ev).unwrap();
        let def = cat
            .contracts
            .iter()
            .find(|c| c.id == "rk-salt-sacks")
            .unwrap();
        let last = day + i64::from(def.days) - 1;
        let rep = clan.story.rep[Faction::Rakers.index()];
        expire(&cat, &mut clan, last, &mut ev);
        assert!(clan.story.contract.is_some(), "the last day is still a day");
        expire(&cat, &mut clan, last + 1, &mut ev);
        assert!(clan.story.contract.is_none() && clan.story.failed == 1);
        assert_eq!(
            clan.story.rep[Faction::Rakers.index()],
            rep - i16::from(def.fail)
        );
        take(&cat, &mut clan, SEED, day, "rk-salt-sacks", &mut ev).unwrap();
        let rep = clan.story.rep[Faction::Rakers.index()];
        give_back(&cat, &mut clan, &mut ev).unwrap();
        let dropped = rep - clan.story.rep[Faction::Rakers.index()];
        assert!(
            dropped > 0 && dropped < i16::from(def.fail),
            "giving back is gentler: {dropped}"
        );
        assert_eq!(
            give_back(&cat, &mut clan, &mut ev),
            Err(ContractError::None)
        );
        assert_eq!(
            take(&cat, &mut clan, SEED, day, "no-such-contract", &mut ev),
            Err(ContractError::NotOffered)
        );
    }
}
