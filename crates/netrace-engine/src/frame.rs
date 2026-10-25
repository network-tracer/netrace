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
}

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
        match self.payload {
            // 14-byte header + 28-byte ARP, padded to the 60-byte minimum.
            Payload::Arp(_) => 60,
            // Ethernet 14 + IPv4 20 + ICMP 8 + data.
            Payload::Ipv4(_) => 14 + 20 + 8 + PING_DATA_BYTES,
        }
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// One-line summary, Wireshark style, for event lists and captures.
    pub fn info(&self) -> String {
        match self.payload {
            Payload::Arp(arp) => match arp.op {
                ArpOp::Request => format!("Who has {}? Tell {}", arp.target_ip, arp.sender_ip),
                ArpOp::Reply => format!("{} is at {}", arp.sender_ip, arp.sender_mac),
            },
            Payload::Ipv4(ip) => {
                let kind = match ip.icmp.kind {
                    IcmpKind::EchoRequest => "Echo (ping) request",
                    IcmpKind::EchoReply => "Echo (ping) reply",
                };
                format!(
                    "{kind}  id={:#06x}, seq={}, ttl={}",
                    ip.icmp.id, ip.icmp.seq, ip.ttl
                )
            }
        }
    }
}
