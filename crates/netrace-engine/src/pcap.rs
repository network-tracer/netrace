//! Frames as bytes on the wire, and a cable's capture as a libpcap file
//! that Wireshark opens. The simulation keeps frames structured; this is
//! the one place they're encoded, with the fields the simulation doesn't
//! model filled in as a real device would send them.

use std::net::Ipv4Addr;

use netrace_doc::LinkId;

use crate::frame::{
    ArpOp, DhcpOp, IcmpKind, Ipv4Packet, L4, OspfMessage, PING_DATA_BYTES, Payload,
};
use crate::{Frame, TraceEvent};

/// The data Windows' ping sends.
const PING_DATA: &[u8; PING_DATA_BYTES] = b"abcdefghijklmnopqrstuvwabcdefghi";

impl Frame {
    /// The frame as sent on the wire, without preamble or FCS: as long as
    /// [`Frame::len`] says.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len());
        out.extend_from_slice(&self.dst.0);
        out.extend_from_slice(&self.src.0);
        match &self.payload {
            Payload::Arp(arp) => {
                out.extend_from_slice(&0x0806u16.to_be_bytes());
                out.extend_from_slice(&[0, 1, 8, 0, 6, 4, 0]);
                out.push(if arp.op == ArpOp::Request { 1 } else { 2 });
                out.extend_from_slice(&arp.sender_mac.0);
                out.extend_from_slice(&arp.sender_ip.octets());
                out.extend_from_slice(&arp.target_mac.0);
                out.extend_from_slice(&arp.target_ip.octets());
            }
            Payload::Bpdu(b) => {
                // 802.3 length, LLC (STP SAPs), then a configuration BPDU.
                out.extend_from_slice(&38u16.to_be_bytes());
                out.extend_from_slice(&[0x42, 0x42, 0x03, 0, 0, 0, 0, 0]);
                let bridge = |out: &mut Vec<u8>, id: &crate::BridgeId| {
                    out.extend_from_slice(&(id.priority as u16).to_be_bytes());
                    out.extend_from_slice(&id.mac.0);
                };
                bridge(&mut out, &b.root);
                out.extend_from_slice(&b.root_cost.to_be_bytes());
                bridge(&mut out, &b.bridge);
                out.extend_from_slice(&(0x8000u16 | b.port as u16).to_be_bytes());
                // Message age 0, max age 20 s, hello 2 s, forward delay 15 s (1/256 s units).
                for seconds in [0u16, 20, 2, 15] {
                    out.extend_from_slice(&(seconds << 8).to_be_bytes());
                }
            }
            Payload::Ipv4(ip) => {
                out.extend_from_slice(&0x0800u16.to_be_bytes());
                out.extend(ipv4(ip));
            }
        }
        // Pad to Ethernet's 60-byte minimum, then add the 802.1Q tag.
        if out.len() < 60 {
            out.resize(60, 0);
        }
        if let Some(vlan) = self.vlan {
            let tag = [0x81, 0x00, (vlan >> 8) as u8 & 0x0f, vlan as u8];
            out.splice(12..12, tag);
        }
        out
    }
}

