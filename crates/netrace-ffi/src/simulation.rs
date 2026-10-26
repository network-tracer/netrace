//! The simulation as seen from Swift: commands in, an explained trace out.

use std::sync::{Arc, Mutex, MutexGuard};

use netrace_doc as doc;
use netrace_engine as engine;
use netrace_sim::SimTime;

use crate::{CoreError, Project};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TraceOutcome {
    Created,
    Forwarded,
    Flooded,
    Replied,
    Accepted,
    Ignored,
    Dropped,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LayerRole {
    /// Read and acted on at this hop.
    Used,
    /// Built or rewritten at this hop.
    Changed,
    /// Not looked at by this device.
    NotUsed,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FieldInfo {
    pub name: String,
    pub value: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LayerNoteInfo {
    /// 1 = physical … 4 = message carried inside IP.
    pub number: u8,
    pub title: String,
    pub role: LayerRole,
    pub note: String,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TraceEventInfo {
    pub id: u64,
    pub time_ns: u64,
    /// When the frame left the previous device (for animating the hop).
    pub sent_ns: Option<u64>,
    pub device_id: u64,
    pub from_device_id: Option<u64>,
    pub link_id: Option<u64>,
    pub ingress: Option<String>,
    pub egress: Vec<String>,
    /// "ARP" or "ICMP"; absent for events without a frame (timeouts).
    pub protocol: Option<String>,
    pub outcome: TraceOutcome,
    /// The plain-language explanation.
    pub summary: String,
    /// One-line, Wireshark-style description of the frame.
    pub info: String,
    /// IP addresses for IP packets, MAC addresses for ARP.
    pub source: Option<String>,
    pub destination: Option<String>,
    pub length: Option<u32>,
    pub layers: Vec<LayerNoteInfo>,
    pub session: Option<u64>,
    /// The access list checked at this hop, with the deciding line.
    pub acl: Option<AclDecisionInfo>,
    /// What NAT rewrote at this hop (or why it didn't).
    pub nat: Option<NatDecisionInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NatDecisionInfo {
    /// "inside-to-outside" or "outside-to-inside".
    pub direction: String,
    pub translated: bool,
    pub before_src: String,
    pub before_dst: String,
    pub after_src: String,
    pub after_dst: String,
    pub rule: Option<String>,
    pub created: bool,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AclLineInfo {
    pub sequence: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AclDecisionInfo {
    pub acl: String,
    /// "in" or "out".
    pub direction: String,
    pub interface: String,
    pub lines: Vec<AclLineInfo>,
    /// The line that matched; none means the implicit deny.
    pub matched: Option<u32>,
    pub permitted: bool,
}

/// An end device's address right now, however it got it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CurrentAddress {
    pub address: String,
    pub prefix_len: u8,
    pub gateway: Option<String>,
    pub dhcp_server: Option<String>,
    /// 169.254.x.x: DHCP failed.
    pub apipa: bool,
}

/// Where services are configured, for canvas badges.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ServiceBadge {
    pub device_id: u64,
    /// Physical port (subinterfaces map to their port).
    pub port: Option<String>,
    /// "acl-in", "acl-out", "relay", "dhcp-server", "nat-inside" or "nat-outside".
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PingSessionInfo {
    pub id: u64,
    pub source_id: u64,
    pub destination: String,
    pub count: u16,
    pub received: u16,
    pub failed: u16,
    pub finished: bool,
    /// Windows-style ping output.
    pub report: String,
}

/// A simulation of one project snapshot. Rebuild it when the topology
/// changes; commands added to it are replayed deterministically.
#[derive(uniffi::Object)]
pub struct Simulation {
    inner: Mutex<engine::Network>,
}

#[uniffi::export]
impl Simulation {
    #[uniffi::constructor]
    pub fn new(project: Arc<Project>) -> Arc<Self> {
        let snapshot = project.lock().clone();
        Arc::new(Simulation {
            inner: Mutex::new(engine::Network::new(snapshot)),
        })
    }

    /// Schedules `count` pings from an end device at `at_ns`; returns the session id.
    pub fn ping(
        &self,
        source: u64,
        destination: String,
        count: u16,
        at_ns: u64,
    ) -> Result<u64, CoreError> {
        let destination = destination
            .trim()
            .parse()
            .map_err(|_| CoreError::InvalidOperation {
                message: format!("\"{}\" is not a valid IPv4 address.", destination.trim()),
            })?;
        self.lock()
            .ping(
                doc::DeviceId(source),
                destination,
                count,
                SimTime::from_nanos(at_ns),
            )
            .map_err(|e| CoreError::InvalidOperation {
                message: e.to_string(),
            })
    }

    /// Schedules a traceroute from a device at `at_ns`; returns the session id.
    pub fn traceroute(
        &self,
        source: u64,
        destination: String,
        at_ns: u64,
    ) -> Result<u64, CoreError> {
        let destination = destination
            .trim()
            .parse()
            .map_err(|_| CoreError::InvalidOperation {
                message: format!("\"{}\" is not a valid IPv4 address.", destination.trim()),
            })?;
        self.lock()
            .traceroute(
                doc::DeviceId(source),
                destination,
                SimTime::from_nanos(at_ns),
            )
            .map_err(|e| CoreError::InvalidOperation {
                message: e.to_string(),
            })
    }

    /// Runs a command typed at an end device's prompt; returns the output.
    pub fn host_command(&self, device: u64, line: String, at_ns: u64) -> String {
        self.lock()
            .host_command(doc::DeviceId(device), &line, SimTime::from_nanos(at_ns))
    }

    pub fn trace(&self) -> Vec<TraceEventInfo> {
        self.lock()
            .trace()
            .iter()
            .map(TraceEventInfo::from)
            .collect()
    }

    pub fn sessions(&self) -> Vec<PingSessionInfo> {
        let network = self.lock();
        network
            .sessions()
            .iter()
            .map(|s| PingSessionInfo {
                id: s.id,
                source_id: s.source.0,
                destination: s.destination.to_string(),
                count: s.count,
                received: s.replies.len() as u16,
                failed: s.failures.len() as u16,
                finished: s.is_finished(),
                report: network.session_report(s.id).unwrap_or_default(),
            })
            .collect()
    }

    pub fn warnings(&self) -> Vec<String> {
        self.lock().warnings().to_vec()
    }

    pub fn end_time_ns(&self) -> u64 {
        self.lock().end_time().as_nanos()
    }

    /// The MAC a device uses on a port, e.g. for the inspector.
    pub fn mac_address(&self, device: u64, port: String) -> Option<String> {
        self.lock()
            .mac(doc::DeviceId(device), &port)
            .map(|m| m.to_string())
    }
}

impl Simulation {
    pub(crate) fn lock(&self) -> MutexGuard<'_, engine::Network> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Time a frame takes to cross one cable, in nanoseconds.
#[uniffi::export]
pub fn hop_latency_ns() -> u64 {
    engine::HOP_LATENCY.as_nanos() as u64
}

impl From<&engine::TraceEvent> for TraceEventInfo {
    fn from(e: &engine::TraceEvent) -> Self {
        let (source, destination) = match e.frame.map(|f| f.payload) {
            Some(engine::Payload::Ipv4(ip)) => (Some(ip.src.to_string()), Some(ip.dst.to_string())),
            Some(engine::Payload::Arp(_) | engine::Payload::Bpdu(_)) => (
                e.frame.map(|f| f.src.to_string()),
                e.frame.map(|f| f.dst.to_string()),
            ),
            None => (None, None),
        };
        TraceEventInfo {
            id: e.id,
            time_ns: e.time.as_nanos(),
            sent_ns: e.sent.map(SimTime::as_nanos),
            device_id: e.device.0,
            from_device_id: e.from.map(|d| d.0),
            link_id: e.link.map(|l| l.0),
            ingress: e.ingress.clone(),
            egress: e.egress.clone(),
            protocol: e.frame.map(|f| f.protocol().name().to_string()),
            outcome: match e.outcome {
                engine::Outcome::Created => TraceOutcome::Created,
                engine::Outcome::Forwarded => TraceOutcome::Forwarded,
                engine::Outcome::Flooded => TraceOutcome::Flooded,
                engine::Outcome::Replied => TraceOutcome::Replied,
                engine::Outcome::Accepted => TraceOutcome::Accepted,
                engine::Outcome::Ignored => TraceOutcome::Ignored,
                engine::Outcome::Dropped => TraceOutcome::Dropped,
                engine::Outcome::Completed => TraceOutcome::Completed,
            },
            summary: e.summary.clone(),
            info: e
                .frame
                .map(|f| f.info())
                .unwrap_or_else(|| e.summary.clone()),
            source,
            destination,
            length: e.frame.map(|f| f.len() as u32),
            layers: e
                .layers
                .iter()
                .map(|l| LayerNoteInfo {
                    number: l.layer.number(),
                    title: l.layer.title().to_string(),
                    role: match l.role {
                        engine::LayerRole::Used => LayerRole::Used,
                        engine::LayerRole::Changed => LayerRole::Changed,
                        engine::LayerRole::NotUsed => LayerRole::NotUsed,
                    },
                    note: l.note.clone(),
                    fields: l
                        .fields
                        .iter()
                        .map(|f| FieldInfo {
                            name: f.name.clone(),
                            value: f.value.clone(),
                            changed: f.changed,
                        })
                        .collect(),
                })
                .collect(),
            session: e.session,
            acl: e.acl.as_ref().map(|a| AclDecisionInfo {
                acl: a.acl.clone(),
                direction: a.direction.ios().to_string(),
                interface: a.interface.clone(),
                lines: a
                    .lines
                    .iter()
                    .map(|(sequence, text)| AclLineInfo {
                        sequence: *sequence,
                        text: text.clone(),
                    })
                    .collect(),
                matched: a.matched,
                permitted: a.permitted,
            }),
            nat: e.nat.as_ref().map(|n| NatDecisionInfo {
                direction: match n.direction {
                    engine::NatDirection::InsideToOutside => "inside-to-outside",
                    engine::NatDirection::OutsideToInside => "outside-to-inside",
                }
                .into(),
                translated: n.translated,
                before_src: n.before_src.clone(),
                before_dst: n.before_dst.clone(),
                after_src: n.after_src.clone(),
                after_dst: n.after_dst.clone(),
                rule: n.rule.clone(),
                created: n.created,
                note: n.note.clone(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceKind, EndpointInfo, Medium, parse_host_ip};

    #[test]
    fn ping_through_the_bridge() {
        let project = Project::new();
        let sw = project.add_device(DeviceKind::L2Switch, 0.0, 0.0);
        let a = project.add_device(DeviceKind::Pc, 0.0, 0.0);
        let b = project.add_device(DeviceKind::Pc, 0.0, 0.0);
        for (pc, port, addr) in [
            (&a, "FastEthernet0/1", "10.0.0.1"),
            (&b, "FastEthernet0/2", "10.0.0.2"),
        ] {
            project
                .connect(
                    EndpointInfo {
                        device_id: sw.id,
                        port: port.into(),
                    },
                    EndpointInfo {
                        device_id: pc.id,
                        port: "FastEthernet0".into(),
                    },
                    Medium::Copper,
                )
                .unwrap();
            let ip = parse_host_ip(addr.into(), "255.255.255.0".into(), "".into()).unwrap();
            project.set_host_ip(pc.id, Some(ip)).unwrap();
        }
        let sim = Simulation::new(project.clone());
        let session = sim.ping(a.id, "10.0.0.2".into(), 1, 0).unwrap();
        let info = &sim.sessions()[session as usize];
        assert_eq!(info.received, 1);
        assert!(info.report.contains("Reply from 10.0.0.2"));
        let trace = sim.trace();
        assert!(trace.iter().any(|e| e.protocol.as_deref() == Some("ARP")));
        assert!(trace.iter().all(|e| !e.summary.is_empty()));
        assert!(
            sim.host_command(a.id, "arp -a".into(), sim.end_time_ns())
                .contains("10.0.0.2")
        );
        assert!(sim.mac_address(a.id, "FastEthernet0".into()).is_some());
    }

    #[test]
    fn form_parsing() {
        let ip = parse_host_ip("192.168.1.10".into(), "/24".into(), "192.168.1.1".into()).unwrap();
        assert_eq!((ip.prefix_len, ip.mask.as_str()), (24, "255.255.255.0"));
        assert!(
            parse_host_ip("192.168.1.10".into(), "255.255.255.0".into(), "".into())
                .unwrap()
                .gateway
                .is_none()
        );
        assert!(parse_host_ip("192.168.1.300".into(), "24".into(), "".into()).is_err());
        assert!(parse_host_ip("192.168.1.10".into(), "255.0.255.0".into(), "".into()).is_err());
        assert!(parse_host_ip("192.168.1.10".into(), "24".into(), "10.0.0.1".into()).is_err());
    }
}

/// How one switch port looks in the VLAN / spanning-tree overlay.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PortOverlay {
    pub device_id: u64,
    pub port: String,
    /// Access VLAN (none for trunks and routed ports).
    pub vlan: Option<u16>,
    pub trunk: bool,
    /// Spanning tree blocks it for at least one VLAN it carries.
    pub blocked: bool,
    pub err_disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RootBridge {
    pub vlan: u16,
    pub device_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Layer2Overlay {
    pub ports: Vec<PortOverlay>,
    pub roots: Vec<RootBridge>,
    /// VLANs in use anywhere (beyond VLAN 1), for the legend.
    pub vlans: Vec<u16>,
}

#[uniffi::export]
impl Simulation {
    /// VLAN membership, trunks, blocked and err-disabled ports, root bridges.
    pub fn layer2_overlay(&self) -> Layer2Overlay {
        let network = self.lock();
        let project = network.project();
        let l2 = network.layer2();
        let mut ports = Vec::new();
        let mut vlans = std::collections::BTreeSet::new();
        for d in project.devices().iter().filter(|d| d.kind.is_switch()) {
            vlans.extend(d.config.vlans.keys().copied());
            let disabled = network.err_disabled_ports(d.id);
            for port in d.kind.port_names() {
                let sp = d.config.interface(d.kind, &port).switchport;
                if sp.routed {
                    continue;
                }
                let blocked = l2.trees.values().any(|t| {
                    t.ports
                        .get(&(d.id, port.clone()))
                        .is_some_and(|p| !p.role.forwards())
                });
                let err_disabled = disabled.contains(&port);
                let interesting = sp.is_trunk() || sp.access_vlan != 1 || blocked || err_disabled;
                if interesting {
                    ports.push(PortOverlay {
                        device_id: d.id.0,
                        port,
                        vlan: (!sp.is_trunk()).then_some(sp.access_vlan),
                        trunk: sp.is_trunk(),
                        blocked,
                        err_disabled,
                    });
                }
            }
        }
        // Only worth a crown where there's a choice of root (two or more bridges).
        let roots = l2
            .trees
            .values()
            .filter(|t| t.bridges.len() > 1)
            .map(|t| RootBridge {
                vlan: t.vlan,
                device_id: t.root_device.0,
            })
            .collect();
        Layer2Overlay {
            ports,
            roots,
            vlans: vlans.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StpRole {
    Root,
    Designated,
    Alternate,
    Backup,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StpPortInfo {
    pub device_id: u64,
    pub port: String,
    pub role: StpRole,
    pub cost: u32,
    pub forwarding: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StpBridgeInfo {
    pub device_id: u64,
    /// "32769 / 02:4e:…".
    pub bridge_id: String,
    pub root_cost: u32,
    pub root_port: Option<String>,
    pub is_root: bool,
    pub reason: String,
}

/// One VLAN's spanning tree, for the STP view.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StpView {
    pub vlan: u16,
    /// Every VLAN with a tree, for the VLAN picker.
    pub vlans: Vec<u16>,
    pub bridges: Vec<StpBridgeInfo>,
    pub ports: Vec<StpPortInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StpChangeInfo {
    pub device_id: u64,
    pub port: Option<String>,
    pub before: Option<StpRole>,
    pub after: Option<StpRole>,
    pub explanation: String,
}

/// The tree after a link fails, and the steps that got it there.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StpFailure {
    pub after: StpView,
    pub changes: Vec<StpChangeInfo>,
}

fn role(r: engine::PortRole) -> StpRole {
    match r {
        engine::PortRole::Root => StpRole::Root,
        engine::PortRole::Designated => StpRole::Designated,
        engine::PortRole::Alternate => StpRole::Alternate,
        engine::PortRole::Backup => StpRole::Backup,
    }
}

fn stp_view(l2: &engine::Layer2, vlan: u16) -> StpView {
    let vlans = l2.trees.keys().copied().collect();
    let Some(tree) = l2.trees.get(&vlan) else {
        return StpView {
            vlan,
            vlans,
            bridges: Vec::new(),
            ports: Vec::new(),
        };
    };
    StpView {
        vlan,
        vlans,
        bridges: tree
            .bridges
            .iter()
            .map(|(id, b)| StpBridgeInfo {
                device_id: id.0,
                bridge_id: b.id.to_string(),
                root_cost: b.root_cost,
                root_port: b.root_port.clone(),
                is_root: *id == tree.root_device,
                reason: b.reason.clone(),
            })
            .collect(),
        ports: tree
            .ports
            .iter()
            .map(|((id, port), p)| StpPortInfo {
                device_id: id.0,
                port: port.clone(),
                role: role(p.role),
                cost: p.cost,
                forwarding: p.role.forwards(),
                reason: p.reason.clone(),
            })
            .collect(),
    }
}

#[uniffi::export]
impl Simulation {
    /// The spanning tree of one VLAN, with the reason for every role.
    pub fn stp_view(&self, vlan: u16) -> StpView {
        stp_view(self.lock().layer2(), vlan)
    }

    /// What the tree of `vlan` becomes if `link_id` fails (the project
    /// itself isn't changed).
    pub fn stp_link_failure(&self, link_id: u64, vlan: u16) -> StpFailure {
        let network = self.lock();
        let (after, changes) =
            engine::Layer2::after_link_failure(network.project(), doc::LinkId(link_id), vlan);
        StpFailure {
            after: stp_view(&after, vlan),
            changes: changes
                .into_iter()
                .map(|c| StpChangeInfo {
                    device_id: c.device.0,
                    port: c.port,
                    before: c.before.map(role),
                    after: c.after.map(role),
                    explanation: c.explanation,
                })
                .collect(),
        }
    }
}

#[uniffi::export]
impl Simulation {
    /// The address an end device has after everything run so far (static,
    /// leased from DHCP, or APIPA).
    pub fn current_address(&self, device: u64) -> Option<CurrentAddress> {
        let network = self.lock();
        let (address, prefix_len, gateway) = network.current_address(doc::DeviceId(device))?;
        Some(CurrentAddress {
            address: address.to_string(),
            prefix_len,
            gateway: gateway.map(|g| g.to_string()),
            dhcp_server: network
                .held_lease(doc::DeviceId(device))
                .map(|l| l.server.to_string()),
            apipa: address.octets()[..2] == [169, 254],
        })
    }

    /// ACLs, DHCP relays and DHCP servers, for badges on the canvas.
    pub fn service_badges(&self) -> Vec<ServiceBadge> {
        let network = self.lock();
        let mut badges = Vec::new();
        for d in network
            .project()
            .devices()
            .iter()
            .filter(|d| d.kind.has_cli())
        {
            if !d.config.dhcp_pools.is_empty() {
                let pools: Vec<String> =
                    d.config.dhcp_pools.iter().map(|p| p.name.clone()).collect();
                badges.push(ServiceBadge {
                    device_id: d.id.0,
                    port: None,
                    kind: "dhcp-server".into(),
                    label: format!("DHCP server: {}", pools.join(", ")),
                });
            }
            for (name, iface) in &d.config.interfaces {
                let port = name.split('.').next().unwrap_or(name).to_string();
                for (kind, acl) in [("acl-in", &iface.acl_in), ("acl-out", &iface.acl_out)] {
                    if let Some(acl) = acl {
                        let direction = if kind == "acl-in" { "in" } else { "out" };
                        badges.push(ServiceBadge {
                            device_id: d.id.0,
                            port: Some(port.clone()),
                            kind: kind.into(),
                            label: format!("ACL {acl} {direction} on {name}"),
                        });
                    }
                }
                if let Some(side) = iface.nat {
                    badges.push(ServiceBadge {
                        device_id: d.id.0,
                        port: Some(port.clone()),
                        kind: format!("nat-{}", side.ios()),
                        label: format!("NAT {} on {name}", side.ios()),
                    });
                }
                if !iface.helper_addresses.is_empty() {
                    let helpers: Vec<String> = iface
                        .helper_addresses
                        .iter()
                        .map(|h| h.to_string())
                        .collect();
                    badges.push(ServiceBadge {
                        device_id: d.id.0,
                        port: Some(port.clone()),
                        kind: "relay".into(),
                        label: format!("DHCP relay to {}", helpers.join(", ")),
                    });
                }
            }
        }
        badges
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OspfIfaceInfo {
    pub name: String,
    /// The physical port the interface uses (for canvas labels).
    pub port: String,
    /// "10.0.12.1/24".
    pub address: String,
    pub area: u32,
    pub cost: u32,
    /// "DR", "BDR", "DROTH", "P2P", "LOOP" or "WAIT".
    pub state: String,
    pub passive: bool,
    /// Why it's in OSPF.
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OspfNeighborInfo {
    pub device_id: u64,
    pub router_id: String,
    /// "FULL", "2WAY" or "DOWN".
    pub state: String,
    /// The neighbour's role: "DR", "BDR", "DROTHER" or "-".
    pub role: String,
    pub iface: String,
    pub their_iface: String,
    pub problem: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OspfRouteInfo {
    /// "10.0.3.0/24".
    pub prefix: String,
    /// "O", "O IA" or "O*E2".
    pub code: String,
    pub cost: u32,
    /// "10.0.12.2 via Gi0/1", one per equal-cost path.
    pub next_hops: Vec<String>,
    pub path: Vec<String>,
    pub reason: String,
    pub alternatives: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OspfRouterInfo {
    pub device_id: u64,
    pub router_id: Option<String>,
    pub router_id_reason: String,
    pub abr: bool,
    pub asbr: bool,
    pub interfaces: Vec<OspfIfaceInfo>,
    pub neighbors: Vec<OspfNeighborInfo>,
    pub routes: Vec<OspfRouteInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct OspfMember {
    pub device_id: u64,
    pub iface: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OspfSegmentInfo {
    /// "10.0.0.0/24".
    pub network: String,
    pub members: Vec<OspfMember>,
    pub dr: Option<OspfMember>,
    pub bdr: Option<OspfMember>,
    pub election: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OspfLsaInfo {
    /// "Router", "Network", "Summary" or "External".
    pub kind: String,
    pub link_id: String,
    pub adv_router: String,
    pub describe: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OspfAreaInfo {
    pub area: u32,
    pub lsas: Vec<OspfLsaInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OspfView {
    pub routers: Vec<OspfRouterInfo>,
    pub segments: Vec<OspfSegmentInfo>,
    pub areas: Vec<OspfAreaInfo>,
    pub external: Vec<OspfLsaInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OspfChangeInfo {
    pub device_id: u64,
    /// The route that changed ("10.0.3.0/24"); `None` for a lost neighbour.
    pub prefix: Option<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OspfFailure {
    pub after: OspfView,
    pub changes: Vec<OspfChangeInfo>,
}

fn lsa_info(l: &engine::Lsa) -> OspfLsaInfo {
    OspfLsaInfo {
        kind: match l.kind {
            engine::LsaKind::Router => "Router",
            engine::LsaKind::Network => "Network",
            engine::LsaKind::Summary => "Summary",
            engine::LsaKind::External => "External",
        }
        .into(),
        link_id: l.link_id.to_string(),
        adv_router: l.adv_router.to_string(),
        describe: l.describe.clone(),
    }
}

fn ospf_view(ospf: &engine::Ospf, project: &doc::Project) -> OspfView {
    let port_of = |id: doc::DeviceId, name: &str| {
        let kind = project.device(id).map(|d| d.kind);
        match kind {
            // A switch's Vlan interface has no one port.
            Some(k) if k.is_switch() && name.starts_with("Vlan") => String::new(),
            _ => name.split('.').next().unwrap_or(name).to_string(),
        }
    };
    let member = |(id, iface): &(doc::DeviceId, String)| OspfMember {
        device_id: id.0,
        iface: iface.clone(),
    };
    OspfView {
        routers: ospf
            .routers
            .values()
            .map(|r| OspfRouterInfo {
                device_id: r.device.0,
                router_id: r.router_id.map(|x| x.to_string()),
                router_id_reason: r.router_id_reason.clone(),
                abr: r.abr,
                asbr: r.asbr,
                interfaces: r
                    .ifaces
                    .iter()
                    .map(|i| OspfIfaceInfo {
                        name: i.name.clone(),
                        port: port_of(r.device, &i.name),
                        address: format!("{}/{}", i.address, i.prefix_len),
                        area: i.area,
                        cost: i.cost,
                        state: i.state.ios().into(),
                        passive: i.passive,
                        why: i.why.clone(),
                    })
                    .collect(),
                neighbors: r
                    .neighbors
                    .iter()
                    .map(|n| OspfNeighborInfo {
                        device_id: n.device.0,
                        router_id: n.router_id.to_string(),
                        state: n.state.ios().into(),
                        role: n.role.into(),
                        iface: n.iface.clone(),
                        their_iface: n.their_iface.clone(),
                        problem: n.problem.clone(),
                        reason: n.reason.clone(),
                    })
                    .collect(),
                routes: r
                    .routes
                    .iter()
                    .map(|x| OspfRouteInfo {
                        prefix: format!("{}/{}", x.network, x.prefix_len),
                        code: match x.kind {
                            engine::OspfRouteKind::Intra => "O",
                            engine::OspfRouteKind::Inter => "O IA",
                            engine::OspfRouteKind::External2 => "O*E2",
                        }
                        .into(),
                        cost: x.cost,
                        next_hops: x
                            .next_hops
                            .iter()
                            .map(|(nh, i)| format!("{nh} via {}", engine::abbreviate_port(i)))
                            .collect(),
                        path: x.path.clone(),
                        reason: x.reason.clone(),
                        alternatives: x.alternatives.clone(),
                    })
                    .collect(),
            })
            .collect(),
        segments: ospf
            .segments
            .iter()
            .filter(|s| s.members.len() > 1)
            .map(|s| OspfSegmentInfo {
                network: format!("{}/{}", s.network, s.prefix_len),
                members: s.members.iter().map(member).collect(),
                dr: s.dr.as_ref().map(member),
                bdr: s.bdr.as_ref().map(member),
                election: s.election.clone(),
            })
            .collect(),
        areas: ospf
            .lsdb
            .iter()
            .map(|(area, lsas)| OspfAreaInfo {
                area: *area,
                lsas: lsas.iter().map(lsa_info).collect(),
            })
            .collect(),
        external: ospf.external.iter().map(lsa_info).collect(),
    }
}

#[uniffi::export]
impl Simulation {
    /// OSPF in its converged state: routers, neighbours, elections, the
    /// link-state database and the routes chosen, each with its reason.
    pub fn ospf_view(&self) -> OspfView {
        let network = self.lock();
        ospf_view(network.ospf(), network.project())
    }

    /// What OSPF does if `link_id` fails (the project isn't changed).
    pub fn ospf_link_failure(&self, link_id: u64) -> OspfFailure {
        let network = self.lock();
        let (after, changes) =
            engine::Ospf::after_link_failure(network.project(), doc::LinkId(link_id));
        OspfFailure {
            after: ospf_view(&after, network.project()),
            changes: changes
                .into_iter()
                .map(|c| OspfChangeInfo {
                    device_id: c.device.0,
                    prefix: c.network.map(|(n, l)| format!("{n}/{l}")),
                    explanation: c.explanation,
                })
                .collect(),
        }
    }
}
