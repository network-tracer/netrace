//! OSPFv2, worked out in its converged state from the topology and the
//! configuration, the way spanning tree is. Every decision keeps the reason
//! it was made: why an interface is in OSPF and in which area, why two
//! routers are (or aren't) neighbours, who won the DR election and why,
//! and why each route took the path it did. The protocol views and `show`
//! commands read these; the routes go into the routing tables as `O`.
//!
//! One OSPF process per router is simulated (the first `router ospf`).

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netrace_doc::{DeviceId, DeviceKind, LinkId, OspfProcess, Project, mask};

use crate::abbreviate_port;
use crate::stack::{Iface, PortStates, Stack};
use crate::stp::Layer2;

/// IOS's administrative distance for OSPF.
pub const OSPF_DISTANCE: u8 = 110;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OspfNetType {
    Broadcast,
    PointToPoint,
    /// Advertised as a /32 host route, whatever its mask.
    Loopback,
}

impl OspfNetType {
    pub fn ios(self) -> &'static str {
        match self {
            OspfNetType::Broadcast => "BROADCAST",
            OspfNetType::PointToPoint => "POINT_TO_POINT",
            OspfNetType::Loopback => "LOOPBACK",
        }
    }
}

/// An interface's role on its segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OspfIfState {
    Dr,
    Bdr,
    DrOther,
    PointToPoint,
    Loopback,
    /// Passive, or alone with nobody to elect.
    Waiting,
}

impl OspfIfState {
    pub fn ios(self) -> &'static str {
        match self {
            OspfIfState::Dr => "DR",
            OspfIfState::Bdr => "BDR",
            OspfIfState::DrOther => "DROTH",
            OspfIfState::PointToPoint => "P2P",
            OspfIfState::Loopback => "LOOP",
            OspfIfState::Waiting => "WAIT",
        }
    }
}

/// One interface running OSPF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OspfIface {
    pub name: String,
    pub address: Ipv4Addr,
    pub prefix_len: u8,
    pub area: u32,
    pub cost: u32,
    pub priority: u8,
    pub hello: u16,
    pub dead: u16,
    pub net_type: OspfNetType,
    pub passive: bool,
    pub state: OspfIfState,
    /// (router ID, interface address) of the segment's DR and BDR.
    pub dr: Option<(Ipv4Addr, Ipv4Addr)>,
    pub bdr: Option<(Ipv4Addr, Ipv4Addr)>,
    /// Why it's in OSPF: the `network` statement or `ip ospf area` that matched.
    pub why: String,
    /// Index into [`Ospf::segments`] (none for loopbacks).
    pub segment: Option<usize>,
}

impl OspfIface {
    pub fn network(&self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.address) & u32::from(mask(self.prefix_len)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NeighborState {
    /// No hellos heard (or they're rejected).
    Down,
    /// Both see each other but stay at 2-Way: neither is DR or BDR.
    TwoWay,
    Full,
}

impl NeighborState {
    pub fn ios(self) -> &'static str {
        match self {
            NeighborState::Down => "DOWN",
            NeighborState::TwoWay => "2WAY",
            NeighborState::Full => "FULL",
        }
    }
}

/// Another router seen (or expected) on one of this router's interfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OspfNeighbor {
    pub device: DeviceId,
    pub router_id: Ipv4Addr,
    pub address: Ipv4Addr,
    pub priority: u8,
    /// The local interface (full name).
    pub iface: String,
    /// The neighbour's interface (full name).
    pub their_iface: String,
    pub state: NeighborState,
    /// The neighbour's role on the segment: "DR", "BDR", "DROTHER" or "-".
    pub role: &'static str,
    pub dead: u16,
    /// Why they're not adjacent (mismatched area, hello timers, ...).
    pub problem: Option<String>,
    /// Plain-language account of the state.
    pub reason: String,
}

/// A broadcast domain with two or more OSPF interfaces, and its election.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OspfSegment {
    /// (device, interface) of every OSPF interface on it.
    pub members: Vec<(DeviceId, String)>,
    pub network: Ipv4Addr,
    pub prefix_len: u8,
    pub point_to_point: bool,
    pub dr: Option<(DeviceId, String)>,
    pub bdr: Option<(DeviceId, String)>,
    pub election: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OspfRouteKind {
    /// `O`: inside the router's own area.
    Intra,
    /// `O IA`: from another area, through an area border router.
    Inter,
    /// `O*E2`: redistributed (here, `default-information originate`).
    External2,
}

/// A route OSPF chose, with the path and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OspfRoute {
    pub network: Ipv4Addr,
    pub prefix_len: u8,
    pub kind: OspfRouteKind,
    pub area: u32,
    pub cost: u32,
    /// Equal-cost (next hop, exit interface) pairs, all installed.
    pub next_hops: Vec<(Ipv4Addr, String)>,
    /// Router names along the chosen path, this router first.
    pub path: Vec<String>,
    pub reason: String,
    /// Other ways it could have gone, and what they'd cost.
    pub alternatives: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LsaKind {
    Router,
    Network,
    Summary,
    External,
}

/// One link-state advertisement in an area's database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lsa {
    pub kind: LsaKind,
    pub area: u32,
    pub link_id: Ipv4Addr,
    pub adv_router: Ipv4Addr,
    pub mask: Option<u8>,
    /// Router LSAs: the number of links; summaries and externals: the metric.
    pub count: u32,
    /// What it says, in words, for the inspector.
    pub describe: String,
}

/// OSPF on one router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OspfRouter {
    pub device: DeviceId,
    pub process_id: u16,
    /// `None` when OSPF can't start: no router ID available.
    pub router_id: Option<Ipv4Addr>,
    pub router_id_reason: String,
    pub ifaces: Vec<OspfIface>,
    pub neighbors: Vec<OspfNeighbor>,
    pub routes: Vec<OspfRoute>,
    pub abr: bool,
    pub asbr: bool,
}

impl OspfRouter {
    pub fn areas(&self) -> BTreeSet<u32> {
        self.ifaces.iter().map(|i| i.area).collect()
    }
}

/// The whole OSPF picture.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ospf {
    pub routers: BTreeMap<DeviceId, OspfRouter>,
    pub segments: Vec<OspfSegment>,
    pub lsdb: BTreeMap<u32, Vec<Lsa>>,
    /// Type 5 LSAs: they belong to no area.
    pub external: Vec<Lsa>,
}

/// Interface bandwidth in kbit/s, for the default cost.
fn bandwidth_kbps(name: &str) -> u64 {
    if name.starts_with("FastEthernet") {
        100_000
    } else if name.starts_with("Ethernet") {
        10_000
    } else if name.starts_with("Loopback") {
        8_000_000
    } else {
        1_000_000
    }
}

/// (area, ABR) → the (prefix, length, cost) summaries it sends into that area.
type Summaries = BTreeMap<(u32, DeviceId), Vec<(Ipv4Addr, u8, u32)>>;

