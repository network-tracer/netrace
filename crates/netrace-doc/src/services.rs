//! Network services configured on routers: DHCP pools and access lists.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

/// `ip dhcp pool NAME` and its settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DhcpPool {
    pub name: String,
    /// `network A M`: the addresses the pool hands out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<(Ipv4Addr, u8)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_router: Option<Ipv4Addr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_server: Option<Ipv4Addr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_name: Option<String>,
}

impl DhcpPool {
    pub fn new(name: &str) -> Self {
        DhcpPool {
            name: name.to_string(),
            network: None,
            default_router: None,
            dns_server: None,
            domain_name: None,
        }
    }

    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        self.network
            .is_some_and(|(net, len)| u32::from(ip) & u32::from(crate::mask(len)) == u32::from(net))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AclKind {
    /// Matches the source address only.
    Standard,
    /// Matches protocol, source, destination and ports.
    Extended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AclAction {
    Permit,
    Deny,
}

impl AclAction {
    pub fn ios(self) -> &'static str {
        match self {
            AclAction::Permit => "permit",
            AclAction::Deny => "deny",
        }
    }
}

/// An address and wildcard: `any`, `host A`, or `A W`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AclAddress {
    pub address: Ipv4Addr,
    /// Wildcard bits: 1 means "don't care". `any` is 255.255.255.255.
    pub wildcard: Ipv4Addr,
}

impl AclAddress {
    pub const ANY: AclAddress = AclAddress {
        address: Ipv4Addr::UNSPECIFIED,
        wildcard: Ipv4Addr::BROADCAST,
    };

    pub fn host(address: Ipv4Addr) -> Self {
        AclAddress {
            address,
            wildcard: Ipv4Addr::UNSPECIFIED,
        }
    }

    pub fn matches(&self, ip: Ipv4Addr) -> bool {
        let care = !u32::from(self.wildcard);
        u32::from(ip) & care == u32::from(self.address) & care
    }

    /// "any", "host 10.0.0.1" or "10.0.0.0 0.0.0.255".
    pub fn ios(&self) -> String {
        if self.wildcard == Ipv4Addr::BROADCAST {
            "any".into()
        } else if self.wildcard == Ipv4Addr::UNSPECIFIED {
            format!("host {}", self.address)
        } else {
            format!("{} {}", self.address, self.wildcard)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AclProtocol {
    Ip,
    Icmp,
    Tcp,
    Udp,
}

impl AclProtocol {
    pub fn ios(self) -> &'static str {
        match self {
            AclProtocol::Ip => "ip",
            AclProtocol::Icmp => "icmp",
            AclProtocol::Tcp => "tcp",
            AclProtocol::Udp => "udp",
        }
    }
}

/// A port condition on TCP / UDP entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortMatch {
    Eq(u16),
    Gt(u16),
    Lt(u16),
    Range(u16, u16),
}

impl PortMatch {
    pub fn matches(self, port: u16) -> bool {
        match self {
            PortMatch::Eq(p) => port == p,
            PortMatch::Gt(p) => port > p,
            PortMatch::Lt(p) => port < p,
            PortMatch::Range(a, b) => (a..=b).contains(&port),
        }
    }

    pub fn ios(self) -> String {
        match self {
            PortMatch::Eq(p) => format!("eq {}", port_name(p)),
            PortMatch::Gt(p) => format!("gt {}", port_name(p)),
            PortMatch::Lt(p) => format!("lt {}", port_name(p)),
            PortMatch::Range(a, b) => format!("range {} {}", port_name(a), port_name(b)),
        }
    }
}

/// IOS shows well-known ports by name.
pub fn port_name(port: u16) -> String {
    match port {
        20 => "ftp-data".into(),
        21 => "ftp".into(),
        22 => "22".into(),
        23 => "telnet".into(),
        25 => "smtp".into(),
        53 => "domain".into(),
        67 => "bootps".into(),
        68 => "bootpc".into(),
        69 => "tftp".into(),
        80 => "www".into(),
        443 => "443".into(),
        p => p.to_string(),
    }
}

/// An ICMP message type an extended entry can match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IcmpMatch {
    Echo,
    EchoReply,
    Unreachable,
    TimeExceeded,
}

