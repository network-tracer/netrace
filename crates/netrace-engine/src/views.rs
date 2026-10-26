//! Read-only views for `show` commands.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netrace_doc::{DeviceId, Project};

use crate::stack::{PortStates, Route, Stack};
use crate::{MacAddr, Network};

/// One line of `show ip interface brief`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceRow {
    pub name: String,
    pub address: Option<Ipv4Addr>,
    pub prefix_len: Option<u8>,
    /// "up", "down" or "administratively down".
    pub status: &'static str,
    /// Line protocol: "up" or "down".
    pub protocol: &'static str,
    pub description: Option<String>,
    pub mac: Option<MacAddr>,
}

impl InterfaceRow {
    pub fn is_up(&self) -> bool {
        self.status == "up" && self.protocol == "up"
    }
}

/// Every interface of a router or switch with its state, physical ports
/// first in panel order, then the virtual ones from the configuration.
pub fn interface_rows(project: &Project, id: DeviceId) -> Vec<InterfaceRow> {
    let Some(d) = project.device(id) else {
        return Vec::new();
    };
    let (ports, l2) = crate::stp::analyse(project);
    let stack = Stack::build(project, id, &ports, &l2);
    let mac_of = |name: &str| {
        stack
            .as_ref()
            .and_then(|s| s.iface(name))
            .map(|i| i.mac)
            .filter(|m| *m != MacAddr::ZERO)
    };
    let mut rows = Vec::new();
    for (index, port) in d.kind.port_names().into_iter().enumerate() {
        let config = d.config.interface(d.kind, &port);
        let (status, protocol) = if config.shutdown {
            ("administratively down", "down")
        } else if ports.is_up(id, &port) {
            ("up", "up")
        } else {
            ("down", "down")
        };
        rows.push(InterfaceRow {
            mac: mac_of(&port).or(Some(MacAddr::for_port(id, index))),
            name: port,
            address: config.address.map(|a| a.address),
            prefix_len: config.address.map(|a| a.prefix_len),
            status,
            protocol,
            description: config.description,
        });
    }
    let mut virtual_names: Vec<&String> = d
        .config
        .interfaces
        .keys()
        .filter(|n| !d.kind.has_port(n))
        .collect();
    if d.kind.is_switch() && !d.config.interfaces.contains_key("Vlan1") {
        rows.push(vlan1_row(project, id, &ports, mac_of("Vlan1")));
    }
    virtual_names.sort_by_key(|n| natural_key(n));
    for name in virtual_names {
        if name == "Vlan1" {
            rows.push(vlan1_row(project, id, &ports, mac_of("Vlan1")));
            continue;
        }
        let config = d.config.interface(d.kind, name);
        let (status, protocol) = if config.shutdown {
            ("administratively down", "down")
        } else if name.starts_with("Loopback") {
            ("up", "up")
        } else if let Some((parent, _)) = name.split_once('.') {
            let parent_shut = d.config.interface(d.kind, parent).shutdown;
            if parent_shut {
                ("administratively down", "down")
            } else if ports.is_up(id, parent) {
                ("up", "up")
            } else {
                ("down", "down")
            }
        } else if stack
            .as_ref()
            .and_then(|s| s.iface(name))
            .is_some_and(|i| i.up)
        {
            ("up", "up")
        } else {
            // No port in the VLAN is up and forwarding (or the VLAN doesn't exist).
            ("up", "down")
        };
        rows.push(InterfaceRow {
            name: name.clone(),
            address: config.address.map(|a| a.address),
            prefix_len: config.address.map(|a| a.prefix_len),
            status,
            protocol,
            description: config.description,
            mac: mac_of(name),
        });
    }
    rows
}

fn vlan1_row(
    project: &Project,
    id: DeviceId,
    ports: &PortStates,
    mac: Option<MacAddr>,
) -> InterfaceRow {
    let d = project.device(id).expect("checked by caller");
    let config = d.config.interface(d.kind, "Vlan1");
    let _ = ports;
    let (ports_check, l2) = crate::stp::analyse(project);
    let any_up = Stack::build(project, id, &ports_check, &l2)
        .and_then(|s| s.iface("Vlan1").map(|i| i.up))
        .unwrap_or(false);
    let (status, protocol) = match (config.shutdown, any_up) {
        (true, _) => ("administratively down", "down"),
        (false, true) => ("up", "up"),
        (false, false) => ("up", "down"),
    };
    InterfaceRow {
        name: "Vlan1".into(),
        address: config.address.map(|a| a.address),
        prefix_len: config.address.map(|a| a.prefix_len),
        status,
        protocol,
        description: config.description,
        mac,
    }
}