/// Node of the SPF graph: a router, or a transit network (by its DR's address).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Node {
    Router(DeviceId),
    Net(usize),
}

/// What SPF found from one router in one area.
struct Spf {
    dist: BTreeMap<Node, u32>,
    hops: BTreeMap<Node, BTreeSet<(Ipv4Addr, String)>>,
    parent: BTreeMap<Node, Node>,
}

impl Ospf {
    /// OSPF for the whole project, from scratch.
    pub fn for_project(project: &Project) -> Ospf {
        let (ports, l2) = crate::stp::analyse(project);
        Ospf::compute(project, &ports, &l2)
    }

    pub(crate) fn compute(project: &Project, ports: &PortStates, l2: &Layer2) -> Ospf {
        let mut ospf = Ospf::default();
        let stacks: BTreeMap<DeviceId, Stack> = project
            .devices()
            .iter()
            .filter(|d| d.kind.has_cli())
            .filter_map(|d| Some((d.id, Stack::build_base(project, d.id, ports, l2)?)))
            .collect();

        // Router IDs and OSPF interfaces.
        for d in project.devices() {
            let routing = d.kind == DeviceKind::Router
                || (d.kind == DeviceKind::L3Switch && d.config.ip_routing);
            let (Some(process), true) = (d.config.ospf.first(), routing) else {
                continue;
            };
            let Some(stack) = stacks.get(&d.id) else {
                continue;
            };
            let (router_id, router_id_reason) = choose_router_id(process, stack);
            let mut ifaces = Vec::new();
            for iface in stack.ifaces.iter().filter(|i| i.up) {
                let Some(addr) = iface.address else { continue };
                let config = d.config.interface(d.kind, &iface.name).ospf;
                let (area, why) = match config.area {
                    Some((pid, area)) if pid == process.process_id => (
                        area,
                        format!(
                            "`ip ospf {pid} area {area}` on {} puts it in area {area}",
                            abbreviate_port(&iface.name)
                        ),
                    ),
                    _ => match process.network_for(addr.address) {
                        Some(n) => (
                            n.area,
                            format!(
                                "`network {} {} area {}` matches {}",
                                n.address, n.wildcard, n.area, addr.address
                            ),
                        ),
                        None => continue,
                    },
                };
                let loopback = iface.name.starts_with("Loopback");
                let net_type = if loopback {
                    OspfNetType::Loopback
                } else if config.point_to_point {
                    OspfNetType::PointToPoint
                } else {
                    OspfNetType::Broadcast
                };
                let cost = config.cost.unwrap_or_else(|| {
                    let reference = u64::from(process.reference_bandwidth) * 1000;
                    let port = iface.port.as_deref().unwrap_or(&iface.name);
                    (reference / bandwidth_kbps(port)).max(1) as u32
                });
                ifaces.push(OspfIface {
                    name: iface.name.clone(),
                    address: addr.address,
                    prefix_len: addr.prefix_len,
                    area,
                    cost,
                    priority: config.priority(),
                    hello: config.hello_interval(),
                    dead: config.dead_interval(),
                    net_type,
                    passive: process.is_passive(&iface.name),
                    state: if loopback {
                        OspfIfState::Loopback
                    } else {
                        OspfIfState::Waiting
                    },
                    dr: None,
                    bdr: None,
                    why,
                    segment: None,
                });
            }
            let abr = {
                let areas: BTreeSet<u32> = ifaces.iter().map(|i| i.area).collect();
                areas.contains(&0) && areas.len() > 1
            };
            let asbr = process.default_originate && stack.routes.iter().any(|r| r.is_default());
            ospf.routers.insert(
                d.id,
                OspfRouter {
                    device: d.id,
                    process_id: process.process_id,
                    router_id,
                    router_id_reason,
                    ifaces,
                    neighbors: Vec::new(),
                    routes: Vec::new(),
                    abr,
                    asbr,
                },
            );
        }

        ospf.find_segments(project, ports, l2, &stacks);
        ospf.form_adjacencies(project);
        ospf.build_lsdb(project);
        ospf.compute_routes(project);
        ospf
    }

    /// The OSPF picture after `link` fails, and what changed, in the order
    /// it matters: lost neighbours, then routes that moved or vanished.
    pub fn after_link_failure(project: &Project, link: LinkId) -> (Ospf, Vec<OspfChange>) {
        let (ports, l2) = crate::stp::analyse(project);
        let before = Ospf::compute(project, &ports, &l2);
        let mut failed = project.clone();
        // A replay, not an edit: the lab's locks don't apply.
        failed.set_lab(None);
        let _ = failed.disconnect(link);
        let (ports, l2) = crate::stp::analyse(&failed);
        let after = Ospf::compute(&failed, &ports, &l2);
        let name = |id: DeviceId| {
            project
                .device(id)
                .map(|d| d.name.clone())
                .unwrap_or_default()
        };
        let mut changes = Vec::new();
        for (id, old) in &before.routers {
            let new = after.routers.get(id);
            for n in old
                .neighbors
                .iter()
                .filter(|n| n.state == NeighborState::Full)
            {
                let still = new.is_some_and(|r| {
                    r.neighbors.iter().any(|m| {
                        m.device == n.device && m.iface == n.iface && m.state == NeighborState::Full
                    })
                });
                if !still {
                    changes.push(OspfChange {
                        device: *id,
                        network: None,
                        explanation: format!(
                            "{} loses its adjacency with {} on {}: the interface went down, so it doesn't wait for \
                             the {} s dead timer. Both routers flood new LSAs without the link.",
                            name(*id),
                            name(n.device),
                            abbreviate_port(&n.iface),
                            n.dead
                        ),
                    });
                }
            }
        }
        for (id, old) in &before.routers {
            let new = after.routers.get(id);
            for r in &old.routes {
                let replacement = new.and_then(|n| {
                    n.routes
                        .iter()
                        .find(|x| x.network == r.network && x.prefix_len == r.prefix_len)
                });
                let prefix = format!("{}/{}", r.network, r.prefix_len);
                let explanation = match replacement {
                    None => format!(
                        "{} loses its route to {prefix}: no path is left in the link-state database.",
                        name(*id)
                    ),
                    Some(x) if x.next_hops != r.next_hops || x.cost != r.cost => format!(
                        "{} reruns SPF: {prefix} now goes {} at cost {} (was {} at cost {}).",
                        name(*id),
                        x.path.join(" → "),
                        x.cost,
                        r.path.join(" → "),
                        r.cost
                    ),
                    Some(_) => continue,
                };
                changes.push(OspfChange {
                    device: *id,
                    network: Some((r.network, r.prefix_len)),
                    explanation,
                });
            }
        }
        (after, changes)
    }

    pub fn router(&self, id: DeviceId) -> Option<&OspfRouter> {
        self.routers.get(&id)
    }

    fn rid(&self, id: DeviceId) -> Ipv4Addr {
        self.routers
            .get(&id)
            .and_then(|r| r.router_id)
            .unwrap_or(Ipv4Addr::UNSPECIFIED)
    }

