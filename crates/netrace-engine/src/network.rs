use std::collections::BTreeMap;
use std::fmt;
use std::net::Ipv4Addr;
use std::time::Duration;

use netrace_doc::{DeviceId, DeviceKind, HostIp, LinkId, Project};
use netrace_sim::{Scheduler, SimTime};

use crate::explain;
use crate::{
    ArpOp, ArpPacket, Frame, Icmp, IcmpKind, Ipv4Packet, Layer, LayerNote, LayerRole, MacAddr,
    Outcome, Payload, TraceEvent, abbreviate_port,
};

/// Time a frame takes to cross one cable. Kept at a readable 1 ms so the
/// event list and round-trip times are easy to follow.
pub const HOP_LATENCY: Duration = Duration::from_millis(1);
const PING_INTERVAL: Duration = Duration::from_secs(1);
const PING_TIMEOUT: Duration = Duration::from_secs(2);
const ARP_TIMEOUT: Duration = Duration::from_secs(1);
const HOST_TTL: u8 = 128;
/// Stops a run that would never end, such as a broadcast circling a
/// switching loop before spanning tree exists (M4).
const MAX_STEPS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    UnknownDevice(DeviceId),
    NotAHost(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::UnknownDevice(id) => write!(f, "device {id} does not exist"),
            EngineError::NotAHost(name) => {
                write!(
                    f,
                    "{name} can't send pings yet (only PCs, laptops, servers, printers and phones can in this version)"
                )
            }
        }
    }
}

impl std::error::Error for EngineError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PingFailure {
    TimedOut,
    Unreachable(String),
}

/// One `ping` (one or more echo requests) and how each attempt ended.
#[derive(Debug, Clone, PartialEq)]
pub struct PingSession {
    pub id: u64,
    pub source: DeviceId,
    pub destination: Ipv4Addr,
    pub count: u16,
    pub started: SimTime,
    /// Sequence number → (round-trip time, TTL of the reply).
    pub replies: BTreeMap<u16, (Duration, u8)>,
    pub failures: BTreeMap<u16, PingFailure>,
    sent_at: BTreeMap<u16, SimTime>,
    /// Attempts that had to wait for ARP before leaving the host.
    waited_for_arp: std::collections::BTreeSet<u16>,
}

impl PingSession {
    pub fn is_finished(&self) -> bool {
        self.replies.len() + self.failures.len() >= usize::from(self.count)
    }

    fn settled(&self, seq: u16) -> bool {
        self.replies.contains_key(&seq) || self.failures.contains_key(&seq)
    }
}

#[derive(Debug, Clone)]
struct Command {
    at: SimTime,
    source: DeviceId,
    destination: Ipv4Addr,
    count: u16,
}

#[derive(Debug, Clone)]
enum Event {
    Deliver {
        to: DeviceId,
        port: String,
        frame: Frame,
        from: DeviceId,
        link: LinkId,
        sent: SimTime,
        /// The ping this frame is part of, carried along so every hop can be grouped.
        tag: Option<u64>,
    },
    PingSend {
        session: u64,
        seq: u16,
    },
    PingTimeout {
        session: u64,
        seq: u16,
    },
    ArpTimeout {
        host: DeviceId,
        ip: Ipv4Addr,
    },
}

/// Where a frame came from, for the trace.
struct Arrival {
    from: DeviceId,
    link: LinkId,
    port: String,
    sent: SimTime,
    tag: Option<u64>,
}

#[derive(Debug, Clone)]
struct Waiting {
    next_hop: Ipv4Addr,
    packet: Ipv4Packet,
    session: Option<u64>,
}

#[derive(Debug, Clone)]
struct Host {
    port: Option<String>,
    mac: MacAddr,
    ip: Option<HostIp>,
    arp: BTreeMap<Ipv4Addr, MacAddr>,
    waiting: Vec<Waiting>,
}

#[derive(Debug, Clone)]
enum Role {
    Host(Host),
    /// Learning switch: MAC address → port.
    Switch(BTreeMap<MacAddr, String>),
    Hub,
    /// Not simulated yet; drops frames with the given explanation.
    Inert(&'static str),
}

#[derive(Debug, Clone)]
struct Node {
    name: String,
    ports: Vec<String>,
    role: Role,
}

/// Read-only view of an end device, for command output.
pub(crate) struct HostView<'a> {
    pub port: Option<&'a str>,
    pub mac: MacAddr,
    pub ip: Option<HostIp>,
    pub arp: &'a BTreeMap<Ipv4Addr, MacAddr>,
}

