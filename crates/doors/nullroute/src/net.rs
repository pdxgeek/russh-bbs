//! Procedural corporate networks.
//!
//! A network is a pure function of the season, the corporation and the tier: the same inputs give
//! the same graph on every process, so a network needs no storage. What changes over a season (the
//! holes players opened, the alert level) lives in the shared world and is applied on top.
//!
//! The graph is layered: one entry node, `tier + 1` layers of two or three mid nodes, and an
//! objective floor with a data vault, an admin console, a server room and, for some corporations,
//! a decoy vault (the honeypot). Hidden links are shortcuts that a scan with enough power reveals.
//!
//! **The RPG rule.** A locked node can be entered only with a break script of the lock's family and
//! enough power, and a black wall only with a purge script of enough power. [`Network::reachable`]
//! says which nodes a deck can get to, and the generator guarantees a fair game: at tier 1 every
//! single break family alone can reach every objective; at tier 2 the favoured family and one more
//! can; at tier 3 the favoured family can. A deck is therefore a real choice, and never a dead end.

use russh_bbs_door::DoorRng;

use crate::content::{Content, CorpDef, Lock, NodeDef, Role};

/// The most nodes a network has (the bit sets are `u32`).
pub const MAX_NODES: usize = 24;

/// A node's id: its index in [`Network::nodes`].
pub type NodeId = u8;

/// What patrols a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Patrol {
    /// Nothing.
    None,
    /// Raises the alarm when you enter.
    Sentry,
    /// Starts counting your hops when you enter.
    Tracer,
    /// Blocks the way until it is purged.
    BlackWall,
}

/// One node.
#[derive(Debug, Clone)]
pub struct Node {
    /// The index in the network.
    pub id: NodeId,
    /// The node type's number (see `nodes.toml`).
    pub kind: u8,
    /// The layer: 0 is the entry.
    pub layer: u8,
    /// The security level (1 to 9): the power a break script needs.
    pub security: u8,
    /// The lock, if any.
    pub lock: Option<Lock>,
    /// The patrol, if any. Its level is the security level.
    pub patrol: Patrol,
    /// A decoy vault.
    pub honeypot: bool,
    /// The data worth copying here (0 where there is none).
    pub value: u32,
    /// The name on the screen, such as `FW-3`.
    pub name: String,
}

/// A link between two nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    /// One end.
    pub a: NodeId,
    /// The other end.
    pub b: NodeId,
    /// 0 for a link anyone can see; otherwise the scan power needed to find it.
    pub stealth: u8,
}

/// What a deck can do, as the strongest script of each kind it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capability {
    /// The strongest break script per lock family: `[crack, spoof, overflow]`.
    pub breaks: [u8; 3],
    /// The strongest purge script.
    pub purge: u8,
    /// The strongest scan or map script.
    pub scan: u8,
}

impl Capability {
    /// Every family at power 9: the full toolkit, for the generator's guarantees.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            breaks: [9; 3],
            purge: 9,
            scan: 9,
        }
    }

    /// Only one break family at power 9, and a scan that finds every link.
    #[must_use]
    pub const fn only(lock: Lock) -> Self {
        let mut breaks = [0; 3];
        breaks[lock.index()] = 9;
        Self {
            breaks,
            purge: 0,
            scan: 9,
        }
    }
}

/// A generated network.
#[derive(Debug, Clone)]
pub struct Network {
    /// The corporation's number.
    pub corp: u8,
    /// The tier, 1 to 3.
    pub tier: u8,
    /// The nodes; node 0 is the entry.
    pub nodes: Vec<Node>,
    /// The links.
    pub links: Vec<Link>,
}

/// The shared world's view of one network, applied on top of the generated graph.
#[derive(Debug, Clone, Copy, Default)]
pub struct Overlay<'a> {
    /// Nodes whose locks players broke and the patch has not closed.
    pub holes: &'a [NodeId],
    /// The corporation's alert level, added to every security level.
    pub alert: u8,
}

fn pick_weighted(rng: &mut DoorRng, weights: &[u32]) -> usize {
    let total: u32 = weights.iter().sum();
    if total == 0 {
        return 0;
    }
    let mut roll = rng.below(total);
    for (index, weight) in weights.iter().enumerate() {
        if roll < *weight {
            return index;
        }
        roll -= weight;
    }
    weights.len().saturating_sub(1)
}

/// The security cap per tier: the network never asks for more than the tier's scripts can give.
const fn security_cap(tier: u8) -> i32 {
    match tier {
        1 => 2,
        2 => 4,
        // The best break script in the shop has power 6, so no tier 3 lock asks for more
        // (M4-N4: it asked for 7, and one tier 3 network in five could not be opened at all).
        _ => 6,
    }
}

const fn tier_add(tier: u8) -> i32 {
    match tier {
        1 => 0,
        2 => 1,
        _ => 3,
    }
}

