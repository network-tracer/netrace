//! Control-plane packets that build the converged state, sent once when
//! the simulation starts: spanning-tree BPDUs from every designated port
//! that faces another switch, then OSPF hellos on every OSPF interface,
//! then LS Updates between full neighbours. Spanning tree and OSPF are
//! worked out from the topology (see `stp` and `ospf`); these packets show
//! that work happening, and each receiver explains itself from the same
//! decisions, so what the trace says matches the computed roles, states
//! and routes. Periodic repeats aren't sent: they'd bury everything else.

use std::time::Duration;

use netrace_doc::DeviceId;
use netrace_sim::SimTime;

use crate::network::{Arrival, Event};
use crate::stack::{Iface, Stack};
use crate::{
    Bpdu, Frame, Ipv4Packet, L4, LayerNote, LayerRole, MacAddr, NeighborState, Network,
    OspfIfState, OspfMessage, OspfNetType, OspfPacket, Outcome, Payload, abbreviate_port, explain,
};

/// Hellos go out just after BPDUs; LS Updates once neighbours have met.
const HELLO_AT: Duration = Duration::from_millis(50);
const FLOOD_AT: Duration = Duration::from_millis(400);

impl Network {
    pub(crate) fn schedule_control_plane(&mut self) {
        if !self.l2.trees.is_empty() {
            let _ = self.scheduler.schedule_at(SimTime::ZERO, Event::SendBpdus);
        }
        if !self.ospf.routers.is_empty() {
            let _ = self
                .scheduler
                .schedule_at(SimTime::ZERO + HELLO_AT, Event::SendOspfHellos);
            let _ = self
                .scheduler
                .schedule_at(SimTime::ZERO + FLOOD_AT, Event::SendOspfUpdates);
        }
    }

    // MARK: Spanning tree

    /// Every designated port facing another switch sends a BPDU per VLAN.
    pub(crate) fn send_bpdus(&mut self) {
        let mut sends = Vec::new();
        for (vlan, tree) in &self.l2.trees {
            for ((device, port), p) in &tree.ports {
                if p.role != crate::PortRole::Designated {
                    continue;
                }
                let faces_switch = self
                    .wires
                    .get(&(*device, port.clone()))
                    .and_then(|(peer, ..)| self.project.device(*peer))
                    .is_some_and(|d| d.kind.is_switch() || d.kind == netrace_doc::DeviceKind::Hub);
                let Some(bridge) = tree.bridges.get(device) else {
                    continue;
                };
                if faces_switch {
                    sends.push((
                        *device,
                        port.clone(),
                        Bpdu {
                            vlan: *vlan,
                            root: tree.root,
                            root_cost: bridge.root_cost,
                            bridge: bridge.id,
                            port: p.number,
                        },
                    ));
                }
            }
        }
        for (device, port, bpdu) in sends {
            let index = self
                .nodes
                .get(&device)
                .and_then(|n| n.ports.iter().position(|x| *x == port))
                .unwrap_or(0);
            let frame = Frame {
                src: MacAddr::for_port(device, index),
                dst: MacAddr::STP_BRIDGES,
                vlan: self.trunk_tag(device, &port, bpdu.vlan),
                payload: Payload::Bpdu(bpdu),
            };
            self.transmit(device, &port, frame, None);
        }
    }

    /// PVST+ tags BPDUs on trunks except for the native VLAN.
    fn trunk_tag(&self, id: DeviceId, port: &str, vlan: u16) -> Option<u16> {
        crate::stp::switchport(&self.project, id, port)
            .filter(|sp| sp.is_trunk() && sp.native_vlan != vlan)
            .map(|_| vlan)
    }

