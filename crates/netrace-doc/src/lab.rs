//! A lab: instructions and automatic checks that travel with a project.
//! Devices are named in checks (as the student sees them), so renaming a
//! device a check refers to makes that check fail with a clear message.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lab {
    pub title: String,
    /// CCNA topic, for grouping: "Switching", "Routing", "Services", ...
    #[serde(default)]
    pub topic: String,
    /// What the lab is about and what to do, in Markdown.
    pub brief: String,
    pub tasks: Vec<LabTask>,
}

/// One step of a lab: done when all its checks pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabTask {
    pub title: String,
    /// Markdown; optional detail under the title.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// Shown on request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub checks: Vec<LabCheck>,
}

fn yes() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

/// Something about the simulated network that must be true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "check", rename_all = "snake_case")]
pub enum LabCheck {
    /// A cable joins two devices (on given ports, if named).
    Cabled {
        a: String,
        b: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        a_port: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        b_port: Option<String>,
    },
    /// The interface is up/up.
    InterfaceUp { device: String, interface: String },
    /// An interface (or an end device, with no interface named) has this address.
    Address {
        device: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        interface: Option<String>,
        address: Ipv4Addr,
        prefix_len: u8,
    },
    /// An end device's default gateway.
    Gateway { device: String, gateway: Ipv4Addr },
    /// A ping from a device to a device (by name) or address gets replies
    /// (or, with `succeeds: false`, doesn't).
    Ping {
        from: String,
        to: String,
        #[serde(default = "yes", skip_serializing_if = "is_true")]
        succeeds: bool,
    },
    /// The routing table has the prefix (from a given source, if named:
    /// "connected", "static", "ospf").
    Route {
        device: String,
        network: Ipv4Addr,
        prefix_len: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    },
    /// A VLAN exists on a switch (with a name, if given).
    Vlan {
        device: String,
        vlan: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// A switch port is an access port in a VLAN.
    AccessPort {
        device: String,
        port: String,
        vlan: u16,
    },
    /// A switch port is a trunk.
    Trunk { device: String, port: String },
    /// A router has this many full OSPF adjacencies.
    OspfNeighbors { device: String, count: u32 },
    /// The spanning-tree root bridge of a VLAN.
    StpRoot { device: String, vlan: u16 },
    /// An end device got its address from DHCP (not 169.254).
    DhcpLease { device: String },
    /// The running-config has a line containing this text.
    Config { device: String, contains: String },
}
