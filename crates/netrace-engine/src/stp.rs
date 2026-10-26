//! Layer 2 topology: which switch ports carry which VLANs, and spanning
//! tree (one instance per VLAN, as PVST+ and Rapid PVST+ run).
//!
//! The converged tree is computed directly from the topology rather than by
//! exchanging BPDUs over time, so a lab never waits for convergence. The
//! result is what 802.1D settles on: the lowest bridge ID is root, every
//! other switch keeps its cheapest path to the root (root port), every
//! segment has one designated port, and the rest block.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use netrace_doc::{DeviceId, DeviceKind, Project, SwitchportConfig};

use crate::MacAddr;
use crate::stack::PortStates;

pub type PortKey = (DeviceId, String);

/// MAC index used for a switch's bridge ID.
const BRIDGE_MAC_INDEX: usize = 0xfe;

/// Port states and the layer 2 picture together. A port BPDU guard
/// disabled takes its link down, so the device at the other end sees it
/// down too.
pub(crate) fn analyse(project: &Project) -> (PortStates, Layer2) {
    let mut ports = PortStates::of(project);
    let l2 = Layer2::of(project, &ports);
    for key in &l2.err_disabled {
        ports.up.remove(key);
        if let Some(link) = project.links().iter().find(|l| {
            (l.a.device, l.a.port.as_str()) == (key.0, key.1.as_str())
                || (l.b.device, l.b.port.as_str()) == (key.0, key.1.as_str())
        }) {
            ports.up.remove(&(link.a.device, link.a.port.clone()));
            ports.up.remove(&(link.b.device, link.b.port.clone()));
        }
    }
    (ports, l2)
}

/// The switchport settings in effect on a port (`None` if the device isn't
/// a switch, or the port is routed).
pub(crate) fn switchport(project: &Project, id: DeviceId, port: &str) -> Option<SwitchportConfig> {
    let d = project.device(id)?;
    if !d.kind.is_switch() || !d.kind.has_port(port) {
        return None;
    }
    let config = d.config.interface(d.kind, port).switchport;
    (!config.routed).then_some(config)
}

pub fn bridge_mac(id: DeviceId) -> MacAddr {
    MacAddr::for_port(id, BRIDGE_MAC_INDEX)
}

/// Port cost by speed (802.1D short method, Cisco's default).
pub(crate) fn port_cost(port: &str) -> u32 {
    if port.starts_with("FastEthernet") {
        19
    } else {
        4
    }
}

/// Bridge ID: priority (with the VLAN added, as PVST's extended system
/// ID does) and MAC address. Lower wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BridgeId {
    pub priority: u32,
    pub mac: MacAddr,
}

impl fmt::Display for BridgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} / {}", self.priority, self.mac)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortRole {
    Root,
    Designated,
    /// Blocked: another path to the root is better.
    Alternate,
    /// Blocked: a second port of the same switch on one segment (via a hub).
    Backup,
}

impl PortRole {
    pub fn ios(self) -> &'static str {
        match self {
            PortRole::Root => "Root",
            PortRole::Designated => "Desg",
            PortRole::Alternate => "Altn",
            PortRole::Backup => "Back",
        }
    }

    pub fn forwards(self) -> bool {
        matches!(self, PortRole::Root | PortRole::Designated)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StpPort {
    pub role: PortRole,
    pub cost: u32,
    /// 128.N, as IOS numbers ports.
    pub number: u32,
    /// The bridge whose port is designated on this port's segment.
    pub designated_bridge: BridgeId,
    /// Why the port has its role, in plain words, from the decision made.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StpBridge {
    pub id: BridgeId,
    pub root_cost: u32,
    pub root_port: Option<String>,
    /// Why this bridge is (or isn't) the root.
    pub reason: String,
}

/// Spanning tree for one VLAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlanTree {
    pub vlan: u16,
    /// The lowest bridge of all; each switched island has its own root (see `roots`).
    pub root: BridgeId,
    pub root_device: DeviceId,
    /// Each bridge's root: switches only elect among those they can reach,
    /// so islands of switches separated by routers have a root each.
    pub roots: BTreeMap<DeviceId, DeviceId>,
    pub bridges: BTreeMap<DeviceId, StpBridge>,
    pub ports: BTreeMap<PortKey, StpPort>,
}