    fn iface(&self, id: DeviceId, name: &str) -> Option<&OspfIface> {
        self.routers
            .get(&id)?
            .ifaces
            .iter()
            .find(|i| i.name == name)
    }

    /// Groups OSPF interfaces by the broadcast domain they're on.
    fn find_segments(
        &mut self,
        project: &Project,
        ports: &PortStates,
        l2: &Layer2,
        stacks: &BTreeMap<DeviceId, Stack>,
    ) {
        let mut assigned: BTreeSet<(DeviceId, String)> = BTreeSet::new();
        let keys: Vec<(DeviceId, String)> = self
            .routers
            .iter()
            .flat_map(|(id, r)| {
                r.ifaces
                    .iter()
                    .filter(|i| i.net_type != OspfNetType::Loopback)
                    .map(move |i| (*id, i.name.clone()))
            })
            .collect();
        for key in keys {
            if assigned.contains(&key) {
                continue;
            }
            let Some(iface) = stacks.get(&key.0).and_then(|s| s.iface(&key.1)) else {
                continue;
            };
            let domain = broadcast_domain(project, ports, l2, stacks, key.0, iface);
            let members: Vec<(DeviceId, String)> = domain
                .into_iter()
                .filter(|m| self.iface(m.0, &m.1).is_some())
                .collect();
            let index = self.segments.len();
            for m in &members {
                assigned.insert(m.clone());
                if let Some(i) = self
                    .routers
                    .get_mut(&m.0)
                    .and_then(|r| r.ifaces.iter_mut().find(|i| i.name == m.1))
                {
                    i.segment = Some(index);
                }
            }
            let first = self.iface(key.0, &key.1).expect("listed above").clone();
            self.segments.push(OspfSegment {
                point_to_point: first.net_type == OspfNetType::PointToPoint,
                network: first.network(),
                prefix_len: first.prefix_len,
                members,
                dr: None,
                bdr: None,
                election: String::new(),
            });
        }
    }

