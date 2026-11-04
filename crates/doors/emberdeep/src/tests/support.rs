//! Helpers shared by the unit tests.

use std::sync::Arc;

use russh_bbs_door::DoorRng;

use crate::content::Content;
use crate::dungeon;
use crate::game::{Action, Event, Game, Place};
use crate::hero::Hero;
use crate::world::World;

pub(crate) const TODAY: i64 = 20_000;

pub(crate) fn content() -> Arc<Content> {
    static CONTENT: std::sync::OnceLock<Arc<Content>> = std::sync::OnceLock::new();
    Arc::clone(
        CONTENT.get_or_init(|| Arc::new(Content::embedded().expect("embedded content loads"))),
    )
}

/// A hero of `calling` with two traits and one point in each attribute, at full health.
pub(crate) fn hero_of(calling: &str, traits: [&str; 2]) -> Hero {
    let c = content();
    let traits: Vec<String> = traits.iter().map(|t| (*t).to_owned()).collect();
    Hero::create(&c, "Tester", calling, &traits, [1, 1, 1, 1]).expect("hero")
}

pub(crate) fn game_with(hero: Hero, seed: u64) -> Game {
    let c = content();
    let mut game = Game::new(hero, DoorRng::from_seed(seed), TODAY - 1, 1, World::new(1));
    let _ = game.new_day(&c, TODAY);
    game
}

pub(crate) fn game_of(calling: &str, traits: [&str; 2], seed: u64) -> Game {
    game_with(hero_of(calling, traits), seed)
}

/// Put the hero in a room built from template `id`; returns where. Panics if no level has one.
pub(crate) fn place(game: &mut Game, template: &str) -> (u8, u8) {
    place_from(game, template, 1)
}

/// Like [`place`], but only on levels `from` and deeper.
pub(crate) fn place_from(game: &mut Game, template: &str, from: u8) -> (u8, u8) {
    let c = content();
    for depth in from..=25 {
        let level = dungeon::generate(&c, game.season, game.epoch, depth);
        for (i, room) in level.rooms.iter().enumerate() {
            if c.rooms.room[room.template].id == template {
                let at = u8::try_from(i).unwrap();
                game.place = Place::Delve { depth, room: at };
                game.prev_room = at;
                return (depth, at);
            }
        }
    }
    panic!("no level has a {template} room");
}

pub(crate) fn labels(game: &Game) -> Vec<String> {
    game.options(&content())
        .into_iter()
        .map(|o| o.label)
        .collect()
}

pub(crate) fn said(events: &[Event]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Say(_, t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

pub(crate) fn run(game: &mut Game, action: Action) -> Vec<Event> {
    let c = content();
    game.apply(&c, action).expect("action allowed")
}

// ---- the town ----

use crate::town::TownAct;
use crate::world::Pledge;

/// A hero in town with coin and salvage to spend.
pub(crate) fn rich_game(calling: &str, traits: [&str; 2], seed: u64) -> Game {
    let mut g = game_of(calling, traits, seed);
    g.hero.coin = 2_000;
    g.hero.stash = 2_000;
    g.hero.level = 10;
    g.hero.turns = 25;
    g
}

/// Do a town action that must be allowed.
pub(crate) fn town(game: &mut Game, act: TownAct) -> Vec<Event> {
    run(game, Action::Town(act))
}

/// Do a town action that must be refused; returns the reason.
pub(crate) fn town_refused(game: &mut Game, act: TownAct) -> String {
    let c = content();
    game.apply(&c, Action::Town(act))
        .expect_err("action refused")
        .0
}

/// Finish a project (and what it needs first) by a donor named `Town`, in `world`.
pub(crate) fn finish(world: &mut World, project: &str) {
    let c = content();
    let def = c.project(project).expect("project").clone();
    for req in &def.requires {
        finish(world, req);
    }
    world.apply(&Change::Pledge(Pledge {
        hero: "Town".to_owned(),
        project: project.to_owned(),
        coin: def.coin,
        salvage: def.salvage,
    }));
}

use crate::world::Change;
