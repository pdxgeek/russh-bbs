//! The town's places as text screens. Each place builds a head (what is here) and a menu of
//! entries; the same menu draws the keys and answers a key press, so the screen and the keys can
//! never disagree. Every command is `[k] label`, which the look-around, a screen reader and the
//! balance bots all read. No rules live here: an entry holds an engine [`Action`].

use russh_bbs_door::DoorStyle;

use super::{Line, Session, WRAP, plain, styled, wrap};
use crate::content::Reach;
use crate::content_town::Learn;
use crate::game::{Action, Game};
use crate::hero::Kit;
use crate::rules::{TRAIN_LEVEL_MAX, gear_level};
use crate::town::{TownAct, Wares, Worn, base_band, item_name, repair_cost};

/// A place in the town.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spot {
    Tavern,
    Market,
    Gear,
    Forge,
    Temple,
    Guild,
    Library,
    Works,
}

/// Keys a menu hands out, in order.
const MENU_KEYS: &str = "123456789abcdefghijklmnopqrstuvwxyz";

/// One command on a place's menu.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) key: char,
    pub(crate) label: String,
    pub(crate) action: Action,
}

#[derive(Default)]
struct Menu(Vec<Entry>);

impl Menu {
    fn add(&mut self, label: impl Into<String>, action: Action) {
        if let Some(key) = MENU_KEYS.chars().nth(self.0.len()) {
            self.0.push(Entry {
                key,
                label: label.into(),
                action,
            });
        }
    }

    fn town(&mut self, label: impl Into<String>, act: TownAct) {
        self.add(label, Action::Town(act));
    }
}

/// Lay a menu out as `[k] label` entries, packed into lines of at most [`WRAP`] columns.
pub(crate) fn key_lines(entries: &[Entry]) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut row = String::new();
    for e in entries {
        let cell = format!("[{}] {}", e.key, e.label);
        if !row.is_empty() && row.chars().count() + 3 + cell.chars().count() > WRAP {
            out.push(plain(std::mem::take(&mut row)));
        }
        if !row.is_empty() {
            row.push_str("   ");
        }
        row.push_str(&cell);
    }
    if !row.is_empty() {
        out.push(plain(row));
    }
    out
}

fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_owned()
    } else {
        let cut: String = text.chars().take(width.saturating_sub(1)).collect();
        format!("{cut}~")
    }
}

impl Session {
    fn kit_note(&self, kit: &Kit) -> String {
        let core = kit
            .core
            .as_ref()
            .and_then(|c| self.content.core(c))
            .map_or("no core", |c| c.name.as_str());
        format!("wear {}%, {core}", kit.wear)
    }

    fn base_stats(&self, base: &str) -> String {
        if let Some(w) = self.content.weapon(base) {
            format!(
                "{}-{} {} {}",
                w.damage[0],
                w.damage[1],
                w.attr.label().to_lowercase(),
                if w.reach == Reach::Ranged {
                    "ranged"
                } else {
                    "melee"
                }
            )
        } else if let Some(a) = self.content.armour(base) {
            format!("armor {}", a.armor)
        } else {
            String::new()
        }
    }

    fn head_with_status(&self, game: &Game, title: &str) -> Vec<Line> {
        let d = game.derived(&self.content);
        let mut head = vec![styled(title, DoorStyle::Strong)];
        head.extend(self.status_lines(game, &d));
        head
    }

    /// The head and the menu of a place.
    pub(crate) fn spot_view(&self, game: &Game, spot: Spot) -> (Vec<Line>, Vec<Entry>) {
        match spot {
            Spot::Tavern => self.tavern_view(game),
            Spot::Market => self.market_view(game),
            Spot::Gear => self.gear_view(game),
            Spot::Forge => self.forge_view(game),
            Spot::Temple => self.temple_view(game),
            Spot::Guild => self.guild_view(game),
            Spot::Library => self.library_view(game),
            Spot::Works => self.works_view(game),
        }
    }