impl IcmpMatch {
    pub fn ios(self) -> &'static str {
        match self {
            IcmpMatch::Echo => "echo",
            IcmpMatch::EchoReply => "echo-reply",
            IcmpMatch::Unreachable => "unreachable",
            IcmpMatch::TimeExceeded => "time-exceeded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclEntry {
    /// 10, 20, 30… as IOS numbers them.
    pub sequence: u32,
    pub action: AclAction,
    /// Always `Ip` for standard lists.
    pub protocol: AclProtocol,
    pub source: AclAddress,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_port: Option<PortMatch>,
    /// `any` for standard lists.
    pub destination: AclAddress,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_port: Option<PortMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icmp: Option<IcmpMatch>,
}

impl AclEntry {
    /// The entry as the running-config writes it.
    pub fn config_line(&self, kind: AclKind) -> String {
        if kind == AclKind::Standard {
            let source = match self.source.ios() {
                s if s.starts_with("host ") => s[5..].to_string(),
                s => s,
            };
            return format!("{} {source}", self.action.ios());
        }
        self.ios(kind)
    }

    /// The entry as `show access-lists` writes it (without the sequence).
    pub fn ios(&self, kind: AclKind) -> String {
        let mut text = format!("{} ", self.action.ios());
        if kind == AclKind::Standard {
            text += &match self.source.ios().as_str() {
                // Standard lists show a bare address for `host`.
                s if s.starts_with("host ") => s[5..].to_string(),
                "any" => "any".to_string(),
                _ => format!(
                    "{}, wildcard bits {}",
                    self.source.address, self.source.wildcard
                ),
            };
            return text;
        }
        text += self.protocol.ios();
        text += &format!(" {}", self.source.ios());
        if let Some(p) = self.source_port {
            text += &format!(" {}", p.ios());
        }
        text += &format!(" {}", self.destination.ios());
        if let Some(p) = self.destination_port {
            text += &format!(" {}", p.ios());
        }
        if let Some(t) = self.icmp {
            text += &format!(" {}", t.ios());
        }
        text
    }
}

/// A numbered (`access-list 10 …`) or named (`ip access-list extended X`) list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessList {
    /// "10", "101" or a name.
    pub name: String,
    pub kind: AclKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<AclEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remarks: Vec<String>,
}

impl AccessList {
    pub fn is_numbered(&self) -> bool {
        self.name.chars().all(|c| c.is_ascii_digit())
    }

    /// The kind a list number means: 1–99 / 1300–1999 standard, 100–199 /
    /// 2000–2699 extended.
    pub fn kind_for_number(number: u32) -> Option<AclKind> {
        match number {
            1..=99 | 1300..=1999 => Some(AclKind::Standard),
            100..=199 | 2000..=2699 => Some(AclKind::Extended),
            _ => None,
        }
    }

    /// Whether the list already has this rule (whatever its sequence). IOS
    /// ignores an entry that repeats one already in the list.
    pub fn has_rule(&self, entry: &AclEntry) -> bool {
        self.entries.iter().any(|e| {
            AclEntry {
                sequence: entry.sequence,
                ..e.clone()
            } == *entry
        })
    }

    /// The next sequence number (10 past the last).
    pub fn next_sequence(&self) -> u32 {
        self.entries.iter().map(|e| e.sequence).max().unwrap_or(0) / 10 * 10 + 10
    }
}

/// Which side of the translation an interface is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NatSide {
    Inside,
    Outside,
}

impl NatSide {
    pub fn ios(self) -> &'static str {
        match self {
            NatSide::Inside => "inside",
            NatSide::Outside => "outside",
        }
    }
}

/// `ip nat pool NAME START END netmask M`: public addresses to hand out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NatPool {
    pub name: String,
    pub start: Ipv4Addr,
    pub end: Ipv4Addr,
    pub prefix_len: u8,
}

