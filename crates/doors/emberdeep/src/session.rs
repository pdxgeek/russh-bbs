//! The minimal playable text screen. One text view (`lines`) is both the drawing and the
//! look-around, and every command is written `[k] label`, so a screen reader, a very small
//! window and the balance bots all read the same thing. The full UI is the next slice (E3);
//! this one has no rules of its own: it maps keys to engine actions.

use std::fmt::Write as _;
use std::sync::Arc;

use russh_bbs_door::{Canvas, Ctx, DoorEnv, DoorKey, DoorSession, DoorStyle, Flow, Start};

use crate::combat::{FightAct, Stance};
use crate::content::{Attr, Content, Row};
use crate::dungeon::{self, Dir, Level};
use crate::game::{Action, Event, Game, Place, Tone};
use crate::hero::{Hero, derive};
use crate::rules::{CREATION_POINTS, CREATION_TRAITS, DEPTH_MAX, RECOVERY_DAYS};
use crate::world::World;

const WRAP: usize = 76;
const LOG_KEEP: usize = 8;
const TRAIT_KEYS: &str = "abcdefghijklmnopqrstuvwx";
const NODE_KEYS: &str = "123456789abcdef";

type Line = (String, DoorStyle);

fn plain(text: impl Into<String>) -> Line {
    (text.into(), DoorStyle::Plain)
}

fn styled(text: impl Into<String>, style: DoorStyle) -> Line {
    (text.into(), style)
}

fn tone_style(tone: Tone) -> DoorStyle {
    match tone {
        Tone::Plain => DoorStyle::Plain,
        Tone::Good => DoorStyle::Good,
        Tone::Bad => DoorStyle::Bad,
        Tone::Dim => DoorStyle::Dim,
    }
}

/// Word-wrap `text` to `width` columns.
pub(crate) fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[derive(Debug, Clone)]
struct Create {
    step: CreateStep,
    calling: usize,
    traits: Vec<usize>,
    cursor: usize,
    points: [u8; 4],
    note: Option<String>,
}