    /// Neighbour checks, DR / BDR elections and neighbour states.
    fn form_adjacencies(&mut self, project: &Project) {
        let name = |id: DeviceId| {
            project
                .device(id)
                .map(|d| d.name.clone())
                .unwrap_or_default()
        };
        // Duplicate router IDs.
        let mut by_rid: BTreeMap<Ipv4Addr, Vec<DeviceId>> = BTreeMap::new();
        for (id, r) in &self.routers {
            if let Some(rid) = r.router_id {
                by_rid.entry(rid).or_default().push(*id);
            }
        }

        for index in 0..self.segments.len() {
            let members = self.segments[index].members.clone();
            // Who can hear whom: (a, b) → problem.
            let mut problems: BTreeMap<(usize, usize), Option<String>> = BTreeMap::new();
            for (ai, a) in members.iter().enumerate() {
                for (bi, b) in members.iter().enumerate() {
                    if ai == bi || a.0 == b.0 {
                        continue;
                    }
                    let (ia, ib) = (
                        self.iface(a.0, &a.1).expect("member"),
                        self.iface(b.0, &b.1).expect("member"),
                    );
                    let (na, nb) = (name(a.0), name(b.0));
                    let (pa, pb) = (abbreviate_port(&ia.name), abbreviate_port(&ib.name));
                    let problem = if self.routers[&a.0].router_id.is_none()
                        || self.routers[&b.0].router_id.is_none()
                    {
                        Some("OSPF isn't running: there's no router ID (no interface has an IP address)".into())
                    } else if ia.passive || ib.passive {
                        let who = if ib.passive { (&nb, &pb) } else { (&na, &pa) };
                        Some(format!(
                            "{} {} is a passive interface: it advertises its network but sends no hellos, so no \
                             neighbour forms there",
                            who.0, who.1
                        ))
                    } else if ia.network() != ib.network() || ia.prefix_len != ib.prefix_len {
                        Some(format!(
                            "{na} {pa} is {}/{} and {nb} {pb} is {}/{}: different subnets (or masks), so they \
                             reject each other's hellos",
                            ia.address, ia.prefix_len, ib.address, ib.prefix_len
                        ))
                    } else if ia.area != ib.area {
                        Some(format!(
                            "{na} {pa} is in area {} and {nb} {pb} is in area {}: hellos carry the area, and they \
                             must match",
                            ia.area, ib.area
                        ))
                    } else if ia.hello != ib.hello || ia.dead != ib.dead {
                        Some(format!(
                            "hello/dead timers differ ({na} {}/{} s, {nb} {}/{} s): they must match exactly",
                            ia.hello, ia.dead, ib.hello, ib.dead
                        ))
                    } else if self.routers[&a.0].router_id == self.routers[&b.0].router_id {
                        Some(format!(
                            "{na} and {nb} both use router ID {}: router IDs must be unique",
                            self.rid(a.0)
                        ))
                    } else if (ia.net_type == OspfNetType::PointToPoint)
                        != (ib.net_type == OspfNetType::PointToPoint)
                    {
                        Some(format!(
                            "{na} {pa} is {} and {nb} {pb} is {}: with different network types they don't agree on \
                             a DR, and routes between them break",
                            ia.net_type.ios().to_lowercase(),
                            ib.net_type.ios().to_lowercase()
                        ))
                    } else {
                        None
                    };
                    problems.insert((ai, bi), problem);
                }
            }
            let _ = &by_rid;

            // DR / BDR election among those that can form adjacencies.
            let seg = &self.segments[index];
            let mut election;
            let (mut dr, mut bdr) = (None, None);
            if !seg.point_to_point {
                let mut candidates: Vec<usize> = (0..members.len())
                    .filter(|i| {
                        let iface = self.iface(members[*i].0, &members[*i].1).expect("member");
                        !iface.passive
                            && iface.priority > 0
                            && self.routers[&members[*i].0].router_id.is_some()
                    })
                    .collect();
                candidates.sort_by_key(|i| {
                    let iface = self.iface(members[*i].0, &members[*i].1).expect("member");
                    std::cmp::Reverse((iface.priority, self.rid(members[*i].0)))
                });
                let describe = |i: usize| {
                    let iface = self.iface(members[i].0, &members[i].1).expect("member");
                    format!(
                        "{} (priority {}, router ID {})",
                        name(members[i].0),
                        iface.priority,
                        self.rid(members[i].0)
                    )
                };
                dr = candidates.first().copied();
                bdr = candidates.get(1).copied();
                election = match (dr, bdr) {
                    (Some(d), Some(b)) => {
                        let (pd, pb) = (
                            self.iface(members[d].0, &members[d].1)
                                .expect("member")
                                .priority,
                            self.iface(members[b].0, &members[b].1)
                                .expect("member")
                                .priority,
                        );
                        let why = if pd > pb {
                            "the highest priority"
                        } else {
                            "the same priority, so the highest router ID decides"
                        };
                        format!(
                            "{} is DR: {why}. {} is BDR, next in line. Everyone forms a full adjacency with the DR \
                             and BDR only. (Real routers keep an existing DR even if a better one joins later; \
                             netrace shows the election as if all started together.)",
                            describe(d),
                            describe(b)
                        )
                    }
                    (Some(d), None) => format!(
                        "{} is DR: it's the only router here that can be elected, so there's no BDR.",
                        describe(d)
                    ),
                    _ => "No DR: every router here is passive or has priority 0.".into(),
                };
                let zero: Vec<String> = members
                    .iter()
                    .filter(|m| self.iface(m.0, &m.1).is_some_and(|i| i.priority == 0))
                    .map(|m| name(m.0))
                    .collect();
                if !zero.is_empty() {
                    election +=
                        &format!(" Priority 0 keeps {} out of the election.", zero.join(", "));
                }
            } else {
                election =
                    "Point-to-point: no DR or BDR, the two routers simply become fully adjacent."
                        .into();
            }
            let dr_key = dr.map(|i| members[i].clone());
            let bdr_key = bdr.map(|i| members[i].clone());
            let dr_info = dr_key.as_ref().map(|k| {
                (
                    self.rid(k.0),
                    self.iface(k.0, &k.1).expect("member").address,
                )
            });
            let bdr_info = bdr_key.as_ref().map(|k| {
                (
                    self.rid(k.0),
                    self.iface(k.0, &k.1).expect("member").address,
                )
            });
            let point_to_point = seg.point_to_point;
            let lonely = members.len() < 2;

            // Interface states.
            for (i, m) in members.iter().enumerate() {
                let state = if point_to_point {
                    OspfIfState::PointToPoint
                } else if Some(i) == dr {
                    OspfIfState::Dr
                } else if Some(i) == bdr {
                    OspfIfState::Bdr
                } else if self.iface(m.0, &m.1).is_some_and(|x| x.passive) {
                    OspfIfState::Waiting
                } else {
                    OspfIfState::DrOther
                };
                let own_rid = self.routers.get(&m.0).and_then(|r| r.router_id);
                if let Some(x) = self
                    .routers
                    .get_mut(&m.0)
                    .and_then(|r| r.ifaces.iter_mut().find(|x| x.name == m.1))
                {
                    x.state = if lonely && !point_to_point && !x.passive {
                        OspfIfState::Dr
                    } else {
                        state
                    };
                    x.dr = if lonely && !x.passive {
                        own_rid.map(|rid| (rid, x.address))
                    } else {
                        dr_info
                    };
                    x.bdr = bdr_info;
                }
            }

            // Neighbour tables.
            for (ai, a) in members.iter().enumerate() {
                for (bi, b) in members.iter().enumerate() {
                    let Some(problem) = problems.get(&(ai, bi)).cloned() else {
                        continue;
                    };
                    let ib = self.iface(b.0, &b.1).expect("member").clone();
                    let role = if point_to_point {
                        "-"
                    } else if Some(bi) == dr {
                        "DR"
                    } else if Some(bi) == bdr {
                        "BDR"
                    } else {
                        "DROTHER"
                    };
                    let (na, nb) = (name(a.0), name(b.0));
                    let (state, reason) = match &problem {
                        Some(p) => (
                            NeighborState::Down,
                            format!("{na} and {nb} don't become neighbours: {p}."),
                        ),
                        None if point_to_point => (
                            NeighborState::Full,
                            format!(
                                "Hellos match on a point-to-point link, so {na} and {nb} go straight from 2-Way through \
                                 ExStart, Exchange and Loading to Full: their databases are in sync."
                            ),
                        ),
                        None if Some(ai) == dr
                            || Some(ai) == bdr
                            || Some(bi) == dr
                            || Some(bi) == bdr =>
                        {
                            (
                                NeighborState::Full,
                                format!(
                                    "Hellos match (area {}, {}/{} s timers, same subnet). {} is {}, so they exchange \
                                     databases (ExStart → Exchange → Loading) and reach Full.",
                                    ib.area,
                                    ib.hello,
                                    ib.dead,
                                    // Name the DR if either is; otherwise the BDR.
                                    match () {
                                        _ if Some(bi) == dr => &nb,
                                        _ if Some(ai) == dr => &na,
                                        _ if Some(bi) == bdr => &nb,
                                        _ => &na,
                                    },
                                    if Some(bi) == dr || Some(ai) == dr {
                                        "the DR"
                                    } else {
                                        "the BDR"
                                    }
                                ),
                            )
                        }
                        None => (
                            NeighborState::TwoWay,
                            format!(
                                "Hellos match, but neither {na} nor {nb} is DR or BDR, so they stay at 2-Way: each \
                                 syncs its database with the DR and BDR instead. That's normal."
                            ),
                        ),
                    };
                    let neighbor = OspfNeighbor {
                        device: b.0,
                        router_id: self.rid(b.0),
                        address: ib.address,
                        priority: ib.priority,
                        iface: a.1.clone(),
                        their_iface: b.1.clone(),
                        state,
                        role,
                        dead: ib.dead,
                        problem,
                        reason,
                    };
                    if let Some(r) = self.routers.get_mut(&a.0) {
                        r.neighbors.push(neighbor);
                    }
                }
            }
            let seg = &mut self.segments[index];
            seg.dr = dr_key;
            seg.bdr = bdr_key;
            seg.election = election;
        }
    }

    fn full(&self, a: DeviceId, iface: &str, b: DeviceId) -> bool {
        self.routers.get(&a).is_some_and(|r| {
            r.neighbors
                .iter()
                .any(|n| n.iface == iface && n.device == b && n.state == NeighborState::Full)
        })
    }

    /// Whether a segment is a transit network: it has a DR fully adjacent to
    /// at least one other router.
    fn transit(&self, index: usize) -> bool {
        let seg = &self.segments[index];
        !seg.point_to_point
            && seg.dr.as_ref().is_some_and(|(d, i)| {
                self.routers[d]
                    .neighbors
                    .iter()
                    .any(|n| n.iface == *i && n.state == NeighborState::Full)
            })
    }