    /// A switch hears a BPDU: it keeps it (BPDUs are never forwarded) and
    /// its port takes the role spanning tree worked out, for this reason.
    pub(crate) fn bpdu_receive(
        &mut self,
        now: SimTime,
        id: DeviceId,
        arrival: Arrival,
        frame: Frame,
        bpdu: Bpdu,
    ) {
        let name = self.name(id);
        let port = abbreviate_port(&arrival.port);
        let sender = self.name(arrival.from);
        let root = self
            .l2
            .trees
            .get(&bpdu.vlan)
            .map(|t| self.name(t.root_device))
            .unwrap_or_default();
        let (role, reason) = match self.l2.port(id, &arrival.port, bpdu.vlan) {
            Some(p) => (p.role, p.reason.clone()),
            None => {
                return self.record(
                    now,
                    id,
                    Some(&arrival),
                    Some(frame),
                    Outcome::Ignored,
                    format!("{name} receives a BPDU on {port}, but spanning tree isn't running there for VLAN {}.", bpdu.vlan),
                    Vec::new(),
                    Vec::new(),
                );
            }
        };
        let what = match role {
            crate::PortRole::Root => format!("{port} becomes {name}'s root port"),
            crate::PortRole::Designated => format!("{port} stays designated"),
            crate::PortRole::Alternate | crate::PortRole::Backup => format!("{port} blocks"),
        };
        let layers = vec![
            explain::physical(format!("Arrived on {port}.")),
            explain::data_link(
                LayerRole::Used,
                "Sent to the spanning-tree group address: switches read BPDUs themselves and never forward them.",
                &frame,
            ),
            bpdu_note(
                format!(
                    "From {sender}: root {root}, {sender}'s cost to it {}.",
                    bpdu.root_cost
                ),
                &bpdu,
            ),
        ];
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            Outcome::Accepted,
            format!(
                "{name} receives {sender}'s BPDU on {port} (VLAN {}): the root is {root}, {} away from {sender}. {what}: \
                 {reason}",
                bpdu.vlan,
                if bpdu.root_cost == 0 { "0".to_string() } else { format!("cost {}", bpdu.root_cost) }
            ),
            layers,
            Vec::new(),
        );
    }

    // MARK: OSPF

    fn ospf_ifaces(&self, id: DeviceId) -> Vec<(Iface, crate::OspfIface)> {
        let (Some(stack), Some(router)) = (self.stack(id), self.ospf.router(id)) else {
            return Vec::new();
        };
        router
            .ifaces
            .iter()
            .filter(|i| !i.passive && i.net_type != OspfNetType::Loopback && i.segment.is_some())
            .filter_map(|o| stack.iface(&o.name).map(|i| (i.clone(), o.clone())))
            .collect()
    }

    fn send_ospf(&mut self, id: DeviceId, iface: &Iface, o: &crate::OspfIface, packet: OspfPacket) {
        let Some(rid) = self.ospf.router(id).and_then(|r| r.router_id) else {
            return;
        };
        let ip = Ipv4Packet {
            src: o.address,
            dst: std::net::Ipv4Addr::new(224, 0, 0, 5),
            ttl: 1,
            l4: L4::Ospf(OspfPacket {
                router_id: rid,
                ..packet
            }),
        };
        let frame = Frame {
            src: iface.mac,
            dst: MacAddr::OSPF_ALL_ROUTERS,
            vlan: None,
            payload: Payload::Ipv4(ip),
        };
        self.stack_transmit_frame(id, iface, frame);
    }

    pub(crate) fn send_ospf_hellos(&mut self) {
        let ids: Vec<DeviceId> = self.ospf.routers.keys().copied().collect();
        for id in ids {
            for (iface, o) in self.ospf_ifaces(id) {
                let packet = OspfPacket {
                    message: OspfMessage::Hello,
                    router_id: std::net::Ipv4Addr::UNSPECIFIED,
                    area: o.area,
                    hello: o.hello,
                    dead: o.dead,
                    lsas: 0,
                };
                self.send_ospf(id, &iface, &o, packet);
            }
        }
    }

    /// Full neighbours send each other their LSAs.
    pub(crate) fn send_ospf_updates(&mut self) {
        let ids: Vec<DeviceId> = self.ospf.routers.keys().copied().collect();
        for id in ids {
            for (iface, o) in self.ospf_ifaces(id) {
                let full = self.ospf.router(id).is_some_and(|r| {
                    r.neighbors
                        .iter()
                        .any(|n| n.iface == o.name && n.state == NeighborState::Full)
                });
                if !full {
                    continue;
                }
                let lsas = self
                    .ospf
                    .lsdb
                    .get(&o.area)
                    .map(|l| l.len())
                    .unwrap_or(1)
                    .min(255) as u8;
                let packet = OspfPacket {
                    message: OspfMessage::LsUpdate,
                    router_id: std::net::Ipv4Addr::UNSPECIFIED,
                    area: o.area,
                    hello: o.hello,
                    dead: o.dead,
                    lsas,
                };
                self.send_ospf(id, &iface, &o, packet);
            }
        }
    }

    /// An OSPF packet reaches a device. End devices' network cards filter
    /// the multicast out silently; routers explain what it does for them.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ospf_receive(
        &mut self,
        now: SimTime,
        id: DeviceId,
        _stack: &Stack,
        iface: &Iface,
        arrival: Arrival,
        frame: Frame,
        ip: Ipv4Packet,
        packet: OspfPacket,
    ) {
        let Some(router) = self.ospf.router(id).cloned() else {
            return;
        };
        let name = self.name(id);
        let port = abbreviate_port(&arrival.port);
        let sender = self
            .ospf
            .routers
            .iter()
            .find(|(_, r)| r.router_id == Some(packet.router_id))
            .map(|(d, _)| self.name(*d))
            .unwrap_or_else(|| packet.router_id.to_string());
        let mine = router.ifaces.iter().find(|i| i.name == iface.name);
        let neighbor = router
            .neighbors
            .iter()
            .find(|n| n.iface == iface.name && n.router_id == packet.router_id);
        let layers = |note: String| -> Vec<LayerNote> {
            vec![
                explain::physical(format!("Arrived on {port}.")),
                explain::data_link(
                    LayerRole::Used,
                    "Sent to OSPF's multicast group, which routers listen to.",
                    &frame,
                ),
                explain::ipv4(
                    LayerRole::Used,
                    "To 224.0.0.5 (all OSPF routers), TTL 1: it never leaves the link.",
                    &ip,
                ),
                ospf_note(note, &packet),
            ]
        };
        let (outcome, summary, note) = match (packet.message, mine, neighbor) {
            (_, None, _) => (
                Outcome::Ignored,
                format!(
                    "{name} receives {sender}'s OSPF {} on {port}, but OSPF isn't enabled on that interface, so it's \
                     ignored.",
                    packet.message.name()
                ),
                "Not an OSPF interface here.".to_string(),
            ),
            (_, Some(m), _) if m.passive => (
                Outcome::Ignored,
                format!(
                    "{name} receives {sender}'s hello on {port}, but {port} is passive, so it takes no part."
                ),
                "Passive interface.".to_string(),
            ),
            (OspfMessage::Hello, Some(_), Some(n)) => match &n.problem {
                Some(p) => (
                    Outcome::Dropped,
                    format!("{name} hears {sender}'s hello on {port} but rejects it: {p}."),
                    "Rejected: the hello doesn't match.".to_string(),
                ),
                None => (
                    Outcome::Accepted,
                    format!(
                        "{name} hears {sender}'s hello on {port} and sees itself listed back: {}",
                        n.reason
                    ),
                    format!(
                        "Hello from {}: neighbour {}.",
                        packet.router_id,
                        state_name(n.state)
                    ),
                ),
            },
            (OspfMessage::Hello, Some(_), None) => (
                Outcome::Ignored,
                format!(
                    "{name} hears {sender}'s hello on {port}, but doesn't treat it as a neighbour."
                ),
                "No neighbour here.".to_string(),
            ),
            (OspfMessage::LsUpdate, Some(m), Some(n)) if n.state == NeighborState::Full => {
                let routes = router.routes.len();
                (
                    Outcome::Accepted,
                    format!(
                        "{name} gets {sender}'s LS Update ({} LSA{}) for area {} and adds them to its link-state \
                         database. Running SPF over the database gives it {routes} OSPF route{}.",
                        packet.lsas,
                        if packet.lsas == 1 { "" } else { "s" },
                        m.area,
                        if routes == 1 { "" } else { "s" }
                    ),
                    "LSAs from a full neighbour: added to the database.".to_string(),
                )
            }
            (OspfMessage::LsUpdate, Some(m), _) => (
                Outcome::Ignored,
                format!(
                    "{name} hears {sender}'s LS Update on {port} but isn't fully adjacent to {sender}{}: it syncs its \
                     database with the DR and BDR instead.",
                    if m.state == OspfIfState::DrOther {
                        " (both are DROthers)"
                    } else {
                        ""
                    }
                ),
                "Not a full neighbour.".to_string(),
            ),
        };
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            outcome,
            summary,
            layers(note),
            Vec::new(),
        );
    }
}