    fn tavern_view(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let c = &self.content;
        let h = &game.hero;
        let d = game.derived(c);
        let mut head = self.head_with_status(game, "THE TAVERN: the Banked Ember");
        let mut menu = Menu::default();
        if let Some(t) = &h.contract {
            if let Some(def) = c.contract(&t.id) {
                let left = (i64::from(def.days) - (game.day - t.day)).max(0);
                head.push(styled(
                    format!(
                        "Your contract: {} ({}, {left} day(s) left)",
                        def.name,
                        if t.done {
                            "done: turn it in"
                        } else {
                            "not done"
                        }
                    ),
                    DoorStyle::Accent,
                ));
                if !t.done {
                    head.extend(
                        wrap(&def.text, WRAP)
                            .into_iter()
                            .map(|l| styled(l, DoorStyle::Dim)),
                    );
                }
            }
            menu.town("Turn in", TownAct::TurnIn);
            menu.town("Drop it", TownAct::Drop);
        } else {
            head.push(plain("The board, today:"));
            let board = game.board(c);
            if board.is_empty() {
                head.push(styled(
                    "  Nothing the board-keeper trusts you with.",
                    DoorStyle::Dim,
                ));
            }
            for (i, p) in board.iter().enumerate() {
                head.push(plain(clip(
                    &format!(
                        "  {}. {}: {} - level {}+, {}c {}xp, {}d",
                        i + 1,
                        p.kind.label(),
                        p.name,
                        p.depth,
                        p.coin,
                        p.xp,
                        p.days
                    ),
                    WRAP,
                )));
                menu.town(format!("Take {}", i + 1), TownAct::Take(i));
            }
        }
        head.push(plain(format!(
            "Drinks {}/3  Dice {}/3  Debt {}",
            game.today.drinks, game.today.gambles, h.debt
        )));
        if let Some(b) = &h.bond
            && let Some(comp) = c.companion(&b.id)
        {
            let rung = if b.level == 0 {
                "an acquaintance"
            } else {
                comp.steps
                    .get(usize::from(b.level) - 1)
                    .map_or("a friend", |s| s.name.as_str())
            };
            head.push(styled(
                format!(
                    "Company: {} ({rung}, {}/2 toward the next)",
                    comp.name, b.progress
                ),
                DoorStyle::Good,
            ));
        }
        self.tavern_menu(game, &d, &mut menu);
        let mut lines = head;
        lines.extend(key_lines(&menu.0));
        (lines, menu.0)
    }

    /// The tavern's standing entries: drink, dice, the crew's book and the people to call on.
    fn tavern_menu(&self, game: &Game, d: &crate::hero::Derived, menu: &mut Menu) {
        let c = &self.content;
        let h = &game.hero;
        menu.town("Drink (4c)", TownAct::Drink);
        for stake in [5, 20, 50] {
            menu.town(
                format!("Dice for {stake}c"),
                TownAct::Gamble {
                    stake,
                    cheat: false,
                },
            );
        }
        if d.has_tag("sneak") {
            for stake in [5, 20] {
                menu.town(
                    format!("Loaded dice for {stake}c"),
                    TownAct::Gamble { stake, cheat: true },
                );
            }
        }
        if h.debt > 0 {
            menu.town("Pay the crew", TownAct::Repay);
        }
        for comp in &c.town.companion {
            let free = h.bond.as_ref().is_none_or(|b| b.id == comp.id);
            if free && !h.closed.contains(&comp.id) {
                menu.town(
                    format!("Call on {} ({}c)", comp.name, comp.gift),
                    TownAct::Court(comp.id.clone()),
                );
            }
        }
    }

