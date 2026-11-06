//! The shared world: what a season remembers of the networks, the contract board and the market.
//!
//! The world is changed in two places only: a run's end (the holes it opened, the alert it raised,
//! the backdoor it planted) through the framework's compare-and-swap world edit, and the nightly
//! tick (patching, the market, new contracts). Both are pure functions of the world, the content,
//! the day and a seeded RNG.
//!
//! **What the tick cannot do.** The framework's tick sees the world and nothing else, so anything
//! that belongs to one player (heat decay, bot income, raids) is applied by that player's own
//! catch-up the next time they open the door; see `Player::rollover`.

use russh_bbs_door::DoorRng;

use crate::content::Content;
use crate::learn::{DECAY_PER_NIGHT, ROWS_MAX};
use crate::net::NodeId;
use crate::run::RunResult;
use crate::social::{HIST_MAX, Social};

/// The most contracts on the board.
pub const BOARD_MAX: usize = 24;

/// New contracts posted each night.
pub const NIGHTLY_CONTRACTS: usize = 8;

/// Days a contract stays on the board.
pub const CONTRACT_DAYS: i64 = 3;

/// The most holes remembered per network.
pub const HOLES_MAX: usize = 24;

/// The most alert a corporation can be on.
pub const ALERT_MAX: u8 = 2;

/// A job a fixer is offering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contract {
    /// Unique: the day it was posted times 16, plus its index that night.
    pub id: u32,
    /// The template's number.
    pub tmpl: u8,
    /// The target corporation's number.
    pub corp: u8,
    /// The fixer posting it.
    pub fixer: u8,
    /// The tier, 1 to 3.
    pub tier: u8,
    /// The pay, in marks.
    pub reward: u32,
    /// The last day it can be taken.
    pub expires: i64,
}

/// How many runs against one corporation used one script identity: what the corporation has
/// learned (see [`crate::learn`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Learned {
    /// The corporation's number.
    pub corp: u8,
    /// The script identity (a stock script's number, or a coded script's signature).
    pub sig: u16,
    /// Runs that used it.
    pub uses: u8,
}

/// What the season remembers about one network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetState {
    /// The corporation's number.
    pub corp: u8,
    /// The tier.
    pub tier: u8,
    /// Alert level, 0 to [`ALERT_MAX`]: added to every security level, falls a step each night.
    pub alert: u8,
    /// Nodes whose locks were broken and not yet patched.
    pub holes: Vec<NodeId>,
    /// A planted backdoor.
    pub backdoor: bool,
    /// Objectives completed against it this season.
    pub breaches: u16,
}

/// The shared world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    /// The last day the tick applied (0 before the first).
    pub day: i64,
    /// The market price level in percent (80 to 130).
    pub market_pct: u32,
    /// Networks with something to remember.
    pub nets: Vec<NetState>,
    /// Open contracts.
    pub board: Vec<Contract>,
    /// Objectives completed on the whole board this season.
    pub breaches: u32,
    /// The season the world was last played in (0 before the first session).
    pub season: u32,
    /// What the corporations have learned of the scripts used against them.
    pub learned: Vec<Learned>,
    /// The market price level of the last nights, oldest first (a real history for the market's
    /// sparkline).
    pub hist: Vec<u8>,
    /// Rivals, hacks, bounties, crews and crew runs, the street word and the hall.
    pub social: Social,
}

impl Default for World {
    fn default() -> Self {
        Self {
            day: 0,
            market_pct: 100,
            nets: Vec::new(),
            board: Vec::new(),
            breaches: 0,
            season: 0,
            learned: Vec::new(),
            hist: Vec::new(),
            social: Social::default(),
        }
    }
}

impl World {
    /// What is remembered about a network.
    #[must_use]
    pub fn net(&self, corp: u8, tier: u8) -> Option<&NetState> {
        self.nets.iter().find(|n| n.corp == corp && n.tier == tier)
    }

