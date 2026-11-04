//! The Saltmarch screens (M4-S3): every screen rendered at 80x24, 100x30 and 132x43 in each
//! icon tier, through the same compositing the board does; the look-around of every screen; and
//! one whole day played through the keys (report, expedition, a scene, a skirmish, home).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::print_stdout,
    clippy::needless_pass_by_value,
    clippy::format_push_string,
    clippy::vec_init_then_push
)]

use std::sync::Arc;

use russh_bbs_core::limits::{Day, ManualClock};
use russh_bbs_door::host::Driver;
use russh_bbs_door::{
    ColorDepth, Composite, DoorEnv, DoorKey, DoorPalette, DoorRng, DoorSurface, Save, Start, Tier,
};
use russh_bbs_door_saltmarch::Saltmarch;
use russh_bbs_door_saltmarch::clan::{Clan, HallEntry, Settings};
use russh_bbs_door_saltmarch::combat::{Fight, Progress, Unit};
use russh_bbs_door_saltmarch::content::Catalog;
use russh_bbs_door_saltmarch::crew::{Bond, Crew, Injured};
use russh_bbs_door_saltmarch::engine::{self, Action, Env};
use russh_bbs_door_saltmarch::map::Map;
use russh_bbs_door_saltmarch::social::ledger::Letter;
use russh_bbs_door_saltmarch::social::letters::LetterBody;
use russh_bbs_door_saltmarch::social::market::{self, Memo, Parcel, Side};
use russh_bbs_door_saltmarch::social::pact::{self};
use russh_bbs_door_saltmarch::social::raid::{Fate, Outcome, Report, Role};
use russh_bbs_door_saltmarch::social::{Body, Defender};
use russh_bbs_door_saltmarch::tide::Tide;
use russh_bbs_door_saltmarch::trip::{Kit, Phase};
use russh_bbs_door_saltmarch::types::{Resource, Trade};
use russh_bbs_door_saltmarch::world::World;

const DAY: i64 = 20_000;

/// The season's world after twelve nights of trading, so the market has a history to draw.
fn a_world(seed: u64) -> World {
    let mut w = World::new(1, seed, DAY - 12);
    let mut rng = DoorRng::from_seed(seed ^ 0xBEEF);
    for day in 0..12i32 {
        w.trade(Resource::Salt, if day % 3 == 0 { 40 } else { -15 });
        w.trade(Resource::Iron, -(day % 5) * 6);
        w.trade(Resource::Timber, 25 - day * 3);
        w.end_day(DAY - 12 + i64::from(day), &mut rng);
    }
    // The other clans on the rolls, with what they have posted, a pact, an offer to the
    // showcase clan, standing orders on the exchange and a season in the book.
    w.register_founded("ann", 0, DAY);
    let guard = |id, name: &str, trade, hp, atk, def| Defender {
        id,
        name: name.to_owned(),
        trade,
        hp,
        atk,
        def,
        spd: 4,
        mass: 6,
        row: 0,
        aquatic: false,
        armed: false,
    };
    for (name, site, founded, score, rating, flood, sluice, guards) in [
        ("Cove", 7u8, DAY - 30, 64u32, 5u8, 8u8, Some(60u8), 4usize),
        ("Dun", 3, DAY - 20, 51, 3, 0, None, 3),
        ("Eelmoor", 11, DAY - 2, 38, 1, 0, None, 2),
    ] {
        w.register_founded(name, site, founded);
        let c = w.clans.last_mut().unwrap();
        c.score = score;
        c.posted.seen = DAY;
        c.posted.buildings = 6;
        c.posted.crew = 6;
        c.posted.rating = rating;
        c.posted.flood = flood;
        c.posted.sluice = sluice;
        c.posted.tallies = 120;
        c.posted.stock[Resource::Salt.index()] = 25;
        c.posted.stock[Resource::Iron.index()] = 10;
        c.posted.stock[Resource::Food.index()] = 15;
        c.posted.guard = [
            guard(1, "Holt Marrow", Trade::Fighter, 22, 5, 2),
            guard(2, "Nan Sedge", Trade::Smith, 20, 4, 2),
            guard(3, "Orrin Pike", Trade::Wader, 15, 3, 0),
            guard(4, "Tamsin Reed", Trade::Healer, 12, 2, 0),
        ]
        .into_iter()
        .take(guards)
        .collect();
    }
    w.clans[0].posted.buildings = 9;
    w.clans[0].posted.crew = 7;
    w.clans[0].score = 57;
    // Cove and Dun keep a pact; Dun has offered one to the showcase clan.
    let signed = w.propose(1, 2, pact::TRUCE | pact::TRADE, DAY - 9).unwrap();
    w.accept(2, signed, DAY - 9).unwrap();
    w.propose_with(
        2,
        0,
        pact::TRUCE | pact::TRADE,
        DAY - 1,
        "offer-trade".into(),
        Some("fair prices, fair weather".into()),
    )
    .unwrap();
    w.social.mail.clear();
    w.social.next_seq = 0;
    for (who, what, qty, price, side) in [
        (1u8, Resource::Salt, 12u32, 5u16, Side::Ask),
        (2, Resource::Salt, 8, 6, Side::Ask),
        (1, Resource::Salt, 10, 4, Side::Bid),
        (2, Resource::Iron, 6, 11, Side::Ask),
    ] {
        market::post(&mut w, who, side, what, qty, price, DAY - 1).unwrap();
    }
    w.social.mail.clear();
    for day in 0..6 {
        w.social.exchange.today[Resource::Salt.index()] = (10, 10 * (4 + day % 3));
        w.social.exchange.end_day();
    }
    w.social.mail.clear();
    w.social
        .book
        .push(russh_bbs_door_saltmarch::social::hall::SeasonRecord::of(
            &w,
            DAY - 1,
        ));
    w.social.book[0].season = 0;
    w
}