    fn market_view(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let c = &self.content;
        let fx = game.town_fx(c);
        let mut head = self.head_with_status(game, "THE MARKET");
        let mut menu = Menu::default();
        head.push(styled(
            format!("Market level {}. Today's stalls:", fx.market),
            DoorStyle::Dim,
        ));
        for (i, s) in game.stock(c).iter().enumerate() {
            let sold = game
                .today
                .bought
                .contains(&u8::try_from(i).unwrap_or(u8::MAX));
            let text = match &s.wares {
                Wares::Gear(g) => format!(
                    "{}  {}  L{}",
                    item_name(c, &g.base, &g.kit),
                    self.base_stats(&g.base),
                    gear_level(base_band(c, &g.base))
                ),
                Wares::Core(id) => c.core(id).map_or_else(String::new, |core| {
                    format!("{}  ({})", core.name, self.effect_text(&core.effects))
                }),
            };
            head.push(plain(clip(
                &format!(
                    "  {}. {}  {}c{}",
                    i + 1,
                    text,
                    s.price,
                    if sold { " SOLD" } else { "" }
                ),
                WRAP,
            )));
            if !sold {
                menu.town(format!("Buy {}", i + 1), TownAct::BuyStock(i));
            }
        }
        menu.town("Sell 10 salvage (8c)", TownAct::SellSalvage);
        let mut lines = head;
        lines.extend(key_lines(&menu.0));
        (lines, menu.0)
    }

    fn effect_text(&self, effects: &[crate::content::Effect]) -> String {
        super::effect_summary(&self.content, effects)
    }

    fn gear_view(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let c = &self.content;
        let h = &game.hero;
        let mut head = self.head_with_status(game, "GEAR");
        let mut menu = Menu::default();
        for (worn, base, kit) in [
            (Worn::Weapon, &h.weapon, &h.wkit),
            (Worn::Armour, &h.armour, &h.akit),
        ] {
            head.push(plain(clip(
                &format!(
                    "{}: {}  {}  ({})",
                    if worn == Worn::Weapon {
                        "Weapon"
                    } else {
                        "Armour"
                    },
                    item_name(c, base, kit),
                    self.base_stats(base),
                    self.kit_note(kit)
                ),
                WRAP,
            )));
        }
        if !h.cores.is_empty() {
            let names: Vec<&str> = h
                .cores
                .iter()
                .filter_map(|id| c.core(id))
                .map(|k| k.name.as_str())
                .collect();
            head.push(plain(clip(
                &format!("Loose cores: {}", names.join(", ")),
                WRAP,
            )));
        }
        if !h.found.is_empty() {
            let names: Vec<String> = h
                .found
                .iter()
                .map(|i| item_name(c, &i.base, &i.kit))
                .collect();
            head.push(styled(
                clip(
                    &format!("Carrying (lost if you die): {}", names.join(", ")),
                    WRAP,
                ),
                DoorStyle::Dim,
            ));
        }
        head.push(plain("Spare gear (safe at home):"));
        if h.pack.is_empty() {
            head.push(styled("  none", DoorStyle::Dim));
        }
        for (i, it) in h.pack.iter().enumerate() {
            head.push(plain(clip(
                &format!(
                    "  {}. {}  {}  L{}  sells {}c",
                    i + 1,
                    item_name(c, &it.base, &it.kit),
                    self.base_stats(&it.base),
                    gear_level(base_band(c, &it.base)),
                    game.sell_price(c, it)
                ),
                WRAP,
            )));
            menu.town(format!("Wield {}", i + 1), TownAct::Equip(i));
        }
        for (i, _) in h.pack.iter().enumerate() {
            menu.town(format!("Sell {}", i + 1), TownAct::Sell(i));
        }
        let mut lines = head;
        lines.extend(key_lines(&menu.0));
        (lines, menu.0)
    }