/// The routes a device has installed, as `show ip route` lists them.
pub fn routing_table(project: &Project, id: DeviceId) -> Vec<Route> {
    let (ports, l2) = crate::stp::analyse(project);
    Stack::build(project, id, &ports, &l2)
        .map(|s| s.routes)
        .unwrap_or_default()
}

/// Sort key that orders interface names the way IOS lists them:
/// "GigabitEthernet0/10" after "GigabitEthernet0/2".
pub fn natural_key(name: &str) -> Vec<(String, u64)> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(digit) = c.to_digit(10) {
            let mut n = u64::from(digit);
            while let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                n = n.saturating_mul(10).saturating_add(u64::from(d));
                chars.next();
            }
            parts.push((std::mem::take(&mut text), n));
        } else {
            text.push(c);
        }
    }
    if !text.is_empty() {
        parts.push((text, 0));
    }
    parts
}

/// One line of `show arp` (IOS) / `show ip arp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArpRow {
    pub address: Ipv4Addr,
    /// Minutes since learned; `None` for the device's own addresses.
    pub age_minutes: Option<u64>,
    pub mac: MacAddr,
    pub iface: String,
}

/// One line of `show mac address-table`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacRow {
    pub vlan: u16,
    pub mac: MacAddr,
    pub port: String,
}

impl Network {
    /// The ARP cache as it stands after everything run so far, with the
    /// device's own addresses (as IOS lists them).
    pub fn arp_rows(&self, id: DeviceId) -> Vec<ArpRow> {
        let Some(stack) = self.stack(id) else {
            return Vec::new();
        };
        let end = self.end_time();
        let mut rows: Vec<ArpRow> = stack
            .arp
            .iter()
            .map(|(ip, e)| ArpRow {
                address: *ip,
                age_minutes: Some((end - e.learned).as_secs() / 60),
                mac: e.mac,
                iface: e.iface.clone(),
            })
            .collect();
        for iface in &stack.ifaces {
            if let (Some(a), true) = (iface.address, iface.up && iface.mac != MacAddr::ZERO) {
                rows.push(ArpRow {
                    address: a.address,
                    age_minutes: None,
                    mac: iface.mac,
                    iface: iface.name.clone(),
                });
            }
        }
        rows.sort_by_key(|r| u32::from(r.address));
        rows
    }

    /// Ports that are err-disabled now: by BPDU guard, or by a port
    /// security violation during the simulation.
    pub fn err_disabled_ports(&self, id: DeviceId) -> Vec<String> {
        let mut ports: Vec<String> = self
            .l2
            .err_disabled
            .iter()
            .chain(self.psec_disabled.iter())
            .filter(|(d, _)| *d == id)
            .map(|(_, p)| p.clone())
            .collect();
        ports.sort_by_key(|p| natural_key(p));
        ports.dedup();
        ports
    }