    fn build_lsdb(&mut self, project: &Project) {
        let name = |id: DeviceId| {
            project
                .device(id)
                .map(|d| d.name.clone())
                .unwrap_or_default()
        };
        let mut lsdb: BTreeMap<u32, Vec<Lsa>> = BTreeMap::new();
        for (id, r) in &self.routers {
            let Some(rid) = r.router_id else { continue };
            for area in r.areas() {
                let mut links = Vec::new();
                for i in r.ifaces.iter().filter(|i| i.area == area) {
                    match (i.net_type, i.segment) {
                        (OspfNetType::Loopback, _) => links.push(format!(
                            "stub {}/32 (loopback {})",
                            i.address,
                            abbreviate_port(&i.name)
                        )),
                        (_, Some(s)) if self.transit(s) && self.full_with_dr(*id, i) => {
                            links.push(format!(
                                "transit network {}/{} via DR {}",
                                i.network(),
                                i.prefix_len,
                                i.dr.map(|d| d.1).unwrap_or(i.address)
                            ))
                        }
                        (OspfNetType::PointToPoint, Some(s)) => {
                            for n in r
                                .neighbors
                                .iter()
                                .filter(|n| n.iface == i.name && n.state == NeighborState::Full)
                            {
                                links.push(format!(
                                    "point-to-point to {} ({})",
                                    n.router_id,
                                    name(n.device)
                                ));
                            }
                            let _ = s;
                            links.push(format!("stub {}/{}", i.network(), i.prefix_len));
                        }
                        _ => links.push(format!("stub {}/{}", i.network(), i.prefix_len)),
                    }
                }
                lsdb.entry(area).or_default().push(Lsa {
                    kind: LsaKind::Router,
                    area,
                    link_id: rid,
                    adv_router: rid,
                    mask: None,
                    count: links.len() as u32,
                    describe: format!("{} ({rid}): {}", name(*id), links.join("; ")),
                });
            }
        }
        for (index, seg) in self.segments.iter().enumerate() {
            if !self.transit(index) {
                continue;
            }
            let Some((d, i)) = &seg.dr else { continue };
            let dr_iface = self.iface(*d, i).expect("member");
            let mut attached = vec![name(*d)];
            for m in &seg.members {
                if m.0 != *d && self.full(m.0, &m.1, *d) {
                    attached.push(name(m.0));
                }
            }
            lsdb.entry(dr_iface.area).or_default().push(Lsa {
                kind: LsaKind::Network,
                area: dr_iface.area,
                link_id: dr_iface.address,
                adv_router: self.rid(*d),
                mask: Some(seg.prefix_len),
                count: attached.len() as u32,
                describe: format!(
                    "Network {}/{} from DR {}: attached {}",
                    seg.network,
                    seg.prefix_len,
                    name(*d),
                    attached.join(", ")
                ),
            });
        }
        self.lsdb = lsdb;
    }

    fn full_with_dr(&self, id: DeviceId, i: &OspfIface) -> bool {
        let Some(s) = i.segment else { return false };
        let Some((d, di)) = &self.segments[s].dr else {
            return false;
        };
        if *d == id {
            return self.routers[&id]
                .neighbors
                .iter()
                .any(|n| n.iface == *di && n.state == NeighborState::Full);
        }
        self.full(id, &i.name, *d)
    }

    /// SPF graph edges in `area` out of `node`: (neighbour node, cost, the
    /// local interface used when leaving the root that way).
    fn edges(&self, area: u32, node: Node) -> Vec<(Node, u32, String)> {
        let mut out = Vec::new();
        match node {
            Node::Router(id) => {
                let Some(r) = self.routers.get(&id) else {
                    return out;
                };
                for i in r.ifaces.iter().filter(|i| i.area == area) {
                    let Some(s) = i.segment else { continue };
                    if self.transit(s) && self.full_with_dr(id, i) {
                        out.push((Node::Net(s), i.cost, i.name.clone()));
                    } else if i.net_type == OspfNetType::PointToPoint {
                        for n in r
                            .neighbors
                            .iter()
                            .filter(|n| n.iface == i.name && n.state == NeighborState::Full)
                        {
                            out.push((Node::Router(n.device), i.cost, i.name.clone()));
                        }
                    }
                }
            }
            Node::Net(s) => {
                let seg = &self.segments[s];
                let Some((d, _)) = &seg.dr else { return out };
                for m in &seg.members {
                    if m.0 == *d || self.full(m.0, &m.1, *d) {
                        out.push((Node::Router(m.0), 0, m.1.clone()));
                    }
                }
            }
        }
        out
    }

    /// Dijkstra from `root` over `area`, keeping every equal-cost first hop.
    fn spf(&self, root: DeviceId, area: u32) -> Spf {
        let mut dist: BTreeMap<Node, u32> = BTreeMap::new();
        let mut hops: BTreeMap<Node, BTreeSet<(Ipv4Addr, String)>> = BTreeMap::new();
        let mut parent: BTreeMap<Node, Node> = BTreeMap::new();
        let mut done: BTreeSet<Node> = BTreeSet::new();
        dist.insert(Node::Router(root), 0);
        hops.insert(Node::Router(root), BTreeSet::new());
        while let Some((&node, &d)) = dist
            .iter()
            .filter(|(n, _)| !done.contains(n))
            // At equal distance, networks first: their zero-cost edges can
            // still reach a router by another equal-cost path.
            .min_by_key(|(n, d)| (**d, matches!(n, Node::Router(_)), **n))
        {
            done.insert(node);
            for (next, cost, via) in self.edges(area, node) {
                let nd = d + cost;
                // First hops: leaving the root, or crossing a network the root is on.
                let first: BTreeSet<(Ipv4Addr, String)> = match (node, next) {
                    (Node::Router(r), Node::Router(n)) if r == root => self.routers[&root]
                        .neighbors
                        .iter()
                        .filter(|x| x.device == n && x.iface == via)
                        .map(|x| (x.address, via.clone()))
                        .collect(),
                    (Node::Router(r), Node::Net(_)) if r == root => {
                        [(Ipv4Addr::UNSPECIFIED, via.clone())].into()
                    }
                    (Node::Net(_), Node::Router(n))
                        if hops[&node].iter().any(|h| h.0.is_unspecified()) =>
                    {
                        let out_iface = hops[&node].iter().next().expect("one").1.clone();
                        match self.iface(n, &via) {
                            Some(x) if n != root => [(x.address, out_iface)].into(),
                            _ => BTreeSet::new(),
                        }
                    }
                    _ => hops[&node].clone(),
                };
                match dist.get(&next) {
                    Some(&old) if old < nd => {}
                    Some(&old) if old == nd => {
                        if !done.contains(&next) {
                            hops.entry(next).or_default().extend(first);
                        }
                    }
                    _ => {
                        dist.insert(next, nd);
                        hops.insert(next, first);
                        parent.insert(next, node);
                    }
                }
            }
        }
        Spf { dist, hops, parent }
    }

    /// Router names from the root to `node`.
    fn path(&self, project: &Project, spf: &Spf, node: Node) -> Vec<String> {
        let mut path = Vec::new();
        let mut at = Some(node);
        while let Some(n) = at {
            if let Node::Router(id) = n {
                path.push(
                    project
                        .device(id)
                        .map(|d| d.name.clone())
                        .unwrap_or_default(),
                );
            }
            at = spf.parent.get(&n).copied();
        }
        path.reverse();
        path
    }