impl VlanTree {
    /// The root bridge `id` follows (its island's root).
    pub fn root_of(&self, id: DeviceId) -> DeviceId {
        self.roots.get(&id).copied().unwrap_or(self.root_device)
    }

    /// The root of every island with a choice of root (two or more bridges).
    pub fn island_roots(&self) -> Vec<DeviceId> {
        let mut counts: BTreeMap<DeviceId, usize> = BTreeMap::new();
        for r in self.roots.values() {
            *counts.entry(*r).or_default() += 1;
        }
        counts
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(r, _)| r)
            .collect()
    }
}

/// The layer 2 picture of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layer2 {
    pub trees: BTreeMap<u16, VlanTree>,
    /// Ports BPDU guard disabled: PortFast ports that face another switch.
    pub err_disabled: BTreeSet<PortKey>,
}

impl Layer2 {
    /// The layer 2 picture of a project (VLAN trees, err-disabled ports).
    pub fn for_project(project: &Project) -> Self {
        analyse(project).1
    }

    pub(crate) fn of(project: &Project, ports: &PortStates) -> Self {
        let mut l2 = Layer2::default();
        let segments = segments(project);
        // BPDU guard: an edge port that hears another switch shuts itself.
        for d in project.devices().iter().filter(|d| d.kind.is_switch()) {
            for port in d.kind.port_names() {
                let Some(sp) = switchport(project, d.id, &port) else {
                    continue;
                };
                if !(sp.portfast && sp.bpdu_guard) || !ports.is_up(d.id, &port) {
                    continue;
                }
                let key = (d.id, port.clone());
                let faces_switch = segments
                    .get(&key)
                    .is_some_and(|seg| seg.iter().any(|other| other.0 != d.id));
                if faces_switch {
                    l2.err_disabled.insert(key);
                }
            }
        }
        let vlans: BTreeSet<u16> = project
            .devices()
            .iter()
            .filter(|d| d.kind.is_switch())
            .flat_map(|d| d.config.vlan_ids())
            .collect();
        for vlan in vlans {
            if let Some(tree) = compute(project, ports, &segments, &l2.err_disabled, vlan) {
                l2.trees.insert(vlan, tree);
            }
        }
        l2
    }

    /// Whether a switch port passes data frames of `vlan` (spanning tree
    /// forwarding, not err-disabled).
    pub fn forwards(&self, id: DeviceId, port: &str, vlan: u16) -> bool {
        let key = (id, port.to_string());
        if self.err_disabled.contains(&key) {
            return false;
        }
        self.trees
            .get(&vlan)
            .and_then(|t| t.ports.get(&key))
            .is_none_or(|p| p.role.forwards())
    }

    pub fn port(&self, id: DeviceId, port: &str, vlan: u16) -> Option<&StpPort> {
        self.trees.get(&vlan)?.ports.get(&(id, port.to_string()))
    }
}

