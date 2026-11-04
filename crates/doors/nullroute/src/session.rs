//! The playable text screen: one list of lines per view, drawn on the canvas and returned as the
//! look-around view. It is the minimum that makes the engine playable over ssh; the real UI (node
//! map, run log and deck panel) is slice N3, and nothing here holds a rule: every decision is
//! made by [`Player`] and [`Run`].

use std::fmt::Write as _;
use std::sync::Arc;

use russh_bbs_door::{Canvas, Ctx, DoorEnv, DoorKey, DoorSession, DoorStyle, Flow};

use crate::code::{self, Build, CODE_CYCLES, MEMORY_BUDGET, NOISE_BUDGET};
use crate::content::{Content, FragKind};
use crate::learn::{self, Stage};
use crate::player::{JACK_IN_CYCLES, PARTS, Player};
use crate::run::{Act, Refused, Run, RunResult, Status, TRACE_MAX};
use crate::save;
use crate::world::{Contract, World};

mod ui;

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
    Code(Draft),
    Run(Box<Active>),
    Debrief(Box<Report>),
    Fixers,
    Heat,
    Profile,
    Black,
}

/// How a run ended: the log's tail and the debrief notes for the look-around view, and the result
/// for the debrief screen.
struct Report {
    notes: Vec<String>,
    result: RunResult,
    contract: Contract,
}

/// The run log with the time (cycles spent) each line was written, kept by the screen so the log
/// can scroll with stamps. The engine's log is the source of the text; this only remembers when
/// each line first appeared.
struct Feed {
    start: u32,
    rows: Vec<(u32, String)>,
}

impl Feed {
    fn new(run: &Run) -> Self {
        let mut feed = Self {
            start: run.cycles,
            rows: Vec::new(),
        };
        feed.sync(run);
        feed
    }

    /// Bring the feed up to the engine's log: keep the stamps of lines still in it, stamp new ones
    /// with the cycles spent so far.
    fn sync(&mut self, run: &Run) {
        let now = self.start.saturating_sub(run.cycles);
        let have = self.rows.len();
        let n = run.log.len();
        let kept = (0..=have.min(n))
            .rev()
            .find(|k| (0..*k).all(|i| self.rows[have - k + i].1 == run.log[i]))
            .unwrap_or(0);
        self.rows.drain(..have - kept);
        for line in &run.log[kept..] {
            self.rows.push((now, line.clone()));
        }
    }
}

/// Where the module editor is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CodeStep {
    Effect,
    Target,
    Modifier,
    Confirm,
}

/// A build being put together at the workbench.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Draft {
    step: CodeStep,
    build: Build,
}

struct Active {
    run: Run,
    contract: Contract,
    /// The slot of a script waiting for its target.
    aim: Option<usize>,
    /// The log with its time stamps.
    feed: Feed,
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
    /// The cursor on a list screen (the row within the page).
    sel: usize,
}

fn run_has_fight(run: &Run) -> bool {
    run.encounter().is_some()
}

/// What heat does, from the engine's thresholds (`player::HEAT_*`), for the heat screen.
const HEAT_LADDER: &[(u32, &str)] = &[
    (crate::player::HEAT_PROXY, "proxies cost half as much again"),
    (
        crate::player::HEAT_RAID,
        "your hideout can be raided (the safe never is)",
    ),
    (crate::player::HEAT_TIER3, "tier 3 fixers stay away"),
    (crate::player::HEAT_BOTS, "your bots earn half"),
    (crate::player::HEAT_TIER2, "tier 2 fixers stay away"),
];

