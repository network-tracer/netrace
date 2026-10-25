use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    // Network devices
    Router,
    L2Switch,
    L3Switch,
    Hub,
    Firewall,
    AccessPoint,
    /// An ISP / internet attachment point.
    Cloud,
    // End devices
    Pc,
    Laptop,
    Server,
    Printer,
    IpPhone,
    /// Wireless only, so it has no wired ports.
    Smartphone,
}

impl DeviceKind {
    pub const ALL: [DeviceKind; 13] = [
        DeviceKind::Router,
        DeviceKind::L2Switch,
        DeviceKind::L3Switch,
        DeviceKind::Hub,
        DeviceKind::Firewall,
        DeviceKind::AccessPoint,
        DeviceKind::Cloud,
        DeviceKind::Pc,
        DeviceKind::Laptop,
        DeviceKind::Server,
        DeviceKind::Printer,
        DeviceKind::IpPhone,
        DeviceKind::Smartphone,
    ];

    /// Prefix for auto-generated hostnames, following CCNA lab conventions (R1, SW1, …).
    pub fn name_prefix(self) -> &'static str {
        match self {
            DeviceKind::Router => "R",
            DeviceKind::L2Switch => "SW",
            DeviceKind::L3Switch => "MLS",
            DeviceKind::Hub => "HUB",
            DeviceKind::Firewall => "FW",
            DeviceKind::AccessPoint => "AP",
            DeviceKind::Cloud => "ISP",
            DeviceKind::Pc => "PC",
            DeviceKind::Laptop => "LAPTOP",
            DeviceKind::Server => "SRV",
            DeviceKind::Printer => "PRN",
            DeviceKind::IpPhone => "PHONE",
            DeviceKind::Smartphone => "MOBILE",
        }
    }

    /// Full interface names of the device's physical ports, in panel order.
    pub fn port_names(self) -> Vec<String> {
        match self {
            DeviceKind::Router => (0..4).map(|n| format!("GigabitEthernet0/{n}")).collect(),
            DeviceKind::L2Switch => (1..=24)
                .map(|n| format!("FastEthernet0/{n}"))
                .chain((1..=2).map(|n| format!("GigabitEthernet0/{n}")))
                .collect(),
            DeviceKind::L3Switch => (1..=24)
                .map(|n| format!("GigabitEthernet1/0/{n}"))
                .collect(),
            DeviceKind::Hub => (1..=8).map(|n| format!("Port{n}")).collect(),
            DeviceKind::Firewall => (1..=8).map(|n| format!("GigabitEthernet1/{n}")).collect(),
            DeviceKind::AccessPoint => vec!["GigabitEthernet0".into()],
            DeviceKind::Cloud => (0..4).map(|n| format!("Ethernet{n}")).collect(),
            DeviceKind::Pc | DeviceKind::Laptop | DeviceKind::Printer => {
                vec!["FastEthernet0".into()]
            }
            DeviceKind::Server => vec!["GigabitEthernet0".into()],
            // Cisco-style phones: uplink to the switch, pass-through to a PC.
            DeviceKind::IpPhone => vec!["Switch".into(), "PC".into()],
            DeviceKind::Smartphone => Vec::new(),
        }
    }

    /// End devices with a wired NIC: they take an IP configuration directly
    /// (routers and switches get addresses per interface in the CLI).
    pub fn is_host(self) -> bool {
        matches!(
            self,
            DeviceKind::Pc
                | DeviceKind::Laptop
                | DeviceKind::Server
                | DeviceKind::Printer
                | DeviceKind::IpPhone
        )
    }

    pub fn has_port(self, port: &str) -> bool {
        self.port_names().iter().any(|p| p == port)
    }
}

/// Physical medium of a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Medium {
    /// Twisted-pair copper. Ports auto-negotiate MDI/MDI-X, so there is no
    /// straight-through vs crossover distinction.
    Copper,
    Fiber,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_counts_match_scope() {
        assert_eq!(DeviceKind::Router.port_names().len(), 4);
        assert_eq!(DeviceKind::L2Switch.port_names().len(), 26);
        assert_eq!(DeviceKind::L3Switch.port_names().len(), 24);
        assert_eq!(DeviceKind::Pc.port_names(), ["FastEthernet0"]);
        assert_eq!(DeviceKind::Server.port_names(), ["GigabitEthernet0"]);
        assert_eq!(DeviceKind::Hub.port_names().len(), 8);
        assert_eq!(DeviceKind::Firewall.port_names().len(), 8);
        assert_eq!(DeviceKind::Cloud.port_names().len(), 4);
        assert_eq!(DeviceKind::IpPhone.port_names(), ["Switch", "PC"]);
        assert!(DeviceKind::Smartphone.port_names().is_empty());
    }

    #[test]
    fn name_prefixes_are_unique() {
        let mut prefixes: Vec<_> = DeviceKind::ALL.iter().map(|k| k.name_prefix()).collect();
        prefixes.sort_unstable();
        prefixes.dedup();
        assert_eq!(prefixes.len(), DeviceKind::ALL.len());
    }

    #[test]
    fn switch_ports_are_in_panel_order() {
        let ports = DeviceKind::L2Switch.port_names();
        assert_eq!(ports.first().unwrap(), "FastEthernet0/1");
        assert_eq!(ports.last().unwrap(), "GigabitEthernet0/2");
    }

    #[test]
    fn has_port_rejects_unknown_names() {
        assert!(DeviceKind::Router.has_port("GigabitEthernet0/3"));
        assert!(!DeviceKind::Router.has_port("GigabitEthernet0/4"));
        assert!(!DeviceKind::Pc.has_port("Gi0"));
    }
}