    fn forge_view(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let c = &self.content;
        let h = &game.hero;
        let d = game.derived(c);
        let fx = game.town_fx(c);
        let mut head =
            self.head_with_status(game, &format!("THE FORGE (level {})", fx.forge.max(1)));
        let mut menu = Menu::default();
        for (worn, base, kit) in [
            (Worn::Weapon, &h.weapon, &h.wkit),
            (Worn::Armour, &h.armour, &h.akit),
        ] {
            let cost = repair_cost(
                kit.wear,
                base_band(c, base),
                fx.forge.max(1),
                d.has_tag("smithcraft"),
            );
            head.push(plain(clip(
                &format!(
                    "{} {}: {}  repair {}c",
                    if worn == Worn::Weapon {
                        "Weapon"
                    } else {
                        "Armour"
                    },
                    item_name(c, base, kit),
                    self.kit_note(kit),
                    cost
                ),
                WRAP,
            )));
            if cost > 0 {
                menu.town(
                    format!("Repair {} ({cost}c)", worn.word()),
                    TownAct::Repair(worn),
                );
            }
        }
        head.push(plain("Recipes you know:"));
        let mut any = false;
        for r in &c.recipes.recipe {
            if !h.knows(r) {
                continue;
            }
            any = true;
            let (salvage, coin) = Game::craft_cost(r, &d);
            let ready = game.craft_refusal(c, &r.id).is_none();
            head.push(plain(clip(
                &format!(
                    "  {}: {}s {}c, forge {}{}",
                    r.name,
                    salvage,
                    coin,
                    r.forge,
                    if ready { "" } else { " (not yet)" }
                ),
                WRAP,
            )));
            menu.town(format!("Make {}", r.name), TownAct::Craft(r.id.clone()));
        }
        if !any {
            head.push(styled("  none yet: find patterns below", DoorStyle::Dim));
        }
        for (n, id) in h.cores.iter().enumerate() {
            let Some(core) = c.core(id) else { continue };
            for worn in [Worn::Weapon, Worn::Armour] {
                if core.slot.fits(worn.slot()) {
                    menu.town(
                        format!("Slot {} in {} (8c)", core.name, worn.word()),
                        TownAct::Slot(n, worn),
                    );
                }
            }
        }
        if d.has_tag("smithcraft") {
            for (worn, kit) in [(Worn::Weapon, &h.wkit), (Worn::Armour, &h.akit)] {
                if kit.core.is_some() {
                    menu.town(
                        format!("Lift out {} core (4c)", worn.word()),
                        TownAct::Unslot(worn),
                    );
                }
            }
        }
        let mut lines = head;
        lines.extend(key_lines(&menu.0));
        (lines, menu.0)
    }

    fn temple_view(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let c = &self.content;
        let h = &game.hero;
        let fx = game.town_fx(c);
        let mut head = self.head_with_status(game, "THE TEMPLE OF THE LOW FLAME");
        let mut menu = Menu::default();
        head.push(styled(
            format!(
                "Temple level {}. Being tended costs {} turn(s).",
                fx.temple,
                game.heal_turns(c)
            ),
            DoorStyle::Dim,
        ));
        menu.town(
            format!("Be tended ({}c)", game.heal_price(c)),
            TownAct::Heal,
        );
        for (n, (id, days)) in h.wounds.iter().enumerate() {
            let Some(def) = c.wound(id) else { continue };
            head.push(plain(format!(
                "  {} ({days}d){}",
                def.name,
                if def.boon { " - a boon" } else { "" }
            )));
            if let Some(price) = game.cure_price(c, n) {
                menu.town(
                    format!("Cure {} ({price}c)", def.name.to_lowercase()),
                    TownAct::Cure(n),
                );
            }
        }
        let mut lines = head;
        lines.extend(key_lines(&menu.0));
        (lines, menu.0)
    }

    fn guild_view(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let c = &self.content;
        let fx = game.town_fx(c);
        let mut head = self.head_with_status(game, "THE GUILD HALL");
        let mut menu = Menu::default();
        head.extend(
            wrap(
                &format!(
                    "The hall drills heroes up to level {TRAIN_LEVEL_MAX}: {} drill(s) a day, {}c and a turn each. Today: {} done.",
                    1 + fx.guild,
                    game.train_price(),
                    game.today.trained
                ),
                WRAP,
            )
            .into_iter()
            .map(plain),
        );
        if game.hero.skill_points > 0 || game.hero.attr_points > 0 {
            head.extend(
                wrap(
                    &format!(
                        "You have {} skill point(s) and {} attribute point(s) to spend; go back and open Skills.",
                        game.hero.skill_points, game.hero.attr_points
                    ),
                    WRAP,
                )
                .into_iter()
                .map(|l| styled(l, DoorStyle::Good)),
            );
        }
        menu.town(
            format!("Drill ({}c, 1 turn)", game.train_price()),
            TownAct::Train,
        );
        let mut lines = head;
        lines.extend(key_lines(&menu.0));
        (lines, menu.0)
    }