fn clamp_u8(value: i32, cap: i32) -> u8 {
    u8::try_from(value.clamp(1, cap)).unwrap_or(1)
}

impl Network {
    /// Generate the network for `corp` at `tier` in `season`. `None` if the content has no nodes.
    #[must_use]
    pub fn generate(content: &Content, season: u32, corp: &CorpDef, tier: u8) -> Option<Self> {
        let tier = tier.clamp(1, 3);
        let entry_kind = content.nodes.node.iter().find(|n| n.role == Role::Entry)?;
        let mids: Vec<&NodeDef> = content
            .nodes
            .node
            .iter()
            .filter(|n| n.role == Role::Mid && n.weight > 0 && n.corp.is_none_or(|c| c == corp.num))
            .collect();
        let objectives: Vec<&NodeDef> = content
            .nodes
            .node
            .iter()
            .filter(|n| n.role == Role::Objective)
            .collect();
        if mids.is_empty() || objectives.is_empty() {
            return None;
        }
        let label = (i64::from(season) << 16) | (i64::from(corp.num) << 8) | i64::from(tier);
        let mut rng = DoorRng::from_label("nullroute-net", label);
        let mut net = Self {
            corp: corp.num,
            tier,
            nodes: Vec::new(),
            links: Vec::new(),
        };
        let mut layers: Vec<Vec<NodeId>> = Vec::new();
        net.push(entry_kind, 0, 1, None, Patrol::None, false, 0);
        layers.push(vec![0]);
        let mid_layers = u8::try_from(usize::from(tier) + 1).unwrap_or(2);
        let weights: Vec<u32> = mids.iter().map(|n| n.weight).collect();
        for layer in 1..=mid_layers {
            let width = 2 + rng.below(2);
            let mut ids = Vec::new();
            for _ in 0..width {
                let def = mids[pick_weighted(&mut rng, &weights)];
                let security = Self::roll_security(&mut rng, def, corp, tier);
                let lock = Self::roll_lock(&mut rng, def, corp, def.lock_pct * corp.lock_pct / 100);
                let mut patrol = Patrol::None;
                if !def.watches && rng.chance(corp.patrol_pct, 100) {
                    patrol = if rng.chance(corp.tracer_share, 100) {
                        Patrol::Tracer
                    } else {
                        Patrol::Sentry
                    };
                }
                if tier >= 2 && layer >= 2 && !def.watches && rng.chance(corp.blackwall_pct, 100) {
                    patrol = Patrol::BlackWall;
                }
                ids.push(net.push(def, layer, security, lock, patrol, false, 0));
            }
            layers.push(ids);
        }
        let last = mid_layers + 1;
        let mut floor = Vec::new();
        for def in &objectives {
            let security = Self::roll_security(&mut rng, def, corp, tier);
            let lock = Self::roll_lock(&mut rng, def, corp, def.lock_pct);
            let value = if def.code == "VT" {
                u32::from(tier) * (40 + rng.below(41))
            } else {
                0
            };
            floor.push(net.push(def, last, security, lock, Patrol::None, false, value));
        }
        if rng.chance(corp.honey_pct, 100)
            && let Some(vault) = objectives.iter().find(|d| d.code == "VT")
        {
            let security = Self::roll_security(&mut rng, vault, corp, tier);
            let lock = Self::roll_lock(&mut rng, vault, corp, vault.lock_pct);
            floor.push(net.push(vault, last, security, lock, Patrol::None, true, 0));
        }
        layers.push(floor);
        net.link_layers(&layers, corp, &mut rng);
        net.guarantee_routes(content, corp, &mut rng);
        Some(net)
    }

    /// The season's finale network: the corporation's tier 3 network with every layer locked in
    /// one family (crack, spoof, overflow, crack, spoof) and no link that skips a layer, so every
    /// path crosses every lock. A deck with one break family cannot get through; a crew whose
    /// decks between them carry all three can. This is the one network the generator does *not*
    /// make fair to a single deck, on purpose: it is built to need crews.
    #[must_use]
    pub fn generate_finale(content: &Content, season: u32, corp: &CorpDef) -> Option<Self> {
        let mut net = Self::generate(content, season ^ 0x00F1_9A1E, corp, 3)?;
        let families = [Lock::Crack, Lock::Spoof, Lock::Overflow];
        for node in &mut net.nodes {
            if node.layer == 0 || node.honeypot {
                continue;
            }
            node.lock = Some(families[usize::from(node.layer - 1) % 3]);
            node.security = 3 + node.layer.min(5) / 2 + u8::from(node.layer >= 5);
            node.security = node.security.min(6);
        }
        let layer_of: Vec<u8> = net.nodes.iter().map(|n| n.layer).collect();
        net.links.retain(|l| {
            let (a, b) = (
                layer_of.get(usize::from(l.a)).copied().unwrap_or(0),
                layer_of.get(usize::from(l.b)).copied().unwrap_or(0),
            );
            a.abs_diff(b) <= 1
        });
        Some(net)
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        def: &NodeDef,
        layer: u8,
        security: u8,
        lock: Option<Lock>,
        patrol: Patrol,
        honeypot: bool,
        value: u32,
    ) -> NodeId {
        let id = u8::try_from(self.nodes.len()).unwrap_or(u8::MAX);
        self.nodes.push(Node {
            id,
            kind: def.num,
            layer,
            security,
            lock,
            patrol,
            honeypot,
            value,
            name: format!("{}-{id}", def.code),
        });
        id
    }

