//! Saves in known states, for the render tests and for pictures: a hero in town with coin and a
//! lived-in world, a room only a runesmith can fix, a fight with four enemies, the dead.
//!
//! This is not game logic and nothing in the door calls it. It builds a hero and a world through
//! the engine's own constructors and hands back the bytes a real save and a real world would be,
//! so a test (or a screenshot tool) opens the door on them with the public API and presses keys
//! like a player, and sees exactly what a player would. It is hidden from the documentation.

// Fixtures: a broken fixture should fail loudly, so these unwrap what a test would.
#![allow(clippy::expect_used, clippy::too_many_lines)]

use russh_bbs_door::DoorRng;

use crate::combat::FightAct;
use crate::content::{Content, Intent, ShortcutKind};
use crate::dungeon;
use crate::game::{Action, Game, Place};
use crate::hero::Hero;
use crate::town::TownAct;
use crate::world::{Change, Fallen, Pledge, World};

/// The calendar day the showcases are set on.
pub const DAY: i64 = 20_000;
/// The season.
pub const SEASON: u32 = 1;

/// One state to open the door on, and the keys to press to reach the screen.
pub struct Showcase {
    /// A short name, used for file names.
    pub name: &'static str,
    /// The hero's save (empty for a hero not made yet).
    pub save: Vec<u8>,
    /// The shared world.
    pub world: Vec<u8>,
    /// Keys, as replay tokens, pressed after opening.
    pub keys: &'static str,
}

fn content() -> Content {
    Content::embedded().expect("embedded content loads")
}

fn hero_game(c: &Content, calling: &str, seed: u64) -> Game {
    let traits = ["quick".to_owned(), "stout".to_owned()];
    let hero = Hero::create(c, "Ember", calling, &traits, [1, 1, 1, 1]).expect("hero");
    let mut game = Game::new(
        hero,
        DoorRng::from_seed(seed),
        DAY - 1,
        SEASON,
        World::new(SEASON),
    );
    let _ = game.new_day(c, DAY);
    game
}

fn finish(world: &mut World, c: &Content, project: &str) {
    let def = c.project(project).expect("project").clone();
    for req in &def.requires {
        finish(world, c, req);
    }
    world.apply(&Change::Pledge(Pledge {
        hero: "Town".to_owned(),
        project: project.to_owned(),
        coin: def.coin,
        salvage: def.salvage,
    }));
}

/// A world with some history: the hall, a slain guardian, a lift, a finished project, the dead
/// and the news.
fn lived_in(c: &Content) -> World {
    let mut w = World::new(SEASON);
    for (name, depth) in [
        ("Brindle", 14),
        ("Ash", 11),
        ("Wren", 9),
        ("Ember", 6),
        ("Tallow", 3),
    ] {
        w.apply(&Change::Depth(name.to_owned(), depth));
    }
    w.apply(&Change::Slain(1, "Brindle".to_owned()));
    w.apply(&Change::Shortcut(6, ShortcutKind::Lift, "Ash".to_owned()));
    w.apply(&Change::Fall(Fallen {
        name: "Tallow".to_owned(),
        depth: 7,
        room: 3,
        day: DAY - 1,
        rescued_by: None,
    }));
    w.apply(&Change::Fall(Fallen {
        name: "Mott".to_owned(),
        depth: 4,
        room: 2,
        day: DAY - 6,
        rescued_by: Some("Wren".to_owned()),
    }));
    w.apply(&Change::News(
        "Brindle felled the guardian of the Upper Works.".to_owned(),
    ));
    w.apply(&Change::News("Ash opened a lift at level 6.".to_owned()));
    finish(&mut w, c, &c.projects.project[0].id);
    w.apply(&Change::Pledge(Pledge {
        hero: "Wren".to_owned(),
        project: c.projects.project[1].id.clone(),
        coin: 40,
        salvage: 10,
    }));
    w
}

fn in_town(c: &Content, calling: &str, world: &World) -> Game {
    let mut g = hero_game(c, calling, 11);
    g.world = world.clone();
    g.hero.coin = 240;
    g.hero.stash = 45;
    g.hero.salvage = 0;
    g.hero.level = 4;
    g.hero.xp = 300;
    g.hero.skill_points = 2;
    g.hero.attr_points = 1;
    g.hero.best_depth = 6;
    g.hero.kills = 31;
    g.hero.turns = 25;
    g
}

/// Put the hero in a room built from template `id`, on a level `from` or deeper, with part of the
/// level explored.
fn place_in(c: &Content, g: &mut Game, template: &str, from: u8) {
    for depth in from..=25 {
        let level = dungeon::generate(c, g.season, g.epoch, depth);
        for (i, room) in level.rooms.iter().enumerate() {
            if c.rooms.room[room.template].id == template {
                let at = u8::try_from(i).unwrap_or(0);
                g.place = Place::Delve { depth, room: at };
                g.prev_room = at;
                while g.explored.len() < usize::from(depth) {
                    g.explored.push(0);
                }
                g.explored[usize::from(depth) - 1] |=
                    (level.all_mask() & 0b1_0111_0110_0111) | (1 << at);
                return;
            }
        }
    }
}

