//! The IP side of a device: interfaces with addresses, a routing table and
//! an ARP cache. End devices, routers and a switch's management interface
//! (`interface Vlan1`) all have one; only routers forward.
//!
//! Stacks are built straight from the project, so `show ip route` and
//! `show ip interface brief` can be answered without running anything.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netrace_doc::{DeviceId, DeviceKind, InterfaceAddress, Project, RouteTarget, mask};
use netrace_sim::SimTime;

use crate::stp::Layer2;
use crate::{Ipv4Packet, MacAddr};

/// Administrative and line state of every physical port in the project.
#[derive(Debug, Clone, Default)]
pub(crate) struct PortStates {
    /// Ports switched off with `shutdown`.
    pub disabled: BTreeSet<(DeviceId, String)>,
    /// Ports whose link is up: cabled, and enabled at both ends.
    pub up: BTreeSet<(DeviceId, String)>,
    /// Ports with a cable plugged in.
    pub cabled: BTreeSet<(DeviceId, String)>,
    /// Devices that are switched off: their links are down.
    pub off: BTreeSet<DeviceId>,
}

impl PortStates {
    pub fn of(project: &Project) -> Self {
        let mut states = PortStates {
            off: project
                .devices()
                .iter()
                .filter(|d| d.power == netrace_doc::Power::Off)
                .map(|d| d.id)
                .collect(),
            ..PortStates::default()
        };
        for d in project.devices() {
            if !d.kind.has_cli() {
                continue;
            }
            for port in d.kind.port_names() {
                if d.config.interface(d.kind, &port).shutdown {
                    states.disabled.insert((d.id, port));
                }
            }
        }
        for link in project.links() {
            let a = (link.a.device, link.a.port.clone());
            let b = (link.b.device, link.b.port.clone());
            states.cabled.insert(a.clone());
            states.cabled.insert(b.clone());
            let powered = !states.off.contains(&a.0) && !states.off.contains(&b.0);
            if powered && !states.disabled.contains(&a) && !states.disabled.contains(&b) {
                states.up.insert(a);
                states.up.insert(b);
            }
        }
        states
    }

    pub fn is_up(&self, device: DeviceId, port: &str) -> bool {
        self.up.contains(&(device, port.to_string()))
    }

