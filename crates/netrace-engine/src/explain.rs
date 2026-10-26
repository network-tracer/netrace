//! Builders for the per-layer notes shown in the packet inspector.

use crate::{
    ArpOp, ArpPacket, Field, Frame, Icmp, Ipv4Packet, Layer, LayerNote, LayerRole, MacAddr, Payload,
};

fn field(name: &str, value: impl ToString, changed: bool) -> Field {
    Field {
        name: name.to_string(),
        value: value.to_string(),
        changed,
    }
}

fn mac_label(mac: MacAddr) -> String {
    if mac.is_broadcast() {
        format!("{mac} (broadcast)")
    } else if mac == MacAddr::OSPF_ALL_ROUTERS {
        format!("{mac} (OSPF routers)")
    } else if mac == MacAddr::STP_BRIDGES {
        format!("{mac} (spanning tree)")
    } else if mac == MacAddr::ZERO {
        format!("{mac} (unknown)")
    } else {
        mac.to_string()
    }
}

pub(crate) fn physical(note: impl Into<String>) -> LayerNote {
    LayerNote {
        layer: Layer::Physical,
        role: LayerRole::Used,
        note: note.into(),
        fields: Vec::new(),
    }
}

pub(crate) fn not_used(layer: Layer, note: impl Into<String>) -> LayerNote {
    LayerNote {
        layer,
        role: LayerRole::NotUsed,
        note: note.into(),
        fields: Vec::new(),
    }
}

pub(crate) fn data_link(role: LayerRole, note: impl Into<String>, frame: &Frame) -> LayerNote {
    let changed = role == LayerRole::Changed;
    LayerNote {
        layer: Layer::DataLink,
        role,
        note: note.into(),
        fields: {
            let mut fields = vec![
                field("Destination MAC", mac_label(frame.dst), changed),
                field("Source MAC", frame.src, changed),
            ];
            if let Some(vlan) = frame.vlan {
                fields.push(field("802.1Q tag", format!("VLAN {vlan}"), changed));
            }
            fields.push(field("Type", frame.ethertype(), changed));
            fields
        },
    }
}

pub(crate) fn arp(role: LayerRole, note: impl Into<String>, arp: &ArpPacket) -> LayerNote {
    let changed = role == LayerRole::Changed;
    let op = match arp.op {
        ArpOp::Request => "Request (1)",
        ArpOp::Reply => "Reply (2)",
    };
    LayerNote {
        layer: Layer::Network,
        role,
        note: note.into(),
        fields: vec![
            field("Protocol", "ARP", changed),
            field("Operation", op, changed),
            field("Sender MAC", arp.sender_mac, changed),
            field("Sender IP", arp.sender_ip, changed),
            field("Target MAC", mac_label(arp.target_mac), changed),
            field("Target IP", arp.target_ip, changed),
        ],
    }
}

pub(crate) fn ipv4(role: LayerRole, note: impl Into<String>, ip: &Ipv4Packet) -> LayerNote {
    let changed = role == LayerRole::Changed;
    LayerNote {
        layer: Layer::Network,
        role,
        note: note.into(),
        fields: vec![
            field("Protocol", "IPv4", changed),
            field("Source IP", ip.src, changed),
            field("Destination IP", ip.dst, changed),
            field("TTL", ip.ttl, changed),
            field(
                "Carries",
                if ip.udp().is_some() {
                    "UDP (17)"
                } else {
                    "ICMP (1)"
                },
                changed,
            ),
        ],
    }
}

pub(crate) fn icmp(role: LayerRole, note: impl Into<String>, icmp: &Icmp) -> LayerNote {
    let changed = role == LayerRole::Changed;
    let mut fields = vec![
        field("Protocol", "ICMP", changed),
        field("Type", icmp.kind.label(), changed),
        field("Code", icmp.kind.code(), changed),
    ];
    if icmp.kind.is_error() {
        fields.push(field(
            "About",
            format!("echo request id {:#06x}, seq {}", icmp.id, icmp.seq),
            changed,
        ));
    } else {
        fields.push(field("Identifier", format!("{:#06x}", icmp.id), changed));
        fields.push(field("Sequence", icmp.seq, changed));
    }
    LayerNote {
        layer: Layer::Message,
        role,
        note: note.into(),
        fields,
    }
}