fn ipv4(ip: &Ipv4Packet) -> Vec<u8> {
    let (protocol, body) = match &ip.l4 {
        L4::Icmp(icmp) => {
            let mut b = vec![
                match icmp.kind {
                    IcmpKind::EchoRequest => 8,
                    IcmpKind::EchoReply => 0,
                    IcmpKind::TimeExceeded => 11,
                    IcmpKind::Unreachable(_) => 3,
                },
                icmp.kind.code(),
                0,
                0,
            ];
            if icmp.kind.is_error() {
                // Unused, then the start of the echo request it answers.
                b.extend_from_slice(&[0; 4]);
                let request = [
                    8,
                    0,
                    0,
                    0,
                    (icmp.id >> 8) as u8,
                    icmp.id as u8,
                    (icmp.seq >> 8) as u8,
                    icmp.seq as u8,
                ];
                b.extend(ip_header(
                    1,
                    ip.dst,
                    Ipv4Addr::UNSPECIFIED,
                    1,
                    request.len(),
                ));
                b.extend_from_slice(&request);
            } else {
                b.extend_from_slice(&icmp.id.to_be_bytes());
                b.extend_from_slice(&icmp.seq.to_be_bytes());
                b.extend_from_slice(PING_DATA);
            }
            let sum = checksum(&b, 0);
            b[2..4].copy_from_slice(&sum.to_be_bytes());
            (1, b)
        }
        L4::Tcp(tcp) => {
            let mut b = Vec::with_capacity(20 + tcp.data_len() as usize);
            b.extend_from_slice(&tcp.src_port.to_be_bytes());
            b.extend_from_slice(&tcp.dst_port.to_be_bytes());
            b.extend_from_slice(&tcp.seq.to_be_bytes());
            b.extend_from_slice(&tcp.ack.to_be_bytes());
            let f = tcp.flags;
            let flags = u8::from(f.fin)
                | u8::from(f.syn) << 1
                | u8::from(f.rst) << 2
                | u8::from(f.psh) << 3
                | u8::from(f.ack) << 4;
            b.extend_from_slice(&[5 << 4, flags, 0xfa, 0xf0, 0, 0, 0, 0]);
            if let Some(data) = tcp.data {
                b.extend_from_slice(data.as_bytes());
            }
            let sum = checksum(&b, pseudo(ip, 6, b.len()));
            b[16..18].copy_from_slice(&sum.to_be_bytes());
            (6, b)
        }
        L4::Udp(udp) => {
            let d = &udp.dhcp;
            let mut m = Vec::with_capacity(300);
            let reply = matches!(d.op, DhcpOp::Offer | DhcpOp::Ack | DhcpOp::Nak);
            m.extend_from_slice(&[if reply { 2 } else { 1 }, 1, 6, 0]);
            m.extend_from_slice(&d.xid.to_be_bytes());
            m.extend_from_slice(&[0, 0, 0x80, 0]); // secs, flags: broadcast
            m.extend_from_slice(&[0; 4]); // ciaddr
            m.extend_from_slice(&d.your_ip.octets());
            m.extend_from_slice(&[0; 4]); // siaddr
            m.extend_from_slice(&d.relay.octets());
            m.extend_from_slice(&d.client_mac.0);
            m.resize(m.len() + 10 + 64 + 128, 0); // chaddr padding, sname, file
            m.extend_from_slice(&[0x63, 0x82, 0x53, 0x63]);
            let kind = match d.op {
                DhcpOp::Discover => 1,
                DhcpOp::Offer => 2,
                DhcpOp::Request => 3,
                DhcpOp::Ack => 5,
                DhcpOp::Nak => 6,
            };
            m.extend_from_slice(&[53, 1, kind]);
            let mut option = |code: u8, address: Ipv4Addr| {
                m.extend_from_slice(&[code, 4]);
                m.extend_from_slice(&address.octets());
            };
            if !d.server.is_unspecified() {
                option(54, d.server);
            }
            if let Some(requested) = d.requested {
                option(50, requested);
            }
            if reply && d.op != DhcpOp::Nak {
                option(1, netrace_doc::mask(d.prefix_len));
                if let Some(router) = d.router {
                    option(3, router);
                }
                if let Some(dns) = d.dns {
                    option(6, dns);
                }
            }
            m.push(255);
            m.resize(300, 0);
            let mut b = Vec::with_capacity(308);
            b.extend_from_slice(&udp.src_port.to_be_bytes());
            b.extend_from_slice(&udp.dst_port.to_be_bytes());
            b.extend_from_slice(&((8 + m.len()) as u16).to_be_bytes());
            b.extend_from_slice(&[0, 0]);
            b.extend(m);
            let sum = checksum(&b, pseudo(ip, 17, b.len()));
            b[6..8].copy_from_slice(&sum.to_be_bytes());
            (17, b)
        }
        L4::Ospf(o) => {
            let mut b = vec![
                2,
                if o.message == OspfMessage::Hello {
                    1
                } else {
                    4
                },
                0,
                0,
            ];
            b.extend_from_slice(&o.router_id.octets());
            b.extend_from_slice(&o.area.to_be_bytes());
            b.extend_from_slice(&[0; 12]); // checksum, auth type, authentication
            match o.message {
                OspfMessage::Hello => {
                    b.extend_from_slice(&[0; 4]); // network mask
                    b.extend_from_slice(&o.hello.to_be_bytes());
                    b.extend_from_slice(&[0x02, 1]); // options (E), priority
                    b.extend_from_slice(&u32::from(o.dead).to_be_bytes());
                    b.extend_from_slice(&[0; 8]); // DR, BDR
                }
                OspfMessage::LsUpdate => {
                    b.extend_from_slice(&u32::from(o.lsas).to_be_bytes());
                    for _ in 0..o.lsas {
                        // A router-LSA header with no links.
                        b.extend_from_slice(&[0, 1, 0x02, 1]);
                        b.extend_from_slice(&o.router_id.octets());
                        b.extend_from_slice(&o.router_id.octets());
                        b.extend_from_slice(&0x8000_0001u32.to_be_bytes());
                        b.extend_from_slice(&[0, 0, 0, 24, 0, 0, 0, 0]);
                    }
                }
            }
            let len = b.len() as u16;
            b[2..4].copy_from_slice(&len.to_be_bytes());
            let sum = checksum(&b, 0);
            b[12..14].copy_from_slice(&sum.to_be_bytes());
            (89, b)
        }
    };
    let mut out = ip_header(protocol, ip.src, ip.dst, ip.ttl, body.len());
    out.extend(body);
    out
}

