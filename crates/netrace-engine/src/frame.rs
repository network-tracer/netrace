use std::net::Ipv4Addr;

use crate::MacAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Protocol {
    Arp,
    Icmp,
}

impl Protocol {
    pub fn name(self) -> &'static str {
        match self {
            Protocol::Arp => "ARP",
            Protocol::Icmp => "ICMP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpOp {
    Request,
    Reply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArpPacket {
    pub op: ArpOp,
    pub sender_mac: MacAddr,
    pub sender_ip: Ipv4Addr,
    /// All zeros in a request: that is the unknown being asked for.
    pub target_mac: MacAddr,
    pub target_ip: Ipv4Addr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcmpKind {
    EchoRequest,
    EchoReply,
    /// A router dropped the packet because its TTL ran out (type 11).
    TimeExceeded,
    /// No route (network) or no answer to ARP (host) (type 3).
    Unreachable(Unreachable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreachable {
    Network,
    Host,
}

impl IcmpKind {
    pub fn is_error(self) -> bool {
        matches!(self, IcmpKind::TimeExceeded | IcmpKind::Unreachable(_))
    }

    /// "8 · Echo request" style label for the inspector.
    pub fn label(self) -> &'static str {
        match self {
            IcmpKind::EchoRequest => "8 · Echo request",
            IcmpKind::EchoReply => "0 · Echo reply",
            IcmpKind::TimeExceeded => "11 · Time exceeded",
            IcmpKind::Unreachable(_) => "3 · Destination unreachable",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            IcmpKind::Unreachable(Unreachable::Host) => 1,
            _ => 0,
        }
    }
}

/// An ICMP message. Error messages (time exceeded, unreachable) carry the
/// identifier and sequence number of the echo request that caused them,
/// as a real one quotes the original packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Icmp {
    pub kind: IcmpKind,
    pub id: u16,
    pub seq: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Packet {
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub ttl: u8,
    pub icmp: Icmp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload {
    Arp(ArpPacket),
    Ipv4(Ipv4Packet),
}

/// An Ethernet II frame. Kept structured rather than as bytes: the inspector
/// shows fields, and nothing in the simulation needs the wire encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub src: MacAddr,
    pub dst: MacAddr,
    /// 802.1Q tag: the VLAN, on trunk links (and router subinterfaces).
    pub vlan: Option<u16>,
    pub payload: Payload,
}

/// Bytes of data in a ping, as Windows sends by default.
pub const PING_DATA_BYTES: usize = 32;

impl Frame {
    pub fn protocol(&self) -> Protocol {
        match self.payload {
            Payload::Arp(_) => Protocol::Arp,
            Payload::Ipv4(_) => Protocol::Icmp,
        }
    }

    pub fn ethertype(&self) -> &'static str {
        match self.payload {
            Payload::Arp(_) => "ARP (0x0806)",
            Payload::Ipv4(_) => "IPv4 (0x0800)",
        }
    }

    /// Length on the wire, excluding preamble and FCS.
    pub fn len(&self) -> usize {
        let tag = if self.vlan.is_some() { 4 } else { 0 };
        tag + match self.payload {
            // 14-byte header + 28-byte ARP, padded to the 60-byte minimum.
            Payload::Arp(_) => 60,
            // Errors quote the original IP header and 8 bytes of it.
            Payload::Ipv4(ip) if ip.icmp.kind.is_error() => 14 + 20 + 8 + 28,
            // Ethernet 14 + IPv4 20 + ICMP 8 + data.
            Payload::Ipv4(_) => 14 + 20 + 8 + PING_DATA_BYTES,
        }
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// The same frame with its 802.1Q tag set (or removed).
    pub fn tagged(self, vlan: Option<u16>) -> Frame {
        Frame { vlan, ..self }
    }

    /// One-line summary, Wireshark style, for event lists and captures.
    pub fn info(&self) -> String {
        match self.payload {
            Payload::Arp(arp) => match arp.op {
                ArpOp::Request => format!("Who has {}? Tell {}", arp.target_ip, arp.sender_ip),
                ArpOp::Reply => format!("{} is at {}", arp.sender_ip, arp.sender_mac),
            },
            Payload::Ipv4(ip) => match ip.icmp.kind {
                IcmpKind::EchoRequest | IcmpKind::EchoReply => format!(
                    "{}  id={:#06x}, seq={}, ttl={}",
                    if ip.icmp.kind == IcmpKind::EchoRequest {
                        "Echo (ping) request"
                    } else {
                        "Echo (ping) reply"
                    },
                    ip.icmp.id,
                    ip.icmp.seq,
                    ip.ttl
                ),
                IcmpKind::TimeExceeded => {
                    "Time-to-live exceeded (Time to live exceeded in transit)".into()
                }
                IcmpKind::Unreachable(Unreachable::Network) => {
                    "Destination unreachable (Network unreachable)".into()
                }
                IcmpKind::Unreachable(Unreachable::Host) => {
                    "Destination unreachable (Host unreachable)".into()
                }
            },
        }
    }
}