    fn compute_routes(&mut self, project: &Project) {
        let name = |id: DeviceId| {
            project
                .device(id)
                .map(|d| d.name.clone())
                .unwrap_or_default()
        };
        let ids: Vec<DeviceId> = self
            .routers
            .iter()
            .filter(|(_, r)| r.router_id.is_some())
            .map(|(id, _)| *id)
            .collect();
        // Prefixes each router advertises in each area: (net, len, cost, where).
        let stubs = |r: &OspfRouter, area: u32| -> Vec<(Ipv4Addr, u8, u32, String)> {
            r.ifaces
                .iter()
                .filter(|i| i.area == area)
                .map(|i| match i.net_type {
                    OspfNetType::Loopback => (i.address, 32, i.cost, abbreviate_port(&i.name)),
                    _ => (i.network(), i.prefix_len, i.cost, abbreviate_port(&i.name)),
                })
                .collect()
        };

        // Intra-area routes.
        let mut intra: BTreeMap<DeviceId, Vec<OspfRoute>> = BTreeMap::new();
        let mut spfs: BTreeMap<(DeviceId, u32), Spf> = BTreeMap::new();
        for &root in &ids {
            let mut best: BTreeMap<(Ipv4Addr, u8), OspfRoute> = BTreeMap::new();
            let own: BTreeSet<(Ipv4Addr, u8)> = self.routers[&root]
                .ifaces
                .iter()
                .flat_map(|i| [(i.network(), i.prefix_len), (i.address, 32)])
                .collect();
            for area in self.routers[&root].areas() {
                let spf = self.spf(root, area);
                for (&node, &d) in &spf.dist {
                    let (prefixes, owner): (Vec<(Ipv4Addr, u8, u32, String)>, String) = match node {
                        Node::Router(id) if id != root => {
                            (stubs(&self.routers[&id], area), name(id))
                        }
                        Node::Net(s) => {
                            let seg = &self.segments[s];
                            (
                                vec![(seg.network, seg.prefix_len, 0, String::new())],
                                "a shared network".into(),
                            )
                        }
                        _ => continue,
                    };
                    for (net, len, cost, iface) in prefixes {
                        // A transit network's stub entry is covered by its network node.
                        if matches!(node, Node::Router(_))
                            && self.segments.iter().enumerate().any(|(i, s)| {
                                s.network == net
                                    && s.prefix_len == len
                                    && spf.dist.contains_key(&Node::Net(i))
                            })
                        {
                            continue;
                        }
                        if own.contains(&(net, len)) {
                            continue;
                        }
                        let total = d + cost;
                        let next_hops: Vec<(Ipv4Addr, String)> = spf.hops[&node]
                            .iter()
                            .filter(|h| !h.0.is_unspecified())
                            .cloned()
                            .collect();
                        if next_hops.is_empty() {
                            continue;
                        }
                        let path = self.path(project, &spf, node);
                        let reason = if iface.is_empty() {
                            format!(
                                "{net}/{len} is a shared network in area {area}. Cheapest path: {}, total cost {total}.",
                                path.join(" → ")
                            )
                        } else {
                            format!(
                                "{net}/{len} is on {owner} ({iface}, cost {cost}) in area {area}. Cheapest path: {}, \
                                 total cost {total} (each router adds the cost of the interface it sends out of).",
                                path.join(" → ")
                            )
                        };
                        let candidate = OspfRoute {
                            network: net,
                            prefix_len: len,
                            kind: OspfRouteKind::Intra,
                            area,
                            cost: total,
                            next_hops,
                            path,
                            reason,
                            alternatives: Vec::new(),
                        };
                        match best.get_mut(&(net, len)) {
                            Some(b) if b.cost < total => {}
                            Some(b) if b.cost == total => {
                                for h in candidate.next_hops {
                                    if !b.next_hops.contains(&h) {
                                        b.next_hops.push(h);
                                    }
                                }
                            }
                            _ => {
                                best.insert((net, len), candidate);
                            }
                        }
                    }
                }
                spfs.insert((root, area), spf);
            }
            intra.insert(root, best.into_values().collect());
        }

        // Inter-area: summaries from area border routers.
        let abrs: Vec<DeviceId> = ids
            .iter()
            .copied()
            .filter(|id| self.routers[id].abr)
            .collect();
        // (area, abr) → (net, len, cost) the ABR advertises into that area.
        let mut summaries: Summaries = BTreeMap::new();
        let intra_in = |id: DeviceId, area: u32| -> Vec<(Ipv4Addr, u8, u32)> {
            let mut v: Vec<(Ipv4Addr, u8, u32)> = intra[&id]
                .iter()
                .filter(|r| r.area == area)
                .map(|r| (r.network, r.prefix_len, r.cost))
                .collect();
            // Its own networks in that area too.
            for i in self.routers[&id].ifaces.iter().filter(|i| i.area == area) {
                let p = match i.net_type {
                    OspfNetType::Loopback => (i.address, 32, i.cost),
                    _ => (i.network(), i.prefix_len, i.cost),
                };
                v.push(p);
            }
            v
        };
        for &b in &abrs {
            let areas = self.routers[&b].areas();
            let mut into0 = Vec::new();
            for &a in areas.iter().filter(|a| **a != 0) {
                into0.extend(intra_in(b, a));
            }
            summaries.insert((0, b), into0);
        }
        // Inter-area routes in the backbone, then what ABRs pass into other areas.
        let mut inter: BTreeMap<DeviceId, Vec<OspfRoute>> = BTreeMap::new();
        let ia_routes = |ospf: &Ospf,
                         root: DeviceId,
                         area: u32,
                         summaries: &Summaries,
                         known: &BTreeSet<(Ipv4Addr, u8)>|
         -> Vec<OspfRoute> {
            let Some(spf) = spfs.get(&(root, area)) else {
                return Vec::new();
            };
            let mut best: BTreeMap<(Ipv4Addr, u8), OspfRoute> = BTreeMap::new();
            for (&(a, abr), list) in summaries {
                if a != area || abr == root {
                    continue;
                }
                let Some(&d) = spf.dist.get(&Node::Router(abr)) else {
                    continue;
                };
                for &(net, len, cost) in list {
                    if known.contains(&(net, len)) {
                        continue;
                    }
                    let total = d + cost;
                    let next_hops: Vec<(Ipv4Addr, String)> = spf.hops[&Node::Router(abr)]
                        .iter()
                        .filter(|h| !h.0.is_unspecified())
                        .cloned()
                        .collect();
                    if next_hops.is_empty() {
                        continue;
                    }
                    let path = ospf.path(project, spf, Node::Router(abr));
                    let candidate = OspfRoute {
                        network: net,
                        prefix_len: len,
                        kind: OspfRouteKind::Inter,
                        area,
                        cost: total,
                        next_hops,
                        reason: format!(
                            "{net}/{len} is in another area. Area border router {} advertises it into area {area} with \
                             a summary LSA (cost {cost}); reaching {} costs {d}, so the total is {total}.",
                            name(abr),
                            name(abr)
                        ),
                        path,
                        alternatives: Vec::new(),
                    };
                    match best.get_mut(&(net, len)) {
                        Some(b) if b.cost <= total => {}
                        _ => {
                            best.insert((net, len), candidate);
                        }
                    }
                }
            }
            best.into_values().collect()
        };
        let known_of = |id: DeviceId| -> BTreeSet<(Ipv4Addr, u8)> {
            let mut k: BTreeSet<(Ipv4Addr, u8)> = intra[&id]
                .iter()
                .map(|r| (r.network, r.prefix_len))
                .collect();
            for i in &self.routers[&id].ifaces {
                k.insert((i.network(), i.prefix_len));
                k.insert((i.address, 32));
            }
            k
        };
        for &root in &ids {
            if self.routers[&root].areas().contains(&0) {
                inter.insert(root, ia_routes(self, root, 0, &summaries, &known_of(root)));
            }
        }
        for &b in &abrs {
            let areas = self.routers[&b].areas();
            for &a in areas.iter().filter(|a| **a != 0) {
                let mut list: Vec<(Ipv4Addr, u8, u32)> = intra_in(b, 0);
                list.extend(
                    inter
                        .get(&b)
                        .into_iter()
                        .flatten()
                        .map(|r| (r.network, r.prefix_len, r.cost)),
                );
                for &other in areas.iter().filter(|o| **o != 0 && **o != a) {
                    list.extend(intra_in(b, other));
                }
                summaries.insert((a, b), list);
            }
        }
        for &root in &ids {
            if self.routers[&root].abr {
                continue;
            }
            for area in self.routers[&root].areas().into_iter().filter(|a| *a != 0) {
                let routes = ia_routes(self, root, area, &summaries, &known_of(root));
                inter.entry(root).or_default().extend(routes);
            }
        }

        // Default route from an ASBR with `default-information originate`.
        let asbrs: Vec<DeviceId> = ids
            .iter()
            .copied()
            .filter(|id| self.routers[id].asbr)
            .collect();
        let mut external: BTreeMap<DeviceId, OspfRoute> = BTreeMap::new();
        for &root in &ids {
            if self.routers[&root].asbr {
                continue;
            }
            let mut best: Option<(u32, OspfRoute)> = None;
            for &asbr in &asbrs {
                // Same area: straight there. Otherwise through the nearest ABR.
                let mut reach: Option<(u32, DeviceId, u32)> = None;
                for area in self.routers[&root].areas() {
                    let Some(spf) = spfs.get(&(root, area)) else {
                        continue;
                    };
                    if let Some(&d) = spf.dist.get(&Node::Router(asbr)) {
                        if reach.is_none_or(|r| d < r.0) {
                            reach = Some((d, asbr, area));
                        }
                    } else {
                        for &b in &abrs {
                            let (Some(&d1), Some(d2)) = (
                                spf.dist.get(&Node::Router(b)),
                                spfs.get(&(b, 0))
                                    .and_then(|s| s.dist.get(&Node::Router(asbr))),
                            ) else {
                                continue;
                            };
                            if reach.is_none_or(|r| d1 + d2 < r.0) {
                                reach = Some((d1 + d2, b, area));
                            }
                        }
                    }
                }
                let Some((d, via, area)) = reach else {
                    continue;
                };
                let spf = &spfs[&(root, area)];
                let next_hops: Vec<(Ipv4Addr, String)> = spf.hops[&Node::Router(via)]
                    .iter()
                    .filter(|h| !h.0.is_unspecified())
                    .cloned()
                    .collect();
                if next_hops.is_empty() {
                    continue;
                }
                if best.as_ref().is_none_or(|b| d < b.0) {
                    let path = self.path(project, spf, Node::Router(via));
                    best = Some((
                        d,
                        OspfRoute {
                            network: Ipv4Addr::UNSPECIFIED,
                            prefix_len: 0,
                            kind: OspfRouteKind::External2,
                            area,
                            cost: 1,
                            next_hops,
                            reason: format!(
                                "{} has a default route and `default-information originate`, so it advertises \
                                 0.0.0.0/0 as an external type 2 route with metric 1. E2 metrics don't grow along \
                                 the way: the cost stays 1 (the path to {} costs {d}).",
                                name(asbr),
                                name(asbr)
                            ),
                            path,
                            alternatives: Vec::new(),
                        },
                    ));
                }
            }
            if let Some((_, r)) = best {
                external.insert(root, r);
            }
        }

        // Alternatives through other neighbours, for "why this path".
        let mut finished: BTreeMap<DeviceId, Vec<OspfRoute>> = BTreeMap::new();
        for &root in &ids {
            let mut routes = intra.remove(&root).unwrap_or_default();
            routes.extend(inter.remove(&root).unwrap_or_default());
            routes.extend(external.remove(&root));
            for r in routes.iter_mut().filter(|r| r.kind == OspfRouteKind::Intra) {
                let used: BTreeSet<Ipv4Addr> = r.next_hops.iter().map(|h| h.0).collect();
                if r.next_hops.len() > 1 {
                    r.reason += &format!(
                        " {} paths cost the same, so all are used and traffic is shared between them.",
                        r.next_hops.len()
                    );
                }
                let neighbors = self.routers[&root].neighbors.clone();
                for n in neighbors.iter().filter(|n| n.state == NeighborState::Full) {
                    if used.contains(&n.address) {
                        continue;
                    }
                    let Some(link) = self.iface(root, &n.iface).map(|i| i.cost) else {
                        continue;
                    };
                    let via = self
                        .routers
                        .get(&n.device)
                        .and_then(|_| self.spf_cost(n.device, r.area, r.network, r.prefix_len));
                    if let Some(rest) = via {
                        let alt = format!(
                            "via {} ({} cost {link}) it would cost {}",
                            name(n.device),
                            abbreviate_port(&n.iface),
                            link + rest
                        );
                        if !r.alternatives.contains(&alt) {
                            r.alternatives.push(alt);
                        }
                    }
                }
            }
            finished.insert(root, routes);
        }
        for (id, routes) in finished {
            if let Some(r) = self.routers.get_mut(&id) {
                r.routes = routes;
            }
        }
        // Summary and external LSAs, for the database view.
        for ((area, abr), list) in &summaries {
            let mut seen = BTreeSet::new();
            for &(net, len, cost) in list {
                if !seen.insert((net, len)) {
                    continue;
                }
                let adv = self.rid(*abr);
                self.lsdb.entry(*area).or_default().push(Lsa {
                    kind: LsaKind::Summary,
                    area: *area,
                    link_id: net,
                    adv_router: adv,
                    mask: Some(len),
                    count: cost,
                    describe: format!(
                        "Summary from ABR {} into area {area}: {net}/{len}, cost {cost}",
                        name(*abr)
                    ),
                });
            }
        }
        for &asbr in &asbrs {
            let adv = self.rid(asbr);
            self.external.push(Lsa {
                kind: LsaKind::External,
                area: 0,
                link_id: Ipv4Addr::UNSPECIFIED,
                adv_router: adv,
                mask: Some(0),
                count: 1,
                describe: format!(
                    "External from ASBR {}: 0.0.0.0/0, type 2, metric 1",
                    name(asbr)
                ),
            });
        }
    }