    fn library_view(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let c = &self.content;
        let fx = game.town_fx(c);
        let h = &game.hero;
        let mut head = self.head_with_status(game, "THE LIBRARY");
        let mut menu = Menu::default();
        if fx.library {
            head.push(plain(format!(
                "The reading room is open. You hold {} relic(s) to be read.",
                h.relics
            )));
            let known: Vec<&str> = c
                .recipes
                .recipe
                .iter()
                .filter(|r| matches!(r.learn, Learn::Library) && h.knows(r))
                .map(|r| r.name.as_str())
                .collect();
            if !known.is_empty() {
                head.push(plain(clip(
                    &format!("Patterns read: {}", known.join(", ")),
                    WRAP,
                )));
            }
            menu.town("Read a relic (20c)", TownAct::Identify);
        } else {
            head.extend(
                wrap(
                    &format!(
                        "The shelves are dust and the reading room is shut. You hold {} relic(s) no one can read yet. The town could build a reading room.",
                        h.relics
                    ),
                    WRAP,
                )
                .into_iter()
                .map(|l| styled(l, DoorStyle::Dim)),
            );
        }
        let mut lines = head;
        lines.extend(key_lines(&menu.0));
        (lines, menu.0)
    }

    fn works_view(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let c = &self.content;
        let h = &game.hero;
        let total = c.projects.project.len();
        let done = c
            .projects
            .project
            .iter()
            .filter(|p| game.world.is_done(c, &p.id))
            .count();
        let mut head = self.head_with_status(
            game,
            &format!("THE TOWN WORKS ({done} of {total} finished)"),
        );
        let mut menu = Menu::default();
        let mut later: Vec<&str> = Vec::new();
        let mut shown = 0;
        for p in &c.projects.project {
            if game.world.is_done(c, &p.id) {
                continue;
            }
            if !game.world.is_open(c, &p.id) {
                later.push(p.name.as_str());
                continue;
            }
            if shown >= 4 {
                continue;
            }
            shown += 1;
            let (pc, ps) = game.world.progress(&p.id);
            head.push(styled(
                clip(
                    &format!("{} ({})", p.name, Game::project_effect_text(p.effect)),
                    WRAP,
                ),
                DoorStyle::Accent,
            ));
            head.push(plain(format!(
                "  coin {pc}/{}  salvage {ps}/{}",
                p.coin, p.salvage
            )));
            let short = clip(&p.name, 24);
            menu.town(
                format!("Give 25c: {short}"),
                TownAct::Donate {
                    project: p.id.clone(),
                    coin: 25,
                    salvage: 0,
                },
            );
            menu.town(
                format!("Give 25s: {short}"),
                TownAct::Donate {
                    project: p.id.clone(),
                    coin: 0,
                    salvage: 25,
                },
            );
        }
        if shown == 0 {
            head.push(styled("Nothing is being built right now.", DoorStyle::Dim));
        }
        if !later.is_empty() {
            head.push(styled(
                clip(&format!("Later: {}", later.join(", ")), WRAP),
                DoorStyle::Dim,
            ));
        }
        let (gc, gs) = h
            .given
            .iter()
            .fold((0u32, 0u32), |(a, b), (_, c, s)| (a + c, b + s));
        head.push(styled(
            format!("You have given {gc} coin and {gs} salvage this season."),
            DoorStyle::Dim,
        ));
        let mut lines = head;
        lines.extend(key_lines(&menu.0));
        (lines, menu.0)
    }

    /// Answer a key on a place's screen.
    pub(super) fn spot_key(&mut self, spot: Spot, ch: char, ctx: &mut russh_bbs_door::Ctx<'_>) {
        let Some(game) = self.game.as_ref() else {
            return;
        };
        let (_, entries) = self.spot_view(game, spot);
        if let Some(e) = entries.into_iter().find(|e| e.key == ch) {
            self.act(e.action, ctx);
        }
    }

    /// The lines of a place's screen (head, then keys are part of the head here).
    pub(super) fn spot_lines(&self, game: &Game, spot: Spot) -> Vec<Line> {
        let (mut lines, _) = self.spot_view(game, spot);
        lines.push(plain("[Esc] Back"));
        lines
    }
}