    pub fn is_disabled(&self, device: DeviceId, port: &str) -> bool {
        self.disabled.contains(&(device, port.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Iface {
    /// Full IOS name: "GigabitEthernet0/0", "Vlan1", "Loopback0", or the
    /// NIC's port name on an end device.
    pub name: String,
    /// The physical port frames leave by; `None` for loopbacks and for a
    /// switch's `Vlan` interface (which uses the switch's own ports).
    pub port: Option<String>,
    pub mac: MacAddr,
    pub address: Option<InterfaceAddress>,
    pub admin_up: bool,
    /// Line protocol up (and admin up): usable for traffic.
    pub up: bool,
    /// 802.1Q VLAN: a router subinterface's tag, or a switch `Vlan`
    /// interface's VLAN.
    pub vlan: Option<u16>,
    /// A native-VLAN subinterface sends and receives untagged frames.
    pub native: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RouteSource {
    /// The network of an interface (`C`).
    Connected,
    /// The interface's own address (`L`).
    Local,
    /// `ip route` (`S`).
    Static,
    /// An end device's default gateway, or a switch's `ip default-gateway`.
    Gateway,
    /// `O`: OSPF, inside the area.
    Ospf,
    /// `O IA`: OSPF, from another area.
    OspfInterArea,
    /// `O E2`: OSPF external type 2.
    OspfExternal2,
}

impl RouteSource {
    pub fn code(self) -> &'static str {
        match self {
            RouteSource::Connected => "C",
            RouteSource::Local => "L",
            RouteSource::Static | RouteSource::Gateway => "S",
            RouteSource::Ospf | RouteSource::OspfInterArea | RouteSource::OspfExternal2 => "O",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub network: Ipv4Addr,
    pub prefix_len: u8,
    pub source: RouteSource,
    /// The router to send to; `None` means the destination is on the link.
    pub next_hop: Option<Ipv4Addr>,
    /// Exit interface (full name).
    pub iface: String,
    pub distance: u8,
    /// The routing protocol's cost (0 for connected and static routes).
    pub metric: u32,
}

impl Route {
    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        u32::from(ip) & u32::from(mask(self.prefix_len)) == u32::from(self.network)
    }

    pub fn is_default(&self) -> bool {
        self.prefix_len == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StackKind {
    Host,
    Router,
    /// A switch's management interface.
    Management,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ArpEntry {
    pub mac: MacAddr,
    pub iface: String,
    pub learned: SimTime,
}

/// A packet held by an end device until ARP resolves its next hop.
#[derive(Debug, Clone)]
pub(crate) struct Waiting {
    pub next_hop: Ipv4Addr,
    pub packet: Ipv4Packet,
    pub session: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct Stack {
    pub kind: StackKind,
    pub ifaces: Vec<Iface>,
    /// Installed routes: the best (lowest distance) route per prefix.
    pub routes: Vec<Route>,
    pub forwarding: bool,
    /// Initial TTL of packets this device creates.
    pub ttl: u8,
    pub arp: BTreeMap<Ipv4Addr, ArpEntry>,
    pub waiting: Vec<Waiting>,
    /// Next hops with an ARP request outstanding.
    pub asking: BTreeSet<Ipv4Addr>,
}

pub(crate) const HOST_TTL: u8 = 128;
pub(crate) const ROUTER_TTL: u8 = 255;
/// MAC index shared by a switch's `Vlan` interfaces (after its physical ports).
const SVI_MAC_INDEX: usize = 0xf0;

impl Stack {
    /// The stack for a device with its OSPF routes, worked out for the
    /// whole project (for views that build one stack at a time).
    pub fn build(
        project: &Project,
        id: DeviceId,
        ports: &PortStates,
        l2: &Layer2,
    ) -> Option<Stack> {
        let ospf = crate::ospf::Ospf::compute(project, ports, l2);
        let mut stack = Stack::build_base(project, id, ports, l2)?;
        stack.install_ospf(&ospf, id);
        Some(stack)
    }

    /// The stack for a device, if it has one: end devices always; routers;
    /// switches through their `Vlan` interfaces (and routed ports). Static
    /// and connected routes only.
    pub(crate) fn build_base(
        project: &Project,
        id: DeviceId,
        ports: &PortStates,
        l2: &Layer2,
    ) -> Option<Stack> {
        let d = project.device(id)?;
        let mut stack = match d.kind {
            k if k.is_host() => {
                let port = k.port_names().into_iter().next()?;
                let up = ports.cabled.contains(&(id, port.clone()));
                let address = d.ip.filter(|_| !d.dhcp).map(|ip| InterfaceAddress {
                    address: ip.address,
                    prefix_len: ip.prefix_len,
                });
                let mut stack = Stack::empty(StackKind::Host, HOST_TTL);
                stack.ifaces.push(Iface {
                    name: port.clone(),
                    port: Some(port.clone()),
                    mac: MacAddr::for_port(id, 0),
                    address,
                    admin_up: true,
                    up,
                    vlan: None,
                    native: false,
                });
                if let Some(gateway) = d.ip.filter(|_| !d.dhcp).and_then(|ip| ip.gateway) {
                    stack.routes.push(Route {
                        network: Ipv4Addr::UNSPECIFIED,
                        prefix_len: 0,
                        source: RouteSource::Gateway,
                        next_hop: Some(gateway),
                        iface: port,
                        distance: 1,
                        metric: 0,
                    });
                }
                stack
            }
            k if k.is_router() => {
                let mut stack = Stack::empty(StackKind::Router, ROUTER_TTL);
                stack.forwarding = true;
                for (index, port) in d.kind.port_names().into_iter().enumerate() {
                    let config = d.config.interface(d.kind, &port);
                    let mac = MacAddr::for_port(id, index);
                    let up = ports.is_up(id, &port);
                    stack.ifaces.push(Iface {
                        mac,
                        address: config.address,
                        admin_up: !config.shutdown,
                        up,
                        name: port.clone(),
                        port: Some(port.clone()),
                        vlan: None,
                        native: false,
                    });
                    // Subinterfaces share the port and its MAC; each has its own VLAN tag.
                    let prefix = format!("{port}.");
                    for (name, sub) in d
                        .config
                        .interfaces
                        .iter()
                        .filter(|(n, _)| n.starts_with(&prefix))
                    {
                        stack.ifaces.push(Iface {
                            name: name.clone(),
                            port: Some(port.clone()),
                            mac,
                            address: sub.address,
                            admin_up: !sub.shutdown,
                            up: up && !sub.shutdown && sub.encapsulation.is_some(),
                            vlan: sub.encapsulation,
                            native: sub.native,
                        });
                    }
                }
                stack.add_loopbacks(&d.config);
                stack
            }
            k if k.is_switch() => {
                let routing = k == DeviceKind::L3Switch && d.config.ip_routing;
                let mut stack = Stack::empty(
                    if routing {
                        StackKind::Router
                    } else {
                        StackKind::Management
                    },
                    ROUTER_TTL,
                );
                stack.forwarding = routing;
                // A `Vlan` interface is up when enabled, its VLAN exists, and at
                // least one port in the VLAN is up and forwarding.
                let mut svis: Vec<String> = d
                    .config
                    .interfaces
                    .keys()
                    .filter(|n| n.starts_with("Vlan"))
                    .cloned()
                    .collect();
                if !svis.iter().any(|n| n == "Vlan1") {
                    svis.insert(0, "Vlan1".into());
                }
                for name in svis {
                    let Ok(vlan) = name["Vlan".len()..].parse::<u16>() else {
                        continue;
                    };
                    let config = d.config.interface(k, &name);
                    let active = d.config.vlan_exists(vlan)
                        && k.port_names().iter().any(|p| {
                            crate::stp::switchport(project, id, p)
                                .is_some_and(|sp| sp.carries(vlan))
                                && ports.is_up(id, p)
                                && l2.forwards(id, p, vlan)
                        });
                    stack.ifaces.push(Iface {
                        name,
                        port: None,
                        mac: MacAddr::for_port(id, SVI_MAC_INDEX),
                        address: config.address,
                        admin_up: !config.shutdown,
                        up: !config.shutdown && active,
                        vlan: Some(vlan),
                        native: false,
                    });
                }
                if k == DeviceKind::L3Switch {
                    // Routed ports (`no switchport`) work like router interfaces.
                    for (index, port) in k.port_names().into_iter().enumerate() {
                        let config = d.config.interface(k, &port);
                        if !config.switchport.routed {
                            continue;
                        }
                        stack.ifaces.push(Iface {
                            mac: MacAddr::for_port(id, index),
                            address: config.address,
                            admin_up: !config.shutdown,
                            up: ports.is_up(id, &port),
                            name: port.clone(),
                            port: Some(port),
                            vlan: None,
                            native: false,
                        });
                    }
                    stack.add_loopbacks(&d.config);
                }
                if let Some(gateway) = d.config.default_gateway {
                    stack.routes.push(Route {
                        network: Ipv4Addr::UNSPECIFIED,
                        prefix_len: 0,
                        source: RouteSource::Gateway,
                        next_hop: Some(gateway),
                        iface: "Vlan1".into(),
                        distance: 1,
                        metric: 0,
                    });
                }
                stack
            }
            _ => return None,
        };
        stack.install_connected();
        if d.kind.has_cli() {
            stack.install_static(&d.config.routes);
        }
        stack.resolve_gateway_routes();
        stack.routes.sort_by(|a, b| {
            (u32::from(a.network), a.prefix_len, a.source).cmp(&(
                u32::from(b.network),
                b.prefix_len,
                b.source,
            ))
        });
        Some(stack)
    }

    /// Adds the routes OSPF chose for `id`, where no route with a lower
    /// administrative distance has the prefix already.
    pub(crate) fn install_ospf(&mut self, ospf: &crate::ospf::Ospf, id: DeviceId) {
        let Some(router) = ospf.router(id) else {
            return;
        };
        for r in &router.routes {
            let taken = self.routes.iter().any(|x| {
                x.network == r.network
                    && x.prefix_len == r.prefix_len
                    && x.distance <= crate::ospf::OSPF_DISTANCE
            });
            if taken {
                continue;
            }
            let source = match r.kind {
                crate::ospf::OspfRouteKind::Intra => RouteSource::Ospf,
                crate::ospf::OspfRouteKind::Inter => RouteSource::OspfInterArea,
                crate::ospf::OspfRouteKind::External2 => RouteSource::OspfExternal2,
            };
            for (next_hop, iface) in &r.next_hops {
                self.routes.push(Route {
                    network: r.network,
                    prefix_len: r.prefix_len,
                    source,
                    next_hop: Some(*next_hop),
                    iface: iface.clone(),
                    distance: crate::ospf::OSPF_DISTANCE,
                    metric: r.cost,
                });
            }
        }
        self.routes.sort_by(|a, b| {
            (u32::from(a.network), a.prefix_len, a.source).cmp(&(
                u32::from(b.network),
                b.prefix_len,
                b.source,
            ))
        });
    }

    fn empty(kind: StackKind, ttl: u8) -> Stack {
        Stack {
            kind,
            ifaces: Vec::new(),
            routes: Vec::new(),
            forwarding: false,
            ttl,
            arp: BTreeMap::new(),
            waiting: Vec::new(),
            asking: BTreeSet::new(),
        }
    }

    fn add_loopbacks(&mut self, config: &netrace_doc::DeviceConfig) {
        for (name, iface) in config
            .interfaces
            .iter()
            .filter(|(n, _)| n.starts_with("Loopback"))
        {
            self.ifaces.push(Iface {
                name: name.clone(),
                port: None,
                mac: MacAddr::ZERO,
                address: iface.address,
                admin_up: !iface.shutdown,
                up: !iface.shutdown,
                vlan: None,
                native: false,
            });
        }
    }

    fn install_connected(&mut self) {
        let mut routes = Vec::new();
        for iface in self.ifaces.iter().filter(|i| i.up) {
            let Some(addr) = iface.address else { continue };
            routes.push(Route {
                network: addr.network(),
                prefix_len: addr.prefix_len,
                source: RouteSource::Connected,
                next_hop: None,
                iface: iface.name.clone(),
                distance: 0,
                metric: 0,
            });
            if self.kind != StackKind::Host {
                routes.push(Route {
                    network: addr.address,
                    prefix_len: 32,
                    source: RouteSource::Local,
                    next_hop: None,
                    iface: iface.name.clone(),
                    distance: 0,
                    metric: 0,
                });
            }
        }
        self.routes.extend(routes);
    }

    /// Static routes whose next hop is reachable (recursively) or whose exit
    /// interface is up. Of several routes to one prefix, the lowest
    /// administrative distance wins (a floating static waits in reserve).
    fn install_static(&mut self, configured: &[netrace_doc::StaticRoute]) {
        let mut candidates: Vec<Route> = Vec::new();
        for r in configured {
            let (next_hop, iface) = match &r.target {
                RouteTarget::NextHop(nh) => match self.exit_for(*nh, configured, 0) {
                    Some(iface) => (Some(*nh), iface),
                    None => continue,
                },
                RouteTarget::Interface(name) => match self.iface(name) {
                    Some(i) if i.up => (None, name.clone()),
                    _ => continue,
                },
            };
            candidates.push(Route {
                network: r.network,
                prefix_len: r.prefix_len,
                source: RouteSource::Static,
                next_hop,
                iface,
                distance: r.distance,
                metric: 0,
            });
        }
        for candidate in candidates {
            let same_prefix =
                |r: &Route| r.network == candidate.network && r.prefix_len == candidate.prefix_len;
            let best = self
                .routes
                .iter()
                .filter(|r| same_prefix(r))
                .map(|r| r.distance)
                .min();
            match best {
                Some(d) if d < candidate.distance => {}
                Some(d) if d > candidate.distance => {
                    self.routes.retain(|r| !same_prefix(r));
                    self.routes.push(candidate);
                }
                // Equal distance: both kept (equal-cost paths; the first is used).
                _ => self.routes.push(candidate),
            }
        }
    }

    /// Exit interface for a next hop: a connected network, or recursively
    /// through another static route (as IOS resolves recursive routes).
    fn exit_for(
        &self,
        next_hop: Ipv4Addr,
        configured: &[netrace_doc::StaticRoute],
        depth: u8,
    ) -> Option<String> {
        if let Some(route) = self
            .routes
            .iter()
            .filter(|r| r.source == RouteSource::Connected && r.contains(next_hop))
            .max_by_key(|r| r.prefix_len)
        {
            return Some(route.iface.clone());
        }
        if depth >= 4 {
            return None;
        }
        configured
            .iter()
            .filter(|r| {
                u32::from(next_hop) & u32::from(r.mask()) == u32::from(r.network)
                    && r.prefix_len < 32
            })
            .max_by_key(|r| r.prefix_len)
            .and_then(|r| match &r.target {
                RouteTarget::NextHop(nh) if *nh != next_hop => {
                    self.exit_for(*nh, configured, depth + 1)
                }
                RouteTarget::Interface(name) => {
                    self.iface(name).filter(|i| i.up).map(|i| i.name.clone())
                }
                _ => None,
            })
    }

    /// A default gateway is only usable if it's on a connected network.
    fn resolve_gateway_routes(&mut self) {
        let connected: Vec<Route> = self
            .routes
            .iter()
            .filter(|r| r.source == RouteSource::Connected)
            .cloned()
            .collect();
        self.routes.retain(|r| {
            r.source != RouteSource::Gateway
                || r.next_hop
                    .is_some_and(|gw| connected.iter().any(|c| c.contains(gw)))
        });
    }

    pub fn iface(&self, name: &str) -> Option<&Iface> {
        self.ifaces.iter().find(|i| i.name == name)
    }

    /// The interface a frame arriving on `port` belongs to (a switch's own
    /// `Vlan` interfaces arrive as "Vlan10"). On a router port, a tagged
    /// frame goes to the subinterface for its VLAN and an untagged one to
    /// the port itself or its native-VLAN subinterface.
    pub fn iface_for_frame(&self, port: &str, tag: Option<u16>) -> Option<&Iface> {
        if let Some(svi) = self
            .ifaces
            .iter()
            .find(|i| i.port.is_none() && i.name == port)
        {
            return Some(svi);
        }
        let on_port = || {
            self.ifaces
                .iter()
                .filter(|i| i.port.as_deref() == Some(port))
        };
        match tag {
            Some(vlan) => on_port().find(|i| i.vlan == Some(vlan) && !i.native),
            None => on_port()
                .find(|i| i.native)
                .or_else(|| on_port().find(|i| i.vlan.is_none())),
        }
    }

    /// The interface on a port (or the named virtual interface), ignoring tags.
    pub fn iface_for_port(&self, port: &str) -> Option<&Iface> {
        self.ifaces.iter().find(|i| {
            (i.port.as_deref() == Some(port) && i.vlan.is_none())
                || (i.port.is_none() && i.name == port)
        })
    }

    /// The interface that owns `ip`, if it's one of this device's addresses.
    pub fn owner_of(&self, ip: Ipv4Addr) -> Option<&Iface> {
        self.ifaces
            .iter()
            .find(|i| i.up && i.address.is_some_and(|a| a.address == ip))
    }

    /// Longest-prefix match.
    pub fn lookup(&self, dst: Ipv4Addr) -> Option<&Route> {
        self.routes
            .iter()
            .filter(|r| r.contains(dst))
            .max_by(|a, b| {
                a.prefix_len
                    .cmp(&b.prefix_len)
                    .then(b.distance.cmp(&a.distance))
            })
    }

    /// Where to send a packet for `dst`: exit interface and the next hop to
    /// ARP for (the destination itself when it's on the link).
    pub fn resolve(&self, dst: Ipv4Addr) -> Option<(&Route, &Iface, Ipv4Addr)> {
        let route = self.lookup(dst)?;
        let iface = self.iface(&route.iface)?;
        Some((route, iface, route.next_hop.unwrap_or(dst)))
    }

    /// The address used as the source of packets leaving `iface`.
    pub fn source_for(&self, iface: &Iface) -> Option<Ipv4Addr> {
        iface.address.map(|a| a.address)
    }

    pub fn learn(&mut self, ip: Ipv4Addr, mac: MacAddr, iface: &str, now: SimTime) {
        self.arp.insert(
            ip,
            ArpEntry {
                mac,
                iface: iface.to_string(),
                learned: now,
            },
        );
        self.asking.remove(&ip);
    }
}