impl Create {
    fn new(note: Option<String>) -> Self {
        Self {
            step: CreateStep::Calling,
            calling: 0,
            traits: Vec::new(),
            cursor: 0,
            points: [0; 4],
            note,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreateStep {
    Calling,
    Traits,
    Points,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Main,
    Skills,
    Sheet,
    Hall,
    Map,
}

enum Mode {
    /// The content failed to load, or the save could not be read. Nothing is written.
    Stuck(String),
    Create(Create),
    Play(Screen),
}

pub(crate) struct Session {
    content: Arc<Content>,
    game: Option<Game>,
    mode: Mode,
    log: Vec<(Tone, String)>,
    fresh_seed: u64,
    handle: String,
    season: u32,
    world: World,
    day: i64,
    target: usize,
    dirty: bool,
}

impl Session {
    pub(crate) fn new(content: Option<&Arc<Content>>, start: &Start<'_>, fresh_seed: u64) -> Self {
        let today = start.today.as_raw();
        let world = World::for_season(start.world, start.season);
        let mut session = Self {
            content: content.map_or_else(|| Arc::new(empty_content()), Arc::clone),
            game: None,
            mode: Mode::Stuck("The Emberdeep is sealed: its content failed to load.".to_owned()),
            log: Vec::new(),
            fresh_seed,
            handle: start.env.handle.clone(),
            season: start.season,
            world: world.clone(),
            day: today,
            target: 0,
            dirty: false,
        };
        if content.is_none() {
            return session;
        }
        match start.save {
            russh_bbs_door::Save::Loaded { bytes, season } if season == start.season => {
                match Game::decode(bytes) {
                    Some(mut game) => {
                        game.world = world;
                        game.season = start.season;
                        let events = game.new_day(&session.content, today);
                        session.absorb(events);
                        session.dirty = !session.log.is_empty() || game.day != today;
                        session.game = Some(game);
                        session.mode = Mode::Play(Screen::Main);
                    }
                    None => {
                        session.mode = Mode::Stuck(
                            "Your hero could not be read (the save is damaged). Nothing was changed; ask the sysop."
                                .to_owned(),
                        );
                    }
                }
            }
            russh_bbs_door::Save::Loaded { .. } => {
                session.mode = Mode::Create(Create::new(Some(
                    "A new season has begun. Your old hero has retired to the hall. Choose a new one.".to_owned(),
                )));
            }
            russh_bbs_door::Save::Fresh => session.mode = Mode::Create(Create::new(None)),
        }
        session
    }

    fn absorb(&mut self, events: Vec<Event>) {
        for event in events {
            if let Event::Say(tone, text) = event {
                self.log.push((tone, text));
            }
        }
        let excess = self.log.len().saturating_sub(LOG_KEEP);
        self.log.drain(..excess);
    }

    fn act(&mut self, action: Action, ctx: &mut Ctx<'_>) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        match game.apply(&self.content, action) {
            Ok(events) => {
                self.dirty = true;
                self.absorb(events);
            }
            Err(refused) => self.absorb(vec![Event::Say(Tone::Dim, refused.0)]),
        }
        self.flush_changes(ctx);
    }

    fn flush_changes(&mut self, ctx: &mut Ctx<'_>) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        let season = self.season;
        for change in game.take_changes() {
            self.world.apply(&change);
            ctx.edit_world(move |old| {
                let mut world = World::for_season(old, season);
                world.apply(&change);
                Some(world.encode())
            });
        }
    }

    // ---- keys ----

    fn create_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        let Mode::Create(c) = &mut self.mode else {
            return Flow::Stay;
        };
        if key == DoorKey::Escape {
            return Flow::Exit;
        }
        let callings = self.content.callings.calling.len();
        let traits = self.content.traits.traits.len();
        match c.step {
            CreateStep::Calling => match key {
                DoorKey::Char(ch) if ch.is_ascii_digit() => {
                    let n = ch.to_digit(10).unwrap_or(0) as usize;
                    if (1..=callings).contains(&n) {
                        c.calling = n - 1;
                        c.step = CreateStep::Traits;
                        c.cursor = 0;
                    }
                }
                _ => {}
            },
            CreateStep::Traits => match key {
                DoorKey::Char(ch) => {
                    if let Some(at) = TRAIT_KEYS.find(ch).filter(|at| *at < traits) {
                        c.cursor = at;
                        if let Some(p) = c.traits.iter().position(|t| *t == at) {
                            c.traits.remove(p);
                        } else if c.traits.len() < CREATION_TRAITS {
                            c.traits.push(at);
                        }
                    }
                }
                DoorKey::Enter if c.traits.len() == CREATION_TRAITS => {
                    c.step = CreateStep::Points;
                    c.points = [0; 4];
                }
                DoorKey::Backspace => c.step = CreateStep::Calling,
                _ => {}
            },
            CreateStep::Points => match key {
                DoorKey::Char(ch) if ('1'..='4').contains(&ch) => {
                    let at = ch as usize - '1' as usize;
                    if c.points.iter().sum::<u8>() < CREATION_POINTS {
                        c.points[at] += 1;
                    }
                }
                DoorKey::Backspace => {
                    if let Some(last) = c.points.iter().rposition(|p| *p > 0) {
                        c.points[last] -= 1;
                    } else {
                        c.step = CreateStep::Traits;
                    }
                }
                DoorKey::Enter if c.points.iter().sum::<u8>() == CREATION_POINTS => {
                    return self.finish_create(ctx);
                }
                _ => {}
            },
        }
        Flow::Stay
    }

    fn finish_create(&mut self, ctx: &mut Ctx<'_>) -> Flow {
        let content = Arc::clone(&self.content);
        let Mode::Create(c) = &self.mode else {
            return Flow::Stay;
        };
        let Some(calling) = content.callings.calling.get(c.calling) else {
            return Flow::Stay;
        };
        let trait_ids: Vec<String> = c
            .traits
            .iter()
            .filter_map(|i| content.traits.traits.get(*i))
            .map(|t| t.id.clone())
            .collect();
        match Hero::create(&content, &self.handle, &calling.id, &trait_ids, c.points) {
            Ok(hero) => {
                let rng = russh_bbs_door::DoorRng::from_seed(self.fresh_seed);
                let mut game = Game::new(hero, rng, self.day - 1, self.season, self.world.clone());
                let events = game.new_day(&content, self.day);
                let line = format!(
                    "{} the {} is ready for the Emberdeep.",
                    game.hero.name,
                    calling.name.to_lowercase()
                );
                self.absorb(events);
                self.log.push((Tone::Good, line));
                self.game = Some(game);
                self.mode = Mode::Play(Screen::Main);
                self.dirty = true;
            }
            Err(_) => {
                if let Mode::Create(c) = &mut self.mode {
                    c.note = Some("That hero could not be made. Pick again.".to_owned());
                    c.step = CreateStep::Calling;
                }
            }
        }
        let _ = ctx;
        Flow::Stay
    }

    fn play_key(&mut self, screen: Screen, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        if screen != Screen::Main {
            match (screen, key) {
                (_, DoorKey::Escape | DoorKey::Backspace) => self.mode = Mode::Play(Screen::Main),
                (Screen::Skills, DoorKey::Char(ch)) => self.skills_key(ch, ctx),
                _ => {}
            }
            return Flow::Stay;
        }
        let Some(game) = self.game.as_ref() else {
            return Flow::Stay;
        };
        if key == DoorKey::Escape {
            return Flow::Exit;
        }
        if game.fight.is_some() {
            self.fight_key(key, ctx);
            return Flow::Stay;
        }
        match game.place.clone() {
            Place::Town => return self.town_key(key, ctx),
            Place::Dead { .. } => {
                if key == DoorKey::Char('g') {
                    self.act(Action::GiveUp, ctx);
                }
            }
            Place::Delve { .. } => self.delve_key(key, ctx),
        }
        Flow::Stay
    }

    fn skills_key(&mut self, ch: char, ctx: &mut Ctx<'_>) {
        let attr = match ch {
            'M' => Some(Attr::Might),
            'F' => Some(Attr::Finesse),
            'W' => Some(Attr::Wits),
            'S' => Some(Attr::Spirit),
            _ => None,
        };
        if let Some(attr) = attr {
            self.act(Action::Raise(attr), ctx);
            return;
        }
        let Some(at) = NODE_KEYS.find(ch) else {
            return;
        };
        let Some(game) = self.game.as_ref() else {
            return;
        };
        let nodes = self.tree(&game.hero.calling);
        if let Some(id) = nodes.get(at) {
            let id = id.clone();
            self.act(Action::Buy(id), ctx);
        }
    }

    fn tree(&self, calling: &str) -> Vec<String> {
        self.content
            .nodes
            .node
            .iter()
            .filter(|n| n.calling == calling)
            .map(|n| n.id.clone())
            .collect()
    }

    fn town_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        let Some(game) = self.game.as_ref() else {
            return Flow::Stay;
        };
        let express: Vec<u8> = game.world.shortcuts.iter().map(|(d, _)| *d).collect();
        match key {
            DoorKey::Char('d') => self.act(Action::Descend(1), ctx),
            DoorKey::Char('s') => self.mode = Mode::Play(Screen::Skills),
            DoorKey::Char('h') => self.mode = Mode::Play(Screen::Sheet),
            DoorKey::Char('r') => self.mode = Mode::Play(Screen::Hall),
            DoorKey::Char('q') => return Flow::Exit,
            DoorKey::Char(ch) if ch.is_ascii_digit() && ch != '0' => {
                let n = ch.to_digit(10).unwrap_or(0) as usize - 1;
                if let Some(depth) = express.get(n) {
                    self.act(Action::Descend(*depth), ctx);
                }
            }
            _ => {}
        }
        Flow::Stay
    }