/// The same world with the night's news for the showcase clan: a raid on its hold, a sale on
/// the exchange and a letter.
fn a_world_with_news(seed: u64) -> World {
    let mut w = a_world(seed);
    w.social.deliver(
        0,
        Body::Raid(Report {
            order: 3,
            role: Role::Defender,
            outcome: Outcome::Won,
            raider: "Cove".into(),
            defender: "ann".into(),
            minute: 410,
            level: 38,
            flooded: false,
            rounds: 6,
            loot: vec![(Resource::Salt, 4), (Resource::Iron, 1)],
            tallies: 20,
            fates: vec![(1, Fate::Hurt)],
            hurt: vec![1],
            taunt: "taunt-keeps".into(),
        }),
    );
    w.social.deliver(
        0,
        Body::Parcel(Parcel {
            goods: None,
            tallies: 49,
            memo: Memo::Sold,
            about: Some((Resource::Salt, 10)),
        }),
    );
    w.social.deliver(
        0,
        Body::Letter(LetterBody {
            from: "Dun".into(),
            template: "greeting-trade".into(),
            line: Some("see you at the weir".into()),
            day: DAY,
        }),
    );
    w
}

fn clock(day: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(day).start()))
}

/// A visit to the door with its own environment.
struct Visit {
    driver: Driver,
    env: DoorEnv,
    last_save: Option<Vec<u8>>,
}

impl Visit {
    fn open(door: &Saltmarch, env: DoorEnv, seed: u64, save: Option<Vec<u8>>, day: i64) -> Self {
        Self::open_in(door, env, seed, save, day, a_world)
    }

    fn open_in(
        door: &Saltmarch,
        env: DoorEnv,
        seed: u64,
        save: Option<Vec<u8>>,
        day: i64,
        world: fn(u64) -> World,
    ) -> Self {
        let world = save
            .as_deref()
            .and_then(Clan::decode)
            .map(|c| world(c.seed).encode());
        let driver = Driver::open(
            door,
            Start {
                save: match &save {
                    Some(bytes) => Save::Loaded { bytes, season: 1 },
                    None => Save::Fresh,
                },
                world: world.as_deref(),
                env: &env,
                today: Day::from_raw(day),
                season: 1,
            },
            seed,
            clock(day),
        );
        Self {
            driver,
            env,
            last_save: None,
        }
    }

    fn key(&mut self, key: DoorKey) {
        self.driver.key(key);
        if let Some(bytes) = self.driver.take_effects().save() {
            self.last_save = Some(bytes.to_vec());
        }
    }

    fn look(&self) -> Vec<String> {
        self.driver.look_around(&self.env)
    }

    /// Press the entry with this tag. `false` if the screen has no such entry.
    fn choose(&mut self, tag: &str) -> bool {
        let found = self.look().iter().find_map(|line| {
            let rest = line.strip_prefix('[')?;
            let (key, text) = rest.split_once("] ")?;
            text.starts_with(tag).then(|| key.chars().next())?
        });
        found.map(|c| self.key(DoorKey::Char(c))).is_some()
    }

    fn said(&self, needle: &str) -> bool {
        self.look().join("\n").contains(needle)
    }

    fn clan(&self) -> Clan {
        Clan::decode(self.last_save.as_deref().unwrap()).unwrap()
    }

