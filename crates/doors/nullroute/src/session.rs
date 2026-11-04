//! The playable text screen: one list of lines per view, drawn on the canvas and returned as the
//! look-around view. It is the minimum that makes the engine playable over ssh; the real UI (node
//! map, run log and deck panel) is slice N3, and nothing here holds a rule: every decision is
//! made by [`Player`] and [`Run`].

use std::sync::Arc;

use russh_bbs_door::{Canvas, Ctx, DoorEnv, DoorKey, DoorSession, DoorStyle, Flow};

use crate::content::Content;
use crate::player::{JACK_IN_CYCLES, PARTS, Player};
use crate::run::{Act, Refused, Run, Status, TRACE_MAX};
use crate::save;
use crate::world::{Contract, World};

type Line = (String, DoorStyle);

/// Items shown per page in the deck and the shop.
const PAGE: usize = 12;

enum View {
    Home,
    Contracts,
    Brief(Contract),
    Deck(usize),
    Shop(usize),
    Rig,
    Hideout,
    Run(Box<Active>),
    Debrief(Vec<String>),
}

struct Active {
    run: Run,
    contract: Contract,
    /// The slot of a script waiting for its target.
    aim: Option<usize>,
}

pub(crate) struct Session {
    content: Arc<Content>,
    p: Player,
    world: World,
    season: u32,
    offers: Vec<Contract>,
    view: View,
    msgs: Vec<Line>,
    saved: Vec<u8>,
}

/// The widest line the door draws (the canvas is 72 columns at 80x24).
const WRAP: usize = 68;