    fn delve_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let dir = match key {
            DoorKey::Char('n') | DoorKey::Up => Some(Dir::North),
            DoorKey::Char('e') | DoorKey::Right => Some(Dir::East),
            DoorKey::Char('s') | DoorKey::Down => Some(Dir::South),
            DoorKey::Char('w') | DoorKey::Left => Some(Dir::West),
            _ => None,
        };
        if let Some(dir) = dir {
            self.act(Action::Move(dir), ctx);
            return;
        }
        match key {
            DoorKey::Char('>') => self.act(Action::Stairs, ctx),
            DoorKey::Char('r') => self.act(Action::Rest, ctx),
            DoorKey::Char('u') => self.act(Action::Climb, ctx),
            DoorKey::Char('m') => self.mode = Mode::Play(Screen::Map),
            DoorKey::Char('c') => self.mode = Mode::Play(Screen::Sheet),
            DoorKey::Char(ch) if ch.is_ascii_digit() && ch != '0' => {
                let n = ch.to_digit(10).unwrap_or(0) as usize - 1;
                self.act(Action::Option(n), ctx);
            }
            _ => {}
        }
    }

    fn fight_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let Some(game) = self.game.as_ref() else {
            return;
        };
        let Some(fight) = game.fight.as_ref() else {
            return;
        };
        let alive: Vec<usize> = fight.alive().map(|(i, _)| i).collect();
        let target = self.current_target(game, &alive);
        let moves: Vec<String> = game.derived(&self.content).moves.into_iter().collect();
        match key {
            DoorKey::Up | DoorKey::Down => {
                if let Some(at) = alive.iter().position(|i| *i == target) {
                    let n = alive.len();
                    let next = if key == DoorKey::Up {
                        (at + n - 1) % n
                    } else {
                        (at + 1) % n
                    };
                    self.target = alive[next];
                }
            }
            DoorKey::Char('x') | DoorKey::Enter => {
                self.act(Action::Fight(FightAct::Attack(target)), ctx);
            }
            DoorKey::Char('a') => {
                self.act(Action::Fight(FightAct::Stance(Stance::Aggressive)), ctx);
            }
            DoorKey::Char('g') => self.act(Action::Fight(FightAct::Stance(Stance::Guarded)), ctx),
            DoorKey::Char('v') => self.act(Action::Fight(FightAct::Stance(Stance::Evasive)), ctx),
            DoorKey::Char('b') => self.act(Action::Fight(FightAct::Step), ctx),
            DoorKey::Char('f') => self.act(Action::Fight(FightAct::Flee), ctx),
            DoorKey::Char('z') => self.act(Action::Fight(FightAct::Quick), ctx),
            DoorKey::Char(ch) if ch.is_ascii_digit() && ch != '0' => {
                let n = ch.to_digit(10).unwrap_or(0) as usize - 1;
                if let Some(id) = moves.get(n) {
                    self.act(Action::Fight(FightAct::Use(id.clone(), target)), ctx);
                }
            }
            _ => {}
        }
    }

    /// The enemy the hero is aiming at: their choice if it still stands, else the first alive
    /// one they can reach with a basic attack.
    fn current_target(&self, game: &Game, alive: &[usize]) -> usize {
        if alive.contains(&self.target) {
            return self.target;
        }
        let Some(fight) = game.fight.as_ref() else {
            return 0;
        };
        let ranged = game
            .weapon(&self.content)
            .is_some_and(|w| w.reach == crate::content::Reach::Ranged);
        alive
            .iter()
            .copied()
            .find(|i| ranged || (fight.row == Row::Front && fight.enemies[*i].row == Row::Front))
            .or_else(|| alive.first().copied())
            .unwrap_or(0)
    }

    // ---- the text view ----

    fn lines(&self) -> (Vec<Line>, Vec<Line>) {
        match &self.mode {
            Mode::Stuck(text) => (
                wrap(text, WRAP)
                    .into_iter()
                    .map(|l| styled(l, DoorStyle::Bad))
                    .collect(),
                vec![plain("[Esc] Leave")],
            ),
            Mode::Create(c) => (self.create_lines(c), Vec::new()),
            Mode::Play(screen) => match (&self.game, screen) {
                (Some(game), Screen::Main) => self.main_lines(game),
                (Some(game), Screen::Skills) => (self.skills_lines(game), Vec::new()),
                (Some(game), Screen::Sheet) => (self.sheet_lines(game), Vec::new()),
                (Some(game), Screen::Hall) => (Self::hall_lines(game), Vec::new()),
                (Some(game), Screen::Map) => (self.map_lines(game), Vec::new()),
                (None, _) => (vec![plain("No hero.")], Vec::new()),
            },
        }
    }

    fn create_lines(&self, c: &Create) -> Vec<Line> {
        let mut out = vec![styled("EMBERDEEP: a new hero", DoorStyle::Strong)];
        if let Some(note) = &c.note {
            out.extend(
                wrap(note, WRAP)
                    .into_iter()
                    .map(|l| styled(l, DoorStyle::Dim)),
            );
        }
        match c.step {
            CreateStep::Calling => {
                out.push(plain("Choose your calling:"));
                for (i, calling) in self.content.callings.calling.iter().enumerate() {
                    out.push(styled(
                        format!("[{}] {}", i + 1, calling.name),
                        DoorStyle::Accent,
                    ));
                    out.extend(
                        wrap(&calling.blurb, WRAP - 4)
                            .into_iter()
                            .map(|l| styled(format!("    {l}"), DoorStyle::Dim)),
                    );
                }
            }
            CreateStep::Traits => {
                out.push(plain(format!(
                    "Choose {CREATION_TRAITS} traits ({} chosen):",
                    c.traits.len()
                )));
                let traits = &self.content.traits.traits;
                for row in 0..12 {
                    let mut text = String::new();
                    for col in 0..2 {
                        let at = row + col * 12;
                        if let Some(t) = traits.get(at) {
                            let mark = if c.traits.contains(&at) { '*' } else { ' ' };
                            let key = TRAIT_KEYS.chars().nth(at).unwrap_or('?');
                            let _ = write!(text, "[{key}]{mark}{:<20}", t.name);
                        }
                    }
                    out.push(plain(text));
                }
                if let Some(t) = traits.get(c.cursor) {
                    out.push(styled(format!("{}: {}", t.name, t.text), DoorStyle::Dim));
                }
                out.push(plain("[Enter] Continue   [Backspace] Back"));
            }
            CreateStep::Points => {
                let left = CREATION_POINTS.saturating_sub(c.points.iter().sum());
                out.push(plain(format!(
                    "Spend {CREATION_POINTS} attribute points ({left} left):"
                )));
                if let Some(calling) = self.content.callings.calling.get(c.calling) {
                    for (i, attr) in Attr::ALL.iter().enumerate() {
                        let base = calling.attrs[i];
                        out.push(plain(format!(
                            "[{}] {:<8} {:>2} (+{})",
                            i + 1,
                            attr.label(),
                            base + u32::from(c.points[i]),
                            c.points[i]
                        )));
                    }
                }
                out.push(plain("[Enter] Begin   [Backspace] Undo"));
            }
        }
        out
    }

    fn status_line(&self, game: &Game, d: &crate::hero::Derived) -> String {
        let h = &game.hero;
        format!(
            "{} L{} {}  HP {}/{}  Turns {}/{}  Fatigue {}  Coin {}  Salvage {}",
            h.name,
            h.level,
            self.content
                .calling(&h.calling)
                .map_or("hero", |c| c.name.as_str()),
            h.hp.max(0),
            d.max_hp,
            h.turns,
            crate::rules::TURNS_PER_DAY,
            h.fatigue,
            h.coin,
            h.salvage
        )
    }

    fn main_lines(&self, game: &Game) -> (Vec<Line>, Vec<Line>) {
        let d = game.derived(&self.content);
        let mut body = vec![
            styled("EMBERDEEP", DoorStyle::Strong),
            plain(self.status_line(game, &d)),
        ];
        let mut keys = Vec::new();
        if game.fight.is_some() {
            self.fight_lines(game, &d, &mut body, &mut keys);
        } else {
            match &game.place {
                Place::Town => self.town_lines(game, &mut body, &mut keys),
                Place::Dead { depth, since, .. } => {
                    body.push(styled(
                        format!("You are dead, on level {depth}."),
                        DoorStyle::Bad,
                    ));
                    let left = (RECOVERY_DAYS - (self.day - since)).max(0);
                    body.extend(wrap(
                        &format!("Your body lies in the dark. Another hero may carry you home: {left} day(s) left. After that you wake in the temple, weakened, and the gear and pack are lost."),
                        WRAP,
                    ).into_iter().map(plain));
                    keys.push(plain("[g] Give up and wake in the temple   [Esc] Leave"));
                }
                Place::Delve { depth, room } => {
                    self.delve_lines(game, *depth, *room, &mut body, &mut keys);
                }
            }
        }
        let mut tail: Vec<Line> = self
            .log
            .iter()
            .map(|(tone, text)| styled(text.clone(), tone_style(*tone)))
            .collect();
        let mut log_lines = Vec::new();
        for (style, text) in tail.drain(..).map(|(t, s)| (s, t)) {
            for l in wrap(&text, WRAP) {
                log_lines.push(styled(l, style));
            }
        }
        body.push(plain(""));
        body.extend(keys);
        (body, log_lines)
    }

    fn town_lines(&self, game: &Game, body: &mut Vec<Line>, keys: &mut Vec<Line>) {
        let h = &game.hero;
        body.push(styled(
            "Cindermere, under the hill. The forges below are warm.",
            DoorStyle::Dim,
        ));
        if h.skill_points > 0 || h.attr_points > 0 {
            body.push(styled(
                format!(
                    "You have {} skill point(s) and {} attribute point(s) to spend.",
                    h.skill_points, h.attr_points
                ),
                DoorStyle::Good,
            ));
        }
        if !h.wounds.is_empty() {
            let list: Vec<String> = h
                .wounds
                .iter()
                .map(|(id, days)| {
                    format!(
                        "{} ({days}d)",
                        self.content
                            .wound(id)
                            .map_or(id.as_str(), |w| w.name.as_str())
                    )
                })
                .collect();
            body.push(styled(
                format!("Wounds: {}", list.join(", ")),
                DoorStyle::Bad,
            ));
        }
        if !h.scars.is_empty() {
            let list: Vec<&str> = h
                .scars
                .iter()
                .filter_map(|s| self.content.scar(s))
                .map(|s| s.name.as_str())
                .collect();
            body.push(styled(
                format!("Scars: {}", list.join(", ")),
                DoorStyle::Dim,
            ));
        }
        body.push(plain("Tavern board:"));
        let news: Vec<&String> = game.world.news.iter().rev().take(3).collect();
        if news.is_empty() {
            body.push(styled("  Nothing yet this season.", DoorStyle::Dim));
        }
        for line in news {
            body.push(styled(format!("  {line}"), DoorStyle::Dim));
        }
        keys.push(plain("[d] Descend into the Emberdeep (1 turn)"));
        for (n, (depth, kind)) in game.world.shortcuts.iter().enumerate().take(9) {
            keys.push(plain(format!(
                "[{}] Take the {} down to level {depth} ({} turn)",
                n + 1,
                kind.label(),
                kind.flat_cost()
            )));
        }
        keys.push(plain(
            "[s] Skills and attributes   [h] Hero sheet   [r] The hall   [q] Leave",
        ));
    }

    fn delve_lines(
        &self,
        game: &Game,
        depth: u8,
        room: u8,
        body: &mut Vec<Line>,
        keys: &mut Vec<Line>,
    ) {
        let level = dungeon::generate(&self.content, game.season, game.epoch, depth);
        let Some(template) = level.template(&self.content, room) else {
            return;
        };
        let band = dungeon::band_of(depth);
        body.push(styled(
            format!("Level {depth} (band {band}): {}", template.name),
            DoorStyle::Accent,
        ));
        body.extend(wrap(&template.text, WRAP).into_iter().map(plain));
        if let Some(shortcut) = game.world.shortcut_at(depth) {
            body.push(styled(
                format!(
                    "A {} runs from this level to the surface.",
                    shortcut.label()
                ),
                DoorStyle::Good,
            ));
        }
        let hostile = game.is_hostile(&self.content, &level, room);
        if hostile {
            body.push(styled(
                "A pack bars the way out of this room.",
                DoorStyle::Bad,
            ));
        }
        for f in &game.world.fallen {
            if f.depth == depth
                && f.room == room
                && f.rescued_by.is_none()
                && f.name != game.hero.name
            {
                body.push(styled(
                    format!(
                        "{} lies here, dead. They can still be carried home.",
                        f.name
                    ),
                    DoorStyle::Bad,
                ));
            }
        }
        self.delve_keys(game, &level, depth, room, hostile, keys);
    }

    fn delve_keys(
        &self,
        game: &Game,
        level: &Level,
        depth: u8,
        room: u8,
        hostile: bool,
        keys: &mut Vec<Line>,
    ) {
        let explored = game
            .explored
            .get(usize::from(depth) - 1)
            .copied()
            .unwrap_or(0);
        if let Some(r) = level.room(room) {
            for dir in Dir::ALL {
                if let Some(next) = r.links[dir.index()] {
                    let known = explored & (1 << next) != 0;
                    let name = if known {
                        level
                            .template(&self.content, next)
                            .map_or("?", |t| t.name.as_str())
                    } else {
                        "unexplored"
                    };
                    let back = if next == game.prev_room && hostile {
                        " (back)"
                    } else {
                        ""
                    };
                    keys.push(plain(format!(
                        "[{}] Go {}: {name}{back}",
                        dir.letter(),
                        dir.word()
                    )));
                }
            }
        }
        for (i, o) in game.options(&self.content).iter().enumerate().take(9) {
            let roll = o.roll.map_or(String::new(), |(attr, dc)| {
                format!(", {} {dc}", attr.label())
            });
            keys.push(plain(format!(
                "[{}] {} ({} turn{})",
                i + 1,
                o.label,
                o.cost,
                roll
            )));
        }
        if room == level.exit && !hostile && depth < DEPTH_MAX {
            keys.push(plain(format!(
                "[>] Take the stairs down to level {} (1 turn)",
                depth + 1
            )));
        }
        if !hostile {
            keys.push(plain(format!(
                "[r] Rest and make camp ({} turns)",
                crate::rules::REST_TURNS
            )));
        }
        keys.push(plain(format!(
            "[u] Climb back to town ({} turns of {})",
            game.ascent_cost(&self.content),
            game.hero.turns
        )));
        keys.push(plain(
            "[m] Map   [c] Hero sheet   [Esc] Leave the door (you stay here)",
        ));
    }

    fn fight_lines(
        &self,
        game: &Game,
        d: &crate::hero::Derived,
        body: &mut Vec<Line>,
        keys: &mut Vec<Line>,
    ) {
        let Some(f) = game.fight.as_ref() else {
            return;
        };
        body.push(styled(
            format!(
                "FIGHT, round {}   stance {}   row {}   focus {}/5   ward {}",
                f.round + 1,
                f.stance.label(),
                if f.row == Row::Front { "front" } else { "back" },
                f.focus,
                f.ward
            ),
            DoorStyle::Accent,
        ));
        let alive: Vec<usize> = f.alive().map(|(i, _)| i).collect();
        let target = self.current_target(game, &alive);
        for (i, e) in f.alive() {
            let Some(m) = self.content.monster(&e.id) else {
                continue;
            };
            let mark = if i == target { '>' } else { ' ' };
            body.push(styled(
                format!(
                    "{mark}({}) {:<18} hp {:>3}  {:<5}  next: {}",
                    i + 1,
                    m.name,
                    e.hp,
                    if e.row == Row::Front { "front" } else { "back" },
                    e.intent.describe()
                ),
                if i == target {
                    DoorStyle::Selected
                } else {
                    DoorStyle::Plain
                },
            ));
        }
        keys.push(plain(
            "[a] Aggressive  [g] Guarded  [v] Evasive  (stances are free)",
        ));
        keys.push(plain("[x] Attack the marked enemy (Up/Down to aim)"));
        for (n, id) in d.moves.iter().enumerate().take(9) {
            if let Some(mv) = self.content.mv(id) {
                keys.push(plain(format!(
                    "[{}] {} ({} focus)",
                    n + 1,
                    mv.name,
                    mv.cost
                )));
            }
        }
        let step = if f.row == Row::Front {
            "back"
        } else {
            "forward"
        };
        let mut more = format!("[b] Step {step}  [f] Flee");
        if f.round == 0 && !f.paid && !f.guardian {
            more.push_str("  [z] Quick-resolve (1 turn, no choices)");
        }
        keys.push(plain(more));
    }

    fn skills_lines(&self, game: &Game) -> Vec<Line> {
        let h = &game.hero;
        let mut out = vec![
            styled("SKILLS", DoorStyle::Strong),
            plain(format!(
                "Skill points {}   Attribute points {}   (a taken skill can close others for good)",
                h.skill_points, h.attr_points
            )),
        ];
        for (i, id) in self.tree(&h.calling).iter().enumerate() {
            let Some(n) = self.content.node(id) else {
                continue;
            };
            let key = NODE_KEYS.chars().nth(i).unwrap_or('?');
            let state = if h.nodes.contains(id) {
                "taken"
            } else if let Some(why) = h.node_refusal(&self.content, id) {
                if why.contains("closed") {
                    "closed"
                } else if why.contains("earlier") || why.contains("experienced") {
                    "locked"
                } else {
                    "ready"
                }
            } else {
                "ready"
            };
            out.push(styled(
                format!("[{key}] T{} {:<22} {state:<6} {}", n.tier, n.name, n.text),
                match state {
                    "taken" => DoorStyle::Good,
                    "ready" => DoorStyle::Plain,
                    _ => DoorStyle::Dim,
                },
            ));
        }
        out.push(plain(
            "Attributes: [M]ight [F]inesse [W]its [S]pirit (capital letters)   [Esc] Back",
        ));
        out
    }

    fn sheet_lines(&self, game: &Game) -> Vec<Line> {
        let h = &game.hero;
        let d = game.derived(&self.content);
        let mut out = vec![
            styled("HERO SHEET", DoorStyle::Strong),
            plain(self.status_line(game, &d)),
        ];
        let attrs: Vec<String> = Attr::ALL
            .iter()
            .map(|a| format!("{} {}", a.label(), d.attr(*a)))
            .collect();
        out.push(plain(attrs.join("  ")));
        out.push(plain(format!(
            "Armor {}  Damage +{}  Crit {}%  Evade {}%  Death-save +{}%  Carry {}  Climb ease {}  Loot +{}%  Trap sense +{}",
            d.armor, d.damage, d.crit, d.evade, d.death_save, d.carry, d.climb_ease, d.loot, d.trap_sense
        )));
        out.push(plain(format!(
            "Weapon {}  Armour {}",
            self.content
                .weapon(&h.weapon)
                .map_or("?", |w| w.name.as_str()),
            self.content
                .armour(&h.armour)
                .map_or("?", |a| a.name.as_str())
        )));
        out.push(plain(format!(
            "Capabilities: {}",
            d.tags.iter().cloned().collect::<Vec<_>>().join(", ")
        )));
        let moves: Vec<&str> = d
            .moves
            .iter()
            .filter_map(|m| self.content.mv(m))
            .map(|m| m.name.as_str())
            .collect();
        out.push(plain(format!("Moves: {}", moves.join(", "))));
        let traits: Vec<&str> = h
            .traits
            .iter()
            .filter_map(|t| self.content.trait_def(t))
            .map(|t| t.name.as_str())
            .collect();
        out.push(plain(format!("Traits: {}", traits.join(", "))));
        let scars: Vec<&str> = h
            .scars
            .iter()
            .filter_map(|t| self.content.scar(t))
            .map(|t| t.name.as_str())
            .collect();
        out.push(plain(format!(
            "Scars: {}",
            if scars.is_empty() {
                "none".to_owned()
            } else {
                scars.join(", ")
            }
        )));
        out.push(plain(format!(
            "Deepest {}  Kills {}  Deaths {}  Relics {}  XP {}",
            h.best_depth, h.kills, h.deaths, h.relics, h.xp
        )));
        out.push(plain("[Esc] Back"));
        out
    }

    fn hall_lines(game: &Game) -> Vec<Line> {
        let w = &game.world;
        let mut out = vec![styled("THE HALL", DoorStyle::Strong)];
        out.push(plain("Deepest heroes:"));
        if w.deepest.is_empty() {
            out.push(styled(
                "  No one has come back from the deep yet.",
                DoorStyle::Dim,
            ));
        }
        for (name, depth) in &w.deepest {
            out.push(plain(format!("  {name}: level {depth}")));
        }
        out.push(plain("Guardians slain:"));
        if w.slain.is_empty() {
            out.push(styled("  None yet.", DoorStyle::Dim));
        }
        for (band, by) in &w.slain {
            out.push(plain(format!("  Band {band}: {by}")));
        }
        out.push(plain("Shortcuts open for everyone:"));
        if w.shortcuts.is_empty() {
            out.push(styled("  None yet.", DoorStyle::Dim));
        }
        for (depth, kind) in &w.shortcuts {
            out.push(plain(format!("  Level {depth}: a {}", kind.label())));
        }
        let missing: Vec<&crate::world::Fallen> =
            w.fallen.iter().filter(|f| f.rescued_by.is_none()).collect();
        out.push(plain("Fallen, waiting to be found:"));
        if missing.is_empty() {
            out.push(styled("  No one.", DoorStyle::Dim));
        }
        for f in missing {
            out.push(plain(format!("  {} on level {}", f.name, f.depth)));
        }
        out.push(plain("[Esc] Back"));
        out
    }

    fn map_lines(&self, game: &Game) -> Vec<Line> {
        let Place::Delve { depth, room } = game.place else {
            return vec![plain("There is no map in town.")];
        };
        let level = dungeon::generate(&self.content, game.season, game.epoch, depth);
        let explored = game
            .explored
            .get(usize::from(depth) - 1)
            .copied()
            .unwrap_or(0);
        let mut out = vec![styled(format!("MAP of level {depth}"), DoorStyle::Strong)];
        out.extend(map_rows(&level, explored, room).into_iter().map(plain));
        out.push(plain(
            "@ you   # visited   ? seen   > way down   ^ way up   S shaft   [Esc] Back",
        ));
        out
    }
}