/// A running simulation of a project's topology.
pub struct Network {
    project: Project,
    commands: Vec<Command>,
    scheduler: Scheduler<Event>,
    nodes: BTreeMap<DeviceId, Node>,
    /// (device, port) → (peer device, peer port, link).
    wires: BTreeMap<(DeviceId, String), (DeviceId, String, LinkId)>,
    trace: Vec<TraceEvent>,
    sessions: Vec<PingSession>,
    warnings: Vec<String>,
    /// Which device owns each MAC, so explanations can say "PC1" not just hex.
    owners: BTreeMap<MacAddr, DeviceId>,
}

impl Network {
    pub fn new(project: Project) -> Self {
        let mut network = Network {
            project,
            commands: Vec::new(),
            scheduler: Scheduler::new(),
            nodes: BTreeMap::new(),
            wires: BTreeMap::new(),
            trace: Vec::new(),
            sessions: Vec::new(),
            warnings: Vec::new(),
            owners: BTreeMap::new(),
        };
        network.rebuild();
        network
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

    /// The MAC address a device uses on a port.
    pub fn mac(&self, device: DeviceId, port: &str) -> Option<MacAddr> {
        let node = self.nodes.get(&device)?;
        let index = node.ports.iter().position(|p| p == port)?;
        Some(MacAddr::for_port(device, index))
    }

    /// Schedules `count` pings from an end device, one second apart, starting
    /// at `at`, then replays everything. Returns the session id.
    pub fn ping(
        &mut self,
        source: DeviceId,
        destination: Ipv4Addr,
        count: u16,
        at: SimTime,
    ) -> Result<u64, EngineError> {
        let device = self
            .project
            .device(source)
            .ok_or(EngineError::UnknownDevice(source))?;
        if !device.kind.is_host() {
            return Err(EngineError::NotAHost(device.name.clone()));
        }
        self.commands.push(Command {
            at,
            source,
            destination,
            count: count.max(1),
        });
        self.rebuild();
        Ok(self.commands.len() as u64 - 1)
    }

    // MARK: Running

    /// Resets every device and replays all commands from time zero.
    fn rebuild(&mut self) {
        self.scheduler = Scheduler::new();
        self.trace.clear();
        self.warnings.clear();
        self.nodes = self
            .project
            .devices()
            .iter()
            .map(|d| {
                let ports = d.kind.port_names();
                let role = match d.kind {
                    k if k.is_host() => Role::Host(Host {
                        port: ports.first().cloned(),
                        mac: MacAddr::for_port(d.id, 0),
                        ip: d.ip,
                        arp: BTreeMap::new(),
                        waiting: Vec::new(),
                    }),
                    DeviceKind::L2Switch | DeviceKind::L3Switch => Role::Switch(BTreeMap::new()),
                    DeviceKind::Hub => Role::Hub,
                    DeviceKind::Router | DeviceKind::Firewall => Role::Inert(
                        "Routing between networks arrives in milestone M3, so for now it drops the frame.",
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
                        ports,
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
            .map(|(i, c)| PingSession {
                id: i as u64,
                source: c.source,
                destination: c.destination,
                count: c.count,
                started: c.at,
                replies: BTreeMap::new(),
                failures: BTreeMap::new(),
                sent_at: BTreeMap::new(),
                waited_for_arp: Default::default(),
            })
            .collect();
        for (i, c) in self.commands.iter().enumerate() {
            for seq in 1..=c.count {
                let at = c.at + PING_INTERVAL * u32::from(seq - 1);
                let _ = self.scheduler.schedule_at(
                    at,
                    Event::PingSend {
                        session: i as u64,
                        seq,
                    },
                );
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
                match self.nodes.get(&to).map(|n| &n.role) {
                    Some(Role::Host(_)) => self.host_receive(now, to, arrival, frame),
                    Some(Role::Switch(_)) => self.switch_receive(now, to, arrival, frame),
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
            Event::PingSend { session, seq } => self.ping_send(now, session, seq),
            Event::PingTimeout { session, seq } => self.ping_timeout(now, session, seq),
            Event::ArpTimeout { host, ip } => self.arp_timeout(now, host, ip),
        }
    }

    // MARK: Hosts

    fn ping_send(&mut self, now: SimTime, session: u64, seq: u16) {
        let Some(s) = self.session(session) else {
            return;
        };
        let (source, destination) = (s.source, s.destination);
        let name = self.name(source);
        let Some(host) = self.host(source).cloned() else {
            return;
        };

        let Some(ip) = host.ip else {
            return self.fail_ping(
                now,
                session,
                seq,
                source,
                format!("{name} can't send the ping: it has no IP address yet. Set one in its IP configuration."),
            );
        };
        let cabled = host
            .port
            .as_ref()
            .is_some_and(|p| self.wires.contains_key(&(source, p.clone())));
        if !cabled {
            let port = host
                .port
                .as_deref()
                .map(abbreviate_port)
                .unwrap_or_default();
            return self.fail_ping(
                now,
                session,
                seq,
                source,
                format!("{name} can't send the ping: nothing is plugged into {port}."),
            );
        }
        if destination == ip.address {
            self.sessions[session as usize]
                .replies
                .insert(seq, (Duration::ZERO, HOST_TTL));
            self.record(
                now,
                source,
                None,
                None,
                Outcome::Completed,
                format!(
                    "{name} pings its own address {destination}. The reply never leaves the PC."
                ),
                Vec::new(),
                Vec::new(),
            );
            return;
        }
        let next_hop = if ip.contains(destination) {
            destination
        } else if let Some(gateway) = ip.gateway {
            gateway
        } else {
            return self.fail_ping(
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
        };

        let packet = Ipv4Packet {
            src: ip.address,
            dst: destination,
            ttl: HOST_TTL,
            icmp: Icmp {
                kind: IcmpKind::EchoRequest,
                id: session as u16 + 1,
                seq,
            },
        };
        self.sessions[session as usize].sent_at.insert(seq, now);
        self.scheduler
            .schedule_in(PING_TIMEOUT, Event::PingTimeout { session, seq });
        self.send_ip(now, source, packet, next_hop, Some(session), true);
    }

    /// Sends an IP packet from a host towards `next_hop`, using ARP first if
    /// the MAC address isn't cached. With `record_send` false the caller has
    /// already explained the send (e.g. a reply) and only ARP is traced.
    fn send_ip(
        &mut self,
        now: SimTime,
        id: DeviceId,
        packet: Ipv4Packet,
        next_hop: Ipv4Addr,
        session: Option<u64>,
        record_send: bool,
    ) {
        let name = self.name(id);
        let Some(host) = self.host(id).cloned() else {
            return;
        };
        let Some(port) = host.port.clone() else {
            return;
        };
        let what = match packet.icmp.kind {
            IcmpKind::EchoRequest => format!("ping {} (an ICMP echo request)", packet.icmp.seq),
            IcmpKind::EchoReply => format!("the reply to ping {}", packet.icmp.seq),
        };
        let via_gateway = next_hop != packet.dst;

        if let Some(&dst_mac) = host.arp.get(&next_hop) {
            let frame = Frame {
                src: host.mac,
                dst: dst_mac,
                payload: Payload::Ipv4(packet),
            };
            self.transmit(id, &port, frame, session);
            if record_send {
                let route = if via_gateway {
                    format!(
                        " {} is on another network, so the frame goes to the default gateway {next_hop}.",
                        packet.dst
                    )
                } else {
                    String::new()
                };
                let layers = vec![
                    explain::physical(format!("Sent out {}.", abbreviate_port(&port))),
                    explain::data_link(
                        LayerRole::Changed,
                        format!(
                            "Addressed to {next_hop}'s MAC {dst_mac}, already in {name}'s ARP cache."
                        ),
                        &frame,
                    ),
                    explain::ipv4(
                        LayerRole::Changed,
                        format!(
                            "From {} to {}. The TTL starts at {HOST_TTL}.",
                            packet.src, packet.dst
                        ),
                        &packet,
                    ),
                    explain::icmp(
                        LayerRole::Changed,
                        format!("Echo request number {}.", packet.icmp.seq),
                        &packet.icmp,
                    ),
                ];
                self.record(
                    now,
                    id,
                    None,
                    Some(frame),
                    Outcome::Created,
                    format!("{name} sends {what} to {}.{route}", packet.dst),
                    layers,
                    vec![port],
                );
                self.set_session(session);
            }
            return;
        }

        let already_asking = host.waiting.iter().any(|w| w.next_hop == next_hop);
        if let Some(Role::Host(h)) = self.nodes.get_mut(&id).map(|n| &mut n.role) {
            h.waiting.push(Waiting {
                next_hop,
                packet,
                session,
            });
        }
        if let Some(s) = session.and_then(|s| self.sessions.get_mut(s as usize))
            && packet.icmp.kind == IcmpKind::EchoRequest
        {
            s.waited_for_arp.insert(packet.icmp.seq);
        }
        if already_asking {
            return;
        }
        let arp = ArpPacket {
            op: ArpOp::Request,
            sender_mac: host.mac,
            sender_ip: packet.src,
            target_mac: MacAddr::ZERO,
            target_ip: next_hop,
        };
        let frame = Frame {
            src: host.mac,
            dst: MacAddr::BROADCAST,
            payload: Payload::Arp(arp),
        };
        self.transmit(id, &port, frame, session);
        self.scheduler.schedule_in(
            ARP_TIMEOUT,
            Event::ArpTimeout {
                host: id,
                ip: next_hop,
            },
        );
        let whose = if via_gateway {
            format!("its default gateway {next_hop}")
        } else {
            next_hop.to_string()
        };
        let layers = vec![
            explain::physical(format!("Sent out {}.", abbreviate_port(&port))),
            explain::data_link(
                LayerRole::Changed,
                "Sent to the broadcast address ff:ff:ff:ff:ff:ff, so every device on this network receives it.",
                &frame,
            ),
            explain::arp(
                LayerRole::Changed,
                format!(
                    "Asks \"who has {next_hop}?\" and includes {name}'s own IP and MAC so the owner can answer."
                ),
                &arp,
            ),
            explain::not_used(
                Layer::Message,
                format!("The ping waits inside {name} until ARP gets an answer."),
            ),
        ];
        self.record(
            now,
            id,
            None,
            Some(frame),
            Outcome::Created,
            format!(
                "{name} wants to send {what} to {}, but doesn't know the MAC address of {whose}. It holds the \
                 ping and first broadcasts an ARP request: \"Who has {next_hop}?\"",
                packet.dst
            ),
            layers,
            vec![port],
        );
        self.set_session(session);
    }

    fn host_receive(&mut self, now: SimTime, id: DeviceId, arrival: Arrival, frame: Frame) {
        let name = self.name(id);
        let Some(host) = self.host(id).cloned() else {
            return;
        };
        let port = abbreviate_port(&arrival.port);
        let arrived = explain::physical(format!("Arrived on {port}."));

        if frame.dst != host.mac && !frame.dst.is_broadcast() {
            let layers = vec![
                arrived,
                explain::data_link(
                    LayerRole::Used,
                    format!(
                        "The destination MAC isn't {name}'s ({}), so the network card discards it.",
                        host.mac
                    ),
                    &frame,
                ),
                explain::not_used(
                    Layer::Network,
                    "Never reached: the frame was discarded at layer 2.",
                ),
            ];
            let summary = format!(
                "{name} receives the frame, but it's addressed to {}, not to {name}, so {name} ignores it.",
                self.who(frame.dst)
            );
            self.record(
                now,
                id,
                Some(&arrival),
                Some(frame),
                Outcome::Ignored,
                summary,
                layers,
                Vec::new(),
            );
            return;
        }
        let accepted = if frame.dst.is_broadcast() {
            "It's a broadcast, so every device reads it.".to_string()
        } else {
            format!("Addressed to {name}'s own MAC, so {name} accepts it.")
        };

        match frame.payload {
            Payload::Arp(arp) if arp.op == ArpOp::Request => {
                let mine = host.ip.is_some_and(|ip| ip.address == arp.target_ip);
                if !mine {
                    let why = match host.ip {
                        Some(ip) => format!(
                            "it asks for {}, not {name}'s address {}",
                            arp.target_ip, ip.address
                        ),
                        None => {
                            format!("it asks for {} and {name} has no IP address", arp.target_ip)
                        }
                    };
                    let layers = vec![
                        arrived,
                        explain::data_link(LayerRole::Used, accepted, &frame),
                        explain::arp(
                            LayerRole::Used,
                            format!("Target IP {} isn't {name}'s.", arp.target_ip),
                            &arp,
                        ),
                    ];
                    let summary = format!(
                        "{name} receives the ARP request, but {why}, so {name} ignores it."
                    );
                    self.record(
                        now,
                        id,
                        Some(&arrival),
                        Some(frame),
                        Outcome::Ignored,
                        summary,
                        layers,
                        Vec::new(),
                    );
                    return;
                }
                let ip = host.ip.expect("checked above");
                self.learn_arp(id, arp.sender_ip, arp.sender_mac);
                let reply = ArpPacket {
                    op: ArpOp::Reply,
                    sender_mac: host.mac,
                    sender_ip: ip.address,
                    target_mac: arp.sender_mac,
                    target_ip: arp.sender_ip,
                };
                let out = Frame {
                    src: host.mac,
                    dst: arp.sender_mac,
                    payload: Payload::Arp(reply),
                };
                let egress = arrival.port.clone();
                self.transmit(id, &egress, out, arrival.tag);
                let layers = vec![
                    explain::physical(format!(
                        "Arrived on {port}; the reply goes back out the same port."
                    )),
                    explain::data_link(LayerRole::Used, accepted, &frame),
                    explain::arp(
                        LayerRole::Used,
                        format!(
                            "{} is {name}'s address, so it answers, and remembers {} is at {}.",
                            arp.target_ip, arp.sender_ip, arp.sender_mac
                        ),
                        &arp,
                    ),
                ];
                let summary = format!(
                    "{name} owns {}. It notes that {} is at {}, then replies directly to that MAC with its own \
                     address: \"{} is at {}.\"",
                    arp.target_ip, arp.sender_ip, arp.sender_mac, ip.address, host.mac
                );
                self.record(
                    now,
                    id,
                    Some(&arrival),
                    Some(frame),
                    Outcome::Replied,
                    summary,
                    layers,
                    vec![egress],
                );
                self.flush_waiting(now, id, arp.sender_ip);
            }
            Payload::Arp(arp) => {
                self.learn_arp(id, arp.sender_ip, arp.sender_mac);
                let waiting = host
                    .waiting
                    .iter()
                    .filter(|w| w.next_hop == arp.sender_ip)
                    .count();
                let then = match waiting {
                    0 => String::new(),
                    1 => " Now it can send the packet it was holding.".into(),
                    n => format!(" Now it can send the {n} packets it was holding."),
                };
                let layers = vec![
                    arrived,
                    explain::data_link(LayerRole::Used, accepted, &frame),
                    explain::arp(
                        LayerRole::Used,
                        format!(
                            "Answer: {} is at {}. Saved in the ARP cache.",
                            arp.sender_ip, arp.sender_mac
                        ),
                        &arp,
                    ),
                ];
                let summary = format!(
                    "{name} learns that {} is at {} and saves it in its ARP cache.{then}",
                    arp.sender_ip, arp.sender_mac
                );
                self.record(
                    now,
                    id,
                    Some(&arrival),
                    Some(frame),
                    Outcome::Accepted,
                    summary,
                    layers,
                    Vec::new(),
                );
                self.flush_waiting(now, id, arp.sender_ip);
            }
            Payload::Ipv4(packet) => {
                let session = session_of(&packet);
                let for_me = host.ip.is_some_and(|ip| ip.address == packet.dst);
                if !for_me {
                    let layers = vec![
                        arrived,
                        explain::data_link(LayerRole::Used, accepted, &frame),
                        explain::ipv4(
                            LayerRole::Used,
                            format!(
                                "Destination {} isn't {name}'s address, and hosts don't forward packets.",
                                packet.dst
                            ),
                            &packet,
                        ),
                    ];
                    let summary = format!(
                        "{name} receives a packet for {}, which isn't its address, so it discards it.",
                        packet.dst
                    );
                    self.record(
                        now,
                        id,
                        Some(&arrival),
                        Some(frame),
                        Outcome::Ignored,
                        summary,
                        layers,
                        Vec::new(),
                    );
                    return;
                }
                let ip = host.ip.expect("checked above");
                match packet.icmp.kind {
                    IcmpKind::EchoRequest => {
                        let reply = Ipv4Packet {
                            src: ip.address,
                            dst: packet.src,
                            ttl: HOST_TTL,
                            icmp: Icmp {
                                kind: IcmpKind::EchoReply,
                                id: packet.icmp.id,
                                seq: packet.icmp.seq,
                            },
                        };
                        let next_hop = if ip.contains(packet.src) {
                            Some(packet.src)
                        } else {
                            ip.gateway
                        };
                        let layers = vec![
                            explain::physical(format!(
                                "Arrived on {port}; the reply goes back out the same port."
                            )),
                            explain::data_link(LayerRole::Used, accepted, &frame),
                            explain::ipv4(
                                LayerRole::Used,
                                format!(
                                    "Destination {} is {name}'s own address, so the packet is for {name}.",
                                    packet.dst
                                ),
                                &packet,
                            ),
                            explain::icmp(
                                LayerRole::Used,
                                "An echo request, so it answers with an echo reply.",
                                &packet.icmp,
                            ),
                        ];
                        let summary = match next_hop {
                            Some(_) => format!(
                                "{name} receives ping {} from {} and answers with an ICMP echo reply.",
                                packet.icmp.seq, packet.src
                            ),
                            None => format!(
                                "{name} receives ping {} from {}, but can't answer: {} is on another network and \
                                 {name} has no default gateway.",
                                packet.icmp.seq, packet.src, packet.src
                            ),
                        };
                        let outcome = if next_hop.is_some() {
                            Outcome::Replied
                        } else {
                            Outcome::Dropped
                        };
                        let egress = if next_hop.is_some() {
                            vec![arrival.port.clone()]
                        } else {
                            Vec::new()
                        };
                        self.record(
                            now,
                            id,
                            Some(&arrival),
                            Some(frame),
                            outcome,
                            summary,
                            layers,
                            egress,
                        );
                        self.set_session(session);
                        if let Some(next_hop) = next_hop {
                            self.send_ip(now, id, reply, next_hop, session, false);
                        }
                    }
                    IcmpKind::EchoReply => {
                        let seq = packet.icmp.seq;
                        let mut late = false;
                        let mut rtt = Duration::ZERO;
                        let mut waited = false;
                        if let Some(s) = session.and_then(|s| self.sessions.get_mut(s as usize)) {
                            waited = s.waited_for_arp.contains(&seq);
                            if s.failures.contains_key(&seq) {
                                late = true;
                            } else if let Some(sent) = s.sent_at.get(&seq) {
                                rtt = now - *sent;
                                s.replies.insert(seq, (rtt, packet.ttl));
                            }
                        }
                        let layers = vec![
                            arrived,
                            explain::data_link(LayerRole::Used, accepted, &frame),
                            explain::ipv4(
                                LayerRole::Used,
                                format!("Addressed to {name}, from {}.", packet.src),
                                &packet,
                            ),
                            explain::icmp(
                                LayerRole::Used,
                                "The echo reply that answers the ping.",
                                &packet.icmp,
                            ),
                        ];
                        let summary = if late {
                            format!(
                                "The reply to ping {seq} reaches {name}, but too late: it had already timed out."
                            )
                        } else {
                            format!(
                                "{name} receives the reply to ping {seq}: {} is reachable. Round trip: {} ms{}.",
                                packet.src,
                                rtt.as_millis(),
                                if waited {
                                    ", including the time spent on ARP first"
                                } else {
                                    ""
                                }
                            )
                        };
                        self.record(
                            now,
                            id,
                            Some(&arrival),
                            Some(frame),
                            Outcome::Completed,
                            summary,
                            layers,
                            Vec::new(),
                        );
                        self.set_session(session);
                    }
                }
            }
        }
    }

    fn learn_arp(&mut self, id: DeviceId, ip: Ipv4Addr, mac: MacAddr) {
        if let Some(Role::Host(h)) = self.nodes.get_mut(&id).map(|n| &mut n.role) {
            h.arp.insert(ip, mac);
        }
    }

    /// Sends every packet that was waiting for `ip` to resolve.
    fn flush_waiting(&mut self, now: SimTime, id: DeviceId, ip: Ipv4Addr) {
        let ready: Vec<Waiting> = match self.nodes.get_mut(&id).map(|n| &mut n.role) {
            Some(Role::Host(h)) => {
                let (ready, rest) = h.waiting.drain(..).partition(|w| w.next_hop == ip);
                h.waiting = rest;
                ready
            }
            _ => return,
        };
        for w in ready {
            self.send_ip(now, id, w.packet, w.next_hop, w.session, true);
        }
    }

    fn ping_timeout(&mut self, now: SimTime, session: u64, seq: u16) {
        let Some(s) = self.sessions.get_mut(session as usize) else {
            return;
        };
        if s.settled(seq) {
            return;
        }
        s.failures.insert(seq, PingFailure::TimedOut);
        let (source, destination) = (s.source, s.destination);
        let name = self.name(source);
        self.record(
            now,
            source,
            None,
            None,
            Outcome::Completed,
            format!("Ping {seq} from {name} to {destination} timed out: no reply came back within 2 seconds."),
            Vec::new(),
            Vec::new(),
        );
        self.set_session(Some(session));
    }

    fn arp_timeout(&mut self, now: SimTime, id: DeviceId, ip: Ipv4Addr) {
        let stranded: Vec<Waiting> = match self.nodes.get_mut(&id).map(|n| &mut n.role) {
            Some(Role::Host(h)) if !h.arp.contains_key(&ip) => {
                let (stranded, rest) = h.waiting.drain(..).partition(|w| w.next_hop == ip);
                h.waiting = rest;
                stranded
            }
            _ => return,
        };
        if stranded.is_empty() {
            return;
        }
        let name = self.name(id);
        let mut session_id = None;
        for w in &stranded {
            if let Some(s) = w.session.and_then(|s| self.sessions.get_mut(s as usize)) {
                s.failures
                    .entry(w.packet.icmp.seq)
                    .or_insert_with(|| PingFailure::Unreachable("no ARP reply".into()));
                session_id = Some(s.id);
            }
        }
        self.record(
            now,
            id,
            None,
            None,
            Outcome::Dropped,
            format!(
                "Nobody answered {name}'s ARP request for {ip}, so {name} can't deliver the ping. Check that {ip} \
                 is the right address, and that a device with it is cabled to this network."
            ),
            Vec::new(),
            Vec::new(),
        );
        self.set_session(session_id);
    }

    fn fail_ping(
        &mut self,
        now: SimTime,
        session: u64,
        seq: u16,
        source: DeviceId,
        reason: String,
    ) {
        if let Some(s) = self.sessions.get_mut(session as usize) {
            s.failures
                .insert(seq, PingFailure::Unreachable(reason.clone()));
        }
        self.record(
            now,
            source,
            None,
            None,
            Outcome::Dropped,
            reason,
            Vec::new(),
            Vec::new(),
        );
        self.set_session(Some(session));
    }

    // MARK: Switches and hubs

    fn switch_receive(&mut self, now: SimTime, id: DeviceId, arrival: Arrival, frame: Frame) {
        let name = self.name(id);
        let ingress = arrival.port.clone();
        let wired = self.wired_ports(id);
        let src = self.who(frame.src);
        let dst = self.who(frame.dst);
        let Some(Role::Switch(table)) = self.nodes.get_mut(&id).map(|n| &mut n.role) else {
            return;
        };

        let previous = table.insert(frame.src, ingress.clone());
        let known = if frame.dst.is_broadcast() {
            None
        } else {
            table.get(&frame.dst).cloned()
        };
        let p = abbreviate_port(&ingress);
        let learned = match previous {
            None => format!("{name} learns that {src} is on {p}"),
            Some(old) if old == ingress => format!("{name} confirms {src} is still on {p}"),
            Some(old) => format!(
                "{name} sees {src} has moved from {} to {p}",
                abbreviate_port(&old)
            ),
        };

        let (outcome, egress, decision) = match known {
            Some(out) if out == ingress => (
                Outcome::Dropped,
                Vec::new(),
                format!(
                    "{dst} is on the same port the frame came in on, so there's nothing to forward and {name} drops it."
                ),
            ),
            Some(out) => {
                let o = abbreviate_port(&out);
                (
                    Outcome::Forwarded,
                    vec![out],
                    format!(
                        "It already knows {dst} is on {o}, so it forwards the frame only out {o}."
                    ),
                )
            }
            None => {
                let outs: Vec<String> = wired.into_iter().filter(|p| *p != ingress).collect();
                let reason = if frame.dst.is_broadcast() {
                    "The frame is a broadcast".to_string()
                } else {
                    format!("It doesn't know where {dst} is yet")
                };
                let decision = if outs.is_empty() {
                    format!(
                        "{reason}, but it has no other connected ports, so the frame goes nowhere."
                    )
                } else {
                    format!(
                        "{reason}, so it floods the frame out every other connected port: {}.",
                        list(&outs)
                    )
                };
                (Outcome::Flooded, outs, decision)
            }
        };
        for port in &egress {
            self.transmit(id, port, frame, arrival.tag);
        }
        let mut layers = vec![
            explain::physical(if egress.is_empty() {
                format!("Arrived on {p}.")
            } else {
                format!("Arrived on {p}; sent out {}.", list(&egress))
            }),
            explain::data_link(
                LayerRole::Used,
                "Reads the source MAC to learn where it is, and the destination MAC to decide where to send the \
                 frame. The frame itself isn't changed.",
                &frame,
            ),
        ];
        layers.extend(explain::upper_not_used(&frame, "A switch"));
        let summary = format!("{learned}. {decision}");
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            outcome,
            summary,
            layers,
            egress,
        );
    }

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

    fn transmit(&mut self, from: DeviceId, port: &str, frame: Frame, tag: Option<u64>) -> bool {
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
                link,
                sent,
                tag,
            },
        );
        true
    }

    /// Ports with a cable attached, in panel order.
    fn wired_ports(&self, id: DeviceId) -> Vec<String> {
        self.nodes
            .get(&id)
            .map(|n| {
                n.ports
                    .iter()
                    .filter(|p| self.wires.contains_key(&(id, (*p).clone())))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn host(&self, id: DeviceId) -> Option<&Host> {
        match &self.nodes.get(&id)?.role {
            Role::Host(h) => Some(h),
            _ => None,
        }
    }

    pub(crate) fn host_view(&self, id: DeviceId) -> Option<HostView<'_>> {
        self.host(id).map(|h| HostView {
            port: h.port.as_deref(),
            mac: h.mac,
            ip: h.ip,
            arp: &h.arp,
        })
    }

    /// "PC1 (02:4e:00:00:02:00)", or the bare address if nobody owns it.
    fn who(&self, mac: MacAddr) -> String {
        if mac.is_broadcast() {
            return "the broadcast address".into();
        }
        match self.owners.get(&mac) {
            Some(id) => format!("{} ({mac})", self.name(*id)),
            None => mac.to_string(),
        }
    }

    pub(crate) fn name(&self, id: DeviceId) -> String {
        self.nodes
            .get(&id)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| format!("device {id}"))
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
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
                Payload::Arp(_) => None,
            })
            .or(arrival.and_then(|a| a.tag));
        self.trace.push(TraceEvent {
            id: self.trace.len() as u64,
            time: now,
            sent: arrival.map(|a| a.sent),
            device,
            from: arrival.map(|a| a.from),
            link: arrival.map(|a| a.link),
            ingress: arrival.map(|a| a.port.clone()),
            egress,
            frame,
            outcome,
            summary,
            layers,
            session,
        });
    }

    /// Tags the most recent event with a session (ARP and failure events
    /// carry no ICMP id of their own).
    fn set_session(&mut self, session: Option<u64>) {
        if let (Some(s), Some(last)) = (session, self.trace.last_mut()) {
            last.session.get_or_insert(s);
        }
    }
}

/// The ping session an ICMP packet belongs to (its identifier is session + 1).
fn session_of(packet: &Ipv4Packet) -> Option<u64> {
    u64::from(packet.icmp.id).checked_sub(1)
}

/// "Fa0/1, Fa0/2 and Gi0/1".
fn list(ports: &[String]) -> String {
    let short: Vec<String> = ports.iter().map(|p| abbreviate_port(p)).collect();
    match short.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