/// For every switch port, the other switch ports on the same segment
/// (directly cabled, or sharing a hub).
fn segments(project: &Project) -> BTreeMap<PortKey, Vec<PortKey>> {
    // Union-find over switch ports and hubs.
    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
    enum Node {
        Port(PortKey),
        Hub(DeviceId),
    }
    let kind = |id: DeviceId| project.device(id).map(|d| d.kind);
    let node = |id: DeviceId, port: &str| match kind(id) {
        Some(k) if k.is_switch() => Some(Node::Port((id, port.to_string()))),
        Some(DeviceKind::Hub) => Some(Node::Hub(id)),
        _ => None,
    };
    let mut parent: BTreeMap<Node, Node> = BTreeMap::new();
    fn find(parent: &mut BTreeMap<Node, Node>, n: Node) -> Node {
        let p = parent.get(&n).cloned().unwrap_or_else(|| n.clone());
        if p == n {
            return n;
        }
        let root = find(parent, p);
        parent.insert(n, root.clone());
        root
    }
    for link in project.links() {
        let a = node(link.a.device, &link.a.port);
        let b = node(link.b.device, &link.b.port);
        for n in [&a, &b].into_iter().flatten() {
            parent.entry(n.clone()).or_insert_with(|| n.clone());
        }
        if let (Some(a), Some(b)) = (a, b) {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
    }
    let mut groups: BTreeMap<Node, Vec<PortKey>> = BTreeMap::new();
    let nodes: Vec<Node> = parent.keys().cloned().collect();
    for n in nodes {
        if let Node::Port(key) = &n {
            let root = find(&mut parent, n.clone());
            groups.entry(root).or_default().push(key.clone());
        }
    }
    let mut out = BTreeMap::new();
    for members in groups.values() {
        for key in members {
            out.insert(
                key.clone(),
                members.iter().filter(|m| *m != key).cloned().collect(),
            );
        }
    }
    out
}

fn compute(
    project: &Project,
    ports: &PortStates,
    segments: &BTreeMap<PortKey, Vec<PortKey>>,
    err_disabled: &BTreeSet<PortKey>,
    vlan: u16,
) -> Option<VlanTree> {
    // Ports taking part: up, carrying the VLAN, on a switch that has it.
    let mut members: BTreeMap<DeviceId, Vec<(String, u32)>> = BTreeMap::new();
    let mut bridge_ids: BTreeMap<DeviceId, BridgeId> = BTreeMap::new();
    for d in project.devices().iter().filter(|d| d.kind.is_switch()) {
        if !d.config.vlan_exists(vlan) {
            continue;
        }
        for (index, port) in d.kind.port_names().into_iter().enumerate() {
            let Some(sp) = switchport(project, d.id, &port) else {
                continue;
            };
            let key = (d.id, port.clone());
            if sp.carries(vlan) && ports.is_up(d.id, &port) && !err_disabled.contains(&key) {
                members
                    .entry(d.id)
                    .or_default()
                    .push((port, index as u32 + 1));
            }
        }
        if members.contains_key(&d.id) {
            bridge_ids.insert(
                d.id,
                BridgeId {
                    priority: u32::from(d.config.spanning_tree.priority(vlan)) + u32::from(vlan),
                    mac: bridge_mac(d.id),
                },
            );
        }
    }
    let (&root_device, &root) = bridge_ids.iter().min_by_key(|(_, b)| **b)?;

    // Segment of a member port: the member ports it shares a wire with.
    let carries = |key: &PortKey| {
        members
            .get(&key.0)
            .is_some_and(|ps| ps.iter().any(|(p, _)| *p == key.1))
    };
    let peers = |key: &PortKey| -> Vec<PortKey> {
        segments
            .get(key)
            .map(|s| s.iter().filter(|k| carries(k)).cloned().collect())
            .unwrap_or_default()
    };
    let number = |key: &PortKey| {
        members[&key.0]
            .iter()
            .find(|(p, _)| *p == key.1)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    };

    // Dijkstra on root path cost, breaking ties as 802.1D does, once per
    // island of connected switches, each from its own lowest bridge.
    let mut cost: BTreeMap<DeviceId, u32> = BTreeMap::new();
    let mut roots: BTreeMap<DeviceId, DeviceId> = BTreeMap::new();
    let mut done: BTreeSet<DeviceId> = BTreeSet::new();
    while let Some((&island_root, _)) = bridge_ids
        .iter()
        .filter(|(b, _)| !done.contains(*b))
        .min_by_key(|(_, b)| **b)
    {
        cost.insert(island_root, 0);
        while let Some((&bridge, &c)) = cost
            .iter()
            .filter(|(b, _)| !done.contains(*b))
            .min_by_key(|(b, c)| (**c, bridge_ids[*b]))
        {
            done.insert(bridge);
            roots.insert(bridge, island_root);
            for (port, _) in &members[&bridge] {
                for peer in peers(&(bridge, port.clone())) {
                    let candidate = c + port_cost(&peer.1);
                    let entry = cost.entry(peer.0).or_insert(u32::MAX);
                    if candidate < *entry {
                        *entry = candidate;
                    }
                }
            }
        }
    }

    // Designated port per segment: lowest (root cost, bridge ID, port number).
    let designation = |key: &PortKey| {
        let mut on_segment = peers(key);
        on_segment.push(key.clone());
        on_segment
            .into_iter()
            .filter(|k| cost.contains_key(&k.0))
            .min_by_key(|k| (cost[&k.0], bridge_ids[&k.0], number(k)))
    };

    let mut tree = VlanTree {
        vlan,
        root,
        root_device,
        roots: roots.clone(),
        bridges: BTreeMap::new(),
        ports: BTreeMap::new(),
    };
    for (&bridge, bridge_ports) in &members {
        let Some(&root_cost) = cost.get(&bridge) else {
            continue;
        };
        let my_root = roots[&bridge];
        let my_root_id = bridge_ids[&my_root];
        // Root port: the best way towards the root through a designated port.
        let root_port = (bridge != my_root)
            .then(|| {
                bridge_ports
                    .iter()
                    .filter_map(|(port, n)| {
                        let key = (bridge, port.clone());
                        let d = designation(&key)?;
                        (d.0 != bridge).then(|| {
                            (
                                (
                                    cost[&d.0] + port_cost(port),
                                    bridge_ids[&d.0],
                                    number(&d),
                                    *n,
                                ),
                                port.clone(),
                            )
                        })
                    })
                    .min_by_key(|(rank, _)| *rank)
                    .map(|(_, port)| port)
            })
            .flatten();
        let name = |id: DeviceId| {
            project
                .device(id)
                .map(|d| d.name.clone())
                .unwrap_or_default()
        };
        let reason = if bridge == my_root {
            format!(
                "{} has the lowest bridge ID (priority {}, MAC {}) of the switches it's connected to, so they elect it \
                 root.",
                name(bridge),
                my_root_id.priority,
                my_root_id.mac
            )
        } else {
            format!(
                "{} isn't root: {} has the lower bridge ID ({} beats {}). Its cheapest path to the root costs {root_cost}.",
                name(bridge),
                name(my_root),
                my_root_id,
                bridge_ids[&bridge]
            )
        };
        tree.bridges.insert(
            bridge,
            StpBridge {
                id: bridge_ids[&bridge],
                root_cost,
                root_port: root_port.clone(),
                reason,
            },
        );
        for (port, n) in bridge_ports {
            let key = (bridge, port.clone());
            let designated = designation(&key).unwrap_or_else(|| key.clone());
            let role = if Some(port) == root_port.as_ref() {
                PortRole::Root
            } else if designated == key {
                PortRole::Designated
            } else if designated.0 == bridge {
                PortRole::Backup
            } else {
                PortRole::Alternate
            };
            let p = crate::abbreviate_port(port);
            let other = name(designated.0);
            let reason = match role {
                PortRole::Root => format!(
                    "Root port: {p} is {}'s cheapest way to the root, total cost {root_cost} (this link costs {}).",
                    name(bridge),
                    port_cost(port)
                ),
                PortRole::Designated if bridge == my_root => {
                    format!(
                        "Designated: every port of the root bridge forwards, so {p} is designated for its link."
                    )
                }
                PortRole::Designated => format!(
                    "Designated: on this link {} offers the best path to the root (cost {root_cost}), so {p} forwards \
                     towards the switches and hosts beyond it.",
                    name(bridge)
                ),
                PortRole::Alternate => format!(
                    "Blocked (alternate): {other} offers this link a better path to the root (cost {} against {}'s \
                     {root_cost}), and {p} isn't {}'s root port. Forwarding here too would make a loop, so it \
                     blocks and waits in reserve.",
                    cost.get(&designated.0).copied().unwrap_or(0),
                    name(bridge),
                    name(bridge)
                ),
                PortRole::Backup => format!(
                    "Blocked (backup): {} has another port on the same segment (through a hub) that is already \
                     designated, so {p} blocks.",
                    name(bridge)
                ),
            };
            tree.ports.insert(
                key,
                StpPort {
                    role,
                    cost: port_cost(port),
                    number: *n,
                    designated_bridge: bridge_ids[&designated.0],
                    reason,
                },
            );
        }
    }
    Some(tree)
}

/// One port or bridge whose spanning-tree state changed when a link failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StpChange {
    pub device: DeviceId,
    /// `None` for a change of root bridge.
    pub port: Option<String>,
    pub before: Option<PortRole>,
    pub after: Option<PortRole>,
    pub explanation: String,
}