    /// The screen as `symbol <tab> fg <tab> bg <tab> flags` per cell, one row per line, for the
    /// picture renderer in `docs/qa/m4-s3-screens/render.py`.
    fn dump(&self, cols: u16, rows: u16, palette: DoorPalette) -> String {
        use russh_bbs_door::style::{Color, Modifier};
        let mut surface = DoorSurface::new(cols, rows, palette);
        surface.draw(|canvas| self.driver.render(canvas, &self.env));
        // `SALTMARCH_DEPTH=16` shows what the board sends today: the sixteen named colours.
        let depth = if !self.env.settings.color() {
            ColorDepth::Mono
        } else if std::env::var("SALTMARCH_DEPTH").as_deref() == Ok("16") {
            ColorDepth::Ansi16
        } else {
            ColorDepth::TrueColor
        };
        let clean = surface.composite(Composite::new(depth, self.env.tier == Tier::Ascii));
        let hex = |c: Color| match c {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            Color::Reset => String::new(),
            other => format!("{other:?}"),
        };
        let mut out = String::new();
        for y in 0..rows {
            for x in 0..cols {
                let cell = &clean.buffer()[(x, y)];
                let m = cell.modifier;
                let flags = format!(
                    "{}{}{}{}",
                    if m.contains(Modifier::BOLD) { "b" } else { "" },
                    if m.contains(Modifier::DIM) { "d" } else { "" },
                    if m.contains(Modifier::REVERSED) {
                        "r"
                    } else {
                        ""
                    },
                    if m.contains(Modifier::UNDERLINED) {
                        "u"
                    } else {
                        ""
                    },
                );
                out.push_str(&format!(
                    "{}\t{}\t{}\t{flags}\n",
                    cell.symbol(),
                    hex(cell.fg),
                    hex(cell.bg)
                ));
            }
            out.push('|');
            out.push('\n');
        }
        out
    }

    fn render(&self, cols: u16, rows: u16, depth: ColorDepth) -> Vec<String> {
        let mut surface = DoorSurface::new(cols, rows, DoorPalette::mono());
        surface.draw(|canvas| self.driver.render(canvas, &self.env));
        surface
            .composite(Composite::new(depth, self.env.tier == Tier::Ascii))
            .rows_text()
    }
}

fn env_for(tier: Tier, color: bool) -> DoorEnv {
    let mut env = DoorEnv::for_tests("ann");
    env.tier = tier;
    env.settings = russh_bbs_core::terminal_settings::TerminalSettings::new(
        None,
        None,
        color,
        true,
        Some(tier),
    )
    .unwrap();
    env
}

/// A clan founded on `DAY`, as the bytes of its save.
fn founded(door: &Saltmarch) -> Vec<u8> {
    let mut v = Visit::open(door, env_for(Tier::Ascii, true), 5, None, DAY);
    v.key(DoorKey::Enter);
    v.last_save.unwrap()
}

/// A clan with some history: more crew (one hurt, a bonded pair, a scar), a built-up hold, a
/// charted marsh, stores and a hall, so the screens show what they are for.
fn showcase(door: &Saltmarch) -> Vec<u8> {
    let mut clan = Clan::decode(&founded(door)).unwrap();
    let cat = Catalog::embedded().unwrap();
    let mut rng = DoorRng::from_seed(77);
    for trade in [
        Trade::Fighter,
        Trade::Healer,
        Trade::Salvager,
        Trade::Boatwright,
    ] {
        let mut c = Crew::generate(&cat, &mut rng, 5, Some(trade));
        c.id = clan.take_id();
        clan.crew.push(c);
    }
    for (i, c) in clan.crew.iter_mut().enumerate() {
        for t in Trade::ALL {
            c.xp[t.index()] = u16::try_from((i * 37 + t.index() * 23) % 160).unwrap();
        }
    }
    clan.crew[1].injuries.push(Injured {
        id: "broken-leg".to_owned(),
        days: 4,
    });
    clan.crew[2].traits.push("limp".to_owned());
    clan.crew[3].injuries.push(Injured {
        id: "chill".to_owned(),
        days: 1,
    });
    let (a, b) = (clan.crew[0].id, clan.crew[4].id);
    clan.bonds.push(Bond { a, b, points: 9 });
    for (plot, id) in [
        (0, "fish-weir"),
        (1, "peat-stack"),
        (2, "hut"),
        (3, "hut"),
        (4, "watchfire"),
        (5, "salt-pan"),
        (6, "longhouse"),
        (7, "palisade"),
        (8, "smokehouse"),
    ] {
        clan.hold.plots[plot] = Some(id.to_owned());
    }
    clan.hold.sluice_level = Some(60);
    for (r, n) in [
        (Resource::Salt, 18),
        (Resource::Peat, 22),
        (Resource::Reed, 14),
        (Resource::Timber, 20),
        (Resource::Iron, 4),
        (Resource::Rope, 6),
        (Resource::Food, 24),
        (Resource::Fish, 9),
        (Resource::Tools, 3),
        (Resource::Arms, 5),
        (Resource::Boats, 1),
    ] {
        clan.stock.add(r, n);
    }
    clan.tallies = 312;
    clan.morale = 72;
    let map = Map::generate(clan.seed, &cat.pois);
    if let Some(&(x, y)) = map.sites.get(usize::from(clan.site)) {
        clan.chart_around(usize::from(x), usize::from(y), 17);
    }
    clan.hall.push(HallEntry {
        season: 1,
        text: "Wick Saltmarsh, raker, was kept by the Deep.".to_owned(),
    });
    clan.hall.push(HallEntry {
        season: 1,
        text: "Mudge Tarrow retired to the hearth, with a good boat and a bad knee.".to_owned(),
    });
    clan.stats.trips = 6;
    clan.stats.returned = 4;
    clan.stats.fights = 7;
    clan.stats.fights_won = 5;
    clan.stats.shifts = 31;
    clan.stats.earned = 640;
    clan.social.raids_won = 2;
    clan.social.repelled = 1;
    clan.social.grudges.push("Cove".to_owned());
    for (from, template, line, offer, read) in [
        (
            "Dun",
            "offer-trade",
            Some("fair prices, fair weather"),
            Some((3u32, 3u8)),
            false,
        ),
        ("Cove", "taunt-keeps", None, None, false),
        (
            "Eelmoor",
            "greeting-reeds",
            Some("the weir is full this week"),
            None,
            true,
        ),
    ] {
        clan.social.receive(Letter {
            seq: 0,
            from: from.to_owned(),
            template: template.to_owned(),
            line: line.map(str::to_owned),
            day: DAY - 1,
            offer,
            read,
        });
    }
    clan.refresh_candidates(&cat, &mut rng);
    clan.encode()
}

