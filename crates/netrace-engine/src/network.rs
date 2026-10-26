use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::Ipv4Addr;
use std::time::Duration;

use netrace_doc::{DeviceId, DeviceKind, LinkId, Project};
use netrace_sim::{Scheduler, SimTime};

use crate::dhcp::{DhcpBinding, DhcpClient};
use crate::explain;
use crate::probe::{PingSession, Probe, ProbeResult, ReportStyle, SessionKind};
use crate::stack::{PortStates, Stack, StackKind};
use crate::stp::{Layer2, PortKey, analyse};
use crate::{
    Frame, Ipv4Packet, Layer, LayerNote, LayerRole, MacAddr, Outcome, Payload, TraceEvent,
    abbreviate_port,
};

/// Time a frame takes to cross one cable. Kept at a readable 1 ms so the
/// event list and round-trip times are easy to follow.
pub const HOP_LATENCY: Duration = Duration::from_millis(1);
pub(crate) const PING_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const ARP_TIMEOUT: Duration = Duration::from_secs(1);
/// Stops a run that would never end, such as a broadcast circling a
/// switching loop before spanning tree exists (M4).
const MAX_STEPS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    UnknownDevice(DeviceId),
    /// The device has no IP stack to send from (hubs, access points…).
    CantSend(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::UnknownDevice(id) => write!(f, "device {id} does not exist"),
            EngineError::CantSend(name) => {
                write!(
                    f,
                    "{name} can't send pings: it has no IP address of its own"
                )
            }
        }
    }
}

impl std::error::Error for EngineError {}

#[derive(Debug, Clone)]
struct Command {
    at: SimTime,
    source: DeviceId,
    destination: Ipv4Addr,
    kind: SessionKind,
    count: u16,
}

#[derive(Debug, Clone)]
pub(crate) enum Event {
    Deliver {
        to: DeviceId,
        port: String,
        frame: Frame,
        from: DeviceId,
        /// `None` inside a device (a switch handing a frame to its own `Vlan1`).
        link: Option<LinkId>,
        sent: SimTime,
        /// The session this frame is part of, carried along so every hop can be grouped.
        tag: Option<u64>,
    },
    ProbeSend {
        session: u64,
        seq: u16,
    },
    ProbeTimeout {
        session: u64,
        seq: u16,
    },
    ArpTimeout {
        device: DeviceId,
        ip: Ipv4Addr,
    },
    /// An end device starts DHCP (at power-on, or `ipconfig /renew`).
    DhcpStart {
        host: DeviceId,
    },
    /// Spanning-tree BPDUs, sent once at the start.
    SendBpdus,
    /// OSPF hellos on every OSPF interface, sent once at the start.
    SendOspfHellos,
    /// LS Updates between full OSPF neighbours, after the hellos.
    SendOspfUpdates,
    /// A TCP client sends (or resends) its SYN.
    TcpSyn {
        conn: u64,
        attempt: usize,
    },
    /// A TCP connection that never got an answer gives up.
    TcpGiveUp {
        conn: u64,
    },
    DhcpRetry {
        host: DeviceId,
        xid: u32,
    },
    DhcpTimeout {
        host: DeviceId,
        xid: u32,
    },
}

