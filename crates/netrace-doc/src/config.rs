//! Configuration of routers and switches, as built up in their command line.
//!
//! This is the structured form of a running-config: the CLI edits it, the
//! engine simulates it and `show running-config` renders it. It is saved in
//! the project, together with the startup-config saved by `copy run start`.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::{AccessList, DeviceKind, DhcpPool, DocError, StpConfig, SwitchportConfig, VlanConfig};

/// An address and prefix configured on an interface (`ip address A M`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InterfaceAddress {
    pub address: Ipv4Addr,
    pub prefix_len: u8,
}

impl InterfaceAddress {
    pub fn mask(&self) -> Ipv4Addr {
        mask(self.prefix_len)
    }

    pub fn network(&self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.address) & u32::from(self.mask()))
    }

    pub fn broadcast(&self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.network()) | !u32::from(self.mask()))
    }

    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        u32::from(ip) & u32::from(self.mask()) == u32::from(self.network())
    }
}

/// The netmask for a prefix length (0–32).
pub fn mask(prefix_len: u8) -> Ipv4Addr {
    Ipv4Addr::from(
        u32::MAX
            .checked_shl(32 - u32::from(prefix_len.min(32)))
            .unwrap_or(0),
    )
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<InterfaceAddress>,
    /// Administratively down (`shutdown`).
    pub shutdown: bool,
    /// 802.1Q VLAN of a subinterface (`encapsulation dot1Q N`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encapsulation: Option<u16>,
    /// `encapsulation dot1Q N native`: this subinterface takes untagged frames.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub native: bool,
    /// Layer 2 settings of a switch port.
    #[serde(skip_serializing_if = "SwitchportConfig::is_default")]
    pub switchport: SwitchportConfig,
    /// `ip helper-address`: DHCP relay to these servers.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub helper_addresses: Vec<Ipv4Addr>,
    /// `ip access-group X in`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acl_in: Option<String>,
    /// `ip access-group X out`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acl_out: Option<String>,
}

/// Where a static route sends traffic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteTarget {
    NextHop(Ipv4Addr),
    /// Directly out an interface: the router ARPs for each destination.
    Interface(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StaticRoute {
    pub network: Ipv4Addr,
    pub prefix_len: u8,
    pub target: RouteTarget,
    /// Administrative distance; above 1 makes a floating static route.
    #[serde(default = "default_distance")]
    pub distance: u8,
}

fn default_distance() -> u8 {
    1
}

impl StaticRoute {
    pub fn mask(&self) -> Ipv4Addr {
        mask(self.prefix_len)
    }
}

/// `line con 0`, `line vty 0 4` and their settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LineConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    pub login: bool,
    pub logging_synchronous: bool,
    /// Minutes and seconds (`exec-timeout 5 0`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec_timeout: Option<(u16, u16)>,
}

/// A router's or switch's configuration. Interfaces not listed have their
/// defaults: see [`DeviceConfig::interface`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceConfig {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub interfaces: BTreeMap<String, InterfaceConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<StaticRoute>,
    /// `ip default-gateway` (switches, for management traffic).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_gateway: Option<Ipv4Addr>,
    /// `ip routing` on a layer 3 switch (routers always route).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ip_routing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_password: Option<String>,
    #[serde(skip_serializing_if = "is_true")]
    pub domain_lookup: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub password_encryption: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner_motd: Option<String>,
    /// Keyed "con 0", "vty 0 4".
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub lines: BTreeMap<String, LineConfig>,
    /// VLANs created on a switch (VLAN 1 always exists).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub vlans: BTreeMap<u16, VlanConfig>,
    #[serde(skip_serializing_if = "StpConfig::is_default")]
    pub spanning_tree: StpConfig,
    /// `cdp run` (on by default on Cisco devices).
    #[serde(skip_serializing_if = "is_true")]
    pub cdp_run: bool,
    /// `lldp run` (off by default).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub lldp_run: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dhcp_pools: Vec<DhcpPool>,
    /// `ip dhcp excluded-address A [B]`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dhcp_excluded: Vec<(Ipv4Addr, Ipv4Addr)>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub access_lists: Vec<AccessList>,
}

