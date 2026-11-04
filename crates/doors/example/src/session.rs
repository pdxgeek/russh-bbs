//! The coin toss screen: one text view that is both the drawing and the look-around.

use russh_bbs_door::{Canvas, Ctx, DoorEnv, DoorKey, DoorSession, DoorStyle, Flow};

use crate::content::Phrases;
use crate::engine::{Call, NAME_MAX, Outcome, State};
use crate::save;
use crate::world::World;

enum Mode {
    Main,
    /// Typing a nickname.
    Name(String),
}

pub(crate) struct Session {
    state: State,
    world: World,
    phrases: Phrases,
    mode: Mode,
    /// What the last toss said, or the last notice.
    message: Option<(String, DoorStyle)>,
}

impl Session {
    pub(crate) fn new(state: State, world: World, phrases: Phrases) -> Self {
        Self {
            state,
            world,
            phrases,
            mode: Mode::Main,
            message: None,
        }
    }

    fn toss(&mut self, call: Call, ctx: &mut Ctx<'_>) {
        self.state.new_day(ctx.today().as_raw());
        let outcome = self.state.toss(call, ctx.rng());
        // The flavor line is drawn from the same RNG, so a replay shows the same words.
        let roll = ctx.rng().below(1_000);
        self.message = Some(match outcome {
            Outcome::Won(_) => (
                format!("You called {}. +1 coin. {}", name_of(call), self.phrases.pick(true, roll)),
                DoorStyle::Good,
            ),
            Outcome::Lost(_) => (
                format!("You called {}. -1 coin. {}", name_of(call), self.phrases.pick(false, roll)),
                DoorStyle::Bad,
            ),
            Outcome::NoTossesLeft => (
                "No tosses left today. Come back tomorrow.".to_owned(),
                DoorStyle::Dim,
            ),
            Outcome::NoCoins => ("Your purse is empty.".to_owned(), DoorStyle::Bad),
        });
        if matches!(outcome, Outcome::Won(_) | Outcome::Lost(_)) {
            self.world.count_toss();
            ctx.edit_world(|world| {
                let mut world = world.and_then(World::decode).unwrap_or_default();
                world.count_toss();
                Some(world.encode())
            });
            ctx.request_save(save::encode(&self.state));
        }
    }

    fn name_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let Mode::Name(buffer) = &mut self.mode else {
            return;
        };
        match key {
            DoorKey::Char(c) if buffer.chars().count() < NAME_MAX => buffer.push(c),
            DoorKey::Backspace => {
                buffer.pop();
            }
            DoorKey::Escape => self.mode = Mode::Main,
            DoorKey::Enter => {
                // The board's username rules, reserved names included. The raw text is never
                // stored: only the checked name.
                match ctx.player_name(buffer) {
                    Ok(name) => {
                        self.state.name = name.as_str().to_owned();
                        ctx.request_save(save::encode(&self.state));
                        self.message = Some((format!("Now known as {}.", self.state.name), DoorStyle::Good));
                        self.mode = Mode::Main;
                    }
                    Err(_) => {
                        self.message = Some((
                            "That name is not allowed here.".to_owned(),
                            DoorStyle::Bad,
                        ));
                        self.mode = Mode::Main;
                    }
                }
            }
            _ => {}
        }
    }

    fn lines(&self) -> Vec<(String, DoorStyle)> {
        let s = &self.state;
        let mut out = vec![
            (
                format!("{} - day {}", s.name, s.day),
                DoorStyle::Accent,
            ),
            (
                format!(
                    "Coins: {}   Tosses left today: {}   Streak: {} (best {})",
                    s.coins, s.left, s.streak, s.best
                ),
                DoorStyle::Plain,
            ),
            (String::new(), DoorStyle::Plain),
        ];
        match &self.mode {
            Mode::Main => out.push((
                "[h] call heads   [t] call tails   [n] change name   [q] leave".to_owned(),
                DoorStyle::Strong,
            )),
            Mode::Name(buffer) => {
                out.push((format!("New name: {buffer}_"), DoorStyle::Strong));
                out.push(("Enter to keep it, Esc to cancel.".to_owned(), DoorStyle::Dim));
            }
        }
        out.push((String::new(), DoorStyle::Plain));
        if let Some((text, style)) = &self.message {
            out.push((text.clone(), *style));
            out.push((String::new(), DoorStyle::Plain));
        }
        out.push((
            format!(
                "The board has tossed {} coins today and {} yesterday.",
                self.world.today, self.world.yesterday
            ),
            DoorStyle::Dim,
        ));
        out.push((
            format!("Days the board has been keeping count: {}.", self.world.days),
            DoorStyle::Dim,
        ));
        out
    }
}

fn name_of(call: Call) -> &'static str {
    match call {
        Call::Heads => "heads",
        Call::Tails => "tails",
    }
}

impl DoorSession for Session {
    fn key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        if matches!(self.mode, Mode::Name(_)) {
            self.name_key(key, ctx);
            return Flow::Stay;
        }
        match key {
            DoorKey::Char('h' | 'H') => self.toss(Call::Heads, ctx),
            DoorKey::Char('t' | 'T') => self.toss(Call::Tails, ctx),
            DoorKey::Char('n' | 'N') => self.mode = Mode::Name(String::new()),
            DoorKey::Char('q' | 'Q') | DoorKey::Escape => return Flow::Exit,
            _ => {}
        }
        Flow::Stay
    }

    fn render(&self, canvas: &mut Canvas<'_>, _env: &DoorEnv) {
        canvas.clear();
        for (y, (text, style)) in self.lines().iter().enumerate() {
            let Ok(y) = u16::try_from(y) else { break };
            canvas.text(1, y, text, *style);
        }
    }

    fn look_around(&self, _env: &DoorEnv) -> Vec<String> {
        self.lines().into_iter().map(|(text, _)| text).collect()
    }
}
