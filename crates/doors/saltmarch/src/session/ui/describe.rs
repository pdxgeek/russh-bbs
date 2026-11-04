//! The text "look around" for every screen (`docs/doors/README.md`): what the picture shows,
//! in sentences, for a screen reader or a very small window. Every map and board screen has
//! one, and `tests/screens.rs` checks that none is empty.
//!
//! These lines are appended to the plain menu lines (`Session::lines`), which carry the keys
//! and tags the simulation bots read, so nothing here may start a line with, or contain, the
//! words those bots look for.

use super::plan;
use super::{MenuId, Mode, Session};
use crate::clan::Clan;
use crate::lore::{Daypart, Water};
use crate::map::{PoiKind, SIZE};
use crate::tide::Access;
use crate::trip::{self, Phase};
use crate::types::Resource;

/// A word for the direction from `(0, 0)` to `(dx, dy)` (y grows to the south).
fn compass(dx: i32, dy: i32) -> &'static str {
    let (ax, ay) = (dx.abs(), dy.abs());
    let ns = if dy < 0 { "north" } else { "south" };
    let ew = if dx < 0 { "west" } else { "east" };
    if dx == 0 && dy == 0 {
        "here"
    } else if ay * 2 <= ax {
        ew
    } else if ax * 2 <= ay {
        ns
    } else if dy < 0 && dx < 0 {
        "north-west"
    } else if dy < 0 {
        "north-east"
    } else if dx < 0 {
        "south-west"
    } else {
        "south-east"
    }
}

fn access_word(a: Access) -> &'static str {
    match a {
        Access::Dry => "dry",
        Access::Wet => "knee-deep",
        Access::Drowned => "under water",
    }
}

/// What the tile at `(x, y)` is, as a phrase.
fn tile_phrase(s: &Session, clan: &Clan, level: i32, x: i32, y: i32) -> String {
    let (Ok(ux), Ok(uy)) = (usize::try_from(x), usize::try_from(y)) else {
        return "the edge of the world".to_owned();
    };
    let Some(tile) = s.map.tile(ux, uy) else {
        return "the edge of the world".to_owned();
    };
    if !clan.is_charted(ux, uy) {
        return "uncharted ground".to_owned();
    }
    format!(
        "{} ({})",
        tile.kind.name(),
        access_word(Access::of(i32::from(tile.elevation), level))
    )
}

