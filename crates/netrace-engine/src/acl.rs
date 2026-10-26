//! Access control lists on router interfaces. Every check records which
//! line matched (or the implicit deny at the end), so the trace can show
//! the exact rule that let a packet through or stopped it.

use netrace_doc::{AccessList, AclAction, AclKind, AclProtocol, DeviceConfig, DeviceId, IcmpMatch};

use crate::{IcmpKind, Ipv4Packet, L4};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AclDirection {
    In,
    Out,
}

impl AclDirection {
    pub fn ios(self) -> &'static str {
        match self {
            AclDirection::In => "in",
            AclDirection::Out => "out",
        }
    }
}

/// The result of checking a packet against an access list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AclDecision {
    pub device: DeviceId,
    pub acl: String,
    pub direction: AclDirection,
    pub interface: String,
    /// The list as IOS prints it: (sequence, text).
    pub lines: Vec<(u32, String)>,
    /// The sequence number that matched; `None` means the implicit deny.
    pub matched: Option<u32>,
    pub permitted: bool,
}

impl AclDecision {
    /// "line 10: deny icmp host 10.0.0.5 any" or "the implicit deny at the end".
    pub fn rule(&self) -> String {
        match self.matched {
            Some(seq) => {
                let text = self
                    .lines
                    .iter()
                    .find(|(s, _)| *s == seq)
                    .map(|(_, t)| t.as_str())
                    .unwrap_or("");
                format!("line {seq}: {text}")
            }
            None => "the implicit \"deny any\" at the end (no line matched)".into(),
        }
    }
}

/// Checks `packet` against the list named on an interface. A list that
/// doesn't exist permits everything, as IOS does.
pub(crate) fn evaluate(
    device: DeviceId,
    config: &DeviceConfig,
    name: &str,
    direction: AclDirection,
    interface: &str,
    packet: &Ipv4Packet,
) -> Option<AclDecision> {
    let acl: &AccessList = config.access_list(name)?;
    let lines = acl
        .entries
        .iter()
        .map(|e| (e.sequence, e.ios(acl.kind)))
        .collect();
    let matched = acl.entries.iter().find(|e| {
        if !e.source.matches(packet.src) {
            return false;
        }
        if acl.kind == AclKind::Standard {
            return true;
        }
        if !e.destination.matches(packet.dst) {
            return false;
        }
        match (e.protocol, &packet.l4) {
            (AclProtocol::Ip, _) => true,
            (AclProtocol::Icmp, L4::Icmp(icmp)) => e.icmp.is_none_or(|t| {
                matches!(
                    (t, icmp.kind),
                    (IcmpMatch::Echo, IcmpKind::EchoRequest)
                        | (IcmpMatch::EchoReply, IcmpKind::EchoReply)
                        | (IcmpMatch::Unreachable, IcmpKind::Unreachable(_))
                        | (IcmpMatch::TimeExceeded, IcmpKind::TimeExceeded)
                )
            }),
            (AclProtocol::Udp, L4::Udp(udp)) => {
                e.source_port.is_none_or(|p| p.matches(udp.src_port))
                    && e.destination_port.is_none_or(|p| p.matches(udp.dst_port))
            }
            (AclProtocol::Tcp, L4::Tcp(tcp)) => {
                e.source_port.is_none_or(|p| p.matches(tcp.src_port))
                    && e.destination_port.is_none_or(|p| p.matches(tcp.dst_port))
            }
            _ => false,
        }
    });
    Some(AclDecision {
        device,
        acl: acl.name.clone(),
        direction,
        interface: interface.to_string(),
        lines,
        matched: matched.map(|e| e.sequence),
        permitted: matched.is_some_and(|e| e.action == AclAction::Permit),
    })
}