impl NatPool {
    /// Every address in the pool, in order.
    pub fn addresses(&self) -> impl Iterator<Item = Ipv4Addr> {
        (u32::from(self.start)..=u32::from(self.end)).map(Ipv4Addr::from)
    }

    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        (u32::from(self.start)..=u32::from(self.end)).contains(&u32::from(ip))
    }

    pub fn config_line(&self) -> String {
        format!(
            "ip nat pool {} {} {} netmask {}",
            self.name,
            self.start,
            self.end,
            crate::mask(self.prefix_len)
        )
    }
}

/// Where dynamic translations take their public address from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NatTarget {
    Pool(String),
    /// The address of an (outside) interface, by full name.
    Interface(String),
}

/// `ip nat inside source ...`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NatRule {
    /// `static LOCAL GLOBAL`: always the same public address, both ways.
    Static { local: Ipv4Addr, global: Ipv4Addr },
    /// `list ACL pool NAME | interface X [overload]`: addresses the list
    /// permits get a public address when they first send something.
    Dynamic {
        acl: String,
        target: NatTarget,
        overload: bool,
    },
}

impl NatRule {
    /// The rule as the running-config writes it (`interface` with the
    /// full name, as IOS does).
    pub fn config_line(&self) -> String {
        match self {
            NatRule::Static { local, global } => {
                format!("ip nat inside source static {local} {global}")
            }
            NatRule::Dynamic {
                acl,
                target,
                overload,
            } => {
                let target = match target {
                    NatTarget::Pool(p) => format!("pool {p}"),
                    NatTarget::Interface(i) => format!("interface {i}"),
                };
                let overload = if *overload { " overload" } else { "" };
                format!("ip nat inside source list {acl} {target}{overload}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_match_like_ios() {
        let net = AclAddress {
            address: "192.168.1.0".parse().unwrap(),
            wildcard: "0.0.0.255".parse().unwrap(),
        };
        assert!(net.matches("192.168.1.77".parse().unwrap()));
        assert!(!net.matches("192.168.2.1".parse().unwrap()));
        assert!(AclAddress::ANY.matches("8.8.8.8".parse().unwrap()));
        assert_eq!(net.ios(), "192.168.1.0 0.0.0.255");
        assert_eq!(
            AclAddress::host("10.0.0.1".parse().unwrap()).ios(),
            "host 10.0.0.1"
        );
        assert_eq!(AccessList::kind_for_number(10), Some(AclKind::Standard));
        assert_eq!(AccessList::kind_for_number(101), Some(AclKind::Extended));
        assert_eq!(AccessList::kind_for_number(200), None);
    }

    #[test]
    fn entries_print_like_ios() {
        let e = AclEntry {
            sequence: 10,
            action: AclAction::Deny,
            protocol: AclProtocol::Icmp,
            source: AclAddress::host("10.0.0.5".parse().unwrap()),
            source_port: None,
            destination: AclAddress::ANY,
            destination_port: None,
            icmp: Some(IcmpMatch::Echo),
        };
        assert_eq!(e.ios(AclKind::Extended), "deny icmp host 10.0.0.5 any echo");
        let pat = NatRule::Dynamic {
            acl: "1".into(),
            target: NatTarget::Interface("GigabitEthernet0/1".into()),
            overload: true,
        };
        assert_eq!(
            pat.config_line(),
            "ip nat inside source list 1 interface GigabitEthernet0/1 overload"
        );
        let pool = NatPool {
            name: "PUB".into(),
            start: "203.0.113.10".parse().unwrap(),
            end: "203.0.113.12".parse().unwrap(),
            prefix_len: 24,
        };
        assert_eq!(pool.addresses().count(), 3);
        assert_eq!(
            pool.config_line(),
            "ip nat pool PUB 203.0.113.10 203.0.113.12 netmask 255.255.255.0"
        );
        assert_eq!(
            AclEntry {
                protocol: AclProtocol::Ip,
                icmp: None,
                ..e.clone()
            }
            .ios(AclKind::Standard),
            "deny 10.0.0.5"
        );
    }
}