/// The map around `(cx, cy)`: the eight neighbours and the places within a few tiles.
fn around(s: &Session, clan: &Clan, level: i32, (cx, cy): (i32, i32), radius: i32) -> Vec<String> {
    let mut out = Vec::new();
    let names = [
        ("north", 0, -1),
        ("north-east", 1, -1),
        ("east", 1, 0),
        ("south-east", 1, 1),
        ("south", 0, 1),
        ("south-west", -1, 1),
        ("west", -1, 0),
        ("north-west", -1, -1),
    ];
    let near: Vec<String> = names
        .iter()
        .map(|(n, dx, dy)| format!("{n}: {}", tile_phrase(s, clan, level, cx + dx, cy + dy)))
        .collect();
    out.push(format!("Next to this tile - {}.", near.join("; ")));
    let mut places: Vec<(i32, String)> = Vec::new();
    for poi in &s.map.pois {
        let (px, py) = (i32::from(poi.x), i32::from(poi.y));
        let (dx, dy) = (px - cx, py - cy);
        let d = dx.abs().max(dy.abs());
        if d > radius || d == 0 || !clan.is_charted(usize::from(poi.x), usize::from(poi.y)) {
            continue;
        }
        let name = s
            .cat
            .pois
            .get(usize::from(poi.template))
            .map_or("a place", |t| t.name.as_str());
        let kind = match poi.kind {
            PoiKind::Camp => "camp",
            PoiKind::Shrine => "shrine",
            PoiKind::Salvage => "salvage site",
            PoiKind::Lair => "lair",
            PoiKind::Hermit => "hermit",
            PoiKind::Trader => "trader",
            PoiKind::Ruin => "ruin",
        };
        places.push((
            d,
            format!("{name}, a {kind}, {d} tiles {}", compass(dx, dy)),
        ));
    }
    places.sort();
    if places.is_empty() {
        out.push(format!("No charted place within {radius} tiles."));
    } else {
        out.push(format!(
            "Places within {radius} tiles - {}.",
            places
                .into_iter()
                .take(6)
                .map(|(_, p)| p)
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if let Some(&(hx, hy)) = s.map.sites.get(usize::from(clan.site)) {
        let (dx, dy) = (i32::from(hx) - cx, i32::from(hy) - cy);
        out.push(format!(
            "Your hold is {} tiles {}.",
            dx.abs().max(dy.abs()),
            compass(dx, dy)
        ));
    }
    let (tx, ty) = s.map.tidecourt;
    let (dx, dy) = (i32::from(tx) - cx, i32::from(ty) - cy);
    out.push(format!(
        "The Tidecourt is {} tiles {}.",
        dx.abs().max(dy.abs()),
        compass(dx, dy)
    ));
    out
}

/// The tide in words.
fn tide_words(s: &Session, minute: i64, level: i32) -> String {
    let (wait, high) = s.tide.next_turn(minute);
    let band = s.tide.warning(level, s.today);
    format!(
        "Tide: water stands at {level} of 100 and is {}, the next {} in {}h{:02}m{}{}.",
        if high { "rising" } else { "falling" },
        if high { "high" } else { "low" },
        wait / 60,
        wait % 60,
        match band {
            crate::tide::Warning::Calm => "",
            crate::tide::Warning::High => ", HIGH WATER warning",
            crate::tide::Warning::Flood => ", FLOOD warning",
            crate::tide::Warning::Storm => ", STORM SURGE warning",
        },
        if s.tide.storm(s.today) {
            "; a storm is on today's tides"
        } else {
            ""
        }
    )
}

/// The look-around text for the screen the session is on.
#[allow(clippy::too_many_lines)]
pub(in crate::session) fn describe(s: &Session) -> Vec<String> {
    let mut out = vec!["Looking around:".to_owned()];
    match &s.mode {
        Mode::Unreadable => {
            out.push("A notice: the saved clan or the season's world cannot be read.".to_owned());
            out.push("Nothing was changed. Escape leaves the door.".to_owned());
            return out;
        }
        Mode::Naming(_) => {
            out.push("The title screen: Saltmarch, over a picture of a hold on a rise of high ground in a flooded marsh at dusk.".to_owned());
            out.push(
                "A box asks for the name of your clan. Type it and press Enter to stake the hold."
                    .to_owned(),
            );
            return out;
        }
        _ => {}
    }
    let Some(clan) = &s.clan else {
        return out;
    };
    let env = s.env();
    let minute = env.minute(clan);
    let level = s.tide.level(minute);
    out.push(tide_words(s, minute, level));
    let day = Daypart::of(minute);
    match &s.mode {
        Mode::Report => {
            out.push(format!(
                "The morning report, in the {day}. It covers the nights that passed while you were away."
            ));
            let mut made: Vec<(Resource, u32)> = Vec::new();
            for e in &s.ledger {
                if let crate::event::Event::NightProduced { what, qty, .. } = e {
                    match made.iter_mut().find(|(r, _)| r == what) {
                        Some((_, n)) => *n += qty,
                        None => made.push((*what, *qty)),
                    }
                }
            }
            if made.is_empty() {
                out.push("Nothing was made overnight.".to_owned());
            } else {
                out.push(format!(
                    "Made overnight: {}.",
                    made.iter()
                        .map(|(r, n)| format!("{n} {}", r.name()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            for e in &s.ledger {
                use crate::event::Event as E;
                if matches!(
                    e,
                    E::RaidWon { .. }
                        | E::RaidFailed { .. }
                        | E::RaidRecalled { .. }
                        | E::RaidSuffered { .. }
                        | E::RaidRepelled { .. }
                        | E::PactSigned { .. }
                        | E::PactBroken { .. }
                        | E::PactOffered { .. }
                        | E::PactDeclined { .. }
                        | E::Parcel { .. }
                        | E::Refused { .. }
                        | E::Unsent { .. }
                ) {
                    out.push(e.to_string());
                }
            }
            if !s.ledger.iter().any(|e| {
                use crate::event::Event as E;
                matches!(
                    e,
                    E::RaidWon { .. }
                        | E::RaidFailed { .. }
                        | E::RaidRecalled { .. }
                        | E::RaidSuffered { .. }
                        | E::RaidRepelled { .. }
                )
            }) {
                out.push("Raids: none.".to_owned());
            }
            out.push(match clan.social.unread() {
                0 => "Letters: none waiting.".to_owned(),
                n => format!("Letters: {n} waiting at the post."),
            });
            let table = s.tide.table(s.today);
            out.push(format!(
                "Today's tide table, {} tides: {}.",
                table.phase,
                table
                    .extremes
                    .iter()
                    .map(|e| format!(
                        "{} at {:02}:{:02} (water {})",
                        if e.high { "high" } else { "low" },
                        e.minute / 60,
                        e.minute % 60,
                        e.level
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            let rank = standing_line(s, clan);
            out.push(rank);
        }
        Mode::Menu(menu @ (MenuId::Main | MenuId::Hold)) => {
            let cat = &*s.cat;
            out.push(if *menu == MenuId::Main {
                "The home screen: the hold at a glance, with the main orders beside it.".to_owned()
            } else {
                "The hold screen: the plot grid, with the build and work orders beside it."
                    .to_owned()
            });
            let open = clan.hold.unlocked(cat);
            out.push(format!(
                "The hold is a grid of {} plots in {} columns and {} rows; {open} are open.",
                crate::hold::PLOTS,
                crate::hold::COLS,
                crate::hold::ROWS
            ));
            let mut built: Vec<String> = clan
                .hold
                .built(cat)
                .map(|(p, b)| format!("plot {p}: {}", b.name))
                .collect();
            for c in clan.hold.building.iter().flatten() {
                built.push(format!(
                    "plot {}: {} under construction, {} turns of labour left",
                    c.plot, c.building, c.remaining
                ));
            }
            out.push(if built.is_empty() {
                "No plot is built on yet.".to_owned()
            } else {
                format!("{}.", built.join("; "))
            });
            let mouths = u32::try_from(clan.crew.len()).unwrap_or(1).max(1);
            let food =
                (clan.stock.get(Resource::Food) + clan.stock.get(Resource::Fish) / 2) / mouths;
            let peat = clan.stock.get(Resource::Peat) / clan.hold.upkeep_peat(cat).max(1);
            out.push(format!(
                "Upkeep: food for {food} nights, peat for {peat} nights, morale {} of 100, crew {} of {}.",
                clan.morale,
                clan.crew.len(),
                clan.hold.crew_cap(cat)
            ));
            out.push(match clan.hold.sluice_level {
                Some(l) => format!("The sluice floods the approach when the water reaches {l}."),
                None => "The sluice, if built, stands open.".to_owned(),
            });
        }
        Mode::Menu(MenuId::Crew) => {
            out.push(format!(
                "The crew roster: {} of {} places filled.",
                clan.crew.len(),
                clan.hold.crew_cap(&s.cat)
            ));
            if let Some(c) = clan.crew.get(s.crew_sel) {
                let traits: Vec<String> = c
                    .traits
                    .iter()
                    .filter_map(|t| s.cat.trait_def(t))
                    .map(|t| format!("{}{}", t.name, if t.scar { " (a scar)" } else { "" }))
                    .collect();
                let hurt: Vec<String> = c
                    .injuries
                    .iter()
                    .filter_map(|i| {
                        s.cat
                            .injury(&i.id)
                            .map(|d| format!("{}, {} days to heal", d.name, i.days))
                    })
                    .collect();
                out.push(format!(
                    "Highlighted: {}, a {} aged {}. Grit {}, wit {}, reach {}. Traits: {}. Injuries: {}.",
                    c.name,
                    c.trade.name(),
                    c.age,
                    c.attrs[0],
                    c.attrs[1],
                    c.attrs[2],
                    if traits.is_empty() { "none".to_owned() } else { traits.join(", ") },
                    if hurt.is_empty() { "none".to_owned() } else { hurt.join("; ") }
                ));
                let skills: Vec<String> = crate::types::Trade::ALL
                    .iter()
                    .filter(|t| c.level(**t) > 0)
                    .map(|t| format!("{} {}", t.name(), c.level(*t)))
                    .collect();
                let partners: Vec<String> = clan
                    .partners(c.id)
                    .iter()
                    .filter_map(|id| clan.crew_by(*id))
                    .map(|p| p.name.clone())
                    .collect();
                out.push(format!(
                    "Skills: {}. Bonds: {}.",
                    if skills.is_empty() {
                        "none yet".to_owned()
                    } else {
                        skills.join(", ")
                    },
                    if partners.is_empty() {
                        "none".to_owned()
                    } else {
                        partners.join(", ")
                    }
                ));
            }
            out.push(format!(
                "{} candidate(s) wait at the gate.",
                clan.candidates.len()
            ));
        }
        Mode::Menu(MenuId::Market) => {
            out.push("The Tidecourt market. Each line: goods, how many you hold, the price in tallies, and the change since yesterday.".to_owned());
            for r in Resource::ALL {
                let trail = s.world.price_trail(r);
                let now = s.world.prices[r.index()];
                let change = match trail[..] {
                    [.., before, now2] if before > 0 => {
                        (i32::from(now2) - i32::from(before)) * 100 / i32::from(before)
                    }
                    _ => 0,
                };
                out.push(format!(
                    "  {}: hold {}, price {}.{:02}, {change:+}% on the day.",
                    r.name(),
                    clan.stock.get(r),
                    now / 100,
                    now % 100
                ));
            }
        }
        Mode::Menu(MenuId::Tides) => {
            let table = s.tide.table(s.today);
            out.push(format!(
                "The quay: today's tide table, {} tides{}.",
                table.phase,
                if table.storm {
                    ", with a storm surge"
                } else {
                    ""
                }
            ));
            for e in &table.extremes {
                out.push(format!(
                    "  {} at {:02}:{:02}, water {}.",
                    if e.high { "High" } else { "Low" },
                    e.minute / 60,
                    e.minute % 60,
                    e.level
                ));
            }
            out.push("Below it, a chart of the fortnight's springs and neaps.".to_owned());
        }
        Mode::Menu(MenuId::Plan) => {
            let pv = plan::preview_for(s, clan, minute);
            out.push(format!(
                "The expedition planner. Party {} of {}; carrying {} of {} units.",
                s.plan.party.len(),
                trip::MAX_PARTY,
                pv.load,
                pv.capacity
            ));
            out.push(match (pv.trip.target, pv.out) {
                (None, _) => "No target is set.".to_owned(),
                (Some(t), None) => format!("Target {},{} cannot be reached today.", t.0, t.1),
                (Some(t), Some(c)) => format!(
                    "Target {},{} is {c} turns away; there and back is {} of the {} turns left today.",
                    t.0,
                    t.1,
                    c * 2,
                    pv.left
                ),
            });
            if let Some((level, access)) = pv.arrival {
                out.push(format!(
                    "On arrival the water would stand at {level} and the ground would be {}.",
                    access_word(access)
                ));
            }
        }
        Mode::Menu(MenuId::Trip) => match clan.trip.as_ref() {
            Some(t) => {
                out.push(format!(
                    "On the marsh at {},{}; it is {day}, water {}.",
                    t.at.0,
                    t.at.1,
                    Water::of(level)
                ));
                match &t.phase {
                    Phase::Free => {
                        out.push(format!(
                            "This tile is {}.",
                            tile_phrase(s, clan, level, i32::from(t.at.0), i32::from(t.at.1))
                        ));
                        out.extend(around(
                            s,
                            clan,
                            level,
                            (i32::from(t.at.0), i32::from(t.at.1)),
                            6,
                        ));
                    }
                    Phase::Scene(sc) => {
                        if let Some(def) = s.cat.event(&sc.event) {
                            out.push(format!("A scene: {}. {}", def.title, def.text));
                            for (n, &i) in sc.offered.iter().enumerate() {
                                if let Some(info) = s.option_info(clan, t, sc, i) {
                                    out.push(format!(
                                        "  Choice {}: {}{}{}.",
                                        n + 1,
                                        info.label,
                                        info.risk.map_or(String::new(), |r| format!(
                                            ", {r}% chance to go badly"
                                        )),
                                        info.fight
                                    ));
                                }
                            }
                        }
                    }
                    Phase::Fight(fight) => {
                        out.push(format!(
                            "A skirmish, round {}. Odds: {}. The order of acting: {}.",
                            fight.round,
                            fight.odds().word(),
                            fight
                                .order
                                .iter()
                                .filter_map(|&i| fight.units.get(usize::from(i)))
                                .filter(|u| u.alive())
                                .map(|u| u.name.clone())
                                .collect::<Vec<_>>()
                                .join(", then ")
                        ));
                        for (side, label) in [
                            (crate::combat::Side::Party, "Your line"),
                            (crate::combat::Side::Foe, "The foes"),
                        ] {
                            for (row, name) in ["front", "middle", "back"].iter().enumerate() {
                                let here: Vec<String> = fight
                                    .units
                                    .iter()
                                    .filter(|u| {
                                        u.side == side && !u.gone && usize::from(u.row) == row
                                    })
                                    .map(|u| {
                                        format!(
                                            "{} {} of {} health{}",
                                            u.name,
                                            u.hp.max(0),
                                            u.max_hp,
                                            if side == crate::combat::Side::Foe {
                                                format!(
                                                    ", {}",
                                                    super::skirmish::intent(u.behaviour)
                                                )
                                            } else {
                                                String::new()
                                            }
                                        )
                                    })
                                    .collect();
                                if !here.is_empty() {
                                    out.push(format!("{label}, {name} rank: {}.", here.join("; ")));
                                }
                            }
                        }
                        out.push(format!("Clan morale {} of 100.", clan.morale));
                    }
                }
            }
            None => out.push("No party is out.".to_owned()),
        },
        Mode::Menu(MenuId::Map) => {
            let home = s
                .map
                .sites
                .get(usize::from(clan.site))
                .copied()
                .unwrap_or((32, 32));
            out.push(format!(
                "The marsh chart, {SIZE} by {SIZE} tiles, {} charted. The arrow keys pan it.",
                clan.charted_count()
            ));
            out.extend(around(
                s,
                clan,
                level,
                (
                    (i32::from(home.0) + i32::from(s.pan.0)).clamp(0, 63),
                    (i32::from(home.1) + i32::from(s.pan.1)).clamp(0, 63),
                ),
                8,
            ));
            out.push("Water is shown as a colour band over the ground, or as a change of symbol when colour is off.".to_owned());
        }
        Mode::Menu(MenuId::Court) => {
            out.push("The Tidecourt, a town on stilts above the highest tide. From here: the market, the exchange where clans trade with each other, the gate for raids, pacts, the post, the league table, the Hall of Tides and the quay.".to_owned());
        }
        Mode::Menu(MenuId::Exchange) => {
            let what = s.good();
            let x = &s.world.social.exchange;
            let asks = x.asks.iter().filter(|o| o.what == what).count();
            let bids = x.bids.iter().filter(|o| o.what == what).count();
            out.push(format!(
                "The exchange, showing {}: {asks} ask(s) for sale and {bids} bid(s) wanted from other clans. Orders you send settle with the post, the next time you open the door.",
                what.name()
            ));
            if let Some(a) = x.best_ask(what, s.me()) {
                out.push(format!("Best ask: {} at {} each.", a.qty, a.price));
            }
            if let Some(b) = x.best_bid(what, s.me()) {
                out.push(format!("Best bid: {} at {} each.", b.qty, b.price));
            }
            out.push(format!(
                "The exchange keeps {}% of a sale and {}% of an order's value is the listing fee.",
                crate::social::caps::LEVY_PCT,
                crate::social::caps::LISTING_FEE_PCT
            ));
        }
        Mode::Menu(MenuId::Raid) => {
            let others = s.others();
            match others.get(s.soc.raid.target % others.len().max(1)) {
                None => out.push("The gate. No other clan is on the Tidecourt's rolls yet, so there is nobody to raid.".to_owned()),
                Some((ix, t)) => {
                    out.push(format!(
                        "The gate, planning a raid on {}, renown {}. Their guard posts {} fit hand(s) with a defence rating of {}{}.",
                        t.name,
                        t.score,
                        t.posted.guard.len(),
                        t.posted.rating,
                        t.posted.sluice.map_or(String::new(), |l| format!(" and a sluice that floods the approach at water {l}"))
                    ));
                    out.push(match s.raid_closed(*ix) {
                        Some(why) => format!("This raid is closed: {}", why.text()),
                        None => "The way is open.".to_owned(),
                    });
                    let party = s.raiders();
                    out.push(format!(
                        "Your raiders: {}. Away from the hold until the night's report.",
                        if party.is_empty() { "nobody is fit to go".to_owned() } else { party.iter().map(|d| d.name.clone()).collect::<Vec<_>>().join(", ") }
                    ));
                    for w in s.windows() {
                        out.push(format!(
                            "Turn of the water tomorrow at {:02}:{:02}: {} {}{}.",
                            w.minute / 60,
                            w.minute % 60,
                            if w.high { "high water" } else { "low water" },
                            w.level,
                            if w.needs_boat { ", needs a boat" } else { ", on foot" }
                        ));
                    }
                }
            }
        }
        Mode::Menu(MenuId::Pacts) => {
            let n = s.me().map_or(0, |m| s.world.pacts_of(m).count());
            out.push(format!(
                "Pacts and diplomacy. You hold {n} pact(s). Terms: a truce is always part of a pact; trade, mutual defence and shared causeways are opened or closed by what your hold has built and what your history says."
            ));
            if let Some(r) = s
                .me()
                .and_then(|m| s.world.clans.get(usize::from(m)))
                .map(|c| &c.record)
            {
                out.push(format!(
                    "Your standing with the Tidecourt is {}; {} betrayal(s) on record.",
                    r.rep, r.betrayals
                ));
            }
        }
        Mode::Menu(MenuId::Letters) => {
            let n = clan.social.inbox.len();
            out.push(format!(
                "The post: {n} letter(s), {} unread. Letters are templates with an optional line from the sender; the board and the clan they are for can read them.",
                clan.social.unread()
            ));
            if let Some(l) = clan.social.inbox.get(s.soc.letter) {
                out.push(format!(
                    "Reading the letter from {}{}.",
                    l.from,
                    if l.offer.is_some() {
                        ", which carries a pact offer"
                    } else {
                        ""
                    }
                ));
            }
        }
        Mode::Menu(MenuId::Compose) | Mode::Line(_) => {
            let others = s.others();
            out.push(match others.get(s.soc.compose.to % others.len().max(1)) {
                Some((_, to)) => format!("Writing a letter to {}. Choose the words, add a line of your own if you like, then send.", to.name),
                None => "Writing a letter, but no other clan is on the rolls yet.".to_owned(),
            });
            if matches!(s.mode, Mode::Line(_)) {
                out.push("Typing your own line: Enter sets it, Esc leaves it.".to_owned());
            }
        }
        Mode::Menu(MenuId::League) => {
            let rn = s.renown();
            out.push("The league table: clans ranked by renown, which rewards a big hold, a skilled crew, expeditions, pacts kept and raids won (raids count for little on their own), less any pact broken.".to_owned());
            out.push(standing_line(s, clan));
            out.push(format!(
                "Your renown: hold {}, crew {}, deeds {}, the Deep {}, pacts {}, raids {}, wealth {}, betrayals minus {}.",
                rn.hold, rn.crew, rn.deeds, rn.deep, rn.pacts, rn.raids, rn.wealth, rn.penalty
            ));
        }
        Mode::Menu(MenuId::Hall) => {
            out.push(format!(
                "The Hall of Tides: {} line(s) of crew who retired or were lost, and of seasons played.",
                clan.hall.len()
            ));
            for h in clan.hall.iter().rev().take(5) {
                out.push(format!("  Season {}: {}", h.season, h.text));
            }
            let now = crate::social::hall::SeasonRecord::of(&s.world, s.today);
            for h in &now.records {
                out.push(format!(
                    "This season, {}: {} ({}).",
                    h.record.title(),
                    h.clan,
                    h.value
                ));
            }
            if let Some(last) = s.world.social.book.last() {
                out.push(format!(
                    "Season {} in the book: {}",
                    last.season, last.ending
                ));
            }
        }
        _ => {}
    }
    out
}

/// "You stand 2nd of 5 clans with a renown of 41."
fn standing_line(s: &Session, clan: &Clan) -> String {
    let mine = s.renown().total();
    let mut rows: Vec<(u32, &str)> = s
        .world
        .clans
        .iter()
        .filter(|c| c.name != clan.name)
        .map(|c| (c.score, c.name.as_str()))
        .collect();
    rows.push((mine, clan.name.as_str()));
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    let rank = rows
        .iter()
        .position(|r| r.1 == clan.name)
        .map_or(1, |i| i + 1);
    format!(
        "Season standings: you are number {rank} of {} clans with a renown of {mine}.",
        rows.len()
    )
}