/// What a kind of machine is called in the plural ("tuned to sentries").
const fn plural(kind: crate::content::CmKind) -> &'static str {
    match kind {
        crate::content::CmKind::Sentry => "sentries",
        crate::content::CmKind::Tracer => "tracers",
        crate::content::CmKind::Wall => "black walls",
        crate::content::CmKind::Warden => "wardens",
    }
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
            sel: 0,
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
        use DoorStyle::{Accent, Bad, Dim, Plain, Strong};
        let p = &self.p;
        let c = &*self.content;
        let mut body: Vec<Line> = Vec::new();
        let mut hint: Vec<Line> = Vec::new();
        match &self.view {
            View::Run(active) => return self.run_lines(active, rows),
            View::Code(draft) => {
                let body = self.code_lines(*draft);
                let hint = match draft.step {
                    CodeStep::Confirm => "[Enter] code it   [Esc] back",
                    CodeStep::Effect => "[1-9] pick   [Esc] back",
                    _ => "[1-9] pick   [0] none   [Esc] back",
                };
                return fit(body, vec![(hint.into(), Strong)], rows);
            }
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
                    "[c]ontracts [f]ixers [d]eck [s]hop [b]lack market [r]ig".into(),
                    Strong,
                ));
                hint.push((
                    "[h]ideout [p]rofile [t]heat [l]ie low [q]uit".into(),
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
                    if let Some(s) = p.script(c, *num) {
                        let mark = if p.deck.contains(num) { '*' } else { ' ' };
                        let skill = if s.min_skill > 0 {
                            format!(" sk{}", s.min_skill)
                        } else {
                            String::new()
                        };
                        body.push((
                            format!(
                                "[{}]{mark}{:<16} {:<7} {:<8} p{} m{} n{}{skill}{}",
                                char::from(b'a' + u8::try_from(i - start).unwrap_or(0)),
                                if s.name.chars().count() > 16 {
                                    s.name.chars().take(16).collect::<String>()
                                } else {
                                    s.name.clone()
                                },
                                s.family.word(),
                                format!("{:?}", s.effect).to_lowercase(),
                                s.power,
                                s.memory,
                                s.noise,
                                s.target
                                    .map_or_else(String::new, |t| format!(" vs {}", t.word())),
                            ),
                            if p.deck.contains(num) { Strong } else { Plain },
                        ));
                    }
                }
                body.push((
                    "* loaded.  p power, m memory, n noise, sk skill needed".into(),
                    Dim,
                ));
                push_wrapped(&mut body, &self.stock_row(), Dim);
                self.push_msgs(&mut body);
                hint.push((
                    "[a-l] load/unload   [A-L] scrap a coded script   [n]/[p] page   [Esc] back"
                        .into(),
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
                            format!("built; {} fragment(s) in the case", p.fragments.len())
                        } else {
                            "not built".to_owned()
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
                        "[b] bot {}  [w] workbench {}  [c] code  [f] fence",
                        h.bot_cost, h.workbench_cost
                    ),
                    Strong,
                ));
                hint.push((
                    format!("[s] safe +{0}  [t] take {0}  [Esc] back", h.stash_step),
                    Strong,
                ));
            }
            View::Fixers => {
                body.push(("STANDINGS".into(), Accent));
                for f in &c.corps.fixer {
                    let standing = p.standing.get(usize::from(f.num)).copied().unwrap_or(0);
                    let clients: Vec<&str> = c
                        .corps
                        .corp
                        .iter()
                        .filter(|k| k.patron == f.num)
                        .map(|k| k.name.as_str())
                        .collect();
                    body.push((
                        format!(
                            "{:<14} {standing:+}  {}  clients: {}",
                            f.name,
                            if standing <= -3 {
                                "will not deal"
                            } else {
                                "deals"
                            },
                            if clients.is_empty() {
                                "none".to_owned()
                            } else {
                                clients.join(", ")
                            }
                        ),
                        if standing <= -3 { Dim } else { Plain },
                    ));
                }
                push_wrapped(
                    &mut body,
                    "A job raises the hirer's standing and lowers the target's patron. At -3 a fixer will not deal with you; tier 3 wants standing 2.",
                    Dim,
                );
                hint.push(("[Up]/[Down] choose   [Esc] back".into(), Strong));
            }
            View::Heat => {
                body.push((
                    format!("HEAT  {}/{}", p.heat, crate::player::HEAT_MAX),
                    Accent,
                ));
                for (at, what) in HEAT_LADDER {
                    body.push((
                        format!(
                            "{at:>3}: {what}{}",
                            if p.heat >= *at { "  (now)" } else { "" }
                        ),
                        if p.heat >= *at { Bad } else { Dim },
                    ));
                }
                push_wrapped(
                    &mut body,
                    &format!(
                        "Heat falls {} a night; lying low costs {} cycles for {} heat.",
                        c.rig.hideout.heat_decay,
                        c.rig.hideout.lay_low_cycles,
                        c.rig.hideout.lay_low_heat
                    ),
                    Dim,
                );
                self.push_msgs(&mut body);
                hint.push(("[l] lie low   [Esc] back".into(), Strong));
            }
            View::Profile => {
                body.push((
                    format!("CHARACTER  {}  day {}  season {}", p.name, p.day, p.season),
                    Accent,
                ));
                body.push((
                    format!(
                        "Reputation {}   Marks {} (safe {})   Earned {}",
                        p.rep, p.marks, p.stash, p.earned
                    ),
                    Plain,
                ));
                body.push((
                    format!("Runs {}   Jobs {}   Burned {}", p.runs, p.jobs, p.burns),
                    Plain,
                ));
                for f in crate::content::Family::ALL {
                    let xp = p.xp.get(f.index()).copied().unwrap_or(0);
                    body.push((
                        format!("{:<8} skill {}  uses {xp}", f.word(), p.skill(f)),
                        Plain,
                    ));
                }
                hint.push(("[Esc] back".into(), Strong));
            }
            View::Black => {
                let h = &c.rig.hideout;
                body.push((
                    format!("BLACK MARKET  marks {} (safe {})", p.marks, p.stash),
                    Accent,
                ));
                for (i, f) in c.rig.fence.iter().enumerate() {
                    body.push((
                        format!(
                            "Fence {}: {} at {} percent{}{}",
                            i,
                            f.name,
                            f.rate,
                            if usize::from(p.fence) == i {
                                "  (yours)"
                            } else {
                                ""
                            },
                            if usize::from(p.fence) + 1 == i {
                                format!("  next costs {}", f.cost)
                            } else {
                                String::new()
                            }
                        ),
                        if usize::from(p.fence) == i {
                            Strong
                        } else {
                            Plain
                        },
                    ));
                }
                body.push((
                    format!(
                        "Proxies: {}/{} bought, {} marks each, each soaks {} trace.",
                        p.proxies,
                        h.proxy_max,
                        p.proxy_price(c),
                        h.proxy_buffer
                    ),
                    Plain,
                ));
                self.push_msgs(&mut body);
                hint.push((
                    format!(
                        "[f] better fence   [p] proxy   [s] safe +{0}   [t] take {0}",
                        h.stash_step
                    ),
                    Strong,
                ));
                hint.push(("[Esc] back".into(), Strong));
            }
            View::Debrief(report) => {
                body.push(("DEBRIEF".into(), Accent));
                for n in &report.notes {
                    push_wrapped(&mut body, n, Plain);
                }
                hint.push(("[Enter] continue".into(), Strong));
            }
        }
        fit(body, hint, rows)
    }

    /// Fragments of one kind in the case, with how many of each, in table order.
    fn fragment_rows(&self, kind: FragKind) -> Vec<(u8, usize)> {
        self.content
            .fragments
            .fragment
            .iter()
            .filter(|f| f.kind == kind)
            .map(|f| (f.num, self.p.fragment_count(f.num)))
            .filter(|(_, n)| *n > 0)
            .collect()
    }

    fn code_lines(&self, d: Draft) -> Vec<Line> {
        use DoorStyle::{Accent, Bad, Dim, Good, Plain};
        let c = &*self.content;
        let mut out = vec![(
            format!("WORKBENCH  budget: memory {MEMORY_BUDGET}, noise {NOISE_BUDGET}"),
            Accent,
        )];
        push_wrapped(
            &mut out,
            &format!("Coding burns the fragments and {CODE_CYCLES} cycles."),
            Dim,
        );
        let kind = match d.step {
            CodeStep::Effect | CodeStep::Confirm => FragKind::Effect,
            CodeStep::Target => FragKind::Target,
            CodeStep::Modifier => FragKind::Modifier,
        };
        if d.build.effect != 0 {
            let name = code::name(c, d.build);
            match code::stats(c, d.build) {
                Ok(s) => out.push((
                    format!(
                        "{name}: power {}, memory {}/{MEMORY_BUDGET}, noise {}/{NOISE_BUDGET}, cycles {}",
                        s.power, s.memory, s.noise, s.cycles
                    ),
                    if s.memory > MEMORY_BUDGET || s.noise > NOISE_BUDGET {
                        Bad
                    } else {
                        Good
                    },
                )),
                Err(why) => out.push((why, Bad)),
            }
        }
        if d.step == CodeStep::Confirm {
            match code::assemble(c, d.build, crate::code::CODED_BASE) {
                Ok(s) => {
                    let tuned = s
                        .target
                        .map_or_else(String::new, |t| format!(", tuned to {}", plural(t)));
                    push_wrapped(
                        &mut out,
                        &format!(
                            "Ready: {}{tuned}. A corporation learns a recipe, so vary it.",
                            s.name
                        ),
                        Plain,
                    );
                }
                Err(why) => out.push((why, Bad)),
            }
        } else {
            {
                let title = match d.step {
                    CodeStep::Effect => "Pick an EFFECT",
                    CodeStep::Target => "Pick a TARGET type (0 for none)",
                    _ => "Pick a MODIFIER (0 for none)",
                };
                out.push((title.into(), Plain));
                let rows = self.fragment_rows(kind);
                if rows.is_empty() {
                    out.push(("  none in the case: find fragments on runs.".into(), Dim));
                }
                for (i, (num, n)) in rows.iter().enumerate().take(9) {
                    if let Some(f) = c.fragment(*num) {
                        out.push((
                            format!(
                                "  {}) {:<14} x{n}  p{:+} m{:+} n{:+}  {}",
                                i + 1,
                                f.name,
                                f.power,
                                f.memory,
                                f.noise,
                                f.blurb
                            ),
                            Plain,
                        ));
                    }
                }
            }
        }
        out.extend(self.msgs.iter().cloned());
        out
    }

    /// A line of the corporations' memory of the scripts, for the deck screen.
    fn stock_row(&self) -> String {
        let n = self.world.learned.len();
        if n == 0 {
            "No corporation has learned any of your scripts yet.".to_owned()
        } else {
            format!(
                "Corporations are learning {n} script(s): the brief for a job shows what they know."
            )
        }
    }

    fn push_msgs(&self, body: &mut Vec<Line>) {
        for (text, style) in &self.msgs {
            push_wrapped(body, text, *style);
        }
    }

    fn deck_names(&self) -> String {
        let names: Vec<String> = self
            .p
            .deck
            .iter()
            .filter_map(|n| self.p.script(&self.content, *n))
            .map(|s| s.name)
            .collect();
        if names.is_empty() {
            "(empty)".into()
        } else {
            names.join(", ")
        }
    }

    /// The scripts of the loaded deck that the corporation is wary of or has patched.
    fn known_scripts(&self, corp: u8) -> Vec<String> {
        let learned = self.world.learned_by(corp);
        self.p
            .deck
            .iter()
            .filter_map(|n| self.p.script(&self.content, *n))
            .filter_map(|s| {
                let uses = learned
                    .iter()
                    .find(|(sig, _)| *sig == s.sig)
                    .map_or(0, |(_, n)| *n);
                match learn::stage(&s, uses) {
                    Stage::Fresh => None,
                    Stage::Wary => Some(format!("{} (wary)", s.name)),
                    Stage::Patched => Some(format!("{} (PATCHED: will not work)", s.name)),
                }
            })
            .collect()
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
        let known = self.known_scripts(contract.corp);
        if !known.is_empty() {
            push_wrapped(&mut out, &format!("They know: {}", known.join(", ")), Bad);
        }
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
        if run_has_fight(&active.run) {
            return self.fight_lines(active, rows);
        }
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
        if let Some(w) = run.warden().filter(|w| w.awake && !w.down) {
            let _ = write!(flags, "[WARDEN HUNTING: {} moves] ", w.arrives_in);
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
                let s = run.script(c, slot.script);
                let left = if slot.left == u32::MAX {
                    String::new()
                } else {
                    format!(" x{}", slot.left)
                };
                let known = match run.stage(c, i) {
                    Stage::Fresh => "",
                    Stage::Wary => " wary",
                    Stage::Patched => " PATCHED",
                };
                format!(
                    "[{}] {} p{}{left}{known}",
                    char::from(b'a' + u8::try_from(i).unwrap_or(0)),
                    s.as_ref().map_or("?", |s| s.name.as_str()),
                    s.as_ref().map_or(0, |s| s.power)
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
                    .and_then(|s| run.script(c, s.script))
                    .map_or_else(|| "?".to_owned(), |s| s.name)
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

    /// The screen of an exchange with a countermeasure: its integrity, what it is about to do, what
    /// is known of its pattern, and what each script in the deck would do to it right now.
    fn fight_lines(&self, active: &Active, rows: usize) -> Vec<Line> {
        use DoorStyle::{Accent, Bad, Dim, Good, Plain, Strong};
        let c = &*self.content;
        let run = &active.run;
        let Some(e) = run.encounter() else {
            return Vec::new();
        };
        let mut body: Vec<Line> = Vec::new();
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
        if run.tracer() {
            flags.push_str("[TRACER] ");
        }
        if run.alarm() {
            flags.push_str("[ALARM] ");
        }
        if run.feed_live() {
            flags.push_str("[CAMERA LIVE] ");
        }
        if !flags.is_empty() {
            body.push((flags.trim_end().to_owned(), Bad));
        }
        let filled = usize::try_from(e.integrity * 20 / e.max.max(1)).unwrap_or(0);
        body.push((
            format!(
                "{} {}  [{}{}] {}/{}  armor {}  weak to {} locks",
                e.kind.word().to_uppercase(),
                e.name,
                "#".repeat(filled),
                "-".repeat(20usize.saturating_sub(filled)),
                e.integrity,
                e.max,
                e.armor_now(),
                e.weak.word()
            ),
            Strong,
        ));
        let intent = e.intent();
        body.push((
            format!(
                "It will {} ({})    move {}/{}",
                intent.word(),
                intent.gloss(),
                e.round + 1,
                crate::cm::ROUNDS_MAX
            ),
            Accent,
        ));
        let forecast = e.forecast(3);
        if forecast.is_empty() {
            let seen: Vec<&str> = e.seen.iter().map(|i| i.word()).collect();
            push_wrapped(
                &mut body,
                &format!(
                    "Pattern unknown. Seen: {}. Watch a full cycle of {} or scan it.",
                    if seen.is_empty() {
                        "nothing yet".to_owned()
                    } else {
                        seen.join(" ")
                    },
                    e.pattern.len()
                ),
                Dim,
            );
        } else {
            let upcoming: Vec<&str> = forecast.iter().map(|i| i.word()).collect();
            body.push((format!("Then: {}", upcoming.join(", ")), Dim));
        }
        let mut notes = Vec::new();
        if e.stun > 0 {
            notes.push("it is blind");
        }
        if e.charged {
            notes.push("the next strike is DOUBLE");
        }
        if let Some(f) = e.sealed {
            notes.push(if f == crate::content::Family::Break {
                "Break is sealed"
            } else {
                "a family is sealed"
            });
        }
        if !notes.is_empty() {
            body.push((notes.join("; "), Bad));
        }
        for (i, slot) in run.deck.iter().enumerate() {
            let s = run.script(c, slot.script);
            let stage = run.stage(c, i);
            let dmg = run.preview(c, i).unwrap_or(0);
            let left = if slot.left == u32::MAX {
                String::new()
            } else {
                format!(" x{}", slot.left)
            };
            let sealed = s.as_ref().is_some_and(|s| e.sealed == Some(s.family));
            let tag = if stage == Stage::Patched {
                "  PATCHED"
            } else if sealed {
                "  SEALED"
            } else if stage == Stage::Wary {
                "  wary"
            } else {
                ""
            };
            body.push((
                format!(
                    "[{}] {:<18} -> {} dmg{left}{tag}",
                    char::from(b'a' + u8::try_from(i).unwrap_or(0)),
                    s.as_ref().map_or("?", |s| s.name.as_str()),
                    dmg
                ),
                if tag.is_empty() { Good } else { Dim },
            ));
        }
        let room = rows.saturating_sub(body.len() + 1).min(run.log.len());
        for line in run.log.iter().skip(run.log.len() - room) {
            let mut wrapped = Vec::new();
            push_wrapped(&mut wrapped, &format!("> {line}"), Dim);
            body.extend(wrapped.into_iter().take(1));
        }
        fit(
            body,
            vec![(
                "[a-f] play a script   [w] brace (1 cycle)   [x] break off".into(),
                Strong,
            )],
            rows,
        )
    }

    /// The plain text drawing, for a window too small for the panels.
    fn draw_text(&self, canvas: &mut Canvas<'_>) {
        for (y, (text, style)) in self.lines(usize::from(canvas.height())).iter().enumerate() {
            let Ok(y) = u16::try_from(y) else { break };
            canvas.text(1, y, text, *style);
        }
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
        let (corp, tier, season) = (contract.corp, contract.tier, self.season);
        self.world.apply_run(corp, tier, &result);
        let applied = result.clone();
        ctx.edit_world(move |old| {
            let mut world = old.and_then(save::decode_world).unwrap_or_default();
            world.sync_season(season);
            world.apply_run(corp, tier, &applied);
            Some(save::encode_world(&world))
        });
        self.view = View::Debrief(Box::new(Report {
            notes,
            result,
            contract,
        }));
    }

    fn run_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let View::Run(active) = &mut self.view else {
            return;
        };
        let content = Arc::clone(&self.content);
        if active.run.encounter().is_some() {
            active.aim = None;
            let act = match key {
                DoorKey::Char('w' | 'W') => Some(Act::Brace),
                DoorKey::Char('x' | 'X') => Some(Act::BreakOff),
                DoorKey::Char(ch) => letter(ch)
                    .filter(|s| *s < active.run.deck.len())
                    .map(|slot| Act::Use { slot, target: None }),
                _ => None,
            };
            if let Some(act) = act
                && let Err(refused) = active.run.act(&content, act, ctx.rng())
            {
                let line = refused.text().to_owned();
                active.run.log.push(line);
            }
            active.feed.sync(&active.run);
            if active.run.status != Status::Active {
                let View::Run(finished) = std::mem::replace(&mut self.view, View::Home) else {
                    return;
                };
                self.finish_run(ctx, &finished);
            }
            return;
        }
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
        active.feed.sync(&active.run);
        if active.run.status != Status::Active {
            let View::Run(finished) = std::mem::replace(&mut self.view, View::Home) else {
                return;
            };
            self.finish_run(ctx, &finished);
        }
    }

    /// Which screen and page the menu is on, so the cursor can start at the top of a new one.
    fn place(&self) -> (u8, usize) {
        match &self.view {
            View::Home => (0, 0),
            View::Contracts => (1, 0),
            View::Brief(_) => (2, 0),
            View::Deck(page) => (3, *page),
            View::Shop(page) => (4, *page),
            View::Rig => (5, 0),
            View::Hideout => (6, 0),
            View::Code(_) => (7, 0),
            View::Run(_) => (8, 0),
            View::Debrief(_) => (9, 0),
            View::Fixers => (10, 0),
            View::Heat => (11, 0),
            View::Profile => (12, 0),
            View::Black => (13, 0),
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
                'f' => {
                    self.msgs.clear();
                    self.view = View::Fixers;
                }
                'b' => {
                    self.msgs.clear();
                    self.view = View::Black;
                }
                'p' => {
                    self.msgs.clear();
                    self.view = View::Profile;
                }
                't' => {
                    self.msgs.clear();
                    self.view = View::Heat;
                }
                'l' => {
                    let r = self.p.lay_low(&c);
                    self.say(r);
                }
                'q' => return Flow::Exit,
                _ => {}
            },
            (View::Home, DoorKey::Escape) => return Flow::Exit,
            (View::Contracts | View::Fixers | View::Deck(_) | View::Shop(_), DoorKey::Up) => {
                self.sel = self.sel.saturating_sub(1);
            }
            (View::Contracts, DoorKey::Down) => {
                self.sel = (self.sel + 1).min(self.offers.len().saturating_sub(1));
            }
            (View::Contracts, DoorKey::Enter) => self.open_brief(self.sel),
            (View::Fixers, DoorKey::Down) => {
                self.sel = (self.sel + 1).min(c.corps.fixer.len().saturating_sub(1));
            }
            (View::Deck(_) | View::Shop(_), DoorKey::Down) => {
                self.sel = (self.sel + 1).min(PAGE - 1);
            }
            // Enter does what the letter beside the cursor row does.
            (View::Deck(_) | View::Shop(_), DoorKey::Enter) => {
                let letter = char::from(b'a' + u8::try_from(self.sel.min(PAGE - 1)).unwrap_or(0));
                return self.menu_key(DoorKey::Char(letter), ctx);
            }
            (View::Heat, DoorKey::Char('l' | 'L')) => {
                let r = self.p.lay_low(&c);
                self.say(r);
            }
            (View::Black, DoorKey::Char(ch)) => {
                let r = match ch.to_ascii_lowercase() {
                    'f' => Some(self.p.upgrade_fence(&c)),
                    'p' => Some(self.p.buy_proxy(&c)),
                    's' => Some(self.p.move_stash(&c, true)),
                    't' => Some(self.p.move_stash(&c, false)),
                    _ => None,
                };
                if let Some(r) = r {
                    self.say(r);
                }
            }
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
                            let feed = Feed::new(&run);
                            self.view = View::Run(Box::new(Active {
                                run,
                                contract,
                                aim: None,
                                feed,
                            }));
                        }
                        Err(why) => self.say(Err(why)),
                    },
                    _ => {}
                }
            }
            (View::Deck(page), DoorKey::Char(ch)) => {
                let page = *page;
                if ch.is_ascii_uppercase()
                    && let Some(num) = letter(ch)
                        .filter(|i| *i < PAGE)
                        .and_then(|i| self.p.owned.get(page * PAGE + i).copied())
                {
                    let r = self.p.scrap(num);
                    self.say(r);
                    return Flow::Stay;
                }
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
            (View::Hideout, DoorKey::Char('c' | 'C')) => {
                if self.p.workbench {
                    self.msgs.clear();
                    self.view = View::Code(Draft {
                        step: CodeStep::Effect,
                        build: Build {
                            effect: 0,
                            target: 0,
                            modifier: 0,
                        },
                    });
                } else {
                    self.say(Err("Build the workbench first.".into()));
                }
            }
            (View::Code(draft), DoorKey::Char(ch)) => {
                let mut d = *draft;
                let kind = match d.step {
                    CodeStep::Effect => Some(FragKind::Effect),
                    CodeStep::Target => Some(FragKind::Target),
                    CodeStep::Modifier => Some(FragKind::Modifier),
                    CodeStep::Confirm => None,
                };
                if let Some(kind) = kind {
                    let rows = self.fragment_rows(kind);
                    let picked = if ch == '0' && d.step != CodeStep::Effect {
                        Some(0)
                    } else {
                        digit(ch).and_then(|n| rows.get(n - 1).map(|(num, _)| *num))
                    };
                    if let Some(num) = picked {
                        self.msgs.clear();
                        match d.step {
                            CodeStep::Effect => {
                                d.build.effect = num;
                                d.step = CodeStep::Target;
                            }
                            CodeStep::Target => {
                                d.build.target = num;
                                d.step = CodeStep::Modifier;
                            }
                            _ => {
                                d.build.modifier = num;
                                d.step = CodeStep::Confirm;
                            }
                        }
                        self.view = View::Code(d);
                    }
                }
            }
            (View::Code(draft), DoorKey::Enter) if draft.step == CodeStep::Confirm => {
                let build = draft.build;
                let r = self.p.code(&c, build);
                let ok = r.is_ok();
                self.say(r);
                self.view = if ok {
                    View::Hideout
                } else {
                    View::Code(Draft {
                        step: CodeStep::Confirm,
                        build,
                    })
                };
            }
            (View::Code(draft), DoorKey::Escape) => {
                let mut d = *draft;
                self.msgs.clear();
                match d.step {
                    CodeStep::Effect => {
                        self.view = View::Hideout;
                        return Flow::Stay;
                    }
                    CodeStep::Target => {
                        d.build.effect = 0;
                        d.step = CodeStep::Effect;
                    }
                    CodeStep::Modifier => {
                        d.build.target = 0;
                        d.step = CodeStep::Target;
                    }
                    CodeStep::Confirm => {
                        d.build.modifier = 0;
                        d.step = CodeStep::Modifier;
                    }
                }
                self.view = View::Code(d);
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
            (
                View::Deck(_)
                | View::Shop(_)
                | View::Rig
                | View::Hideout
                | View::Fixers
                | View::Heat
                | View::Profile
                | View::Black,
                DoorKey::Escape,
            )
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
        let before = self.place();
        let flow = if matches!(self.view, View::Run(_)) {
            self.run_key(key, ctx);
            Flow::Stay
        } else {
            self.menu_key(key, ctx)
        };
        if self.place() != before {
            self.sel = 0;
        }
        let bytes = save::encode(&self.p);
        if bytes != self.saved {
            self.saved.clone_from(&bytes);
            ctx.request_save(bytes);
        }
        flow
    }

    fn render(&self, canvas: &mut Canvas<'_>, env: &DoorEnv) {
        canvas.clear();
        ui::draw(self, canvas, env);
    }

    fn look_around(&self, _env: &DoorEnv) -> Vec<String> {
        self.lines(usize::MAX / 4)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }
}