fn ip_header(protocol: u8, src: Ipv4Addr, dst: Ipv4Addr, ttl: u8, body: usize) -> Vec<u8> {
    let mut h = vec![0x45, 0];
    h.extend_from_slice(&((20 + body) as u16).to_be_bytes());
    h.extend_from_slice(&[0, 0, 0x40, 0, ttl, protocol, 0, 0]);
    h.extend_from_slice(&src.octets());
    h.extend_from_slice(&dst.octets());
    let sum = checksum(&h, 0);
    h[10..12].copy_from_slice(&sum.to_be_bytes());
    h
}

/// The TCP/UDP pseudo-header's contribution to the checksum.
fn pseudo(ip: &Ipv4Packet, protocol: u8, len: usize) -> u32 {
    let words = |a: Ipv4Addr| {
        let o = a.octets();
        u32::from(u16::from_be_bytes([o[0], o[1]])) + u32::from(u16::from_be_bytes([o[2], o[3]]))
    };
    words(ip.src) + words(ip.dst) + u32::from(protocol) + len as u32
}

/// The Internet checksum (RFC 1071), starting from `sum`.
fn checksum(bytes: &[u8], mut sum: u32) -> u16 {
    for chunk in bytes.chunks(2) {
        sum += u32::from(u16::from_be_bytes([chunk[0], *chunk.get(1).unwrap_or(&0)]));
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Every frame that crossed `link`, as a libpcap file (Ethernet, microsecond
/// timestamps from the start of the simulation).
pub fn pcap(trace: &[TraceEvent], link: LinkId) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0xa1b2_c3d4u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&[0; 8]); // timezone, accuracy
    out.extend_from_slice(&65535u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // LINKTYPE_ETHERNET
    for event in trace.iter().filter(|e| e.link == Some(link)) {
        let Some(frame) = &event.frame else { continue };
        let bytes = frame.to_bytes();
        let micros = event.time.as_nanos() / 1_000;
        out.extend_from_slice(&((micros / 1_000_000) as u32).to_le_bytes());
        out.extend_from_slice(&((micros % 1_000_000) as u32).to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend(bytes);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::checksum;

    #[test]
    fn checksums_match_rfc_1071() {
        // The example IPv4 header from RFC 1071's discussion (checksum 0xb861).
        let header = [
            0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8,
            0x00, 0x01, 0xc0, 0xa8, 0x00, 0xc7,
        ];
        assert_eq!(checksum(&header, 0), 0xb861);
    }
}