fn state_name(state: NeighborState) -> &'static str {
    match state {
        NeighborState::Full => "Full",
        NeighborState::TwoWay => "2-Way",
        NeighborState::Down => "Down",
    }
}

fn ospf_note(note: String, o: &OspfPacket) -> LayerNote {
    let mut fields = vec![
        crate::Field {
            name: "Protocol".into(),
            value: "OSPF (IP protocol 89)".into(),
            changed: false,
        },
        crate::Field {
            name: "Message".into(),
            value: o.message.name().into(),
            changed: false,
        },
        crate::Field {
            name: "Router ID".into(),
            value: o.router_id.to_string(),
            changed: false,
        },
        crate::Field {
            name: "Area".into(),
            value: o.area.to_string(),
            changed: false,
        },
    ];
    match o.message {
        OspfMessage::Hello => fields.push(crate::Field {
            name: "Hello / Dead".into(),
            value: format!("{} s / {} s", o.hello, o.dead),
            changed: false,
        }),
        OspfMessage::LsUpdate => fields.push(crate::Field {
            name: "LSAs".into(),
            value: o.lsas.to_string(),
            changed: false,
        }),
    }
    LayerNote {
        layer: crate::Layer::Message,
        role: LayerRole::Used,
        note,
        fields,
    }
}

fn bpdu_note(note: String, b: &Bpdu) -> LayerNote {
    let field = |name: &str, value: String| crate::Field {
        name: name.into(),
        value,
        changed: false,
    };
    LayerNote {
        layer: crate::Layer::Message,
        role: LayerRole::Used,
        note,
        fields: vec![
            field("Protocol", "Spanning tree (PVST+)".into()),
            field("VLAN", b.vlan.to_string()),
            field("Root bridge", b.root.to_string()),
            field("Root path cost", b.root_cost.to_string()),
            field("Sender bridge", b.bridge.to_string()),
            field("Port", format!("128.{}", b.port % 128)),
        ],
    }
}