fn is_true(value: &bool) -> bool {
    *value
}

impl Default for DeviceConfig {
    fn default() -> Self {
        DeviceConfig {
            interfaces: BTreeMap::new(),
            routes: Vec::new(),
            default_gateway: None,
            ip_routing: false,
            enable_secret: None,
            enable_password: None,
            domain_lookup: true,
            password_encryption: false,
            banner_motd: None,
            lines: BTreeMap::new(),
            vlans: BTreeMap::new(),
            spanning_tree: StpConfig::default(),
            cdp_run: true,
            lldp_run: false,
            dhcp_pools: Vec::new(),
            dhcp_excluded: Vec::new(),
            access_lists: Vec::new(),
        }
    }
}

impl DeviceConfig {
    pub fn is_default(&self) -> bool {
        *self == DeviceConfig::default()
    }

    /// VLAN 1 and every created VLAN, in order.
    pub fn vlan_ids(&self) -> Vec<u16> {
        let mut ids: Vec<u16> = std::iter::once(1)
            .chain(self.vlans.keys().copied())
            .collect();
        ids.dedup();
        ids
    }

    pub fn access_list(&self, name: &str) -> Option<&AccessList> {
        self.access_lists
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
    }

    pub fn dhcp_excluded(&self, ip: Ipv4Addr) -> bool {
        self.dhcp_excluded
            .iter()
            .any(|(a, b)| (u32::from(*a)..=u32::from(*b)).contains(&u32::from(ip)))
    }

    pub fn vlan_exists(&self, id: u16) -> bool {
        id == 1 || self.vlans.contains_key(&id)
    }

    pub fn vlan_name(&self, id: u16) -> String {
        self.vlans
            .get(&id)
            .and_then(|v| v.name.clone())
            .unwrap_or_else(|| VlanConfig::default_name(id))
    }

    /// An interface's settings, or its factory defaults: router ports and
    /// `Vlan` interfaces start shut down; switch ports, loopbacks and
    /// subinterfaces start up.
    pub fn interface(&self, kind: DeviceKind, name: &str) -> InterfaceConfig {
        self.interfaces
            .get(name)
            .cloned()
            .unwrap_or_else(|| InterfaceConfig {
                shutdown: kind.interface_starts_shut(name),
                ..InterfaceConfig::default()
            })
    }

    /// Mutable access, creating the interface with its defaults.
    pub fn interface_mut(&mut self, kind: DeviceKind, name: &str) -> &mut InterfaceConfig {
        self.interfaces
            .entry(name.to_string())
            .or_insert_with(|| InterfaceConfig {
                shutdown: kind.interface_starts_shut(name),
                ..InterfaceConfig::default()
            })
    }

    /// Checks interface names and routes against the device kind.
    pub(crate) fn validate(&self, kind: DeviceKind) -> Result<(), DocError> {
        for name in self.interfaces.keys() {
            if !kind.accepts_interface(name) {
                return Err(DocError::InvalidConfig(format!(
                    "{kind:?} has no interface named \"{name}\""
                )));
            }
        }
        for &id in self.vlans.keys() {
            if !(1..=4094).contains(&id) {
                return Err(DocError::InvalidConfig(format!(
                    "VLAN {id} is out of range"
                )));
            }
        }
        for (&vlan, &priority) in &self.spanning_tree.priorities {
            if priority > 61440 || priority % 4096 != 0 || !(1..=4094).contains(&vlan) {
                return Err(DocError::InvalidConfig(format!(
                    "spanning-tree priority {priority} for VLAN {vlan} is invalid"
                )));
            }
        }
        for route in &self.routes {
            if route.prefix_len > 32 {
                return Err(DocError::InvalidConfig(format!(
                    "route prefix /{} is longer than 32 bits",
                    route.prefix_len
                )));
            }
            if u32::from(route.network) & !u32::from(route.mask()) != 0 {
                return Err(DocError::InvalidConfig(format!(
                    "route {}/{} has host bits set",
                    route.network, route.prefix_len
                )));
            }
        }
        Ok(())
    }
}

