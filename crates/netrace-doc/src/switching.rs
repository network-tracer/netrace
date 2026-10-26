//! Switching configuration: VLANs, access and trunk ports, spanning tree,
//! port security, CDP and LLDP.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The VLANs a switch knows (`vlan 10` / `name Sales`). VLAN 1 always
/// exists and isn't stored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VlanConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl VlanConfig {
    /// "VLAN0010": the name IOS gives a VLAN until it's named.
    pub fn default_name(id: u16) -> String {
        if id == 1 {
            "default".into()
        } else {
            format!("VLAN{id:04}")
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortMode {
    /// The factory default (`dynamic auto`). With no DTP partner it acts
    /// as an access port, which is all netrace simulates of DTP.
    #[default]
    DynamicAuto,
    Access,
    Trunk,
}

/// A set of VLANs, as `switchport trunk allowed vlan 10,20-30` writes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VlanSet(pub Vec<(u16, u16)>);

impl VlanSet {
    pub fn all() -> Self {
        VlanSet(vec![(1, 4094)])
    }

    pub fn none() -> Self {
        VlanSet(Vec::new())
    }

    pub fn is_all(&self) -> bool {
        self.contains_range(1, 4094)
    }

    pub fn contains(&self, vlan: u16) -> bool {
        self.0.iter().any(|(a, b)| (*a..=*b).contains(&vlan))
    }

    fn contains_range(&self, from: u16, to: u16) -> bool {
        (from..=to).all(|v| self.contains(v))
    }

    /// Parses "10,20-30" (IOS list syntax). `None` if malformed.
    pub fn parse(text: &str) -> Option<Self> {
        let mut ranges = Vec::new();
        for part in text.split(',') {
            let part = part.trim();
            let (a, b) = match part.split_once('-') {
                Some((a, b)) => (a.trim().parse().ok()?, b.trim().parse().ok()?),
                None => {
                    let v = part.parse().ok()?;
                    (v, v)
                }
            };
            if a == 0 || b > 4094 || a > b {
                return None;
            }
            ranges.push((a, b));
        }
        Some(VlanSet(ranges).normalised())
    }

    pub fn union(&self, other: &VlanSet) -> VlanSet {
        let mut ranges = self.0.clone();
        ranges.extend(other.0.iter().copied());
        VlanSet(ranges).normalised()
    }

    pub fn minus(&self, other: &VlanSet) -> VlanSet {
        let kept: Vec<u16> = (1..=4094)
            .filter(|v| self.contains(*v) && !other.contains(*v))
            .collect();
        VlanSet::from_ids(&kept)
    }

    fn from_ids(ids: &[u16]) -> VlanSet {
        let mut ranges: Vec<(u16, u16)> = Vec::new();
        for &v in ids {
            match ranges.last_mut() {
                Some((_, end)) if *end + 1 == v => *end = v,
                _ => ranges.push((v, v)),
            }
        }
        VlanSet(ranges)
    }

    fn normalised(self) -> VlanSet {
        let mut ranges = self.0;
        ranges.sort_unstable();
        let mut merged: Vec<(u16, u16)> = Vec::new();
        for (a, b) in ranges {
            match merged.last_mut() {
                Some((_, end)) if a <= end.saturating_add(1) => *end = (*end).max(b),
                _ => merged.push((a, b)),
            }
        }
        VlanSet(merged)
    }

    /// "1-4094", "10,20-30", "none".
    pub fn to_ios(&self) -> String {
        if self.0.is_empty() {
            return "none".into();
        }
        self.0
            .iter()
            .map(|(a, b)| {
                if a == b {
                    a.to_string()
                } else {
                    format!("{a}-{b}")
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl Default for VlanSet {
    fn default() -> Self {
        VlanSet::all()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Violation {
    /// Disable the port (err-disabled). The default.
    #[default]
    Shutdown,
    /// Drop the frames and count them, log a message.
    Restrict,
    /// Drop the frames silently.
    Protect,
}

impl Violation {
    pub fn ios(self) -> &'static str {
        match self {
            Violation::Shutdown => "shutdown",
            Violation::Restrict => "restrict",
            Violation::Protect => "protect",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortSecurity {
    /// `switchport port-security`: the other settings only apply once on.
    pub enabled: bool,
    pub maximum: u16,
    /// Learned addresses become secure (`mac-address sticky`).
    pub sticky: bool,
    pub violation: Violation,
    /// Configured secure MACs, "aaaa.bbbb.cccc".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub static_macs: Vec<String>,
}

impl Default for PortSecurity {
    fn default() -> Self {
        PortSecurity {
            enabled: false,
            maximum: 1,
            sticky: false,
            violation: Violation::Shutdown,
            static_macs: Vec::new(),
        }
    }
}

/// How a switch port behaves at layer 2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SwitchportConfig {
    pub mode: PortMode,
    pub access_vlan: u16,
    pub native_vlan: u16,
    pub allowed_vlans: VlanSet,
    /// `switchport nonegotiate` (no DTP; accepted and shown).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub nonegotiate: bool,
    /// `no switchport` on a layer 3 switch: a routed port.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub routed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_security: Option<PortSecurity>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub portfast: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bpdu_guard: bool,
    /// `no cdp enable`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cdp_disabled: bool,
}

impl Default for SwitchportConfig {
    fn default() -> Self {
        SwitchportConfig {
            mode: PortMode::DynamicAuto,
            access_vlan: 1,
            native_vlan: 1,
            allowed_vlans: VlanSet::all(),
            nonegotiate: false,
            routed: false,
            port_security: None,
            portfast: false,
            bpdu_guard: false,
            cdp_disabled: false,
        }
    }
}

impl SwitchportConfig {
    pub fn is_default(&self) -> bool {
        *self == SwitchportConfig::default()
    }

    pub fn is_trunk(&self) -> bool {
        self.mode == PortMode::Trunk
    }

    /// The VLAN untagged frames belong to on this port.
    pub fn untagged_vlan(&self) -> u16 {
        if self.is_trunk() {
            self.native_vlan
        } else {
            self.access_vlan
        }
    }

    /// Whether frames of `vlan` cross this port.
    pub fn carries(&self, vlan: u16) -> bool {
        if self.is_trunk() {
            self.allowed_vlans.contains(vlan)
        } else {
            vlan == self.access_vlan
        }
    }

    pub fn port_security_on(&self) -> bool {
        self.port_security.as_ref().is_some_and(|p| p.enabled)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StpMode {
    #[default]
    Pvst,
    RapidPvst,
}

impl StpMode {
    pub fn ios(self) -> &'static str {
        match self {
            StpMode::Pvst => "pvst",
            StpMode::RapidPvst => "rapid-pvst",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StpConfig {
    pub mode: StpMode,
    /// Bridge priority per VLAN (0–61440 in steps of 4096; default 32768).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub priorities: BTreeMap<u16, u16>,
}

impl StpConfig {
    pub const DEFAULT_PRIORITY: u16 = 32768;

    pub fn is_default(&self) -> bool {
        *self == StpConfig::default()
    }

    pub fn priority(&self, vlan: u16) -> u16 {
        self.priorities
            .get(&vlan)
            .copied()
            .unwrap_or(Self::DEFAULT_PRIORITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlan_lists() {
        let set = VlanSet::parse("10, 20-30,31").unwrap();
        assert_eq!(set.to_ios(), "10,20-31");
        assert!(set.contains(25) && !set.contains(11));
        assert_eq!(VlanSet::all().to_ios(), "1-4094");
        assert!(VlanSet::all().is_all());
        assert_eq!(
            set.union(&VlanSet::parse("11").unwrap()).to_ios(),
            "10-11,20-31"
        );
        assert_eq!(
            set.minus(&VlanSet::parse("25").unwrap()).to_ios(),
            "10,20-24,26-31"
        );
        assert!(
            VlanSet::parse("0").is_none()
                && VlanSet::parse("5000").is_none()
                && VlanSet::parse("x").is_none()
        );
    }

    #[test]
    fn ports_carry_their_vlans() {
        let mut port = SwitchportConfig {
            access_vlan: 10,
            ..SwitchportConfig::default()
        };
        assert!(port.carries(10) && !port.carries(1));
        port.mode = PortMode::Trunk;
        port.allowed_vlans = VlanSet::parse("10,20").unwrap();
        assert!(port.carries(20) && !port.carries(30));
        assert_eq!(port.untagged_vlan(), 1);
        assert!(SwitchportConfig::default().is_default());
    }
}
