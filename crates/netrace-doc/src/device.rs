use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Router,
    L2Switch,
    L3Switch,
    Pc,
    Server,
}

impl DeviceKind {
    pub const ALL: [DeviceKind; 5] = [
        DeviceKind::Router,
        DeviceKind::L2Switch,
        DeviceKind::L3Switch,
        DeviceKind::Pc,
        DeviceKind::Server,
    ];

    /// Prefix for auto-generated hostnames, following CCNA lab conventions (R1, SW1, …).
    pub fn name_prefix(self) -> &'static str {
        match self {
            DeviceKind::Router => "R",
            DeviceKind::L2Switch => "SW",
            DeviceKind::L3Switch => "MLS",
            DeviceKind::Pc => "PC",
            DeviceKind::Server => "SRV",
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
            DeviceKind::Pc => vec!["FastEthernet0".into()],
            DeviceKind::Server => vec!["GigabitEthernet0".into()],
        }
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
