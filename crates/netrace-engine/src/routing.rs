//! What a device with an IP stack does with frames: answer ARP, accept
//! packets for itself, forward (routers), and send packets of its own.

use std::net::Ipv4Addr;

use netrace_doc::DeviceId;
use netrace_sim::SimTime;

use crate::explain;
use crate::network::{ARP_TIMEOUT, Arrival, Event, session_of};
use crate::probe::ProbeResult;
use crate::stack::{Iface, Route, RouteSource, Stack, StackKind, Waiting};
use crate::{
    ArpOp, ArpPacket, Frame, Icmp, IcmpKind, Ipv4Packet, Layer, LayerRole, MacAddr, Network,
    Outcome, Payload, Unreachable, abbreviate_port,
};

impl Network {
    pub(crate) fn stack_receive(
        &mut self,
        now: SimTime,
        id: DeviceId,
        arrival: Arrival,
        frame: Frame,
    ) {
        let name = self.name(id);
        let Some(stack) = self.stack(id).cloned() else {
            return;
        };
        let Some(iface) = stack.iface_for_frame(&arrival.port, frame.vlan).cloned() else {
            let reason = match frame.vlan {
                Some(vlan) => format!(
                    "the frame is tagged for VLAN {vlan} and {name} has no subinterface for VLAN {vlan} on {} \
                     (encapsulation dot1Q {vlan}), so it's dropped.",
                    abbreviate_port(&arrival.port)
                ),
                None => format!(
                    "the frame is untagged and {} only has tagged subinterfaces, so it's dropped.",
                    abbreviate_port(&arrival.port)
                ),
            };
            let layers = vec![
                explain::physical(format!("Arrived on {}.", abbreviate_port(&arrival.port))),
                explain::data_link(
                    LayerRole::Used,
                    "No interface takes frames with this tag.",
                    &frame,
                ),
            ];
            self.record(
                now,
                id,
                Some(&arrival),
                Some(frame),
                Outcome::Dropped,
                format!("{name} receives the frame, but {reason}"),
                layers,
                Vec::new(),
            );
            return;
        };
        let port = abbreviate_port(&arrival.port);
        let arrived = explain::physical(format!("Arrived on {port}."));

        if frame.dst != iface.mac && !frame.dst.is_broadcast() {
            let layers = vec![
                arrived,
                explain::data_link(
                    LayerRole::Used,
                    format!(
                        "The destination MAC isn't {name}'s ({}), so the network card discards it.",
                        iface.mac
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
                self.arp_request_received(now, id, &stack, &iface, arrival, frame, arp, accepted)
            }
            Payload::Arp(arp) => {
                let waiting = stack
                    .waiting
                    .iter()
                    .filter(|w| w.next_hop == arp.sender_ip)
                    .count();
                if let Some(s) = self.stack_mut(id) {
                    s.learn(arp.sender_ip, arp.sender_mac, &iface.name, now);
                }
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
                if let Some(own) = stack.owner_of(packet.dst) {
                    let own = own.clone();
                    self.deliver_locally(now, id, &stack, &own, arrival, frame, packet, accepted);
                } else if stack.forwarding {
                    self.forward(now, id, &stack, &iface, arrival, frame, packet);
                } else {
                    let what = match stack.kind {
                        StackKind::Host => "hosts don't forward packets",
                        _ => "it doesn't route (that needs `ip routing`)",
                    };
                    let layers = vec![
                        arrived,
                        explain::data_link(LayerRole::Used, accepted, &frame),
                        explain::ipv4(
                            LayerRole::Used,
                            format!(
                                "Destination {} isn't {name}'s address, and {what}.",
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
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn arp_request_received(
        &mut self,
        now: SimTime,
        id: DeviceId,
        stack: &Stack,
        iface: &Iface,
        arrival: Arrival,
        frame: Frame,
        arp: ArpPacket,
        accepted: String,
    ) {
        let name = self.name(id);
        let port = abbreviate_port(&arrival.port);
        let mine = iface.address.is_some_and(|a| a.address == arp.target_ip);
        if !mine {
            let why = match (stack.kind, iface.address) {
                (StackKind::Host, Some(a)) => format!(
                    "it asks for {}, not {name}'s address {}",
                    arp.target_ip, a.address
                ),
                (StackKind::Host, None) => {
                    format!("it asks for {} and {name} has no IP address", arp.target_ip)
                }
                (_, Some(a)) => format!(
                    "it asks for {}, and {name}'s {} is {}",
                    arp.target_ip,
                    abbreviate_port(&iface.name),
                    a.address
                ),
                (_, None) => format!(
                    "it asks for {}, and {name}'s {} has no IP address (set one with `ip address`)",
                    arp.target_ip,
                    abbreviate_port(&iface.name)
                ),
            };
            let target_note = match stack.kind {
                StackKind::Host => format!("Target IP {} isn't {name}'s.", arp.target_ip),
                _ => format!(
                    "Target IP {} isn't this interface's address.",
                    arp.target_ip
                ),
            };
            let layers = vec![
                explain::physical(format!("Arrived on {port}.")),
                explain::data_link(LayerRole::Used, accepted, &frame),
                explain::arp(LayerRole::Used, target_note, &arp),
            ];
            let summary =
                format!("{name} receives the ARP request, but {why}, so {name} ignores it.");
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
        let addr = iface.address.expect("checked above");
        if let Some(s) = self.stack_mut(id) {
            s.learn(arp.sender_ip, arp.sender_mac, &iface.name, now);
        }
        let reply = ArpPacket {
            op: ArpOp::Reply,
            sender_mac: iface.mac,
            sender_ip: addr.address,
            target_mac: arp.sender_mac,
            target_ip: arp.sender_ip,
        };
        let out = Frame {
            vlan: None,
            src: iface.mac,
            dst: arp.sender_mac,
            payload: Payload::Arp(reply),
        };
        let egress = self.stack_transmit(id, iface, out, arrival.tag);
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
            arp.target_ip, arp.sender_ip, arp.sender_mac, addr.address, iface.mac
        );
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            Outcome::Replied,
            summary,
            layers,
            egress,
        );
        self.flush_waiting(now, id, arp.sender_ip);
    }

    #[allow(clippy::too_many_arguments)]
    fn deliver_locally(
        &mut self,
        now: SimTime,
        id: DeviceId,
        stack: &Stack,
        own: &Iface,
        arrival: Arrival,
        frame: Frame,
        packet: Ipv4Packet,
        accepted: String,
    ) {
        let name = self.name(id);
        let port = abbreviate_port(&arrival.port);
        let session = session_of(&packet);
        let arrived = explain::physical(format!("Arrived on {port}."));
        let whose = if stack.kind == StackKind::Host
            || own.port.as_deref() == Some(arrival.port.as_str())
        {
            format!("{name}'s own address")
        } else {
            format!("the address of {name}'s {}", abbreviate_port(&own.name))
        };
        match packet.icmp.kind {
            IcmpKind::EchoRequest => {
                let reply = Ipv4Packet {
                    src: packet.dst,
                    dst: packet.src,
                    ttl: stack.ttl,
                    icmp: Icmp {
                        kind: IcmpKind::EchoReply,
                        id: packet.icmp.id,
                        seq: packet.icmp.seq,
                    },
                };
                let route = stack.resolve(packet.src).map(|(r, ..)| r.clone());
                let layers = vec![
                    explain::physical(format!(
                        "Arrived on {port}; the reply goes back out the same port."
                    )),
                    explain::data_link(LayerRole::Used, accepted, &frame),
                    explain::ipv4(
                        LayerRole::Used,
                        format!(
                            "Destination {} is {whose}, so the packet is for {name}.",
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
                let summary = match (&route, stack.kind) {
                    (Some(_), _) => format!(
                        "{name} receives ping {} from {} and answers with an ICMP echo reply.",
                        packet.icmp.seq, packet.src
                    ),
                    (None, StackKind::Host) => format!(
                        "{name} receives ping {} from {}, but can't answer: {} is on another network and \
                         {name} has no default gateway.",
                        packet.icmp.seq, packet.src, packet.src
                    ),
                    (None, _) => format!(
                        "{name} receives ping {} from {}, but can't answer: it has no route back to {}.",
                        packet.icmp.seq, packet.src, packet.src
                    ),
                };
                let (outcome, egress) = match route {
                    Some(_) => (Outcome::Replied, vec![arrival.port.clone()]),
                    None => (Outcome::Dropped, Vec::new()),
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
                if route.is_some() {
                    self.send_from(now, id, reply, session, false);
                }
            }
            IcmpKind::EchoReply => {
                let seq = packet.icmp.seq;
                let mut late = false;
                let mut rtt = std::time::Duration::ZERO;
                let mut waited = false;
                if let Some(s) = session.and_then(|s| self.sessions.get(s as usize))
                    && let Some(probe) = s.probe(seq)
                {
                    waited = probe.waited_for_arp;
                    late = probe.result.is_some();
                    rtt = probe.sent.map(|sent| now - sent).unwrap_or_default();
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
                let traceroute = session
                    .and_then(|s| self.sessions.get(s as usize))
                    .is_some_and(|s| s.kind == crate::SessionKind::Traceroute);
                let summary = if late {
                    format!(
                        "The reply to ping {seq} reaches {name}, but too late: it had already timed out."
                    )
                } else if traceroute {
                    format!(
                        "{name} receives an echo reply from the destination {}: the traceroute has reached it \
                         ({} ms).",
                        packet.src,
                        rtt.as_millis()
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
                if let Some(s) = session {
                    self.settle(
                        now,
                        s,
                        seq,
                        ProbeResult::Reply {
                            rtt,
                            ttl: packet.ttl,
                            from: packet.src,
                        },
                    );
                }
            }
            IcmpKind::TimeExceeded | IcmpKind::Unreachable(_) => {
                let seq = packet.icmp.seq;
                let probe = session
                    .and_then(|s| self.sessions.get(s as usize))
                    .and_then(|s| s.probe(seq))
                    .cloned();
                let rtt = probe
                    .as_ref()
                    .and_then(|p| p.sent)
                    .map(|sent| now - sent)
                    .unwrap_or_default();
                let reporter = self.who_ip(packet.src);
                let (note, summary, result) = match packet.icmp.kind {
                    IcmpKind::TimeExceeded => {
                        let ttl = probe.map(|p| p.ttl).unwrap_or(0);
                        (
                            "Time exceeded: a router dropped the probe when its TTL ran out.",
                            format!(
                                "{name} receives ICMP Time Exceeded from {reporter}: the probe sent with TTL {ttl} \
                                 ran out there. That's how traceroute learns hop {ttl}."
                            ),
                            ProbeResult::TimeExceeded {
                                rtt,
                                from: packet.src,
                            },
                        )
                    }
                    IcmpKind::Unreachable(code) => (
                        "Destination unreachable: a router couldn't deliver the packet.",
                        format!(
                            "{name} receives ICMP Destination Unreachable from {reporter}: it has no way to \
                             reach {}.",
                            self.sessions
                                .get(session.unwrap_or(u64::MAX) as usize)
                                .map(|s| s.destination.to_string())
                                .unwrap_or_else(|| "the destination".into())
                        ),
                        ProbeResult::Unreachable {
                            rtt,
                            from: packet.src,
                            code,
                        },
                    ),
                    _ => unreachable!(),
                };
                let layers = vec![
                    arrived,
                    explain::data_link(LayerRole::Used, accepted, &frame),
                    explain::ipv4(
                        LayerRole::Used,
                        format!("Addressed to {name}, from {}.", packet.src),
                        &packet,
                    ),
                    explain::icmp(LayerRole::Used, note, &packet.icmp),
                ];
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
                if let Some(s) = session {
                    self.settle(now, s, seq, result);
                }
            }
        }
    }

    /// A router passing a packet on: TTL check, routing table lookup, new
    /// MAC addresses, out the exit interface.
    #[allow(clippy::too_many_arguments)]
    fn forward(
        &mut self,
        now: SimTime,
        id: DeviceId,
        stack: &Stack,
        in_iface: &Iface,
        arrival: Arrival,
        frame: Frame,
        packet: Ipv4Packet,
    ) {
        let name = self.name(id);
        let port = abbreviate_port(&arrival.port);
        let session = session_of(&packet);
        let accepted =
            format!("Addressed to {name}'s {port} MAC: a frame for the router to route.");

        if packet.ttl <= 1 {
            let layers = vec![
                explain::physical(format!("Arrived on {port}.")),
                explain::data_link(LayerRole::Used, accepted, &frame),
                explain::ipv4(
                    LayerRole::Used,
                    format!(
                        "TTL is {}: lowering it would reach 0, so the packet is discarded.",
                        packet.ttl
                    ),
                    &packet,
                ),
                explain::not_used(Layer::Message, "Not forwarded."),
            ];
            let summary = format!(
                "The packet for {} reaches {name} with TTL {}. Lowering it would reach 0, so {name} discards it \
                 and sends an ICMP Time Exceeded message back to {}. Traceroute uses exactly this to find each router.",
                packet.dst, packet.ttl, packet.src
            );
            self.record(
                now,
                id,
                Some(&arrival),
                Some(frame),
                Outcome::Dropped,
                summary,
                layers,
                Vec::new(),
            );
            self.set_session(session);
            self.send_error(now, id, stack, in_iface, packet, IcmpKind::TimeExceeded);
            return;
        }

        let Some((route, out_iface, next_hop)) = stack.resolve(packet.dst) else {
            let layers = vec![
                explain::physical(format!("Arrived on {port}.")),
                explain::data_link(LayerRole::Used, accepted, &frame),
                explain::ipv4(
                    LayerRole::Used,
                    format!(
                        "No route matches {} and there's no default route.",
                        packet.dst
                    ),
                    &packet,
                ),
                explain::not_used(Layer::Message, "Not forwarded."),
            ];
            let summary = format!(
                "{name} has no route to {} (nothing in its routing table matches, and no default route), so it \
                 discards the packet and tells {} with ICMP Destination Unreachable.",
                packet.dst, packet.src
            );
            self.record(
                now,
                id,
                Some(&arrival),
                Some(frame),
                Outcome::Dropped,
                summary,
                layers,
                Vec::new(),
            );
            self.set_session(session);
            self.send_error(
                now,
                id,
                stack,
                in_iface,
                packet,
                IcmpKind::Unreachable(Unreachable::Network),
            );
            return;
        };
        let (route, out_iface) = (route.clone(), out_iface.clone());
        let out = Ipv4Packet {
            ttl: packet.ttl - 1,
            ..packet
        };
        let because = describe_route(&route, packet.dst);
        let o = abbreviate_port(&out_iface.name);

        if let Some(entry) = stack.arp.get(&next_hop) {
            let new_frame = Frame {
                vlan: None,
                src: out_iface.mac,
                dst: entry.mac,
                payload: Payload::Ipv4(out),
            };
            let egress = self.stack_transmit(id, &out_iface, new_frame, arrival.tag);
            let layers = vec![
                explain::physical(format!("Arrived on {port}; sent out {o}.")),
                explain::data_link(
                    LayerRole::Changed,
                    format!(
                        "The old Ethernet header is removed. The new one goes from {name}'s {o} to {} ({}).",
                        self.who_ip(next_hop),
                        entry.mac
                    ),
                    &new_frame,
                ),
                explain::ipv4(
                    LayerRole::Changed,
                    format!(
                        "Read the destination {} to choose a route. Addresses stay the same; TTL lowered from {} to {}.",
                        packet.dst, packet.ttl, out.ttl
                    ),
                    &out,
                ),
                explain::not_used(Layer::Message, "Carried along untouched."),
            ];
            let summary = format!(
                "{name} routes the packet for {}: {because}. It lowers the TTL to {}, puts a new Ethernet header \
                 on it (from its {o} to {}), and sends it out {o}.",
                packet.dst,
                out.ttl,
                self.who_ip(next_hop)
            );
            self.record(
                now,
                id,
                Some(&arrival),
                Some(frame),
                Outcome::Forwarded,
                summary,
                layers,
                egress,
            );
            self.set_session(session);
            return;
        }

        // Unknown next hop MAC: routers ask with ARP and drop the packet meanwhile.
        let asked = self.arp_request(now, id, &out_iface, next_hop, session);
        let layers = vec![
            explain::physical(format!("Arrived on {port}.")),
            explain::data_link(LayerRole::Used, accepted, &frame),
            explain::ipv4(
                LayerRole::Used,
                format!("Route found ({because}), but {next_hop}'s MAC address isn't known yet."),
                &packet,
            ),
            explain::not_used(Layer::Message, "Dropped with the packet."),
        ];
        let summary = format!(
            "{name} would route the packet for {} out {o} ({because}), but doesn't know the MAC address of {} yet. \
             Like a real Cisco router it drops this packet and asks with an ARP broadcast; the next one will get \
             through. That's why the first ping across a router often times out.",
            packet.dst,
            self.who_ip(next_hop)
        );
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            Outcome::Dropped,
            summary,
            layers,
            asked,
        );
        self.set_session(session);
    }

    /// ICMP error back to the packet's source (never about another error).
    fn send_error(
        &mut self,
        now: SimTime,
        id: DeviceId,
        stack: &Stack,
        in_iface: &Iface,
        original: Ipv4Packet,
        kind: IcmpKind,
    ) {
        if original.icmp.kind.is_error() {
            return;
        }
        let Some(src) = in_iface.address.map(|a| a.address).or_else(|| {
            stack
                .resolve(original.src)
                .and_then(|(_, i, _)| i.address.map(|a| a.address))
        }) else {
            return;
        };
        if stack.resolve(original.src).is_none() {
            return;
        }
        let error = Ipv4Packet {
            src,
            dst: original.src,
            ttl: stack.ttl,
            icmp: Icmp {
                kind,
                id: original.icmp.id,
                seq: original.icmp.seq,
            },
        };
        self.send_from(now, id, error, session_of(&original), true);
    }

    /// Sends a packet this device created (a probe, a reply, an ICMP error),
    /// resolving the next hop's MAC first. With `announce` false the caller
    /// has already explained the send and only ARP is traced.
    pub(crate) fn send_from(
        &mut self,
        now: SimTime,
        id: DeviceId,
        packet: Ipv4Packet,
        session: Option<u64>,
        announce: bool,
    ) {
        let name = self.name(id);
        let Some(stack) = self.stack(id).cloned() else {
            return;
        };
        let Some((route, iface, next_hop)) = stack.resolve(packet.dst) else {
            return;
        };
        let (route, iface) = (route.clone(), iface.clone());
        let what = describe_packet(&packet);
        let o = abbreviate_port(iface.port.as_deref().unwrap_or(&iface.name));

        if let Some(entry) = stack.arp.get(&next_hop) {
            let frame = Frame {
                vlan: None,
                src: iface.mac,
                dst: entry.mac,
                payload: Payload::Ipv4(packet),
            };
            let egress = self.stack_transmit(id, &iface, frame, session);
            if announce {
                let (route_text, ttl_text) = match stack.kind {
                    StackKind::Host => (
                        if next_hop != packet.dst {
                            format!(
                                " {} is on another network, so the frame goes to the default gateway {next_hop}.",
                                packet.dst
                            )
                        } else {
                            String::new()
                        },
                        format!(
                            "From {} to {}. The TTL starts at {}.",
                            packet.src, packet.dst, packet.ttl
                        ),
                    ),
                    _ => (
                        format!(
                            " Its routing table says {}, so it leaves by {o}.",
                            describe_route(&route, packet.dst)
                        ),
                        format!("From {} to {}. TTL {}.", packet.src, packet.dst, packet.ttl),
                    ),
                };
                let layers = vec![
                    explain::physical(format!("Sent out {o}.")),
                    explain::data_link(
                        LayerRole::Changed,
                        format!(
                            "Addressed to {next_hop}'s MAC {}, already in {name}'s ARP cache.",
                            entry.mac
                        ),
                        &frame,
                    ),
                    explain::ipv4(LayerRole::Changed, ttl_text, &packet),
                    explain::icmp(LayerRole::Changed, icmp_note(&packet), &packet.icmp),
                ];
                self.record(
                    now,
                    id,
                    None,
                    Some(frame),
                    Outcome::Created,
                    format!("{name} sends {what} to {}.{route_text}", packet.dst),
                    layers,
                    egress,
                );
                self.set_session(session);
            }
            return;
        }

        let via_gateway = next_hop != packet.dst;
        let whose = if via_gateway && stack.kind == StackKind::Host {
            format!("its default gateway {next_hop}")
        } else if via_gateway {
            format!("the next hop {next_hop}")
        } else {
            next_hop.to_string()
        };

        if stack.kind != StackKind::Host {
            // Routers and switches don't hold packets while ARP resolves.
            let egress = self.arp_request(now, id, &iface, next_hop, session);
            let summary = format!(
                "{name} wants to send {what} to {}, but doesn't know the MAC address of {whose}. Like a real Cisco \
                 device it drops this packet and asks with an ARP broadcast, so the first ping from a router \
                 usually shows a '.'.",
                packet.dst
            );
            let layers = vec![
                explain::physical(format!("ARP request sent out {o}.")),
                explain::not_used(
                    Layer::Network,
                    format!("The {what} is dropped while ARP resolves {next_hop}."),
                ),
            ];
            self.record(
                now,
                id,
                None,
                None,
                Outcome::Dropped,
                summary,
                layers,
                egress,
            );
            self.set_session(session);
            return;
        }

        let already_asking = stack.waiting.iter().any(|w| w.next_hop == next_hop);
        if let Some(s) = self.stack_mut(id) {
            s.waiting.push(Waiting {
                next_hop,
                packet,
                session,
            });
        }
        if let Some(s) = session.and_then(|s| self.sessions.get_mut(s as usize))
            && packet.icmp.kind == IcmpKind::EchoRequest
            && let Some(probe) = s.probe_mut(packet.icmp.seq)
        {
            probe.waited_for_arp = true;
        }
        if already_asking {
            return;
        }
        let Some(sender_ip) = iface.address.map(|a| a.address) else {
            return;
        };
        let arp = ArpPacket {
            op: ArpOp::Request,
            sender_mac: iface.mac,
            sender_ip,
            target_mac: MacAddr::ZERO,
            target_ip: next_hop,
        };
        let frame = Frame {
            vlan: None,
            src: iface.mac,
            dst: MacAddr::BROADCAST,
            payload: Payload::Arp(arp),
        };
        let egress = self.stack_transmit(id, &iface, frame, session);
        self.scheduler.schedule_in(
            ARP_TIMEOUT,
            Event::ArpTimeout {
                device: id,
                ip: next_hop,
            },
        );
        let layers = vec![
            explain::physical(format!("Sent out {o}.")),
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
            egress,
        );
        self.set_session(session);
    }

    /// Broadcasts an ARP request for `next_hop` from a router or switch
    /// (unless one is already outstanding). Returns the ports it left by.
    fn arp_request(
        &mut self,
        _now: SimTime,
        id: DeviceId,
        iface: &Iface,
        next_hop: Ipv4Addr,
        session: Option<u64>,
    ) -> Vec<String> {
        let Some(stack) = self.stack_mut(id) else {
            return Vec::new();
        };
        if !stack.asking.insert(next_hop) {
            return Vec::new();
        }
        let Some(sender_ip) = iface.address.map(|a| a.address) else {
            return Vec::new();
        };
        let frame = Frame {
            vlan: None,
            src: iface.mac,
            dst: MacAddr::BROADCAST,
            payload: Payload::Arp(ArpPacket {
                op: ArpOp::Request,
                sender_mac: iface.mac,
                sender_ip,
                target_mac: MacAddr::ZERO,
                target_ip: next_hop,
            }),
        };
        self.scheduler.schedule_in(
            ARP_TIMEOUT,
            Event::ArpTimeout {
                device: id,
                ip: next_hop,
            },
        );
        self.stack_transmit(id, iface, frame, session)
    }

    /// Puts a frame on the wire from an interface: its own port, or through
    /// the switch for a `Vlan1` interface. Returns the ports it left by.
    fn stack_transmit(
        &mut self,
        id: DeviceId,
        iface: &Iface,
        frame: Frame,
        tag: Option<u64>,
    ) -> Vec<String> {
        match &iface.port {
            Some(port) => {
                // Subinterfaces tag their frames (except the native VLAN's).
                let frame = frame.tagged(if iface.native { None } else { iface.vlan });
                if self.transmit(id, port, frame, tag) {
                    vec![port.clone()]
                } else {
                    Vec::new()
                }
            }
            None => match iface.vlan {
                Some(vlan) if iface.name.starts_with("Vlan") => {
                    self.switch_from_self(id, vlan, frame.tagged(None), tag)
                }
                _ => Vec::new(),
            },
        }
    }

    /// Sends every packet that was waiting for `ip` to resolve.
    fn flush_waiting(&mut self, now: SimTime, id: DeviceId, ip: Ipv4Addr) {
        let ready: Vec<Waiting> = match self.stack_mut(id) {
            Some(s) => {
                let (ready, rest) = s.waiting.drain(..).partition(|w| w.next_hop == ip);
                s.waiting = rest;
                ready
            }
            None => return,
        };
        for w in ready {
            self.send_from(now, id, w.packet, w.session, true);
        }
    }

    pub(crate) fn arp_timeout(&mut self, now: SimTime, id: DeviceId, ip: Ipv4Addr) {
        let (stranded, was_asking) = match self.stack_mut(id) {
            Some(s) if !s.arp.contains_key(&ip) => {
                let (stranded, rest): (Vec<Waiting>, Vec<Waiting>) =
                    s.waiting.drain(..).partition(|w| w.next_hop == ip);
                s.waiting = rest;
                (stranded, s.asking.remove(&ip))
            }
            _ => return,
        };
        if stranded.is_empty() && !was_asking {
            return;
        }
        let name = self.name(id);
        let mut session_id = None;
        for w in &stranded {
            if let Some(s) = w.session {
                session_id = Some(s);
                self.settle(
                    now,
                    s,
                    w.packet.icmp.seq,
                    ProbeResult::Failed("no ARP reply".into()),
                );
            }
        }
        let what = if stranded.is_empty() {
            "the packet"
        } else {
            "the ping"
        };
        self.record(
            now,
            id,
            None,
            None,
            Outcome::Dropped,
            format!(
                "Nobody answered {name}'s ARP request for {ip}, so {name} can't deliver {what}. Check that {ip} \
                 is the right address, and that a device with it is cabled to this network."
            ),
            Vec::new(),
            Vec::new(),
        );
        self.set_session(session_id);
    }
}

/// "192.168.2.0/24 is directly connected on Gi0/1", in plain words.
pub(crate) fn describe_route(route: &Route, dst: Ipv4Addr) -> String {
    let iface = abbreviate_port(&route.iface);
    match (route.source, route.next_hop) {
        (RouteSource::Connected | RouteSource::Local, _) => {
            format!(
                "{}/{} is directly connected on {iface}",
                route.network, route.prefix_len
            )
        }
        (_, Some(nh)) if route.is_default() => {
            format!("{dst} matches no specific route, so the default route sends it to {nh}")
        }
        (_, Some(nh)) => format!(
            "a static route sends {}/{} to {nh} via {iface}",
            route.network, route.prefix_len
        ),
        (_, None) => format!(
            "a static route sends {}/{} straight out {iface}",
            route.network, route.prefix_len
        ),
    }
}

fn describe_packet(packet: &Ipv4Packet) -> String {
    match packet.icmp.kind {
        IcmpKind::EchoRequest => format!("ping {} (an ICMP echo request)", packet.icmp.seq),
        IcmpKind::EchoReply => format!("the reply to ping {}", packet.icmp.seq),
        IcmpKind::TimeExceeded => "an ICMP Time Exceeded message".into(),
        IcmpKind::Unreachable(_) => "an ICMP Destination Unreachable message".into(),
    }
}

fn icmp_note(packet: &Ipv4Packet) -> String {
    match packet.icmp.kind {
        IcmpKind::EchoRequest => format!("Echo request number {}.", packet.icmp.seq),
        IcmpKind::EchoReply => format!("Echo reply number {}.", packet.icmp.seq),
        IcmpKind::TimeExceeded => {
            "Time exceeded: tells the sender where its packet's TTL ran out.".into()
        }
        IcmpKind::Unreachable(_) => {
            "Destination unreachable: tells the sender there's no way through.".into()
        }
    }
}