impl Layer2 {
    /// What happens to the spanning tree of `vlan` when `link` fails: the
    /// tree recomputed without it, and the changes in the order they matter
    /// (root first, then lost links, then ports that unblock or change role).
    pub fn after_link_failure(
        project: &Project,
        link: netrace_doc::LinkId,
        vlan: u16,
    ) -> (Layer2, Vec<StpChange>) {
        let before = Layer2::for_project(project);
        let mut failed = project.clone();
        // A replay, not an edit: the lab's locks don't apply.
        failed.set_lab(None);
        let _ = failed.disconnect(link);
        let after = Layer2::for_project(&failed);
        let name = |id: DeviceId| {
            project
                .device(id)
                .map(|d| d.name.clone())
                .unwrap_or_default()
        };
        let mut changes = Vec::new();
        let (old, new) = (before.trees.get(&vlan), after.trees.get(&vlan));
        if let (Some(o), Some(n)) = (old, new)
            && o.root_device != n.root_device
        {
            changes.push(StpChange {
                device: n.root_device,
                port: None,
                before: None,
                after: None,
                explanation: format!(
                    "The switches lose contact with {} and elect a new root: {} now has the lowest bridge ID they can reach.",
                    name(o.root_device),
                    name(n.root_device)
                ),
            });
        }
        let empty = BTreeMap::new();
        let old_ports = old.map(|t| &t.ports).unwrap_or(&empty);
        let new_ports = new.map(|t| &t.ports).unwrap_or(&empty);
        let mut keys: Vec<&PortKey> = old_ports.keys().chain(new_ports.keys()).collect();
        keys.sort();
        keys.dedup();
        let mut lost = Vec::new();
        let mut moved = Vec::new();
        for key in keys {
            let (b, a) = (
                old_ports.get(key).map(|p| p.role),
                new_ports.get(key).map(|p| p.role),
            );
            if b == a {
                continue;
            }
            let p = crate::abbreviate_port(&key.1);
            let who = name(key.0);
            let change = match (b, a) {
                (Some(_), None) => StpChange {
                    device: key.0,
                    port: Some(key.1.clone()),
                    before: b,
                    after: None,
                    explanation: format!("{who} {p} goes down with the failed link."),
                },
                (_, Some(role)) => StpChange {
                    device: key.0,
                    port: Some(key.1.clone()),
                    before: b,
                    after: Some(role),
                    explanation: match (b, role) {
                        (Some(PortRole::Alternate | PortRole::Backup), PortRole::Root) => format!(
                            "{who} {p} stops blocking and becomes the root port: it's now {who}'s best remaining path to the root."
                        ),
                        (Some(PortRole::Alternate | PortRole::Backup), PortRole::Designated) => {
                            format!(
                                "{who} {p} stops blocking and becomes designated: nothing else serves that link now."
                            )
                        }
                        (_, PortRole::Alternate | PortRole::Backup) => {
                            format!("{who} {p} now blocks: {}", new_ports[key].reason)
                        }
                        _ => format!("{who} {p}: {}", new_ports[key].reason),
                    },
                },
                (None, None) => continue,
            };
            if change.after.is_none() {
                lost.push(change)
            } else {
                moved.push(change)
            }
        }
        changes.extend(lost);
        changes.extend(moved);
        (after, changes)
    }
}