    /// A new season starts the corporations from nothing: the networks are regenerated, so what
    /// was open and what was learned no longer applies. The board and the market carry on. The
    /// season that closes goes into the hall (its ending chosen by what the board did), the
    /// season's tallies, finale, bounties and crew runs start over, and crews and the hideouts
    /// posted carry on.
    pub fn sync_season(&mut self, content: &Content, season: u32) {
        if self.season != season {
            if self.season != 0 {
                self.close_season(content);
                self.nets.clear();
                self.learned.clear();
                self.breaches = 0;
            }
            self.season = season;
        }
    }

    /// Write the season that just ended into the hall.
    fn close_season(&mut self, content: &Content) {
        let s = &mut self.social;
        let ending = content
            .seasons
            .season_ending
            .iter()
            .find(|e| {
                crate::cond::Cond::parse(&e.rule, &[]).is_ok_and(|c| c.eval(None, &s.tallies))
            })
            .map_or(0, |e| e.num);
        let mut top: Vec<(String, u32)> =
            s.posted.iter().map(|p| (p.handle.clone(), p.rep)).collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        top.truncate(5);
        s.hall.push(crate::social::SeasonRecord {
            season: self.season,
            ending,
            top,
            tallies: std::mem::take(&mut s.tallies),
            finale: s.finale.take().unwrap_or_default(),
        });
        while s.hall.len() > crate::social::HALL_MAX {
            s.hall.remove(0);
        }
        s.bounties.clear();
        s.orders.clear();
        s.ops.clear();
        s.pairs.clear();
    }

    /// What `corp` has learned, as (script identity, runs) pairs.
    #[must_use]
    pub fn learned_by(&self, corp: u8) -> Vec<(u16, u32)> {
        self.learned
            .iter()
            .filter(|l| l.corp == corp)
            .map(|l| (l.sig, u32::from(l.uses)))
            .collect()
    }

    fn learn(&mut self, corp: u8, sig: u16) {
        if let Some(row) = self
            .learned
            .iter_mut()
            .find(|l| l.corp == corp && l.sig == sig)
        {
            row.uses = row.uses.saturating_add(1);
            return;
        }
        if self.learned.len() >= ROWS_MAX
            && let Some(at) = self
                .learned
                .iter()
                .enumerate()
                .min_by_key(|(_, l)| l.uses)
                .map(|(i, _)| i)
        {
            self.learned.remove(at);
        }
        self.learned.push(Learned { corp, sig, uses: 1 });
    }

    fn net_mut(&mut self, corp: u8, tier: u8) -> &mut NetState {
        if let Some(at) = self
            .nets
            .iter()
            .position(|n| n.corp == corp && n.tier == tier)
        {
            return &mut self.nets[at];
        }
        self.nets.push(NetState {
            corp,
            tier,
            alert: 0,
            holes: Vec::new(),
            backdoor: false,
            breaches: 0,
        });
        let last = self.nets.len() - 1;
        &mut self.nets[last]
    }

    /// Record what a run did to the network: holes it opened (also when it was burned), and, if it
    /// finished the objective, a breach, a higher alert and a backdoor if one was planted.
    pub fn apply_run(&mut self, corp: u8, tier: u8, result: &RunResult) {
        let done = result.objective;
        for sig in &result.used {
            self.learn(corp, *sig);
        }
        let net = self.net_mut(corp, tier);
        for hole in &result.broken {
            if !net.holes.contains(hole) && net.holes.len() < HOLES_MAX {
                net.holes.push(*hole);
            }
        }
        if done {
            net.alert = (net.alert + 1).min(ALERT_MAX);
            net.breaches = net.breaches.saturating_add(1);
            if result.planted {
                net.backdoor = true;
            }
        }
        if done {
            self.breaches = self.breaches.saturating_add(1);
        }
    }

    /// Count a finished solo run toward the season's tallies and the street word.
    pub fn record_run(&mut self, handle: &str, tier: u8, result: &RunResult, day: i64) {
        use crate::run::Status;
        use crate::social::Lines;
        let s = &mut self.social;
        if result.status == Status::Burned {
            s.tallies.burns += 1;
            if tier == 3 {
                s.deed(day, Lines::DEED_BURN, handle, 3);
            }
        }
        if result.objective {
            s.tallies.jobs += 1;
            if tier == 3 {
                s.tallies.deep_breaches += 1;
                s.deed(day, Lines::DEED_DEEP, handle, 3);
            }
        }
        if result.warden_down {
            s.tallies.wardens += 1;
            s.deed(day, Lines::DEED_WARDEN, handle, 1);
        }
    }