/// What `copy running-config startup-config` saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupConfig {
    pub hostname: String,
    pub config: DeviceConfig,
}

impl DeviceKind {
    /// Routers and switches have a command line and a configuration.
    pub fn has_cli(self) -> bool {
        matches!(
            self,
            DeviceKind::Router | DeviceKind::L2Switch | DeviceKind::L3Switch
        )
    }

    pub fn is_switch(self) -> bool {
        matches!(self, DeviceKind::L2Switch | DeviceKind::L3Switch)
    }

    /// Whether `name` (a full interface name) can exist on this device:
    /// a physical port, `LoopbackN` (routers and L3 switches), `VlanN`
    /// (switches) or a subinterface `port.N` (routers).
    pub fn accepts_interface(self, name: &str) -> bool {
        if self.has_port(name) {
            return true;
        }
        let number = |prefix: &str| {
            name.strip_prefix(prefix)
                .and_then(|n| n.parse::<u32>().ok())
                .filter(|n| n.to_string() == name[prefix.len()..])
        };
        match self {
            DeviceKind::Router => {
                number("Loopback").is_some()
                    || name.split_once('.').is_some_and(|(port, sub)| {
                        self.has_port(port)
                            && sub.parse::<u32>().is_ok_and(|n| n > 0)
                            && !sub.starts_with('0')
                    })
            }
            DeviceKind::L3Switch => {
                number("Loopback").is_some()
                    || number("Vlan").is_some_and(|v| (1..=4094).contains(&v))
            }
            DeviceKind::L2Switch => number("Vlan").is_some_and(|v| (1..=4094).contains(&v)),
            _ => false,
        }
    }

    pub(crate) fn interface_starts_shut(self, name: &str) -> bool {
        if name.starts_with("Vlan") {
            return true;
        }
        self == DeviceKind::Router && self.has_port(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_names_follow_the_device() {
        let r = DeviceKind::Router;
        assert!(r.accepts_interface("GigabitEthernet0/0"));
        assert!(r.accepts_interface("GigabitEthernet0/1.10"));
        assert!(r.accepts_interface("Loopback0"));
        assert!(!r.accepts_interface("GigabitEthernet0/1.0"));
        assert!(!r.accepts_interface("GigabitEthernet0/9.10"));
        assert!(!r.accepts_interface("Vlan1"));
        assert!(!r.accepts_interface("Loopback01"));
        let sw = DeviceKind::L2Switch;
        assert!(sw.accepts_interface("Vlan1"));
        assert!(sw.accepts_interface("FastEthernet0/24"));
        assert!(!sw.accepts_interface("Vlan4095"));
        assert!(!sw.accepts_interface("Loopback0"));
        assert!(DeviceKind::L3Switch.accepts_interface("Loopback1"));
    }

    #[test]
    fn defaults_match_ios() {
        let c = DeviceConfig::default();
        assert!(
            c.interface(DeviceKind::Router, "GigabitEthernet0/0")
                .shutdown
        );
        assert!(
            !c.interface(DeviceKind::L2Switch, "FastEthernet0/1")
                .shutdown
        );
        assert!(c.interface(DeviceKind::L2Switch, "Vlan1").shutdown);
        assert!(!c.interface(DeviceKind::Router, "Loopback0").shutdown);
        assert!(
            !c.interface(DeviceKind::Router, "GigabitEthernet0/0.10")
                .shutdown
        );
        assert!(c.is_default());
        assert_eq!(serde_json::to_string(&c).unwrap(), "{}");
    }

    #[test]
    fn address_math() {
        let a = InterfaceAddress {
            address: "10.1.2.3".parse().unwrap(),
            prefix_len: 30,
        };
        assert_eq!(a.network(), "10.1.2.0".parse::<Ipv4Addr>().unwrap());
        assert_eq!(a.broadcast(), "10.1.2.3".parse::<Ipv4Addr>().unwrap());
        assert!(a.contains("10.1.2.1".parse().unwrap()));
        assert!(!a.contains("10.1.2.4".parse().unwrap()));
        assert_eq!(mask(0), Ipv4Addr::UNSPECIFIED);
        assert_eq!(mask(32), Ipv4Addr::BROADCAST);
    }
}