/// The message inside an IP packet: ICMP or DHCP.
pub(crate) fn message(role: LayerRole, note: impl Into<String>, packet: &Ipv4Packet) -> LayerNote {
    match packet.l4 {
        crate::L4::Icmp(icmp) => self::icmp(role, note, &icmp),
        crate::L4::Udp(udp) => dhcp(role, note, &udp),
        crate::L4::Tcp(tcp) => self::tcp(role, note, &tcp),
        crate::L4::Ospf(o) => LayerNote {
            layer: Layer::Message,
            role,
            note: note.into(),
            fields: vec![
                field("Protocol", "OSPF", false),
                field("Message", o.message.name(), false),
                field("Router ID", o.router_id, false),
            ],
        },
    }
}

pub(crate) fn tcp(role: LayerRole, note: impl Into<String>, tcp: &crate::Tcp) -> LayerNote {
    let changed = role == LayerRole::Changed;
    let service = match crate::service(tcp.dst_port) {
        "" => crate::service(tcp.src_port),
        s => s,
    };
    let mut fields = vec![
        field(
            "Protocol",
            if service.is_empty() {
                "TCP".to_string()
            } else {
                format!("TCP ({service})")
            },
            changed,
        ),
        field(
            "Ports",
            format!("{} → {}", tcp.src_port, tcp.dst_port),
            changed,
        ),
        field("Flags", tcp.flags.names(), changed),
        field("Sequence", tcp.seq, changed),
        field("Acknowledgement", tcp.ack, changed),
    ];
    if let Some(d) = tcp.data {
        fields.push(field("Data", d, changed));
    }
    LayerNote {
        layer: Layer::Message,
        role,
        note: note.into(),
        fields,
    }
}

pub(crate) fn dhcp(role: LayerRole, note: impl Into<String>, udp: &crate::Udp) -> LayerNote {
    let changed = role == LayerRole::Changed;
    let d = udp.dhcp;
    let mut fields = vec![
        field("Protocol", "DHCP over UDP", changed),
        field(
            "Ports",
            format!("{} → {}", udp.src_port, udp.dst_port),
            changed,
        ),
        field("Message", d.op.name(), changed),
        field("Transaction ID", format!("{:#010x}", d.xid), changed),
        field("Client MAC", d.client_mac, changed),
    ];
    if !d.your_ip.is_unspecified() {
        fields.push(field(
            "Your IP",
            format!("{}/{}", d.your_ip, d.prefix_len),
            changed,
        ));
    }
    if let Some(r) = d.requested {
        fields.push(field("Requested IP", r, changed));
    }
    if !d.server.is_unspecified() {
        fields.push(field("Server", d.server, changed));
    }
    if !d.relay.is_unspecified() {
        fields.push(field("Relay agent", d.relay, changed));
    }
    if let Some(r) = d.router {
        fields.push(field("Router (gateway)", r, changed));
    }
    if let Some(dns) = d.dns {
        fields.push(field("DNS server", dns, changed));
    }
    LayerNote {
        layer: Layer::Message,
        role,
        note: note.into(),
        fields,
    }
}

/// Upper layers a switch or hub does not read.
pub(crate) fn upper_not_used(frame: &Frame, who: &str) -> Vec<LayerNote> {
    let mut notes = vec![not_used(
        Layer::Network,
        format!("{who} doesn't read IP or ARP; it never looks past the Ethernet header."),
    )];
    if matches!(frame.payload, Payload::Ipv4(_)) {
        notes.push(not_used(Layer::Message, "Carried along untouched."));
    }
    notes
}