    /// What `from` pays to reach a prefix in `area` (0 if it's its own).
    fn spf_cost(&self, from: DeviceId, area: u32, net: Ipv4Addr, len: u8) -> Option<u32> {
        let r = self.routers.get(&from)?;
        if let Some(i) = r.ifaces.iter().find(|i| {
            i.area == area
                && match i.net_type {
                    OspfNetType::Loopback => (i.address, 32) == (net, len),
                    _ => (i.network(), i.prefix_len) == (net, len),
                }
        }) {
            return Some(if i.net_type == OspfNetType::Loopback {
                i.cost
            } else if i.segment.is_some_and(|s| self.transit(s)) {
                0
            } else {
                i.cost
            });
        }
        let spf = self.spf(from, area);
        let mut best: Option<u32> = None;
        for (&node, &d) in &spf.dist {
            let cost = match node {
                Node::Router(id) => self.routers.get(&id).and_then(|x| {
                    x.ifaces
                        .iter()
                        .filter(|i| i.area == area)
                        .find(|i| match i.net_type {
                            OspfNetType::Loopback => (i.address, 32) == (net, len),
                            _ => (i.network(), i.prefix_len) == (net, len),
                        })
                        .map(|i| i.cost)
                }),
                Node::Net(s) => {
                    let seg = &self.segments[s];
                    ((seg.network, seg.prefix_len) == (net, len)).then_some(0)
                }
            };
            if let Some(c) = cost {
                best = Some(best.map_or(d + c, |b| b.min(d + c)));
            }
        }
        best
    }
}

