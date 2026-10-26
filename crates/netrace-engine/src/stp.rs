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

pub(crate) type PortKey = (DeviceId, String);

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StpBridge {
    pub id: BridgeId,
    pub root_cost: u32,
    pub root_port: Option<String>,
}

/// Spanning tree for one VLAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlanTree {
    pub vlan: u16,
    pub root: BridgeId,
    pub root_device: DeviceId,
    pub bridges: BTreeMap<DeviceId, StpBridge>,
    pub ports: BTreeMap<PortKey, StpPort>,
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

    // Dijkstra on root path cost, breaking ties as 802.1D does.
    let mut cost: BTreeMap<DeviceId, u32> = BTreeMap::new();
    cost.insert(root_device, 0);
    let mut done: BTreeSet<DeviceId> = BTreeSet::new();
    while let Some((&bridge, &c)) = cost
        .iter()
        .filter(|(b, _)| !done.contains(*b))
        .min_by_key(|(b, c)| (**c, bridge_ids[*b]))
    {
        done.insert(bridge);
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
        bridges: BTreeMap::new(),
        ports: BTreeMap::new(),
    };
    for (&bridge, bridge_ports) in &members {
        let Some(&root_cost) = cost.get(&bridge) else {
            continue;
        };
        // Root port: the best way towards the root through a designated port.
        let root_port = (bridge != root_device)
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
        tree.bridges.insert(
            bridge,
            StpBridge {
                id: bridge_ids[&bridge],
                root_cost,
                root_port: root_port.clone(),
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
            tree.ports.insert(
                key,
                StpPort {
                    role,
                    cost: port_cost(port),
                    number: *n,
                    designated_bridge: bridge_ids[&designated.0],
                },
            );
        }
    }
    Some(tree)
}