    fn roll_security(rng: &mut DoorRng, def: &NodeDef, corp: &CorpDef, tier: u8) -> u8 {
        let raw =
            def.sec + corp.sec_bonus + tier_add(tier) + i32::try_from(rng.below(2)).unwrap_or(0);
        clamp_u8(raw, security_cap(tier))
    }

    fn roll_lock(rng: &mut DoorRng, def: &NodeDef, corp: &CorpDef, chance: u32) -> Option<Lock> {
        if !rng.chance(chance, 100) {
            return None;
        }
        let weights: Vec<u32> = Lock::ALL
            .iter()
            .enumerate()
            .map(|(i, lock)| def.bias[i] * if *lock == corp.favoured { 3 } else { 1 })
            .collect();
        Lock::ALL.get(pick_weighted(rng, &weights)).copied()
    }

    fn connect(&mut self, a: NodeId, b: NodeId, stealth: u8) {
        if a != b && !self.linked(a, b) {
            self.links.push(Link { a, b, stealth });
        }
    }

    /// Whether `a` and `b` share a link.
    #[must_use]
    pub fn linked(&self, a: NodeId, b: NodeId) -> bool {
        self.links
            .iter()
            .any(|l| (l.a == a && l.b == b) || (l.a == b && l.b == a))
    }

    fn link_layers(&mut self, layers: &[Vec<NodeId>], corp: &CorpDef, rng: &mut DoorRng) {
        let tier = self.tier;
        let stealth = |rng: &mut DoorRng| -> u8 {
            2 + u8::try_from(rng.below(u32::from(tier) * 2)).unwrap_or(0)
        };
        for k in 0..layers.len().saturating_sub(1) {
            let (Some(here), Some(next)) = (layers.get(k), layers.get(k + 1)) else {
                continue;
            };
            for &child in next {
                let parent = here[rng.below(u32::try_from(here.len()).unwrap_or(1)) as usize];
                self.connect(parent, child, 0);
            }
            for &parent in here {
                if !next.iter().any(|&c| self.linked(parent, c)) {
                    let child = next[rng.below(u32::try_from(next.len()).unwrap_or(1)) as usize];
                    self.connect(parent, child, 0);
                }
            }
            for &parent in here {
                for &child in next {
                    if !self.linked(parent, child) && rng.chance(25, 100) {
                        let hidden = rng.chance(corp.hidden_pct, 100);
                        let level = if hidden { stealth(rng) } else { 0 };
                        self.connect(parent, child, level);
                    }
                }
            }
            if let Some(far) = layers.get(k + 2) {
                for &parent in here {
                    for &child in far {
                        if rng.chance(10, 100) {
                            let hidden = rng.chance((corp.hidden_pct * 2).min(100), 100);
                            let level = if hidden { stealth(rng) } else { 0 };
                            self.connect(parent, child, level);
                        }
                    }
                }
            }
        }
    }