/// The grid of a level as text.
pub(crate) fn map_rows(level: &Level, explored: u32, here: u8) -> Vec<String> {
    let known = |i: u8| explored & (1 << i) != 0;
    let seen = |i: u8| {
        known(i)
            || level.rooms.iter().enumerate().any(|(j, r)| {
                known(u8::try_from(j).unwrap_or(0)) && r.links.iter().flatten().any(|n| *n == i)
            })
    };
    let mut rows = Vec::new();
    for y in 0..dungeon::GRID_H {
        let mut line = String::new();
        let mut below = String::new();
        for x in 0..dungeon::GRID_W {
            let at = level.rooms.iter().position(|r| r.x == x && r.y == y);
            let Some(at) = at else {
                line.push_str("   ");
                below.push_str("   ");
                continue;
            };
            let id = u8::try_from(at).unwrap_or(0);
            let r = &level.rooms[at];
            let glyph = if id == here {
                '@'
            } else if !seen(id) {
                ' '
            } else if !known(id) {
                '?'
            } else if id == level.exit {
                '>'
            } else if id == level.entry {
                '^'
            } else if id == level.shaft {
                'S'
            } else {
                '#'
            };
            line.push(glyph);
            let east = r.links[Dir::East.index()].is_some_and(|n| seen(id) && seen(n));
            line.push_str(if east { "--" } else { "  " });
            let south = r.links[Dir::South.index()].is_some_and(|n| seen(id) && seen(n));
            below.push_str(if south { "|  " } else { "   " });
        }
        rows.push(line.trim_end().to_owned());
        rows.push(below.trim_end().to_owned());
    }
    rows
}