fn ids_in(clan: &Clan) -> Vec<u16> {
    clan.trip
        .as_ref()
        .map(|t| t.party.clone())
        .unwrap_or_default()
}

fn engine_env<'a>(
    cat: &'a Catalog,
    map: &'a Map,
    tide: &'a Tide,
    prices: &'a [u16; 13],
) -> Env<'a> {
    Env {
        cat,
        map,
        tide,
        prices,
        settings: Settings::default(),
        day: DAY,
    }
}

/// A clan on the marsh with a skirmish in front of it.
fn in_a_fight(door: &Saltmarch) -> Vec<u8> {
    let mut clan = Clan::decode(&showcase(door)).unwrap();
    let cat = Catalog::embedded().unwrap();
    let map = Map::generate(clan.seed, &cat.pois);
    let tide = Tide::new(clan.seed);
    let prices = World::new(1, clan.seed, DAY).prices;
    let env = engine_env(&cat, &map, &tide, &prices);
    let ids: Vec<u16> = clan
        .crew
        .iter()
        .filter(|c| c.fit(&cat))
        .take(4)
        .map(|c| c.id)
        .collect();
    engine::apply(
        &env,
        &mut clan,
        Action::Depart {
            party: ids,
            kit: Kit {
                food: 4,
                ..Kit::default()
            },
            target: None,
        },
        &mut DoorRng::from_seed(1),
    )
    .unwrap();
    let units = clan
        .crew
        .iter()
        .filter(|c| ids_in(&clan).contains(&c.id))
        .map(|c| Unit::from_crew(&cat, c, false))
        .collect();
    let mut fight = Fight::begin(&cat, units, "hound-pack", false, 40, 0, 0);
    assert!(matches!(
        fight.advance(&mut DoorRng::from_seed(2), &mut Vec::new()),
        Progress::Turn(_)
    ));
    clan.trip.as_mut().unwrap().phase = Phase::Fight(Box::new(fight));
    clan.encode()
}

/// Walk until a trip waits on a scene; the save at that moment.
fn at_a_scene(door: &Saltmarch) -> Vec<u8> {
    let base = showcase(door);
    for seed in 1..=60u64 {
        let mut v = Visit::open(
            door,
            env_for(Tier::Ascii, true),
            seed,
            Some(base.clone()),
            DAY,
        );
        assert!(v.choose("go:expedition"));
        for tag in ["party:1", "party:3", "kit:standard", "depart"] {
            assert!(v.choose(tag));
        }
        for leg in 0..60 {
            if v.said("choose:0") {
                return v.last_save.unwrap();
            }
            let dirs = ["explore:n", "explore:e", "explore:w", "explore:s"];
            if !(v.choose(dirs[leg % 4]) || v.choose("gather") || v.choose("camp:3")) {
                break;
            }
        }
    }
    panic!("no scene in 60 walks");
}

