use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::DocError;

/// An end device's IPv4 settings, as typed into its IP configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HostIp {
    pub address: Ipv4Addr,
    pub prefix_len: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<Ipv4Addr>,
}

impl HostIp {
    /// Validates the settings the way a student would expect a real host to:
    /// a usable host address for the prefix, and a gateway on the same network.
    pub fn new(
        address: Ipv4Addr,
        prefix_len: u8,
        gateway: Option<Ipv4Addr>,
    ) -> Result<Self, DocError> {
        let invalid = |reason: String| Err(DocError::InvalidIp(reason));
        if !(1..=30).contains(&prefix_len) {
            return invalid(format!(
                "prefix /{prefix_len} leaves no usable host addresses (use /1 to /30)"
            ));
        }
        let ip = HostIp {
            address,
            prefix_len,
            gateway,
        };
        if address == ip.network() {
            return invalid(format!(
                "{address} is the network address of {}/{prefix_len}",
                ip.network()
            ));
        }
        if address == ip.broadcast() {
            return invalid(format!(
                "{address} is the broadcast address of {}/{prefix_len}",
                ip.network()
            ));
        }
        if address.is_loopback() || address.is_multicast() || address.is_unspecified() {
            return invalid(format!("{address} can't be assigned to a host"));
        }
        if let Some(gw) = gateway {
            if !ip.contains(gw) {
                return invalid(format!(
                    "gateway {gw} is not on this network ({}/{prefix_len})",
                    ip.network()
                ));
            }
            if gw == address {
                return invalid("the gateway can't be the host's own address".into());
            }
        }
        Ok(ip)
    }

    pub fn mask(&self) -> Ipv4Addr {
        Ipv4Addr::from(
            u32::MAX
                .checked_shl(32 - u32::from(self.prefix_len))
                .unwrap_or(0),
        )
    }

    pub fn network(&self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.address) & u32::from(self.mask()))
    }

    pub fn broadcast(&self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.network()) | !u32::from(self.mask()))
    }

    /// True if `other` is on the same network as this host.
    pub fn contains(&self, other: Ipv4Addr) -> bool {
        u32::from(other) & u32::from(self.mask()) == u32::from(self.network())
    }
}

/// Parses a dotted mask (255.255.255.0) into a prefix length, rejecting
/// non-contiguous masks.
pub fn prefix_from_mask(mask: Ipv4Addr) -> Option<u8> {
    let bits = u32::from(mask);
    let prefix = bits.leading_ones();
    (bits.checked_shl(prefix).unwrap_or(0) == 0).then_some(prefix as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn derived_addresses() {
        let h = HostIp::new(ip("192.168.1.10"), 24, Some(ip("192.168.1.1"))).unwrap();
        assert_eq!(h.mask(), ip("255.255.255.0"));
        assert_eq!(h.network(), ip("192.168.1.0"));
        assert_eq!(h.broadcast(), ip("192.168.1.255"));
        assert!(h.contains(ip("192.168.1.200")));
        assert!(!h.contains(ip("192.168.2.1")));
    }

    #[test]
    fn rejects_unusable_settings() {
        for (addr, prefix, gw) in [
            ("192.168.1.0", 24, None),
            ("192.168.1.255", 24, None),
            ("10.0.0.1", 31, None),
            ("10.0.0.1", 0, None),
            ("127.0.0.1", 8, None),
            ("192.168.1.10", 24, Some("192.168.2.1")),
            ("192.168.1.10", 24, Some("192.168.1.10")),
        ] {
            assert!(
                matches!(
                    HostIp::new(ip(addr), prefix, gw.map(ip)),
                    Err(DocError::InvalidIp(_))
                ),
                "{addr}/{prefix} gw {gw:?} should be rejected"
            );
        }
    }

    #[test]
    fn masks_to_prefixes() {
        assert_eq!(prefix_from_mask(ip("255.255.255.0")), Some(24));
        assert_eq!(prefix_from_mask(ip("255.255.255.252")), Some(30));
        assert_eq!(prefix_from_mask(ip("255.0.255.0")), None);
        assert_eq!(prefix_from_mask(ip("0.0.0.0")), Some(0));
    }
}