fn empty_content() -> Content {
    // Only used when the real content failed to load; the session then shows a message and
    // never reads it.
    Content {
        callings: crate::content::Callings {
            calling: Vec::new(),
        },
        nodes: crate::content::Nodes { node: Vec::new() },
        traits: crate::content::Traits {
            traits: Vec::new(),
            scar: Vec::new(),
        },
        moves: crate::content::Moves { moves: Vec::new() },
        bestiary: crate::content::Bestiary {
            monster: Vec::new(),
            trap: Vec::new(),
        },
        rooms: crate::content::Rooms { room: Vec::new() },
        wounds: crate::content::Wounds { wound: Vec::new() },
        gear: crate::content::Gear {
            weapon: Vec::new(),
            armour: Vec::new(),
        },
    }
}

impl DoorSession for Session {
    fn key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        // The day may turn while the door is open.
        let today = ctx.today().as_raw();
        if today > self.day {
            self.day = today;
            if let Some(game) = self.game.as_mut() {
                let events = game.new_day(&self.content, today);
                self.absorb(events);
                self.dirty = true;
            }
        }
        let flow = match &self.mode {
            Mode::Stuck(_) => {
                if key == DoorKey::Escape {
                    Flow::Exit
                } else {
                    Flow::Stay
                }
            }
            Mode::Create(_) => self.create_key(key, ctx),
            Mode::Play(screen) => {
                let screen = *screen;
                self.play_key(screen, key, ctx)
            }
        };
        if self.dirty {
            if let Some(game) = self.game.as_ref() {
                ctx.request_save(game.encode());
            }
            self.dirty = false;
        }
        flow
    }

    fn render(&self, canvas: &mut Canvas<'_>, _env: &DoorEnv) {
        canvas.clear();
        let (body, log) = self.lines();
        let height = usize::from(canvas.height());
        let room_for_log = height.saturating_sub(body.len());
        let skip = log.len().saturating_sub(room_for_log);
        let shown: Vec<&Line> = body.iter().chain(log.iter().skip(skip)).collect();
        for (y, (text, style)) in shown.into_iter().take(height).enumerate() {
            canvas.text(0, u16::try_from(y).unwrap_or(u16::MAX), text, *style);
        }
    }

    fn look_around(&self, _env: &DoorEnv) -> Vec<String> {
        let (body, log) = self.lines();
        body.into_iter().chain(log).map(|(text, _)| text).collect()
    }
}

#[allow(dead_code)]
fn unused(_: &Hero) {
    let _ = derive;
}