fn delving(c: &Content, calling: &str, template: &str, turns: u32) -> Game {
    let mut g = hero_game(c, calling, 9);
    place_in(c, &mut g, template, 1);
    g.hero.turns = turns;
    g
}

fn fighting(c: &Content, calling: &str, ids: &[&str], guardian: bool, round: u8) -> Game {
    let mut g = hero_game(c, calling, 5);
    place_in(c, &mut g, "bone-pit", 1);
    g.hero.turns = 17;
    let ids: Vec<String> = ids.iter().map(|s| (*s).to_owned()).collect();
    let mut ev = Vec::new();
    g.start_fight(c, &ids, guardian, &mut ev);
    if let Some(f) = g.fight.as_mut() {
        f.round = round;
        f.focus = 3;
        f.ward = 4;
        if let Some(e) = f.enemies.get_mut(0) {
            e.hp = (e.hp * 2 / 3).max(1);
            e.mark_pct = 25;
            e.intent = Intent::Heavy;
        }
        if let Some(e) = f.enemies.get_mut(1) {
            e.bleed = 3;
            e.intent = Intent::Brace;
        }
        if let Some(e) = f.enemies.get_mut(2) {
            e.intent = Intent::Rally;
        }
    }
    g
}

/// The states, with the keys that reach each screen.
#[must_use]
pub fn showcases() -> Vec<Showcase> {
    let c = content();
    let world = lived_in(&c);
    let wb = world.encode();
    let mut out: Vec<Showcase> = Vec::new();
    let mut add = |name: &'static str, game: &Game, keys: &'static str| {
        out.push(Showcase {
            name,
            save: game.encode(),
            world: wb.clone(),
            keys,
        });
    };

    // The town and its places. The hero has been hurt, owes the crew and holds spare gear.
    let town = in_town(&c, "warden", &world);
    add("town", &town, "");
    add("sheet-numbers", &town, "h down");
    add("sheet-marks", &town, "h right");
    add("sheet-moves", &town, "h right right");
    add("hall-standings", &town, "r");
    add("hall-dead", &town, "r right");
    add("hall-world", &town, "r right right");
    let mut smith = in_town(&c, "runesmith", &world);
    smith.hero.hp = smith.derived(&c).max_hp * 2 / 3;
    smith.hero.debt = 18;
    smith.hero.wounds.push(("gash".to_owned(), 3));
    let _ = smith.apply(&c, Action::Town(TownAct::BuyStock(0)));
    for (name, keys) in [
        ("market", "m down"),
        ("gear", "i down"),
        ("forge", "f"),
        ("temple", "t down"),
        ("guild", "g"),
        ("tavern", "b down"),
        ("library", "l"),
        ("works", "w"),
    ] {
        add(name, &smith, keys);
    }
    let thief = in_town(&c, "cutthroat", &world);
    add("skills", &thief, "s down down down");

    // The dungeon: two builds in one room, a lair, a forge, a nearly dark torch, the map.
    add(
        "delve-lift-runesmith",
        &delving(&c, "runesmith", "broken-lift", 19),
        "",
    );
    add(
        "delve-lift-warden",
        &delving(&c, "warden", "broken-lift", 19),
        "",
    );
    add(
        "delve-torch-low",
        &delving(&c, "warden", "sooty-gallery", 3),
        "",
    );
    add("delve-lair", &delving(&c, "duelist", "bone-pit", 12), "");
    add(
        "delve-forge",
        &delving(&c, "ember-priest", "hot-forge", 22),
        "",
    );
    add("map", &delving(&c, "wanderer", "sooty-gallery", 14), "m");

    // Combat.
    add(
        "fight-one",
        &fighting(&c, "warden", &["slag-rat"], false, 0),
        "",
    );
    add(
        "fight-four",
        &fighting(
            &c,
            "duelist",
            &["slag-rat", "cinder-imp", "rust-warden", "slag-rat"],
            false,
            2,
        ),
        "",
    );
    add(
        "fight-guardian",
        &fighting(&c, "ember-priest", &["bellows-golem", "slag-rat"], true, 4),
        "",
    );

    // The dead.
    let mut dead = hero_game(&c, "warden", 3);
    place_in(&c, &mut dead, "bone-pit", 1);
    if let Place::Delve { depth, room } = dead.place {
        dead.place = Place::Dead {
            depth,
            room,
            since: DAY - 1,
        };
    }
    add("dead", &dead, "");

    // Making a hero: no save at all.
    for (name, keys) in [
        ("create-calling", "down down"),
        ("create-traits", "3 o down"),
        ("create-points", "3 a b enter 1"),
    ] {
        out.push(Showcase {
            name,
            save: Vec::new(),
            world: wb.clone(),
            keys,
        });
    }
    let _ = FightAct::Flee;
    out
}