    /// Why a port is err-disabled, as IOS names it.
    pub fn err_disabled_reason(&self, id: DeviceId, port: &str) -> Option<&'static str> {
        let key = (id, port.to_string());
        if self.psec_disabled.contains(&key) {
            Some("psecure-violation")
        } else if self.l2.err_disabled.contains(&key) {
            Some("bpduguard")
        } else {
            None
        }
    }

    /// MACs port security has learned on a port, and its violation count.
    pub fn port_security_state(&self, id: DeviceId, port: &str) -> (Vec<MacAddr>, u32) {
        let key = (id, port.to_string());
        (
            self.secure.get(&key).cloned().unwrap_or_default(),
            self.violations.get(&key).copied().unwrap_or(0),
        )
    }

    /// How often each ACL line has matched, by (list, sequence).
    pub fn acl_hits(&self, id: DeviceId) -> BTreeMap<(String, u32), u32> {
        self.acl_hits
            .iter()
            .filter(|((d, _, _), _)| *d == id)
            .map(|((_, acl, seq), n)| ((acl.clone(), *seq), *n))
            .collect()
    }

    /// VLAN membership and spanning tree as the simulation uses them.
    pub fn layer2(&self) -> &crate::Layer2 {
        &self.l2
    }

    /// The MAC addresses a switch has learned, by port.
    pub fn mac_rows(&self, id: DeviceId) -> Vec<MacRow> {
        let Some(table) = self.mac_table(id) else {
            return Vec::new();
        };
        let mut rows: Vec<MacRow> = table
            .iter()
            .filter(|(_, port)| !port.starts_with("Vlan"))
            .map(|((vlan, mac), port)| MacRow {
                vlan: *vlan,
                mac: *mac,
                port: port.clone(),
            })
            .collect();
        // IOS lists by VLAN, then address.
        rows.sort_by(|a, b| a.vlan.cmp(&b.vlan).then(a.mac.cmp(&b.mac)));
        rows
    }
}

/// One line of `show cdp neighbors` / `show lldp neighbors`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    pub device: DeviceId,
    pub name: String,
    pub local_port: String,
    pub remote_port: String,
    /// CDP capability codes: R router, S switch, I IGMP, H host, P phone.
    pub capability: &'static str,
    pub platform: &'static str,
    /// The neighbour's address on the shared link, if it has one.
    pub address: Option<Ipv4Addr>,
}

/// What a device advertises over CDP/LLDP, if it runs them.
fn discovery_identity(kind: netrace_doc::DeviceKind) -> Option<(&'static str, &'static str)> {
    use netrace_doc::DeviceKind::*;
    match kind {
        Router => Some(("R", "ISR4331")),
        L2Switch => Some(("S I", "WS-C2960")),
        L3Switch => Some(("R S I", "WS-C3650")),
        IpPhone => Some(("H P", "IP Phone")),
        AccessPoint => Some(("T", "AIR-AP")),
        _ => None,
    }
}

/// CDP neighbours (Cisco Discovery Protocol: on by default) or LLDP
/// neighbours (off until `lldp run` on both ends). Found from the cabling,
/// as the protocols would after their first advertisement.
pub fn discovery_neighbors(project: &Project, id: DeviceId, lldp: bool) -> Vec<Neighbor> {
    let Some(me) = project.device(id) else {
        return Vec::new();
    };
    let runs = |d: &netrace_doc::Device| {
        if !d.kind.has_cli() {
            // Phones and access points always speak CDP, never LLDP here.
            return !lldp && discovery_identity(d.kind).is_some();
        }
        if lldp {
            d.config.lldp_run
        } else {
            d.config.cdp_run
        }
    };
    let port_enabled = |d: &netrace_doc::Device, port: &str| {
        let config = d.config.interface(d.kind, port);
        !config.shutdown && (lldp || !config.switchport.cdp_disabled)
    };
    if !runs(me) {
        return Vec::new();
    }
    let rows = interface_rows(project, id);
    let mut out = Vec::new();
    for link in project.links() {
        let (mine, theirs) = if link.a.device == id {
            (&link.a, &link.b)
        } else if link.b.device == id {
            (&link.b, &link.a)
        } else {
            continue;
        };
        let Some(peer) = project.device(theirs.device) else {
            continue;
        };
        let Some((capability, platform)) = discovery_identity(peer.kind) else {
            continue;
        };
        if !runs(peer) || !port_enabled(me, &mine.port) || !port_enabled(peer, &theirs.port) {
            continue;
        }
        if me.kind.has_cli() && !rows.iter().any(|r| r.name == mine.port && r.is_up()) {
            continue;
        }
        let address = peer
            .config
            .interface(peer.kind, &theirs.port)
            .address
            .map(|a| a.address)
            .or_else(|| {
                peer.config
                    .interface(peer.kind, "Vlan1")
                    .address
                    .map(|a| a.address)
            });
        out.push(Neighbor {
            device: peer.id,
            name: peer.name.clone(),
            local_port: mine.port.clone(),
            remote_port: theirs.port.clone(),
            capability,
            platform,
            address,
        });
    }
    out.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(natural_key(&a.local_port).cmp(&natural_key(&b.local_port)))
    });
    out
}
