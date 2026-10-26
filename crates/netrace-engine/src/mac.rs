use std::fmt;

use netrace_doc::DeviceId;

/// A 48-bit Ethernet MAC address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub const BROADCAST: MacAddr = MacAddr([0xff; 6]);
    pub const ZERO: MacAddr = MacAddr([0; 6]);
    /// OSPF's AllSPFRouters group (224.0.0.5).
    pub const OSPF_ALL_ROUTERS: MacAddr = MacAddr([0x01, 0x00, 0x5e, 0x00, 0x00, 0x05]);
    /// Where spanning-tree BPDUs go: switches keep them, never forward them.
    pub const STP_BRIDGES: MacAddr = MacAddr([0x01, 0x80, 0xc2, 0x00, 0x00, 0x00]);

    /// Deterministic, locally administered address for a device's port, so
    /// the same project always shows the same MACs.
    pub fn for_port(device: DeviceId, port_index: usize) -> Self {
        let id = device.0;
        MacAddr([
            0x02,
            0x4e,
            (id >> 16) as u8,
            (id >> 8) as u8,
            id as u8,
            port_index as u8,
        ])
    }

    pub fn is_broadcast(&self) -> bool {
        *self == Self::BROADCAST
    }

    /// Group addresses (the low bit of the first octet), broadcast included.
    pub fn is_multicast(&self) -> bool {
        self.0[0] & 1 == 1
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0;
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_and_derivation() {
        assert_eq!(MacAddr::BROADCAST.to_string(), "ff:ff:ff:ff:ff:ff");
        let a = MacAddr::for_port(DeviceId(0x01_0203), 4);
        assert_eq!(a.to_string(), "02:4e:01:02:03:04");
        assert_ne!(a, MacAddr::for_port(DeviceId(0x01_0203), 5));
        assert!(!a.is_broadcast());
    }
}