/// Every screen as `(name, visit)` for an environment.
fn all_screens(door: &Saltmarch, env: &DoorEnv) -> Vec<(&'static str, Visit)> {
    let base = showcase(door);
    let open = |save: Option<Vec<u8>>, day| Visit::open(door, env.clone(), 9, save, day);
    let at = |tag: &str| {
        let mut v = open(Some(base.clone()), DAY);
        assert!(v.choose(tag), "{tag}");
        v
    };
    let mut out: Vec<(&'static str, Visit)> = Vec::new();
    out.push(("naming", open(None, DAY)));
    out.push(("report", open(Some(base.clone()), DAY + 2)));
    out.push((
        "report-news",
        Visit::open_in(
            door,
            env.clone(),
            9,
            Some(base.clone()),
            DAY + 2,
            a_world_with_news,
        ),
    ));
    out.push(("home", open(Some(base.clone()), DAY)));
    out.push(("hold", at("go:hold")));
    out.push(("crew", at("go:crew")));
    out.push(("market", at("go:market")));
    out.push(("tides", at("go:tides")));
    out.push(("map", at("go:map")));
    out.push(("court", at("go:court")));
    {
        let mut v = at("go:court");
        assert!(v.choose("go:league"));
        out.push(("league", v));
        let mut v = at("go:court");
        assert!(v.choose("go:hall"));
        out.push(("hall", v));
        let mut v = at("go:court");
        assert!(v.choose("go:letters"));
        out.push(("letters", v));
        let mut v = at("go:court");
        assert!(v.choose("go:exchange"));
        out.push(("exchange", v));
        let mut v = at("go:court");
        assert!(v.choose("go:raid"));
        out.push(("raid", v));
        let mut v = at("go:court");
        assert!(v.choose("go:pacts"));
        out.push(("pacts", v));
        let mut v = at("go:court");
        assert!(v.choose("go:letters"));
        assert!(v.choose("compose"));
        out.push(("compose", v));
        let mut v = at("go:court");
        assert!(v.choose("go:letters"));
        assert!(v.choose("compose"));
        assert!(v.choose("line"));
        for c in "see you at the weir".chars() {
            v.key(DoorKey::Char(c));
        }
        out.push(("compose-typing", v));
    }
    {
        let mut v = at("go:expedition");
        for tag in ["party:1", "party:3", "kit:standard"] {
            assert!(v.choose(tag), "{tag}");
        }
        out.push(("plan", v));
    }
    {
        let mut v = at("go:expedition");
        for tag in ["party:1", "party:3", "kit:standard", "depart"] {
            assert!(v.choose(tag), "{tag}");
        }
        out.push(("marsh", v));
    }
    out.push(("scene", open(Some(at_a_scene(door)), DAY)));
    out.push(("fight", open(Some(in_a_fight(door)), DAY)));
    out
}

/// Print every screen: `SALTMARCH_SHOW=1 cargo test -p russh-bbs-door-saltmarch --test screens
/// show -- --nocapture`.
#[test]
fn show() {
    if std::env::var("SALTMARCH_SHOW").is_err() {
        return;
    }
    let (cols, rows): (u16, u16) = std::env::var("SALTMARCH_SIZE")
        .ok()
        .and_then(|s| {
            let (c, r) = s.split_once('x')?;
            Some((c.parse().ok()?, r.parse().ok()?))
        })
        .unwrap_or((80, 24));
    let tier = match std::env::var("SALTMARCH_TIER").as_deref() {
        Ok("ascii") => Tier::Ascii,
        Ok("nerd") => Tier::NerdFont,
        _ => Tier::Unicode,
    };
    let color = std::env::var("SALTMARCH_COLOR").as_deref() != Ok("0");
    let door = Saltmarch::new().unwrap();
    let env = env_for(tier, color);
    let only = std::env::var("SALTMARCH_ONLY").ok();
    let dump_dir = std::env::var("SALTMARCH_DUMP").ok();
    for (name, v) in all_screens(&door, &env) {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        if let Some(dir) = &dump_dir {
            use russh_bbs_door::style::{Color, Modifier, Style};
            // A theme like the board's default: the roles the screens ask for.
            let palette = DoorPalette::new([
                Style::new(),
                Style::new().fg(Color::Rgb(128, 138, 144)),
                Style::new()
                    .fg(Color::Rgb(238, 240, 236))
                    .add_modifier(Modifier::BOLD),
                Style::new().fg(Color::Rgb(116, 200, 214)),
                Style::new().fg(Color::Rgb(124, 200, 120)),
                Style::new().fg(Color::Rgb(226, 100, 84)),
                Style::new()
                    .fg(Color::Rgb(16, 22, 28))
                    .bg(Color::Rgb(116, 200, 214)),
            ]);
            std::fs::create_dir_all(dir).unwrap();
            let file = format!(
                "{dir}/{name}-{cols}x{rows}-{}{}.cells",
                match tier {
                    Tier::Ascii => "ascii",
                    Tier::Unicode => "unicode",
                    Tier::NerdFont => "nerd",
                },
                if color { "" } else { "-mono" }
            );
            std::fs::write(file, v.dump(cols, rows, palette)).unwrap();
            continue;
        }
        println!("===== {name} {cols}x{rows} {tier:?} color={color}");
        for line in v.render(cols, rows, ColorDepth::Mono) {
            println!("{line}");
        }
    }
}

// ------------------------------------------------------------------------------------------
// The screens at every size, in every tier.

const SIZES: [(u16, u16); 3] = [(80, 24), (100, 30), (132, 43)];
const TIERS: [Tier; 3] = [Tier::Ascii, Tier::Unicode, Tier::NerdFont];

fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Ascii => "ascii",
        Tier::Unicode => "unicode",
        Tier::NerdFont => "nerd",
    }
}