/// One neighbour or route that changed when a link failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OspfChange {
    pub device: DeviceId,
    /// The route that changed; `None` for a lost neighbour.
    pub network: Option<(Ipv4Addr, u8)>,
    pub explanation: String,
}

/// `router-id`, else the highest loopback address, else the highest
/// address on an up interface, as IOS chooses.
fn choose_router_id(process: &OspfProcess, stack: &Stack) -> (Option<Ipv4Addr>, String) {
    if let Some(rid) = process.router_id {
        return (Some(rid), format!("`router-id {rid}` is configured."));
    }
    let highest = |loopback: bool| {
        stack
            .ifaces
            .iter()
            .filter(|i| i.up && i.name.starts_with("Loopback") == loopback)
            .filter_map(|i| i.address.map(|a| (a.address, i.name.clone())))
            .max_by_key(|(a, _)| u32::from(*a))
    };
    if let Some((a, n)) = highest(true) {
        return (
            Some(a),
            format!(
                "No `router-id`, so it uses its highest loopback address: {a} on {}.",
                abbreviate_port(&n)
            ),
        );
    }
    if let Some((a, n)) = highest(false) {
        return (
            Some(a),
            format!(
                "No `router-id` and no loopback, so it uses its highest active interface address: {a} on {}. Setting \
                 a router-id (or a loopback) keeps it from changing when interfaces do.",
                abbreviate_port(&n)
            ),
        );
    }
    (
        None,
        "No `router-id` and no interface with an IP address, so OSPF can't start.".into(),
    )
}

/// The OSPF-capable interfaces (routers, L3 switch SVIs and routed ports)
/// in the same broadcast domain as `start`, following cables through
/// switches (by VLAN, through forwarding ports) and hubs.
fn broadcast_domain(
    project: &Project,
    ports: &PortStates,
    l2: &Layer2,
    stacks: &BTreeMap<DeviceId, Stack>,
    start_device: DeviceId,
    start: &Iface,
) -> BTreeSet<(DeviceId, String)> {
    let mut members = BTreeSet::new();
    members.insert((start_device, start.name.clone()));
    let peers: BTreeMap<(DeviceId, String), (DeviceId, String)> = project
        .links()
        .iter()
        .flat_map(|l| {
            [
                (
                    (l.a.device, l.a.port.clone()),
                    (l.b.device, l.b.port.clone()),
                ),
                (
                    (l.b.device, l.b.port.clone()),
                    (l.a.device, l.a.port.clone()),
                ),
            ]
        })
        .collect();
    // Frames leaving (device, port) with a tag.
    let mut queue: Vec<(DeviceId, String, Option<u16>)> = Vec::new();
    let mut seen: BTreeSet<(DeviceId, String, Option<u16>)> = BTreeSet::new();
    let mut switches_seen: BTreeSet<(DeviceId, u16)> = BTreeSet::new();
    let enter_switch = |id: DeviceId,
                        vlan: u16,
                        from: Option<&str>,
                        queue: &mut Vec<(DeviceId, String, Option<u16>)>,
                        members: &mut BTreeSet<(DeviceId, String)>,
                        switches_seen: &mut BTreeSet<(DeviceId, u16)>| {
        if !switches_seen.insert((id, vlan)) {
            return;
        }
        let Some(d) = project.device(id) else { return };
        if let Some(svi) = stacks
            .get(&id)
            .and_then(|s| s.iface(&format!("Vlan{vlan}")))
            .filter(|i| i.up)
        {
            members.insert((id, svi.name.clone()));
        }
        for p in d.kind.port_names() {
            if Some(p.as_str()) == from || !ports.is_up(id, &p) {
                continue;
            }
            let Some(sp) = crate::stp::switchport(project, id, &p) else {
                continue;
            };
            if !sp.carries(vlan) || !l2.forwards(id, &p, vlan) {
                continue;
            }
            let tag = (sp.is_trunk() && vlan != sp.native_vlan).then_some(vlan);
            queue.push((id, p, tag));
        }
    };
    match start.port.as_deref() {
        Some(port) => {
            let tag = start.vlan.filter(|_| !start.native);
            queue.push((start_device, port.to_string(), tag));
        }
        None => {
            if let Some(vlan) = start.vlan {
                enter_switch(
                    start_device,
                    vlan,
                    None,
                    &mut queue,
                    &mut members,
                    &mut switches_seen,
                );
            }
        }
    }
    while let Some((id, port, tag)) = queue.pop() {
        if !seen.insert((id, port.clone(), tag)) || !ports.is_up(id, &port) {
            continue;
        }
        let Some((peer, peer_port)) = peers.get(&(id, port.clone())).cloned() else {
            continue;
        };
        let Some(d) = project.device(peer) else {
            continue;
        };
        if let Some(sp) = crate::stp::switchport(project, peer, &peer_port) {
            if tag.is_some() && !sp.is_trunk() {
                continue;
            }
            let vlan = tag.unwrap_or_else(|| sp.untagged_vlan());
            if !sp.carries(vlan) || !l2.forwards(peer, &peer_port, vlan) {
                continue;
            }
            enter_switch(
                peer,
                vlan,
                Some(&peer_port),
                &mut queue,
                &mut members,
                &mut switches_seen,
            );
        } else if d.kind == DeviceKind::Hub {
            for p in d.kind.port_names() {
                if p != peer_port {
                    queue.push((peer, p, tag));
                }
            }
        } else if let Some(iface) = stacks
            .get(&peer)
            .and_then(|s| s.iface_for_frame(&peer_port, tag))
            .filter(|i| i.up)
        {
            members.insert((peer, iface.name.clone()));
        }
    }
    members
}
