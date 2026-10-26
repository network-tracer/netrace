use std::net::Ipv4Addr;

use crate::MacAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Protocol {
    Arp,
    Icmp,
    Dhcp,
    Tcp,
    Ospf,
    Stp,
}

impl Protocol {
    pub fn name(self) -> &'static str {
        match self {
            Protocol::Arp => "ARP",
            Protocol::Icmp => "ICMP",
            Protocol::Dhcp => "DHCP",
            Protocol::Tcp => "TCP",
            Protocol::Ospf => "OSPF",
            Protocol::Stp => "STP",
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

/// TCP control flags set on a segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TcpFlags {
    pub syn: bool,
    pub ack: bool,
    pub psh: bool,
    pub fin: bool,
    pub rst: bool,
}

impl TcpFlags {
    pub const SYN: TcpFlags = TcpFlags {
        syn: true,
        ack: false,
        psh: false,
        fin: false,
        rst: false,
    };
    pub const SYN_ACK: TcpFlags = TcpFlags {
        syn: true,
        ack: true,
        psh: false,
        fin: false,
        rst: false,
    };
    pub const ACK: TcpFlags = TcpFlags {
        syn: false,
        ack: true,
        psh: false,
        fin: false,
        rst: false,
    };
    pub const PSH_ACK: TcpFlags = TcpFlags {
        syn: false,
        ack: true,
        psh: true,
        fin: false,
        rst: false,
    };
    pub const FIN_ACK: TcpFlags = TcpFlags {
        syn: false,
        ack: true,
        psh: false,
        fin: true,
        rst: false,
    };
    pub const RST_ACK: TcpFlags = TcpFlags {
        syn: false,
        ack: true,
        psh: false,
        fin: false,
        rst: true,
    };

    /// "SYN, ACK", as Wireshark lists them.
    pub fn names(self) -> String {
        let mut names = Vec::new();
        for (on, name) in [
            (self.fin, "FIN"),
            (self.syn, "SYN"),
            (self.rst, "RST"),
            (self.psh, "PSH"),
            (self.ack, "ACK"),
        ] {
            if on {
                names.push(name);
            }
        }
        names.join(", ")
    }
}

/// A TCP segment. Only what the handshake, one request and one response
/// and the close need; `data` names the application message it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tcp {
    pub src_port: u16,
    pub dst_port: u16,
    pub flags: TcpFlags,
    pub seq: u32,
    pub ack: u32,
    pub data: Option<&'static str>,
}

impl Tcp {
    /// Bytes of payload (the length of the message it stands for).
    pub fn data_len(&self) -> u32 {
        self.data.map(|d| d.len() as u32).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OspfMessage {
    /// Finds neighbours and checks they agree (area, timers, subnet).
    Hello,
    /// Carries LSAs to a fully adjacent neighbour.
    LsUpdate,
}

impl OspfMessage {
    pub fn name(self) -> &'static str {
        match self {
            OspfMessage::Hello => "Hello",
            OspfMessage::LsUpdate => "LS Update",
        }
    }
}

/// An OSPF packet (IP protocol 89), with the fields its explanation needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OspfPacket {
    pub message: OspfMessage,
    pub router_id: Ipv4Addr,
    pub area: u32,
    pub hello: u16,
    pub dead: u16,
    /// LS Update: how many LSAs it carries.
    pub lsas: u8,
}

/// A spanning-tree BPDU (PVST+: one per VLAN).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bpdu {
    pub vlan: u16,
    pub root: crate::BridgeId,
    pub root_cost: u32,
    pub bridge: crate::BridgeId,
    /// 128.N.
    pub port: u32,
}

/// What an IPv4 packet carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum L4 {
    Icmp(Icmp),
    Udp(Udp),
    Tcp(Tcp),
    Ospf(OspfPacket),
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
            _ => None,
        }
    }

    pub fn udp(&self) -> Option<&Udp> {
        match &self.l4 {
            L4::Udp(udp) => Some(udp),
            _ => None,
        }
    }

    pub fn tcp(&self) -> Option<&Tcp> {
        match &self.l4 {
            L4::Tcp(tcp) => Some(tcp),
            _ => None,
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
    Bpdu(Bpdu),
}

/// An Ethernet II frame. Kept structured rather than as bytes: the inspector
/// shows fields, and only capture export needs the wire encoding (`pcap.rs`).
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
                L4::Tcp(_) => Protocol::Tcp,
                L4::Ospf(_) => Protocol::Ospf,
            },
            Payload::Bpdu(_) => Protocol::Stp,
        }
    }

    pub fn ethertype(&self) -> &'static str {
        match self.payload {
            Payload::Arp(_) => "ARP (0x0806)",
            Payload::Ipv4(_) => "IPv4 (0x0800)",
            Payload::Bpdu(_) => "802.3 LLC (STP)",
        }
    }

    /// Length on the wire, excluding preamble and FCS.
    pub fn len(&self) -> usize {
        let tag = if self.vlan.is_some() { 4 } else { 0 };
        tag + match self.payload {
            // 14-byte header + 28-byte ARP, padded to the 60-byte minimum.
            Payload::Arp(_) | Payload::Bpdu(_) => 60,
            // Ethernet 14 + IPv4 20 + OSPF header 24 + a hello, or an LS
            // Update's count and LSA headers.
            Payload::Ipv4(Ipv4Packet {
                l4: L4::Ospf(o), ..
            }) => match o.message {
                OspfMessage::Hello => 14 + 20 + 24 + 20,
                OspfMessage::LsUpdate => 14 + 20 + 24 + 4 + 24 * o.lsas as usize,
            },
            // Errors quote the original IP header and 8 bytes of it.
            Payload::Ipv4(ip) if ip.is_icmp_error() => 14 + 20 + 8 + 28,
            // Ethernet 14 + IPv4 20 + UDP 8 + a minimal DHCP message.
            Payload::Ipv4(Ipv4Packet { l4: L4::Udp(_), .. }) => 14 + 20 + 8 + 300,
            // Ethernet 14 + IPv4 20 + TCP 20 + data, padded to the 60-byte minimum.
            Payload::Ipv4(Ipv4Packet {
                l4: L4::Tcp(tcp), ..
            }) => (14 + 20 + 20 + tcp.data_len() as usize).max(60),
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
            Payload::Bpdu(b) => format!(
                "Conf. Root = {}  Cost = {}  Port = 0x{:04x}  VLAN {}",
                b.root, b.root_cost, b.port, b.vlan
            ),
            Payload::Ipv4(Ipv4Packet {
                l4: L4::Ospf(o), ..
            }) => match o.message {
                OspfMessage::Hello => format!(
                    "Hello Packet  Router ID {}  Area {}  Hello {} / Dead {}",
                    o.router_id, o.area, o.hello, o.dead
                ),
                OspfMessage::LsUpdate => format!(
                    "LS Update  Router ID {}  {} LSA{}",
                    o.router_id,
                    o.lsas,
                    if o.lsas == 1 { "" } else { "s" }
                ),
            },
            Payload::Ipv4(Ipv4Packet {
                l4: L4::Tcp(tcp), ..
            }) => {
                let mut text = format!(
                    "{} → {} [{}] Seq={} Ack={} Len={}",
                    tcp.src_port,
                    tcp.dst_port,
                    tcp.flags.names(),
                    tcp.seq,
                    tcp.ack,
                    tcp.data_len()
                );
                if let Some(d) = tcp.data {
                    text += &format!("  {d}");
                }
                text
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
