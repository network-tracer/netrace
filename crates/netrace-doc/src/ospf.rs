//! OSPFv2 configuration: `router ospf` processes and per-interface settings.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

/// `network A W area N` under `router ospf`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OspfNetwork {
    pub address: Ipv4Addr,
    pub wildcard: Ipv4Addr,
    pub area: u32,
}

impl OspfNetwork {
    pub fn matches(&self, ip: Ipv4Addr) -> bool {
        let care = !u32::from(self.wildcard);
        u32::from(ip) & care == u32::from(self.address) & care
    }

    /// Wildcard bits set: fewer is more specific.
    pub fn width(&self) -> u32 {
        u32::from(self.wildcard).count_ones()
    }
}

fn default_reference_bandwidth() -> u32 {
    100
}

fn is_default_reference_bandwidth(value: &u32) -> bool {
    *value == default_reference_bandwidth()
}

/// `router ospf N` and its settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OspfProcess {
    pub process_id: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub router_id: Option<Ipv4Addr>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub networks: Vec<OspfNetwork>,
    /// `passive-interface X` (full names).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passive: Vec<String>,
    /// `passive-interface default`; `no passive-interface X` lands in
    /// `active`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub passive_default: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active: Vec<String>,
    /// `default-information originate`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default_originate: bool,
    /// `auto-cost reference-bandwidth N` in Mbit/s (100 by default).
    #[serde(
        default = "default_reference_bandwidth",
        skip_serializing_if = "is_default_reference_bandwidth"
    )]
    pub reference_bandwidth: u32,
}

impl OspfProcess {
    pub fn new(process_id: u16) -> Self {
        OspfProcess {
            process_id,
            router_id: None,
            networks: Vec::new(),
            passive: Vec::new(),
            passive_default: false,
            active: Vec::new(),
            default_originate: false,
            reference_bandwidth: default_reference_bandwidth(),
        }
    }

    pub fn is_passive(&self, iface: &str) -> bool {
        if self.passive_default {
            !self.active.iter().any(|a| a == iface)
        } else {
            self.passive.iter().any(|p| p == iface)
        }
    }

    /// The `network` statement that puts `ip` in OSPF: the most specific one,
    /// as IOS picks it.
    pub fn network_for(&self, ip: Ipv4Addr) -> Option<&OspfNetwork> {
        self.networks
            .iter()
            .filter(|n| n.matches(ip))
            .min_by_key(|n| n.width())
    }
}

/// OSPF settings on one interface (all default to IOS's values).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OspfInterface {
    /// `ip ospf cost N`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<u32>,
    /// `ip ospf priority N` (1 by default; 0 never becomes DR or BDR).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    /// `ip ospf hello-interval N` (10 s by default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hello: Option<u16>,
    /// `ip ospf dead-interval N` (four hellos by default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead: Option<u16>,
    /// `ip ospf PROCESS area N`, instead of a `network` statement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area: Option<(u16, u32)>,
    /// `ip ospf network point-to-point`: no DR election.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub point_to_point: bool,
}

impl OspfInterface {
    pub fn is_default(&self) -> bool {
        *self == OspfInterface::default()
    }

    pub fn hello_interval(&self) -> u16 {
        self.hello.unwrap_or(10)
    }

    pub fn dead_interval(&self) -> u16 {
        self.dead.unwrap_or(self.hello_interval() * 4)
    }

    pub fn priority(&self) -> u8 {
        self.priority.unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_specific_network_statement_wins() {
        let mut p = OspfProcess::new(1);
        p.networks.push(OspfNetwork {
            address: "10.0.0.0".parse().unwrap(),
            wildcard: "0.255.255.255".parse().unwrap(),
            area: 1,
        });
        p.networks.push(OspfNetwork {
            address: "10.0.12.0".parse().unwrap(),
            wildcard: "0.0.0.255".parse().unwrap(),
            area: 0,
        });
        assert_eq!(p.network_for("10.0.12.1".parse().unwrap()).unwrap().area, 0);
        assert_eq!(p.network_for("10.9.9.9".parse().unwrap()).unwrap().area, 1);
        assert!(p.network_for("192.168.1.1".parse().unwrap()).is_none());
        p.passive_default = true;
        p.active.push("GigabitEthernet0/1".into());
        assert!(p.is_passive("GigabitEthernet0/0"));
        assert!(!p.is_passive("GigabitEthernet0/1"));
        let i = OspfInterface {
            hello: Some(5),
            ..OspfInterface::default()
        };
        assert_eq!(i.dead_interval(), 20);
    }
}