    /// The nodes a contract can aim at: objective nodes with `code`, decoys excluded.
    pub fn targets<'a>(
        &'a self,
        content: &'a Content,
        code: &'a str,
    ) -> impl Iterator<Item = &'a Node> {
        self.nodes
            .iter()
            .filter(move |n| !n.honeypot && content.node(n.kind).is_some_and(|d| d.code == code))
    }

    /// The neighbours of `id` through links with `stealth` at most `scan` (0 for the visible ones).
    #[must_use]
    pub fn neighbours(&self, id: NodeId, scan: u8) -> Vec<NodeId> {
        let mut out: Vec<NodeId> = self
            .links
            .iter()
            .filter(|l| l.stealth <= scan)
            .filter_map(|l| {
                if l.a == id {
                    Some(l.b)
                } else if l.b == id {
                    Some(l.a)
                } else {
                    None
                }
            })
            .collect();
        out.sort_unstable();
        out
    }

    /// The effective security level of a node under an alert.
    #[must_use]
    pub fn security(&self, id: NodeId, alert: u8) -> u8 {
        self.nodes
            .get(usize::from(id))
            .map_or(9, |n| n.security.saturating_add(alert).min(9))
    }

    /// Whether `capability` can enter node `id` (ignoring trace): the lock's family must be
    /// carried at enough power, and a black wall needs a purge of enough power. A hole is open.
    #[must_use]
    pub fn can_enter(&self, id: NodeId, capability: &Capability, overlay: &Overlay<'_>) -> bool {
        let Some(node) = self.nodes.get(usize::from(id)) else {
            return false;
        };
        if overlay.holes.contains(&id) {
            return true;
        }
        let need = self.security(id, overlay.alert);
        if let Some(lock) = node.lock
            && capability.breaks[lock.index()] < need
        {
            return false;
        }
        !(node.patrol == Patrol::BlackWall && capability.purge < need)
    }

    /// Every node `capability` can get to from the entry, as a bit set (ignoring trace).
    #[must_use]
    pub fn reachable(&self, capability: &Capability, overlay: &Overlay<'_>) -> u32 {
        let mut seen: u32 = 1;
        let mut queue = vec![0u8];
        while let Some(at) = queue.pop() {
            for next in self.neighbours(at, capability.scan) {
                if seen & (1 << next) == 0 && self.can_enter(next, capability, overlay) {
                    seen |= 1 << next;
                    queue.push(next);
                }
            }
        }
        seen
    }

    /// Whether `capability` can reach node `id`.
    #[must_use]
    pub fn reaches(&self, id: NodeId, capability: &Capability, overlay: &Overlay<'_>) -> bool {
        self.reachable(capability, overlay) & (1 << id) != 0
    }

    /// The shortest path from the entry to `to`, over every link and ignoring locks, avoiding
    /// `avoid`. `None` if there is none.
    fn path(&self, to: NodeId, avoid: impl Fn(&Node) -> bool) -> Option<Vec<NodeId>> {
        let mut parent: Vec<Option<NodeId>> = vec![None; self.nodes.len()];
        let mut seen: u32 = 1;
        let mut queue = std::collections::VecDeque::from([0u8]);
        while let Some(at) = queue.pop_front() {
            if at == to {
                let mut path = vec![at];
                let mut cursor = at;
                while let Some(Some(p)) = parent.get(usize::from(cursor)) {
                    path.push(*p);
                    cursor = *p;
                }
                path.reverse();
                return Some(path);
            }
            for next in self.neighbours(at, u8::MAX) {
                let blocked = self.nodes.get(usize::from(next)).is_none_or(&avoid);
                if seen & (1 << next) == 0 && !blocked {
                    seen |= 1 << next;
                    if let Some(slot) = parent.get_mut(usize::from(next)) {
                        *slot = Some(at);
                    }
                    queue.push_back(next);
                }
            }
        }
        None
    }

    /// Make the fairness guarantee in the module docs true. Repairs rewrite locks along one path
    /// at a time; if rewriting cannot settle after a few rounds, the locks on the path come off
    /// (which can only help every deck).
    fn guarantee_routes(&mut self, content: &Content, corp: &CorpDef, rng: &mut DoorRng) {
        let mut needed: Vec<Lock> = match self.tier {
            1 => Lock::ALL.to_vec(),
            2 => {
                let others: Vec<Lock> = Lock::ALL
                    .into_iter()
                    .filter(|l| *l != corp.favoured)
                    .collect();
                let extra = others[rng.below(u32::try_from(others.len()).unwrap_or(1)) as usize];
                vec![corp.favoured, extra]
            }
            _ => vec![corp.favoured],
        };
        needed.dedup();
        let objective_ids: Vec<NodeId> = ["VT", "AC", "SR"]
            .iter()
            .flat_map(|code| {
                self.targets(content, code)
                    .map(|n| n.id)
                    .collect::<Vec<_>>()
            })
            .collect();
        for round in 0..6 {
            let mut changed = false;
            for &target in &objective_ids {
                for &lock in &needed {
                    let cap = Capability::only(lock);
                    if self.reaches(target, &cap, &Overlay::default()) {
                        continue;
                    }
                    let path = self
                        .path(target, |n| n.honeypot || n.patrol == Patrol::BlackWall)
                        .or_else(|| self.path(target, |n| n.honeypot));
                    let Some(path) = path else { continue };
                    for id in path {
                        if let Some(node) = self.nodes.get_mut(usize::from(id)) {
                            if node.patrol == Patrol::BlackWall {
                                node.patrol = Patrol::None;
                            }
                            if node.lock.is_some() {
                                node.lock = if round >= 4 { None } else { Some(lock) };
                            }
                        }
                    }
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// The objective floor's nodes, in id order.
    pub fn floor(&self) -> impl Iterator<Item = &Node> {
        let last = self.nodes.iter().map(|n| n.layer).max().unwrap_or(0);
        self.nodes.iter().filter(move |n| n.layer == last)
    }
}