/// Where a frame came from, for the trace.
pub(crate) struct Arrival {
    pub from: DeviceId,
    pub link: Option<LinkId>,
    pub port: String,
    pub sent: SimTime,
    pub tag: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) enum Role {
    /// End devices and routers.
    Stack(Stack),
    /// Learning switch ((VLAN, MAC address) → port), plus its `Vlan`
    /// interfaces and routed ports.
    Switch {
        table: BTreeMap<(u16, MacAddr), String>,
        svi: Option<Stack>,
    },
    Hub,
    /// Not simulated yet; drops frames with the given explanation.
    Inert(&'static str),
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub name: String,
    pub ports: Vec<String>,
    pub role: Role,
}

/// A running simulation of a project's topology.
pub struct Network {
    pub(crate) project: Project,
    commands: Vec<Command>,
    pub(crate) scheduler: Scheduler<Event>,
    pub(crate) nodes: BTreeMap<DeviceId, Node>,
    /// (device, port) → (peer device, peer port, link).
    pub(crate) wires: BTreeMap<(DeviceId, String), (DeviceId, String, LinkId)>,
    pub(crate) ports: PortStates,
    /// VLAN membership and spanning tree, worked out from the topology.
    pub(crate) l2: Layer2,
    /// Switchport settings of every switch port, read once per rebuild:
    /// the forwarding hot path asks for them per port, per frame.
    pub(crate) switchports: BTreeMap<PortKey, netrace_doc::SwitchportConfig>,
    /// Switches using anything beyond VLAN 1 (explanations mention VLANs then).
    pub(crate) vlan_switches: BTreeSet<DeviceId>,
    /// Port security: MACs learned as secure on each port.
    pub(crate) secure: BTreeMap<PortKey, Vec<MacAddr>>,
    /// Ports shut down by a port security violation.
    pub(crate) psec_disabled: BTreeSet<PortKey>,
    pub(crate) violations: BTreeMap<PortKey, u32>,
    pub(crate) dhcp_clients: BTreeMap<DeviceId, DhcpClient>,
    /// Addresses servers have offered and are holding, by (server, client MAC).
    pub(crate) dhcp_offers: BTreeMap<(DeviceId, MacAddr), Ipv4Addr>,
    pub(crate) dhcp_bindings: BTreeMap<DeviceId, BTreeMap<MacAddr, DhcpBinding>>,
    /// The lease each DHCP client holds.
    pub(crate) dhcp_leases_held: BTreeMap<DeviceId, crate::Dhcp>,
    /// `ipconfig /renew` typed on end devices, replayed with everything else.
    renewals: Vec<(SimTime, DeviceId)>,
    /// TCP connections opened from end devices (`curl`, `telnet`), replayed.
    pub(crate) tcp_commands: Vec<crate::tcp::TcpCommand>,
    pub(crate) tcp_conns: Vec<crate::TcpConn>,
    /// ACL line hits, by (device, list, sequence), for `show access-lists`.
    pub(crate) acl_hits: BTreeMap<(DeviceId, String, u32), u32>,
    /// The ACL decision to attach to the next event this device records.
    pub(crate) pending_acl: Option<crate::AclDecision>,
    /// Each router's NAT translation table.
    pub(crate) nat_tables: BTreeMap<DeviceId, Vec<crate::NatEntry>>,
    /// NAT (hits, misses) per router, for `show ip nat statistics`.
    pub(crate) nat_hits: BTreeMap<DeviceId, (u32, u32)>,
    /// The NAT decision to attach to the next event this device records.
    pub(crate) pending_nat: Option<crate::NatDecision>,
    /// OSPF in its converged state, worked out with the topology.
    pub(crate) ospf: crate::Ospf,
    pub(crate) trace: Vec<TraceEvent>,
    pub(crate) sessions: Vec<PingSession>,
    warnings: Vec<String>,
    /// Which device owns each MAC, so explanations can say "PC1" not just hex.
    owners: BTreeMap<MacAddr, DeviceId>,
    /// Which device owns each IP address, for the same reason.
    ip_owners: BTreeMap<Ipv4Addr, DeviceId>,
}

impl Network {
    pub fn new(project: Project) -> Self {
        let mut network = Network {
            project,
            commands: Vec::new(),
            scheduler: Scheduler::new(),
            nodes: BTreeMap::new(),
            wires: BTreeMap::new(),
            ports: PortStates::default(),
            l2: Layer2::default(),
            secure: BTreeMap::new(),
            switchports: BTreeMap::new(),
            vlan_switches: BTreeSet::new(),
            psec_disabled: BTreeSet::new(),
            violations: BTreeMap::new(),
            dhcp_clients: BTreeMap::new(),
            dhcp_offers: BTreeMap::new(),
            dhcp_bindings: BTreeMap::new(),
            dhcp_leases_held: BTreeMap::new(),
            renewals: Vec::new(),
            tcp_commands: Vec::new(),
            tcp_conns: Vec::new(),
            acl_hits: BTreeMap::new(),
            pending_acl: None,
            nat_tables: BTreeMap::new(),
            nat_hits: BTreeMap::new(),
            pending_nat: None,
            ospf: crate::Ospf::default(),
            trace: Vec::new(),
            sessions: Vec::new(),
            warnings: Vec::new(),
            owners: BTreeMap::new(),
            ip_owners: BTreeMap::new(),
        };
        network.rebuild();
        network
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn trace(&self) -> &[TraceEvent] {
        &self.trace
    }

    pub fn sessions(&self) -> &[PingSession] {
        &self.sessions
    }

    pub fn session(&self, id: u64) -> Option<&PingSession> {
        self.sessions.get(usize::try_from(id).ok()?)
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Time of the last thing that happened.
    pub fn end_time(&self) -> SimTime {
        self.trace.last().map(|e| e.time).unwrap_or(SimTime::ZERO)
    }

    /// The MAC address a device uses on a port (or on `Vlan1`).
    pub fn mac(&self, device: DeviceId, port: &str) -> Option<MacAddr> {
        if let Some(stack) = self.stack(device)
            && let Some(iface) = stack.iface(port).or_else(|| stack.iface_for_port(port))
            && iface.mac != MacAddr::ZERO
        {
            return Some(iface.mac);
        }
        let node = self.nodes.get(&device)?;
        let index = node.ports.iter().position(|p| p == port)?;
        Some(MacAddr::for_port(device, index))
    }

    /// Schedules `count` pings from a device at `at`, then replays
    /// everything. End devices ping Windows-style (one a second); routers
    /// and switches the IOS way (back to back). Returns the session id.
    pub fn ping(
        &mut self,
        source: DeviceId,
        destination: Ipv4Addr,
        count: u16,
        at: SimTime,
    ) -> Result<u64, EngineError> {
        self.add_command(source, destination, SessionKind::Ping, count.max(1), at)
    }

    /// Schedules a traceroute (TTL 1, 2, 3… with three probes per hop).
    pub fn traceroute(
        &mut self,
        source: DeviceId,
        destination: Ipv4Addr,
        at: SimTime,
    ) -> Result<u64, EngineError> {
        self.add_command(source, destination, SessionKind::Traceroute, 0, at)
    }

    /// `ipconfig /renew` on an end device at `at`: a new DHCP exchange.
    pub fn renew(&mut self, host: DeviceId, at: SimTime) {
        self.renewals.push((at, host));
        self.rebuild();
    }

    fn add_command(
        &mut self,
        source: DeviceId,
        destination: Ipv4Addr,
        kind: SessionKind,
        count: u16,
        at: SimTime,
    ) -> Result<u64, EngineError> {
        let device = self
            .project
            .device(source)
            .ok_or(EngineError::UnknownDevice(source))?;
        if !device.kind.is_host() && !device.kind.has_cli() {
            return Err(EngineError::CantSend(device.name.clone()));
        }
        self.commands.push(Command {
            at,
            source,
            destination,
            kind,
            count,
        });
        self.rebuild();
        Ok(self.commands.len() as u64 - 1)
    }

    // MARK: Running

    /// Resets every device and replays all commands from time zero.
    pub(crate) fn rebuild(&mut self) {
        self.scheduler = Scheduler::new();
        self.trace.clear();
        self.warnings.clear();
        (self.ports, self.l2) = analyse(&self.project);
        self.switchports.clear();
        self.vlan_switches.clear();
        for d in self.project.devices().iter().filter(|d| d.kind.is_switch()) {
            for port in d.kind.ports() {
                if let Some(sp) = crate::stp::switchport(&self.project, d.id, port) {
                    if sp.is_trunk() {
                        self.vlan_switches.insert(d.id);
                    }
                    self.switchports.insert((d.id, port.clone()), sp);
                }
            }
            if !d.config.vlans.is_empty() {
                self.vlan_switches.insert(d.id);
            }
        }
        self.secure.clear();
        self.psec_disabled.clear();
        self.violations.clear();
        self.dhcp_clients.clear();
        self.dhcp_offers.clear();
        self.dhcp_bindings.clear();
        self.dhcp_leases_held.clear();
        self.acl_hits.clear();
        self.pending_acl = None;
        self.nat_tables.clear();
        self.nat_hits.clear();
        self.pending_nat = None;
        self.ospf = crate::Ospf::compute(&self.project, &self.ports, &self.l2);
        let ports = &self.ports;
        let l2 = &self.l2;
        let project = &self.project;
        let ospf = &self.ospf;
        let build = |id: DeviceId| {
            Stack::build_base(project, id, ports, l2).map(|mut s| {
                s.install_ospf(ospf, id);
                s
            })
        };
        self.nodes = project
            .devices()
            .iter()
            .map(|d| {
                let role = match d.kind {
                    k if k.is_host() || k == DeviceKind::Router => {
                        Role::Stack(build(d.id).expect("hosts and routers have stacks"))
                    }
                    k if k.is_switch() => Role::Switch {
                        table: BTreeMap::new(),
                        svi: build(d.id),
                    },
                    DeviceKind::Hub => Role::Hub,
                    DeviceKind::Firewall => Role::Inert(
                        "Firewall rules and NAT arrive in milestone M5, so for now it drops the frame.",
                    ),
                    DeviceKind::AccessPoint => {
                        Role::Inert("Wireless bridging comes after v1, so for now it drops the frame.")
                    }
                    DeviceKind::Cloud => Role::Inert("The ISP connection is simulated from milestone M5."),
                    _ => Role::Inert("It has no wired connection to handle the frame."),
                };
                (
                    d.id,
                    Node {
                        name: d.name.clone(),
                        ports: d.kind.port_names(),
                        role,
                    },
                )
            })
            .collect();
        self.owners = self
            .nodes
            .iter()
            .flat_map(|(id, n)| (0..n.ports.len()).map(move |i| (MacAddr::for_port(*id, i), *id)))
            .collect();
        self.ip_owners.clear();
        for (id, node) in &self.nodes {
            let stack = match &node.role {
                Role::Stack(s) => Some(s),
                Role::Switch { svi, .. } => svi.as_ref(),
                _ => None,
            };
            for iface in stack.into_iter().flat_map(|s| &s.ifaces) {
                if iface.mac != MacAddr::ZERO {
                    self.owners.insert(iface.mac, *id);
                }
                if let Some(a) = iface.address {
                    self.ip_owners.entry(a.address).or_insert(*id);
                }
            }
        }
        self.wires = BTreeMap::new();
        for link in self.project.links() {
            self.wires.insert(
                (link.a.device, link.a.port.clone()),
                (link.b.device, link.b.port.clone(), link.id),
            );
            self.wires.insert(
                (link.b.device, link.b.port.clone()),
                (link.a.device, link.a.port.clone(), link.id),
            );
        }
        self.sessions = self
            .commands
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let ios = self
                    .project
                    .device(c.source)
                    .is_some_and(|d| d.kind.has_cli());
                PingSession::new(
                    i as u64,
                    c.source,
                    c.destination,
                    c.kind,
                    if ios {
                        ReportStyle::Ios
                    } else {
                        ReportStyle::Windows
                    },
                    c.count,
                    c.at,
                )
            })
            .collect();
        // DHCP clients ask for an address as soon as they're on.
        for d in self
            .project
            .devices()
            .iter()
            .filter(|d| d.kind.is_host() && d.dhcp)
        {
            let _ = self
                .scheduler
                .schedule_at(SimTime::ZERO, Event::DhcpStart { host: d.id });
        }
        for &(at, host) in &self.renewals {
            let _ = self.scheduler.schedule_at(at, Event::DhcpStart { host });
        }
        self.schedule_control_plane();
        self.schedule_tcp();
        for (i, c) in self.commands.iter().enumerate() {
            let session = i as u64;
            if self.sessions[i].is_sequential() {
                let _ = self
                    .scheduler
                    .schedule_at(c.at, Event::ProbeSend { session, seq: 1 });
            } else {
                for seq in 1..=c.count {
                    let at = c.at + PING_INTERVAL * u32::from(seq - 1);
                    let _ = self
                        .scheduler
                        .schedule_at(at, Event::ProbeSend { session, seq });
                }
            }
        }
        self.run();
    }

    fn run(&mut self) {
        let mut steps = 0;
        while let Some((now, event)) = self.scheduler.pop() {
            steps += 1;
            if steps > MAX_STEPS {
                self.warnings.push(
                    "The simulation stopped early: frames kept circulating, which usually means \
                     switches are cabled in a loop. Spanning tree (milestone M4) will prevent this."
                        .into(),
                );
                break;
            }
            self.handle(now, event);
        }
    }

    fn handle(&mut self, now: SimTime, event: Event) {
        match event {
            Event::Deliver {
                to,
                port,
                frame,
                from,
                link,
                sent,
                tag,
            } => {
                let arrival = Arrival {
                    from,
                    link,
                    port,
                    sent,
                    tag,
                };
                if self.ports.is_disabled(to, &arrival.port) {
                    return self.arrive_on_disabled_port(now, to, arrival, frame);
                }
                match self.nodes.get(&to).map(|n| &n.role) {
                    Some(Role::Stack(_)) => self.stack_receive(now, to, arrival, frame),
                    // Its own `Vlan` interfaces, and routed ports, go to the IP stack.
                    Some(Role::Switch { .. })
                        if arrival.link.is_none() || self.is_routed_port(to, &arrival.port) =>
                    {
                        self.stack_receive(now, to, arrival, frame)
                    }
                    Some(Role::Switch { .. }) => self.switch_receive(now, to, arrival, frame),
                    Some(Role::Hub) => self.hub_receive(now, to, arrival, frame),
                    Some(Role::Inert(reason)) => {
                        let reason = *reason;
                        let name = self.name(to);
                        let port = abbreviate_port(&arrival.port);
                        let mut layers = vec![
                            explain::physical(format!("Arrived on {port}.")),
                            explain::data_link(
                                LayerRole::Used,
                                "Received, but not processed further.",
                                &frame,
                            ),
                        ];
                        layers.extend(explain::upper_not_used(&frame, &name));
                        self.record(
                            now,
                            to,
                            Some(&arrival),
                            Some(frame),
                            Outcome::Dropped,
                            format!("{name} receives the frame on {port}. {reason}"),
                            layers,
                            Vec::new(),
                        );
                    }
                    None => {}
                }
            }
            Event::ProbeSend { session, seq } => self.probe_send(now, session, seq),
            Event::ProbeTimeout { session, seq } => self.probe_timeout(now, session, seq),
            Event::ArpTimeout { device, ip } => self.arp_timeout(now, device, ip),
            Event::DhcpStart { host } => self.dhcp_start(now, host),
            Event::TcpSyn { conn, attempt } => self.tcp_syn(now, conn, attempt),
            Event::SendBpdus => self.send_bpdus(),
            Event::SendOspfHellos => self.send_ospf_hellos(),
            Event::SendOspfUpdates => self.send_ospf_updates(),
            Event::TcpGiveUp { conn } => self.tcp_give_up(now, conn),
            Event::DhcpRetry { host, xid } => self.dhcp_retry(now, host, xid),
            Event::DhcpTimeout { host, xid } => self.dhcp_timeout(now, host, xid),
        }
    }

    fn arrive_on_disabled_port(
        &mut self,
        now: SimTime,
        id: DeviceId,
        arrival: Arrival,
        frame: Frame,
    ) {
        let name = self.name(id);
        let port = abbreviate_port(&arrival.port);
        let layers = vec![
            explain::physical(format!("{port} is shut down, so the link is down.")),
            explain::not_used(Layer::DataLink, "Never read: the port is switched off."),
        ];
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            Outcome::Dropped,
            format!(
                "The frame reaches {name}'s {port}, but that interface is administratively down \
                 (shutdown), so it's lost. Enter `no shutdown` on {port} to bring it up."
            ),
            layers,
            Vec::new(),
        );
    }

    // MARK: Probes (ping and traceroute)

    fn probe_send(&mut self, now: SimTime, session: u64, seq: u16) {
        let Some(s) = self.session(session) else {
            return;
        };
        let (source, destination) = (s.source, s.destination);
        let name = self.name(source);
        let Some(stack) = self.stack(source).cloned() else {
            return self.fail_probe(
                now,
                session,
                seq,
                source,
                format!("{name} has no IP address to send from."),
            );
        };
        let ttl = self.sessions[session as usize].ttl_for(seq, stack.ttl);
        self.sessions[session as usize].probes.push(Probe {
            seq,
            ttl,
            sent: None,
            result: None,
            waited_for_arp: false,
        });

        if stack.kind == StackKind::Host {
            let iface = &stack.ifaces[0];
            let Some(ip) = iface.address else {
                return self.fail_probe(
                    now,
                    session,
                    seq,
                    source,
                    format!("{name} can't send the ping: it has no IP address yet. Set one in its IP configuration."),
                );
            };
            if !iface.up {
                return self.fail_probe(
                    now,
                    session,
                    seq,
                    source,
                    format!(
                        "{name} can't send the ping: nothing is plugged into {}.",
                        abbreviate_port(&iface.name)
                    ),
                );
            }
            if !ip.contains(destination)
                && destination != ip.address
                && !stack.routes.iter().any(|r| r.is_default())
            {
                return self.fail_probe(
                    now,
                    session,
                    seq,
                    source,
                    format!(
                        "{destination} is on a different network from {name} ({}/{}), and {name} has no \
                         default gateway to send it to.",
                        ip.network(),
                        ip.prefix_len
                    ),
                );
            }
        }
        if stack.owner_of(destination).is_some() {
            self.sessions[session as usize]
                .probe_mut(seq)
                .expect("just pushed")
                .sent = Some(now);
            let what = if stack.kind == StackKind::Host {
                "PC"
            } else {
                "device"
            };
            self.record(
                now,
                source,
                None,
                None,
                Outcome::Completed,
                format!(
                    "{name} pings its own address {destination}. The reply never leaves the {what}."
                ),
                Vec::new(),
                Vec::new(),
            );
            self.set_session(Some(session));
            return self.settle(
                now,
                session,
                seq,
                ProbeResult::Reply {
                    rtt: Duration::ZERO,
                    ttl: stack.ttl,
                    from: destination,
                },
            );
        }
        let Some((_, iface, _)) = stack.resolve(destination) else {
            let reason = if stack.kind == StackKind::Host {
                format!("{name} has no route to {destination}.")
            } else {
                format!(
                    "{name} has no route to {destination}: no connected network contains it and no static \
                     or default route covers it. Check `show ip route`."
                )
            };
            return self.fail_probe(now, session, seq, source, reason);
        };
        let Some(src) = stack.source_for(iface) else {
            return self.fail_probe(
                now,
                session,
                seq,
                source,
                format!(
                    "{name}'s {} has no IP address to send from.",
                    abbreviate_port(&iface.name)
                ),
            );
        };
        let packet = Ipv4Packet {
            src,
            dst: destination,
            ttl,
            l4: crate::L4::Icmp(crate::Icmp {
                kind: crate::IcmpKind::EchoRequest,
                id: session as u16 + 1,
                seq,
            }),
        };
        self.sessions[session as usize]
            .probe_mut(seq)
            .expect("just pushed")
            .sent = Some(now);
        self.scheduler
            .schedule_in(PROBE_TIMEOUT, Event::ProbeTimeout { session, seq });
        self.send_from(now, source, packet, Some(session), true);
    }

    fn probe_timeout(&mut self, now: SimTime, session: u64, seq: u16) {
        let Some(s) = self.sessions.get(session as usize) else {
            return;
        };
        if s.settled(seq) {
            return;
        }
        let (source, destination, kind) = (s.source, s.destination, s.kind);
        let ttl = s.probe(seq).map(|p| p.ttl).unwrap_or(0);
        let name = self.name(source);
        let summary = match kind {
            SessionKind::Ping => format!(
                "Ping {seq} from {name} to {destination} timed out: no reply came back within 2 seconds."
            ),
            SessionKind::Traceroute => format!(
                "Traceroute probe {seq} from {name} (TTL {ttl}) got no answer within 2 seconds, so that \
                 hop shows *."
            ),
        };
        self.record(
            now,
            source,
            None,
            None,
            Outcome::Completed,
            summary,
            Vec::new(),
            Vec::new(),
        );
        self.set_session(Some(session));
        self.settle(now, session, seq, ProbeResult::TimedOut);
    }

    fn fail_probe(
        &mut self,
        now: SimTime,
        session: u64,
        seq: u16,
        source: DeviceId,
        reason: String,
    ) {
        self.record(
            now,
            source,
            None,
            None,
            Outcome::Dropped,
            reason.clone(),
            Vec::new(),
            Vec::new(),
        );
        self.set_session(Some(session));
        self.settle(now, session, seq, ProbeResult::Failed(reason));
    }

    /// Records a probe's result and, for back-to-back sessions, sends the next.
    pub(crate) fn settle(&mut self, now: SimTime, session: u64, seq: u16, result: ProbeResult) {
        let Some(s) = self.sessions.get_mut(session as usize) else {
            return;
        };
        if !s.settle(seq, result) || !s.is_sequential() {
            return;
        }
        if let Some(next) = s.next_after(seq) {
            let _ = self
                .scheduler
                .schedule_at(now, Event::ProbeSend { session, seq: next });
        }
    }

    // MARK: Switches and hubs

    fn hub_receive(&mut self, now: SimTime, id: DeviceId, arrival: Arrival, frame: Frame) {
        let name = self.name(id);
        let outs: Vec<String> = self
            .wired_ports(id)
            .into_iter()
            .filter(|p| *p != arrival.port)
            .collect();
        for port in &outs {
            self.transmit(id, port, frame, arrival.tag);
        }
        let mut layers = vec![
            explain::physical(format!(
                "Arrived on {}; the signal is repeated out {}.",
                abbreviate_port(&arrival.port),
                if outs.is_empty() {
                    "nowhere".to_string()
                } else {
                    list(&outs)
                }
            )),
            explain::not_used(Layer::DataLink, "A hub doesn't read MAC addresses at all."),
        ];
        layers.extend(explain::upper_not_used(&frame, "A hub"));
        let summary = format!(
            "{name} is a hub: it repeats the signal out of every other connected port ({}) without reading any \
             addresses. Every device behind it gets a copy.",
            if outs.is_empty() {
                "none".to_string()
            } else {
                list(&outs)
            }
        );
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            Outcome::Flooded,
            summary,
            layers,
            outs,
        );
    }

    // MARK: Helpers

    pub(crate) fn transmit(
        &mut self,
        from: DeviceId,
        port: &str,
        frame: Frame,
        tag: Option<u64>,
    ) -> bool {
        if self.ports.is_disabled(from, port) {
            return false;
        }
        let Some((peer, peer_port, link)) = self.wires.get(&(from, port.to_string())).cloned()
        else {
            return false;
        };
        let sent = self.scheduler.now();
        self.scheduler.schedule_in(
            HOP_LATENCY,
            Event::Deliver {
                to: peer,
                port: peer_port,
                frame,
                from,
                link: Some(link),
                sent,
                tag,
            },
        );
        true
    }

    /// Ports with a cable attached and not shut down, in panel order.
    pub(crate) fn wired_ports(&self, id: DeviceId) -> Vec<String> {
        self.nodes
            .get(&id)
            .map(|n| {
                n.ports
                    .iter()
                    .filter(|p| {
                        self.wires.contains_key(&(id, (*p).clone()))
                            && !self.ports.is_disabled(id, p)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn stack(&self, id: DeviceId) -> Option<&Stack> {
        match &self.nodes.get(&id)?.role {
            Role::Stack(s) => Some(s),
            Role::Switch { svi, .. } => svi.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn stack_mut(&mut self, id: DeviceId) -> Option<&mut Stack> {
        match &mut self.nodes.get_mut(&id)?.role {
            Role::Stack(s) => Some(s),
            Role::Switch { svi, .. } => svi.as_mut(),
            _ => None,
        }
    }

    fn is_routed_port(&self, id: DeviceId, port: &str) -> bool {
        self.project.device(id).is_some_and(|d| {
            d.kind.has_port(port) && d.config.interface(d.kind, port).switchport.routed
        })
    }

    pub(crate) fn mac_table(&self, id: DeviceId) -> Option<&BTreeMap<(u16, MacAddr), String>> {
        match &self.nodes.get(&id)?.role {
            Role::Switch { table, .. } => Some(table),
            _ => None,
        }
    }

    /// "PC1 (02:4e:00:00:02:00)", or the bare address if nobody owns it.
    pub(crate) fn who(&self, mac: MacAddr) -> String {
        if mac.is_broadcast() {
            return "the broadcast address".into();
        }
        match self.owners.get(&mac) {
            Some(id) => format!("{} ({mac})", self.name(*id)),
            None => mac.to_string(),
        }
    }

    /// "R1 (10.0.0.1)", or the bare address if no device has it.
    pub(crate) fn who_ip(&self, ip: Ipv4Addr) -> String {
        match self.ip_owners.get(&ip) {
            Some(id) => format!("{} ({ip})", self.name(*id)),
            None => ip.to_string(),
        }
    }

    pub(crate) fn name(&self, id: DeviceId) -> String {
        self.nodes
            .get(&id)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| format!("device {id}"))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record(
        &mut self,
        now: SimTime,
        device: DeviceId,
        arrival: Option<&Arrival>,
        frame: Option<Frame>,
        outcome: Outcome,
        summary: String,
        layers: Vec<LayerNote>,
        egress: Vec<String>,
    ) {
        let session = frame
            .and_then(|f| match f.payload {
                Payload::Ipv4(p) => session_of(&p),
                Payload::Arp(_) | Payload::Bpdu(_) => None,
            })
            .or(arrival.and_then(|a| a.tag));
        self.trace.push(TraceEvent {
            id: self.trace.len() as u64,
            time: now,
            sent: arrival.map(|a| a.sent),
            device,
            from: arrival.map(|a| a.from),
            link: arrival.and_then(|a| a.link),
            ingress: arrival.map(|a| a.port.clone()),
            egress,
            frame,
            outcome,
            summary,
            layers,
            session,
            acl: self.pending_acl.take().filter(|a| a.device == device),
            nat: self.pending_nat.take().filter(|n| n.device == device),
        });
    }

    /// Tags the most recent event with a session (ARP and failure events
    /// carry no ICMP id of their own).
    pub(crate) fn set_session(&mut self, session: Option<u64>) {
        if let (Some(s), Some(last)) = (session, self.trace.last_mut()) {
            last.session.get_or_insert(s);
        }
    }
}

/// The session an ICMP packet belongs to (its identifier is session + 1;
/// errors quote the identifier of the probe that caused them).
pub(crate) fn session_of(packet: &Ipv4Packet) -> Option<u64> {
    packet
        .icmp()
        .and_then(|icmp| u64::from(icmp.id).checked_sub(1))
}

/// "Fa0/1, Fa0/2 and Gi0/1".
pub(crate) fn list(ports: &[String]) -> String {
    let short: Vec<String> = ports.iter().map(|p| abbreviate_port(p)).collect();
    match short.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