/// Every screen at one size as text, in the order of `all_screens`.
fn rendered(
    door: &Saltmarch,
    env: &DoorEnv,
    (cols, rows): (u16, u16),
) -> Vec<(&'static str, Vec<String>)> {
    all_screens(door, env)
        .into_iter()
        .map(|(name, v)| (name, v.render(cols, rows, ColorDepth::Mono)))
        .collect()
}

#[test]
fn every_screen_fits_and_is_legible_at_every_size_in_every_tier_with_and_without_colour() {
    let door = Saltmarch::new().unwrap();
    for tier in TIERS {
        for color in [true, false] {
            let env = env_for(tier, color);
            for (cols, rows) in SIZES {
                for (name, lines) in rendered(&door, &env, (cols, rows)) {
                    let text = lines.join("\n");
                    let ctx = format!("{name} {cols}x{rows} {tier:?} colour={color}\n{text}");
                    assert_eq!(lines.len(), usize::from(rows), "{ctx}");
                    assert!(
                        lines.iter().all(|l| l.chars().count() <= usize::from(cols)),
                        "{ctx}"
                    );
                    assert!(text.chars().any(char::is_alphabetic), "{ctx}");
                    assert!(!text.contains('\u{FFFD}'), "a neutralized glyph\n{ctx}");
                    if tier == Tier::Ascii {
                        assert!(text.is_ascii(), "the ASCII tier is ASCII only\n{ctx}");
                    }
                    // Every screen but the title and the notice has the clan and the tide on
                    // top, in words, in every tier.
                    if name != "naming" {
                        assert!(
                            lines[0].contains("ann") && lines[0].contains("tallies"),
                            "{ctx}"
                        );
                        assert!(lines[1].contains("TIDE"), "{ctx}");
                    }
                    // Used rows, not a screen that is mostly blank.
                    let used = lines.iter().filter(|l| !l.trim().is_empty()).count();
                    assert!(
                        name == "naming" || used * 2 >= usize::from(rows),
                        "{used} used rows\n{ctx}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_nerd_font_tier_draws_what_the_unicode_tier_draws() {
    // The board neutralizes private-use glyphs in a door's cells (D104), so a door cannot use
    // the Nerd Font glyphs; the two tiers must therefore be the same picture, and the same
    // picture must contain no private-use glyph.
    let door = Saltmarch::new().unwrap();
    for size in SIZES {
        let unicode = rendered(&door, &env_for(Tier::Unicode, true), size);
        let nerd = rendered(&door, &env_for(Tier::NerdFont, true), size);
        assert_eq!(unicode, nerd, "{size:?}");
        for (name, lines) in &unicode {
            assert!(
                lines
                    .iter()
                    .flat_map(|l| l.chars())
                    .all(|c| !('\u{E000}'..='\u{F8FF}').contains(&c)),
                "{name}"
            );
        }
    }
}

/// Snapshots: one file per size and tier, every screen in it. `SALTMARCH_BLESS=1` rewrites them.
#[test]
fn render_snapshots_of_every_screen_at_every_size_in_the_ascii_and_unicode_tiers() {
    let door = Saltmarch::new().unwrap();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let bless = std::env::var("SALTMARCH_BLESS").is_ok();
    for tier in [Tier::Ascii, Tier::Unicode] {
        for (cols, rows) in SIZES {
            let mut text = String::new();
            for (name, lines) in rendered(&door, &env_for(tier, true), (cols, rows)) {
                text.push_str(&format!("===== {name} {cols}x{rows} {}\n", tier_name(tier)));
                for line in lines {
                    text.push_str(line.trim_end());
                    text.push('\n');
                }
            }
            let file = dir.join(format!("{}-{cols}x{rows}.txt", tier_name(tier)));
            if bless {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(&file, &text).unwrap();
                continue;
            }
            let want = std::fs::read_to_string(&file)
                .unwrap_or_else(|_| panic!("no snapshot {file:?}; run with SALTMARCH_BLESS=1"));
            if want != text {
                let first = want
                    .lines()
                    .zip(text.lines())
                    .enumerate()
                    .find(|(_, (a, b))| a != b);
                panic!(
                    "{file:?} changed (SALTMARCH_BLESS=1 rewrites it); first difference: {first:?}"
                );
            }
        }
    }
}

// ------------------------------------------------------------------------------------------
// The look-around.

#[test]
fn every_screen_has_a_look_around_that_describes_it() {
    let door = Saltmarch::new().unwrap();
    let env = env_for(Tier::Ascii, true);
    let mut seen = std::collections::BTreeSet::new();
    for (name, v) in all_screens(&door, &env) {
        let lines = v.look();
        let at = lines
            .iter()
            .position(|l| l == "Looking around:")
            .unwrap_or_else(|| panic!("{name}: no look-around\n{lines:?}"));
        let described = &lines[at + 1..];
        assert!(
            described.len() >= 2 && described.iter().all(|l| !l.trim().is_empty()),
            "{name}: {described:?}"
        );
        // It says what is on the screen, in words.
        assert!(
            described.iter().map(String::len).sum::<usize>() >= 80,
            "{name}: {described:?}"
        );
        assert!(
            seen.insert(described.join("\n")),
            "{name} describes the same as another screen"
        );
    }
    assert_eq!(seen.len(), 22);
}

#[test]
fn the_map_screens_describe_the_ground_the_places_and_the_water() {
    let door = Saltmarch::new().unwrap();
    let env = env_for(Tier::Ascii, true);
    for (name, v) in all_screens(&door, &env) {
        if name != "map" && name != "marsh" {
            continue;
        }
        let text = v.look().join("\n");
        for needle in [
            "Next to this tile",
            "north:",
            "south-west:",
            "Your hold is",
            "The Tidecourt is",
            "Tide: water stands at",
        ] {
            assert!(text.contains(needle), "{name} lacks {needle:?}\n{text}");
        }
        assert!(text.contains("Places within"), "{name}\n{text}");
    }
}

#[test]
fn the_fight_and_scene_screens_say_what_is_at_stake_in_words() {
    let door = Saltmarch::new().unwrap();
    let env = env_for(Tier::Ascii, true);
    let screens = all_screens(&door, &env);
    let find = |n: &str| {
        screens
            .iter()
            .find(|(name, _)| *name == n)
            .unwrap()
            .1
            .look()
            .join("\n")
    };
    let fight = find("fight");
    for needle in [
        "A skirmish, round 1",
        "Odds:",
        "Your line, front rank",
        "The foes, front rank",
        "health",
        "hits the front",
    ] {
        assert!(fight.contains(needle), "{needle:?}\n{fight}");
    }
    let scene = find("scene");
    assert!(
        scene.contains("A scene:") && scene.contains("Choice 1:"),
        "{scene}"
    );
}

// ------------------------------------------------------------------------------------------
// The tide on the map.

#[test]
fn the_tide_changes_the_map_with_colour_as_a_colour_band_and_without_it_as_glyphs() {
    let door = Saltmarch::new().unwrap();
    let base = showcase(&door);
    let map_at = |env: &DoorEnv, day: i64| -> (Vec<String>, String) {
        let mut v = Visit::open(&door, env.clone(), 9, Some(base.clone()), day);
        if v.said("Any key to go on") {
            v.key(DoorKey::Char(' '));
        }
        assert!(v.choose("go:map"));
        (
            v.render(80, 24, ColorDepth::Mono),
            v.dump(80, 24, DoorPalette::mono()),
        )
    };
    // Without colour, in the ASCII tier: the water shows as `~` (under) and `-` (knee-deep).
    let plain = env_for(Tier::Ascii, false);
    let counts: Vec<usize> = (0..14)
        .map(|d| {
            map_at(&plain, DAY + d)
                .0
                .iter()
                .flat_map(|l| l.chars())
                .filter(|c| matches!(c, '~' | '-'))
                .count()
        })
        .collect();
    assert!(
        counts.iter().max() > counts.iter().min(),
        "the glyph count follows the water over a fortnight: {counts:?}"
    );
    // With colour, in the Unicode tier: the water is a colour on the map's cells, and the
    // legend names the band in words as well.
    let rich = env_for(Tier::Unicode, true);
    let (lines, dump) = map_at(&rich, DAY);
    assert!(lines.concat().contains('▀'), "half blocks");
    assert!(dump.contains('#'), "colours are in the cells");
    assert!(
        lines.concat().contains("knee-deep"),
        "the legend says it in words"
    );
}

// ------------------------------------------------------------------------------------------
// One whole day through the keys.

#[test]
fn one_whole_day_report_expedition_scene_skirmish_and_home() {
    let door = Saltmarch::new().unwrap();
    let env = env_for(Tier::Unicode, true);
    let base = showcase(&door);
    // The morning after a night away: the report.
    let mut v = Visit::open(&door, env.clone(), 31, Some(base.clone()), DAY + 1);
    let screen = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(
        screen.contains("While you were away") && screen.contains("Standings"),
        "{screen}"
    );
    assert!(v.said("Any key to go on"));
    v.key(DoorKey::Char(' '));
    let screen = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(
        screen.contains("The hold of ann") && screen.contains("Upkeep"),
        "{screen}"
    );
    let day_one = v.clan().stats.trips;

    // Plan: two crew, a standard kit, the nearest charted place.
    assert!(v.choose("go:expedition"));
    for tag in ["party:1", "party:3", "kit:standard"] {
        assert!(v.choose(tag), "{tag}");
    }
    let screen = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(
        screen.contains("Cost preview") && screen.contains("No target"),
        "{screen}"
    );
    assert!(v.choose("depart"));
    let screen = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(
        screen.contains("The marsh") && screen.contains("Underfoot"),
        "{screen}"
    );

    // On the marsh the arrow keys walk.
    let before = v.clan().trip.as_ref().unwrap().at;
    let mut moved = false;
    for key in [DoorKey::Right, DoorKey::Down, DoorKey::Left, DoorKey::Up] {
        v.key(key);
        moved |= v.clan().trip.as_ref().is_some_and(|t| t.at != before);
    }
    assert!(moved, "an arrow key is a step");

    // Walk on until a scene asks something.
    let mut met = false;
    for leg in 0..80 {
        if v.said("choose:0") {
            met = true;
            break;
        }
        let dirs = ["explore:n", "explore:e", "explore:w", "explore:s"];
        if !(v.choose(dirs[leg % 4]) || v.choose("gather") || v.choose("camp:3")) {
            break;
        }
    }
    assert!(met, "a scene on the way");
    let screen = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(screen.contains("What do you do?"), "{screen}");
    assert!(v.choose("choose:0"), "the scene can be answered");

    // The trip goes on to a skirmish (put one in front of the party, as a dropped connection
    // and a return the same day would find it) and the player fights it out.
    let mut clan = v.clan();
    let cat = Catalog::embedded().unwrap();
    if clan.trip.is_none() {
        // The scene's answer ended the trip early (swept or cut off): nothing more to fight.
        return;
    }
    let party = clan.trip.as_ref().unwrap().party.clone();
    let units = clan
        .crew
        .iter()
        .filter(|c| party.contains(&c.id))
        .map(|c| Unit::from_crew(&cat, c, false))
        .collect();
    let mut fight = Fight::begin(&cat, units, "hound-pack", false, 40, 0, 0);
    assert!(matches!(
        fight.advance(&mut DoorRng::from_seed(2), &mut Vec::new()),
        Progress::Turn(_)
    ));
    clan.trip.as_mut().unwrap().phase = Phase::Fight(Box::new(fight));
    let mut v = Visit::open(&door, env.clone(), 32, Some(clan.encode()), DAY + 1);
    let screen = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(
        screen.contains("SKIRMISH") && screen.contains("Bog-hound"),
        "{screen}"
    );
    for _ in 0..12 {
        if !v.said("Skirmish, round") {
            break;
        }
        assert!(v.choose("quick") || v.choose("brace"));
    }
    assert!(!v.said("Skirmish, round"), "the skirmish is over");

    // And home: march back, stand down, and the clan is at the hold again.
    for _ in 0..40 {
        if v.look().iter().any(|l| l.starts_with("Plots ")) || v.said("go:hold") {
            break;
        }
        if v.said("choose:0") && v.choose("choose:0") {
            continue;
        }
        if v.said("Skirmish, round") && v.choose("quick") {
            continue;
        }
        if !(v.choose("home") || v.choose("camp:3")) {
            break;
        }
    }
    let after = v.clan();
    assert!(after.trip.is_none(), "back at the hold");
    assert!(after.stats.trips > day_one, "the expedition is counted");
    let screen = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(screen.contains("The hold of ann"), "{screen}");
}

#[test]
fn the_smaller_area_the_board_leaves_inside_an_80_by_24_window_works_too() {
    // The board takes a frame and two heading rows off the window, so a door in an 80x24
    // window draws in about 74x18 (the real size is in docs/qa/m4-s3-walk.md); 60x16 is the
    // least the screens are laid out for.
    let door = Saltmarch::new().unwrap();
    for tier in TIERS {
        for color in [true, false] {
            let env = env_for(tier, color);
            for (cols, rows) in [(74, 18), (76, 19), (60, 16)] {
                for (name, lines) in rendered(&door, &env, (cols, rows)) {
                    let text = lines.join("\n");
                    let ctx = format!("{name} {cols}x{rows} {tier:?} colour={color}\n{text}");
                    assert_eq!(lines.len(), usize::from(rows), "{ctx}");
                    assert!(
                        lines.iter().all(|l| l.chars().count() <= usize::from(cols)),
                        "{ctx}"
                    );
                    assert!(!text.contains('\u{FFFD}'), "{ctx}");
                    if tier == Tier::Ascii {
                        assert!(text.is_ascii(), "{ctx}");
                    }
                    if name != "naming" {
                        assert!(text.contains("TIDE") || cols < 78, "{ctx}");
                    }
                    assert!(
                        text.chars().filter(|c| c.is_alphabetic()).count() > 60,
                        "{ctx}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_window_too_small_for_the_screens_gets_the_plain_text_screen() {
    let door = Saltmarch::new().unwrap();
    let env = env_for(Tier::Ascii, true);
    for (name, v) in all_screens(&door, &env) {
        if name == "naming" || name == "report" {
            continue;
        }
        let lines = v.render(40, 12, ColorDepth::Mono);
        assert_eq!(lines.len(), 12, "{name}");
        // The plain screen has the turns and the tide line as words.
        assert!(
            lines.join("\n").contains("Turns"),
            "{name}\n{}",
            lines.join("\n")
        );
    }
}