    /// One night passes: corporations patch, alert falls, the market moves, old contracts lapse
    /// and new ones are posted.
    pub fn nightly(&mut self, content: &Content, rng: &mut DoorRng, day: i64) {
        let roll = i64::from(rng.range(0, 10)) - 5;
        self.market_pct =
            u32::try_from((i64::from(self.market_pct) + roll).clamp(80, 130)).unwrap_or(100);
        for net in &mut self.nets {
            let patch = content.corp(net.corp).map_or(30, |c| c.patch_pct);
            net.holes.retain(|_| !rng.chance(patch, 100));
            if net.backdoor && rng.chance(patch / 2, 100) {
                net.backdoor = false;
            }
            net.alert = net.alert.saturating_sub(1);
        }
        let decay = u8::try_from(DECAY_PER_NIGHT).unwrap_or(1);
        for row in &mut self.learned {
            row.uses = row.uses.saturating_sub(decay);
        }
        self.learned.retain(|l| l.uses > 0);
        self.nets
            .retain(|n| !n.holes.is_empty() || n.alert > 0 || n.backdoor || n.breaches > 0);
        self.hist
            .push(u8::try_from(self.market_pct.min(255)).unwrap_or(100));
        while self.hist.len() > HIST_MAX {
            self.hist.remove(0);
        }
        self.social.settle(content, rng, day);
        self.ops_night(content, day);
        self.board.retain(|c| c.expires >= day);
        self.board
            .extend(gen_contracts(content, rng, day, NIGHTLY_CONTRACTS));
        while self.board.len() > BOARD_MAX {
            self.board.remove(0);
        }
        self.day = day;
    }
}

/// Post `count` contracts for `day`. The tier mix is 55 percent tier 1, 30 percent tier 2 and 15
/// percent tier 3; the fixer is never one whose client the target is.
#[must_use]
pub fn gen_contracts(
    content: &Content,
    rng: &mut DoorRng,
    day: i64,
    count: usize,
) -> Vec<Contract> {
    let mut out = Vec::new();
    for k in 0..count {
        let roll = rng.below(100);
        let tier: u8 = if roll < 55 {
            1
        } else if roll < 85 {
            2
        } else {
            3
        };
        let templates: Vec<_> = content
            .contracts
            .contract
            .iter()
            .filter(|c| c.tier == tier)
            .collect();
        if templates.is_empty() || content.corps.corp.is_empty() {
            continue;
        }
        let tmpl = templates[rng.below(u32::try_from(templates.len()).unwrap_or(1)) as usize];
        let corp = &content.corps.corp
            [rng.below(u32::try_from(content.corps.corp.len()).unwrap_or(1)) as usize];
        let fixers: Vec<_> = content
            .corps
            .fixer
            .iter()
            .filter(|f| f.num != corp.patron)
            .collect();
        if fixers.is_empty() {
            continue;
        }
        let fixer = fixers[rng.below(u32::try_from(fixers.len()).unwrap_or(1)) as usize];
        out.push(Contract {
            id: u32::try_from(day.max(0)).unwrap_or(0).saturating_mul(16)
                + u32::try_from(k).unwrap_or(0),
            tmpl: tmpl.num,
            corp: corp.num,
            fixer: fixer.num,
            tier,
            reward: rng.range(tmpl.reward_min, tmpl.reward_max),
            expires: day + CONTRACT_DAYS - 1,
        });
    }
    out
}

/// The board a visitor sees before any nightly tick has run (a new board, or a replay with no
/// world): the same jobs for everyone on a given day.
#[must_use]
pub fn first_board(content: &Content, today: i64) -> Vec<Contract> {
    gen_contracts(
        content,
        &mut DoorRng::from_label("nullroute-first-board", today),
        today,
        NIGHTLY_CONTRACTS,
    )
}
