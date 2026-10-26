use std::net::Ipv4Addr;

use crate::MacAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Protocol {
    Arp,
    Icmp,
    Dhcp,
}

impl Protocol {
    pub fn name(self) -> &'static str {
        match self {
            Protocol::Arp => "ARP",
            Protocol::Icmp => "ICMP",
            Protocol::Dhcp => "DHCP",
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
    /// Code 13: an access list on a router stopped the packet.
    Prohibited,
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
            IcmpKind::Unreachable(Unreachable::Prohibited) => 13,
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
pub enum DhcpOp {
    Discover,
    Offer,
    Request,
    Ack,
    Nak,
}

impl DhcpOp {
    pub fn name(self) -> &'static str {
        match self {
            DhcpOp::Discover => "Discover",
            DhcpOp::Offer => "Offer",
            DhcpOp::Request => "Request",
            DhcpOp::Ack => "ACK",
            DhcpOp::Nak => "NAK",
        }
    }
}

/// A DHCP message (carried in UDP 67/68). Only the fields the exchange
/// needs, named as Wireshark names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dhcp {
    pub op: DhcpOp,
    /// Transaction ID, the same in all four messages of one exchange.
    pub xid: u32,
    pub client_mac: MacAddr,
    /// "Your" address: what the server offers or assigns.
    pub your_ip: Ipv4Addr,
    /// The relay agent's address (giaddr), 0.0.0.0 when not relayed.
    pub relay: Ipv4Addr,
    /// Option 54: the server that made the offer.
    pub server: Ipv4Addr,
    /// Option 50: the address the client asks for in its request.
    pub requested: Option<Ipv4Addr>,
    pub prefix_len: u8,
    pub router: Option<Ipv4Addr>,
    pub dns: Option<Ipv4Addr>,
}

pub const DHCP_SERVER_PORT: u16 = 67;
pub const DHCP_CLIENT_PORT: u16 = 68;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Udp {
    pub src_port: u16,
    pub dst_port: u16,
    pub dhcp: Dhcp,
}

/// What an IPv4 packet carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum L4 {
    Icmp(Icmp),
    Udp(Udp),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Packet {
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub ttl: u8,
    pub l4: L4,
}

impl Ipv4Packet {
    pub fn icmp(&self) -> Option<&Icmp> {
        match &self.l4 {
            L4::Icmp(icmp) => Some(icmp),
            L4::Udp(_) => None,
        }
    }

    pub fn udp(&self) -> Option<&Udp> {
        match &self.l4 {
            L4::Udp(udp) => Some(udp),
            L4::Icmp(_) => None,
        }
    }

    /// ICMP errors are never answered with another ICMP error.
    pub fn is_icmp_error(&self) -> bool {
        self.icmp().is_some_and(|i| i.kind.is_error())
    }
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
            Payload::Ipv4(ip) => match ip.l4 {
                L4::Icmp(_) => Protocol::Icmp,
                L4::Udp(_) => Protocol::Dhcp,
            },
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
            Payload::Ipv4(ip) if ip.is_icmp_error() => 14 + 20 + 8 + 28,
            // Ethernet 14 + IPv4 20 + UDP 8 + a minimal DHCP message.
            Payload::Ipv4(Ipv4Packet { l4: L4::Udp(_), .. }) => 14 + 20 + 8 + 300,
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
            Payload::Ipv4(Ipv4Packet {
                l4: L4::Udp(udp), ..
            }) => {
                let d = udp.dhcp;
                match d.op {
                    DhcpOp::Discover | DhcpOp::Request => {
                        format!("DHCP {} - Transaction ID {:#010x}", d.op.name(), d.xid)
                    }
                    _ => format!(
                        "DHCP {} - Transaction ID {:#010x}, your IP {}",
                        d.op.name(),
                        d.xid,
                        d.your_ip
                    ),
                }
            }
            Payload::Ipv4(Ipv4Packet {
                l4: L4::Icmp(icmp),
                ttl,
                ..
            }) => match icmp.kind {
                IcmpKind::EchoRequest | IcmpKind::EchoReply => format!(
                    "{}  id={:#06x}, seq={}, ttl={}",
                    if icmp.kind == IcmpKind::EchoRequest {
                        "Echo (ping) request"
                    } else {
                        "Echo (ping) reply"
                    },
                    icmp.id,
                    icmp.seq,
                    ttl
                ),
                IcmpKind::TimeExceeded => {
                    "Time-to-live exceeded (Time to live exceeded in transit)".into()
                }
                IcmpKind::Unreachable(Unreachable::Network) => {
                    "Destination unreachable (Network unreachable)".into()
                }
                IcmpKind::Unreachable(Unreachable::Prohibited) => {
                    "Destination unreachable (Communication administratively filtered)".into()
                }
                IcmpKind::Unreachable(Unreachable::Host) => {
                    "Destination unreachable (Host unreachable)".into()
                }
            },
        }
    }
}