/// Split `text` at spaces into lines of at most [`WRAP`] characters.
fn push_wrapped(out: &mut Vec<Line>, text: &str, style: DoorStyle) {
    let mut line = String::new();
    for word in text.split(' ') {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > WRAP {
            out.push((std::mem::take(&mut line), style));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push((line, style));
}

/// The body cut to leave room for the hint lines, then the hint lines: the keys are always shown.
fn fit(mut body: Vec<Line>, hint: Vec<Line>, rows: usize) -> Vec<Line> {
    let room = rows.saturating_sub(hint.len());
    body.truncate(room);
    body.extend(hint);
    body
}

fn letter(c: char) -> Option<usize> {
    let c = c.to_ascii_lowercase();
    c.is_ascii_lowercase().then(|| (c as u8 - b'a') as usize)
}

fn digit(c: char) -> Option<usize> {
    c.to_digit(10)
        .and_then(|d| usize::try_from(d).ok())
        .filter(|d| *d >= 1)
}

fn bar(trace: u32) -> String {
    let filled = usize::try_from(trace.min(TRACE_MAX) / 5).unwrap_or(0);
    format!(
        "[{}{}] {trace:>3}/{TRACE_MAX}",
        "#".repeat(filled),
        "-".repeat(20 - filled)
    )
}

impl Session {
    pub(crate) fn new(
        content: Arc<Content>,
        mut p: Player,
        world: World,
        season: u32,
        today: i64,
        saved: Vec<u8>,
    ) -> Self {
        let mut msgs: Vec<Line> = Vec::new();
        for note in p.rollover(&content, today) {
            msgs.push((note, DoorStyle::Accent));
        }
        let mut offers: Vec<Contract> = if world.board.is_empty() {
            crate::world::first_board(&content, today)
        } else {
            world.board.clone()
        };
        offers.retain(|c| c.expires >= today);
        // Jobs you can take first, the best pay first.
        offers.sort_by_key(|c| (p.closed(&content, c).is_some(), std::cmp::Reverse(c.reward)));
        offers.truncate(9);
        Self {
            content,
            p,
            world,
            season,
            offers,
            view: View::Home,
            msgs,
            saved,
        }
    }

    fn say(&mut self, result: Result<String, String>) {
        self.msgs = vec![match result {
            Ok(text) => (text, DoorStyle::Good),
            Err(text) => (text, DoorStyle::Bad),
        }];
    }

    fn corp_name(&self, num: u8) -> &str {
        self.content.corp(num).map_or("?", |c| c.name.as_str())
    }

    fn fixer_name(&self, num: u8) -> &str {
        self.content.fixer(num).map_or("?", |f| f.name.as_str())
    }

    fn contract_line(&self, n: usize, c: &Contract) -> Line {
        let tmpl = self.content.contract(c.tmpl);
        let text = format!(
            "{n}) {:<6} {:<21} T{} {:>4}  {}",
            tmpl.map_or("?", |t| t.verb.as_str()),
            self.corp_name(c.corp),
            c.tier,
            c.reward,
            self.fixer_name(c.fixer),
        );
        if self.p.closed(&self.content, c).is_some() {
            return (format!("{text} (closed)"), DoorStyle::Dim);
        }
        match tmpl {
            // Say so before the player walks to the briefing: the deck has no tool for it.
            Some(t)
                if !self
                    .p
                    .deck
                    .iter()
                    .filter_map(|n| self.content.script(*n))
                    .any(|s| s.effect == t.effect) =>
            {
                (format!("{text} (no {:?})", t.effect), DoorStyle::Dim)
            }
            _ => (text, DoorStyle::Plain),
        }
    }

    /// The lines of the current view, fitted to `rows` (the body is cut, never the key hints).
    fn lines(&self, rows: usize) -> Vec<Line> {
        use DoorStyle::{Accent, Dim, Plain, Strong};
        let p = &self.p;
        let c = &*self.content;
        let mut body: Vec<Line> = Vec::new();
        let mut hint: Vec<Line> = Vec::new();
        match &self.view {
            View::Run(active) => return self.run_lines(active, rows),
            View::Home => {
                body.push((format!("NULLROUTE  {}  day {}", p.name, p.day), Accent));
                body.push((
                    format!(
                        "Cycles {}/{}   Marks {} (safe {})   Heat {}   Rep {}",
                        p.cycles,
                        p.max_cycles(),
                        p.marks,
                        p.stash,
                        p.heat,
                        p.rep
                    ),
                    Plain,
                ));
                let parts: Vec<String> = (0..PARTS)
                    .map(|i| {
                        format!(
                            "{} {}{}",
                            c.rig.part.get(i).map_or("?", |d| d.name.as_str()),
                            p.rig.get(i).copied().unwrap_or(0),
                            if p.fried & (1 << i) != 0 {
                                " FRIED"
                            } else {
                                ""
                            }
                        )
                    })
                    .collect();
                push_wrapped(&mut body, &format!("Rig: {}", parts.join(", ")), Plain);
                push_wrapped(
                    &mut body,
                    &format!(
                        "Deck {}/{} slots, memory {}/{}: {}",
                        p.deck.len(),
                        p.slots(),
                        p.deck_memory(c),
                        p.memory(),
                        self.deck_names()
                    ),
                    Plain,
                );
                let skills: Vec<String> = crate::content::Family::ALL
                    .iter()
                    .map(|f| format!("{} {}", f.word(), p.skill(*f)))
                    .collect();
                push_wrapped(&mut body, &format!("Skills: {}", skills.join("  ")), Dim);
                let fixers: Vec<String> = c
                    .corps
                    .fixer
                    .iter()
                    .map(|f| {
                        format!(
                            "{} {:+}",
                            f.name,
                            p.standing.get(usize::from(f.num)).copied().unwrap_or(0)
                        )
                    })
                    .collect();
                push_wrapped(&mut body, &format!("Fixers: {}", fixers.join("  ")), Dim);
                body.push((
                    format!("Runs {}  Jobs {}  Burned {}", p.runs, p.jobs, p.burns),
                    Dim,
                ));
                body.push((String::new(), Plain));
                self.push_msgs(&mut body);
                hint.push((
                    "[c]ontracts [d]eck [s]hop [r]ig [h]ideout [l]ie low [q]uit".into(),
                    Strong,
                ));
            }
            View::Contracts => {
                body.push(("CONTRACTS".into(), Accent));
                if self.offers.is_empty() {
                    body.push(("The fixers have nothing for you today.".into(), Dim));
                }
                for (i, offer) in self.offers.iter().enumerate() {
                    body.push(self.contract_line(i + 1, offer));
                }
                self.push_msgs(&mut body);
                hint.push(("[1-9] look at a job   [Esc] back".into(), Strong));
            }
            View::Brief(contract) => {
                body = self.brief_lines(contract);
                hint.push(("[j] jack in   [p] buy a proxy   [Esc] back".into(), Strong));
            }
            View::Deck(page) => {
                body.push((
                    format!(
                        "DECK  {}/{} slots  memory {}/{}",
                        p.deck.len(),
                        p.slots(),
                        p.deck_memory(c),
                        p.memory()
                    ),
                    Accent,
                ));
                let start = page * PAGE;
                for (i, num) in p.owned.iter().enumerate().skip(start).take(PAGE) {
                    if let Some(s) = c.script(*num) {
                        let mark = if p.deck.contains(num) { '*' } else { ' ' };
                        let skill = if s.min_skill > 0 {
                            format!(" sk{}", s.min_skill)
                        } else {
                            String::new()
                        };
                        body.push((
                            format!(
                                "[{}]{mark}{:<16} {:<7} {:<8} p{} m{} n{}{skill}",
                                char::from(b'a' + u8::try_from(i - start).unwrap_or(0)),
                                s.name,
                                s.family.word(),
                                format!("{:?}", s.effect).to_lowercase(),
                                s.power,
                                s.memory,
                                s.noise,
                            ),
                            if p.deck.contains(num) { Strong } else { Plain },
                        ));
                    }
                }
                body.push((
                    "* loaded.  p power, m memory, n noise, sk skill needed".into(),
                    Dim,
                ));
                self.push_msgs(&mut body);
                hint.push((
                    "[a-l] load/unload   [n]/[p] page   [Esc] back".into(),
                    Strong,
                ));
            }
            View::Shop(page) => {
                body.push((
                    format!(
                        "SCRIPT SHOP  marks {}  market {}%",
                        p.marks, self.world.market_pct
                    ),
                    Accent,
                ));
                let start = page * PAGE;
                for (i, s) in self
                    .for_sale()
                    .into_iter()
                    .enumerate()
                    .skip(start)
                    .take(PAGE)
                {
                    let price = s.price * self.world.market_pct / 100;
                    let gate = if p.rep < s.min_rep {
                        format!(" rep {}", s.min_rep)
                    } else {
                        String::new()
                    };
                    body.push((
                        format!(
                            "[{}] {:<16} {:<7} {:<8} p{} m{} {:>4}m{gate}",
                            char::from(b'a' + u8::try_from(i - start).unwrap_or(0)),
                            s.name,
                            s.family.word(),
                            format!("{:?}", s.effect).to_lowercase(),
                            s.power,
                            s.memory,
                            price,
                        ),
                        if p.rep < s.min_rep || p.marks < price {
                            Dim
                        } else {
                            Plain
                        },
                    ));
                }
                self.push_msgs(&mut body);
                hint.push(("[a-l] buy   [n]/[p] page   [Esc] back".into(), Strong));
            }
            View::Rig => {
                body.push((format!("RIG  marks {}", p.marks), Accent));
                for i in 0..PARTS {
                    let name = c.rig.part.get(i).map_or("?", |d| d.name.as_str());
                    let lvl = usize::from(p.rig.get(i).copied().unwrap_or(0));
                    let now = if lvl == 0 {
                        "stock".to_owned()
                    } else {
                        c.rig
                            .part
                            .get(i)
                            .and_then(|d| d.levels.get(lvl - 1))
                            .map_or("?".into(), |l| l.name.clone())
                    };
                    let next = p
                        .upgrade_cost(c, i)
                        .map_or("maxed".to_owned(), |cost| format!("next {cost}"));
                    let fried = if p.fried & (1 << i) != 0 {
                        format!(" FRIED, repair {}", p.repair_cost(c, i))
                    } else {
                        String::new()
                    };
                    body.push((
                        format!("[{}] {name:<14} {now:<19} {next}{fried}", i + 1),
                        Plain,
                    ));
                }
                body.push((
                    "CPU +4 cycles. Memory +4 mem and a slot. Cooling: fewer fries.".into(),
                    Dim,
                ));
                body.push(("Interface: +5% success. Shield: 6% less trace.".into(), Dim));
                self.push_msgs(&mut body);
                hint.push((
                    "[1-5] upgrade   [x] repair a fried part   [Esc] back".into(),
                    Strong,
                ));
            }
            View::Hideout => {
                let h = &c.rig.hideout;
                body.push((
                    format!("HIDEOUT  marks {} (safe {})", p.marks, p.stash),
                    Accent,
                ));
                body.push((
                    format!(
                        "Bots: {}/{}, {} marks each a night.",
                        p.bots, h.bot_max, h.bot_income
                    ),
                    Plain,
                ));
                body.push((
                    format!(
                        "Workbench: {}",
                        if p.workbench {
                            "built (coding comes later)"
                        } else {
                            "not built"
                        }
                    ),
                    Plain,
                ));
                body.push((
                    format!(
                        "Fence: {} at {} percent.",
                        c.rig
                            .fence
                            .get(usize::from(p.fence))
                            .map_or("?", |f| f.name.as_str()),
                        c.fence_rate(p.fence)
                    ),
                    Plain,
                ));
                body.push((
                    "Heat 50 and up brings raids; the safe is never touched.".into(),
                    Dim,
                ));
                self.push_msgs(&mut body);
                hint.push((
                    format!(
                        "[b] bot {}  [w] workbench {}  [f] better fence",
                        h.bot_cost, h.workbench_cost
                    ),
                    Strong,
                ));
                hint.push((
                    format!("[s] safe +{0}  [t] take {0}  [Esc] back", h.stash_step),
                    Strong,
                ));
            }
            View::Debrief(notes) => {
                body.push(("DEBRIEF".into(), Accent));
                for n in notes {
                    push_wrapped(&mut body, n, Plain);
                }
                hint.push(("[Enter] continue".into(), Strong));
            }
        }
        fit(body, hint, rows)
    }

    fn push_msgs(&self, body: &mut Vec<Line>) {
        for (text, style) in &self.msgs {
            push_wrapped(body, text, *style);
        }
    }

    fn deck_names(&self) -> String {
        let names: Vec<&str> = self
            .p
            .deck
            .iter()
            .filter_map(|n| self.content.script(*n))
            .map(|s| s.name.as_str())
            .collect();
        if names.is_empty() {
            "(empty)".into()
        } else {
            names.join(", ")
        }
    }

    fn for_sale(&self) -> Vec<&crate::content::ScriptDef> {
        self.content
            .scripts
            .script
            .iter()
            .filter(|s| s.price > 0 && !self.p.owned.contains(&s.num))
            .collect()
    }

    fn brief_lines(&self, contract: &Contract) -> Vec<Line> {
        use DoorStyle::{Accent, Bad, Dim, Plain};
        let c = &*self.content;
        let tmpl = c.contract(contract.tmpl);
        let corp = c.corp(contract.corp);
        let mut out = vec![(
            format!(
                "{}  ({})",
                tmpl.map_or("?", |t| t.title.as_str()),
                self.fixer_name(contract.fixer)
            ),
            Accent,
        )];
        if let Some(why) = self.p.closed(c, contract) {
            push_wrapped(&mut out, &why, Bad);
        }
        if let Some(corp) = corp {
            out.push((
                format!("Target: {} (tier {})", corp.name, contract.tier),
                Plain,
            ));
            push_wrapped(&mut out, &crate::run::corp_intel(corp), Dim);
        }
        if let Some(t) = tmpl {
            push_wrapped(
                &mut out,
                &format!(
                    "Objective: {} at the {} node; the deck needs a {:?} script.",
                    t.verb, t.target, t.effect
                ),
                Plain,
            );
        }
        out.push((
            format!(
                "Pays {}.  Heat on success about +{}.",
                contract.reward,
                tmpl.map_or(0, |t| t.heat)
            ),
            Plain,
        ));
        let word = if let Some(state) = self.world.net(contract.corp, contract.tier) {
            format!(
                "Street word: alert {}, {} open hole(s){}.",
                state.alert,
                state.holes.len(),
                if state.backdoor {
                    ", a backdoor is in"
                } else {
                    ""
                }
            )
        } else {
            "Street word: nothing has been broken there lately.".to_owned()
        };
        push_wrapped(&mut out, &word, Dim);
        push_wrapped(&mut out, &format!("Deck: {}", self.deck_names()), Plain);
        push_wrapped(
            &mut out,
            &format!(
                "Proxies bought: {} (each {} marks, soaks {} trace). Jacking in costs {JACK_IN_CYCLES} cycles.",
                self.p.proxies,
                self.p.proxy_price(c),
                c.rig.hideout.proxy_buffer
            ),
            Plain,
        );
        self.push_msgs(&mut out);
        out
    }

    fn run_lines(&self, active: &Active, rows: usize) -> Vec<Line> {
        use DoorStyle::{Accent, Bad, Dim, Good, Plain, Strong};
        let c = &*self.content;
        let run = &active.run;
        let net = run.net();
        let mut body: Vec<Line> = Vec::new();
        let (target, effect) = run.objective_target();
        body.push((
            format!(
                "{} T{}  {:?} at {target}{}",
                self.corp_name(net.corp),
                net.tier,
                effect,
                if run.objective_done() {
                    "  DONE: jack out"
                } else {
                    ""
                }
            ),
            Accent,
        ));
        body.push((
            format!(
                "TRACE {}  cycles {}  soak {}",
                bar(run.trace),
                run.cycles,
                run.buffer()
            ),
            if run.trace >= 70 { Bad } else { Plain },
        ));
        let mut flags = String::new();
        if run.feed_live() {
            flags.push_str("[CAMERA LIVE] ");
        }
        if run.tracer() {
            flags.push_str("[TRACER] ");
        }
        if run.alarm() {
            flags.push_str("[ALARM] ");
        }
        if !flags.is_empty() {
            body.push((flags.trim_end().to_owned(), Bad));
        }
        body.push((format!("At {}", run.describe(c, run.at)), Strong));
        let exits = run.exits();
        for (i, id) in exits.iter().enumerate().take(7) {
            body.push((
                format!("  {}) {}", i + 1, run.describe(c, *id)),
                if run.is_open(*id) { Plain } else { Dim },
            ));
        }
        if exits.is_empty() {
            body.push(("  no known exits: scan for hidden links".into(), Dim));
        }
        let elsewhere: Vec<&str> = net
            .nodes
            .iter()
            .filter(|n| run.is_known(n.id) && n.id != run.at && !exits.contains(&n.id))
            .map(|n| n.name.as_str())
            .collect();
        if !elsewhere.is_empty() {
            push_wrapped(
                &mut body,
                &format!("Known elsewhere: {}", elsewhere.join(" ")),
                Dim,
            );
        }
        let mut deck: Vec<String> = run
            .deck
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let s = c.script(slot.script);
                let left = if slot.left == u32::MAX {
                    String::new()
                } else {
                    format!(" x{}", slot.left)
                };
                format!(
                    "[{}] {} p{}{left}",
                    char::from(b'a' + u8::try_from(i).unwrap_or(0)),
                    s.map_or("?", |s| s.name.as_str()),
                    s.map_or(0, |s| s.power)
                )
            })
            .collect();
        if deck.len() % 2 == 1 {
            deck.push(String::new());
        }
        for pair in deck.chunks(2) {
            body.push((
                format!("{:<34}{}", pair[0], pair.get(1).map_or("", String::as_str)),
                Good,
            ));
        }
        let hint = match active.aim {
            Some(slot) => format!(
                "Aim {}: 1-9 picks the node, Esc cancels",
                run.deck
                    .get(slot)
                    .and_then(|s| c.script(s.script))
                    .map_or("?", |s| s.name.as_str())
            ),
            None => "[1-9] move   [a-f] script   [o] jack out".to_owned(),
        };
        // The log fills whatever room is left.
        let room = rows.saturating_sub(body.len() + 1).min(run.log.len());
        for line in run.log.iter().skip(run.log.len() - room) {
            let mut wrapped = Vec::new();
            push_wrapped(&mut wrapped, &format!("> {line}"), Dim);
            body.extend(wrapped.into_iter().take(1));
        }
        fit(body, vec![(hint, Strong)], rows)
    }

    fn open_brief(&mut self, index: usize) {
        if let Some(c) = self.offers.get(index).copied() {
            self.msgs.clear();
            self.view = View::Brief(c);
        }
    }

    fn finish_run(&mut self, ctx: &mut Ctx<'_>, active: &Active) {
        let result = active.run.result();
        let contract = active.contract;
        let mut notes: Vec<String> = active.run.log.iter().rev().take(6).rev().cloned().collect();
        notes.push(String::new());
        notes.extend(
            self.p
                .finish_run(&self.content, &contract, &result, ctx.rng()),
        );
        let (corp, tier) = (contract.corp, contract.tier);
        self.world.apply_run(corp, tier, &result);
        let applied = result.clone();
        ctx.edit_world(move |old| {
            let mut world = old.and_then(save::decode_world).unwrap_or_default();
            world.apply_run(corp, tier, &applied);
            Some(save::encode_world(&world))
        });
        self.view = View::Debrief(notes);
    }

    fn run_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let View::Run(active) = &mut self.view else {
            return;
        };
        let content = Arc::clone(&self.content);
        let act = match (key, active.aim) {
            (DoorKey::Escape, Some(_)) => {
                active.aim = None;
                return;
            }
            (DoorKey::Char(ch), Some(slot)) => digit(ch)
                .and_then(|n| active.run.exits().get(n - 1).copied())
                .map(|t| {
                    active.aim = None;
                    Act::Use {
                        slot,
                        target: Some(t),
                    }
                }),
            (DoorKey::Char(ch), None) => {
                if let Some(n) = digit(ch) {
                    active.run.exits().get(n - 1).copied().map(Act::Move)
                } else if matches!(ch, 'o' | 'O') {
                    Some(Act::JackOut)
                } else if let Some(slot) = letter(ch).filter(|s| *s < active.run.deck.len()) {
                    let needs = active
                        .run
                        .deck
                        .get(slot)
                        .and_then(|s| content.script(s.script))
                        .is_some_and(|s| s.effect.needs_target());
                    if needs {
                        active.aim = Some(slot);
                        None
                    } else {
                        Some(Act::Use { slot, target: None })
                    }
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some(act) = act else { return };
        if let Err(refused) = active.run.act(&content, act, ctx.rng()) {
            let line = refused.text().to_owned();
            active.run.log.push(line);
            if matches!(refused, Refused::NoCycles) && active.run.status == Status::Active {
                // The player can still jack out; nothing else to do.
            }
        }
        if active.run.status != Status::Active {
            let View::Run(finished) = std::mem::replace(&mut self.view, View::Home) else {
                return;
            };
            self.finish_run(ctx, &finished);
        }
    }

    fn menu_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        let c = Arc::clone(&self.content);
        match (&self.view, key) {
            (View::Home, DoorKey::Char(ch)) => match ch.to_ascii_lowercase() {
                'c' => {
                    self.msgs.clear();
                    self.view = View::Contracts;
                }
                'd' => {
                    self.msgs.clear();
                    self.view = View::Deck(0);
                }
                's' => {
                    self.msgs.clear();
                    self.view = View::Shop(0);
                }
                'r' => {
                    self.msgs.clear();
                    self.view = View::Rig;
                }
                'h' => {
                    self.msgs.clear();
                    self.view = View::Hideout;
                }
                'l' => {
                    let r = self.p.lay_low(&c);
                    self.say(r);
                }
                'q' => return Flow::Exit,
                _ => {}
            },
            (View::Home, DoorKey::Escape) => return Flow::Exit,
            (View::Contracts, DoorKey::Char(ch)) => {
                if let Some(n) = digit(ch) {
                    self.open_brief(n - 1);
                }
            }
            (View::Brief(contract), DoorKey::Char(ch)) => {
                let contract = *contract;
                match ch.to_ascii_lowercase() {
                    'p' => {
                        let r = self.p.buy_proxy(&c);
                        self.say(r);
                    }
                    'j' => match self.p.begin_run(&c, self.season, &self.world, &contract) {
                        Ok(run) => {
                            self.msgs.clear();
                            self.view = View::Run(Box::new(Active {
                                run,
                                contract,
                                aim: None,
                            }));
                        }
                        Err(why) => self.say(Err(why)),
                    },
                    _ => {}
                }
            }
            (View::Deck(page), DoorKey::Char(ch)) => {
                let page = *page;
                match ch.to_ascii_lowercase() {
                    'n' if (page + 1) * PAGE < self.p.owned.len() => {
                        self.view = View::Deck(page + 1);
                    }
                    'p' if page > 0 => self.view = View::Deck(page - 1),
                    other => {
                        if let Some(num) = letter(other)
                            .filter(|i| *i < PAGE)
                            .and_then(|i| self.p.owned.get(page * PAGE + i).copied())
                        {
                            let r = if self.p.deck.contains(&num) {
                                self.p.unload(&c, num)
                            } else {
                                self.p.load(&c, num)
                            };
                            self.say(r);
                        }
                    }
                }
            }
            (View::Shop(page), DoorKey::Char(ch)) => {
                let page = *page;
                let for_sale: Vec<u8> = self.for_sale().iter().map(|s| s.num).collect();
                match ch.to_ascii_lowercase() {
                    'n' if (page + 1) * PAGE < for_sale.len() => self.view = View::Shop(page + 1),
                    'p' if page > 0 => self.view = View::Shop(page - 1),
                    other => {
                        if let Some(num) = letter(other)
                            .filter(|i| *i < PAGE)
                            .and_then(|i| for_sale.get(page * PAGE + i).copied())
                        {
                            let r = self.p.buy_script(&c, num, self.world.market_pct);
                            self.say(r);
                        }
                    }
                }
            }
            (View::Rig, DoorKey::Char(ch)) => {
                if let Some(n) = digit(ch).filter(|n| *n <= PARTS) {
                    let r = self.p.upgrade(&c, n - 1);
                    self.say(r);
                } else if matches!(ch, 'x' | 'X') {
                    let r = match (0..PARTS).find(|i| self.p.fried & (1 << i) != 0) {
                        Some(part) => self.p.repair(&c, part),
                        None => Err("Nothing is fried.".into()),
                    };
                    self.say(r);
                }
            }
            (View::Hideout, DoorKey::Char(ch)) => {
                let r = match ch.to_ascii_lowercase() {
                    'b' => Some(self.p.buy_bot(&c)),
                    'w' => Some(self.p.buy_workbench(&c)),
                    'f' => Some(self.p.upgrade_fence(&c)),
                    's' => Some(self.p.move_stash(&c, true)),
                    't' => Some(self.p.move_stash(&c, false)),
                    _ => None,
                };
                if let Some(r) = r {
                    self.say(r);
                }
            }
            (View::Contracts | View::Brief(_), DoorKey::Escape) => {
                let back = matches!(self.view, View::Brief(_));
                self.msgs.clear();
                self.view = if back { View::Contracts } else { View::Home };
            }
            (View::Deck(_) | View::Shop(_) | View::Rig | View::Hideout, DoorKey::Escape)
            | (View::Debrief(_), DoorKey::Enter | DoorKey::Escape | DoorKey::Char(' ')) => {
                self.msgs.clear();
                self.view = View::Home;
            }
            _ => {}
        }
        let _ = ctx;
        Flow::Stay
    }
}

impl DoorSession for Session {
    fn key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        let day = ctx.today().as_raw();
        if day > self.p.day && matches!(self.view, View::Home | View::Contracts) {
            for note in self.p.rollover(&self.content, day) {
                self.msgs.push((note, DoorStyle::Accent));
            }
        }
        let flow = if matches!(self.view, View::Run(_)) {
            self.run_key(key, ctx);
            Flow::Stay
        } else {
            self.menu_key(key, ctx)
        };
        let bytes = save::encode(&self.p);
        if bytes != self.saved {
            self.saved.clone_from(&bytes);
            ctx.request_save(bytes);
        }
        flow
    }

    fn render(&self, canvas: &mut Canvas<'_>, _env: &DoorEnv) {
        canvas.clear();
        for (y, (text, style)) in self.lines(usize::from(canvas.height())).iter().enumerate() {
            let Ok(y) = u16::try_from(y) else { break };
            canvas.text(1, y, text, *style);
        }
    }

    fn look_around(&self, _env: &DoorEnv) -> Vec<String> {
        self.lines(usize::MAX / 4)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }
}
