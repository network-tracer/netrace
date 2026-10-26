//! Snapshots: saved states of the topology to go back to.

use std::sync::Arc;

use netrace_doc as doc;

use crate::{CoreError, Project};

/// A snapshot (or the state a restore replaced), held by the app for undo.
#[derive(uniffi::Object)]
pub struct SavedState {
    state: doc::Snapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SnapshotInfo {
    pub name: String,
    /// Seconds since 1970.
    pub taken: u64,
    pub devices: u32,
    pub cables: u32,
}

fn unknown(e: doc::DocError) -> CoreError {
    CoreError::InvalidOperation {
        message: e.to_string(),
    }
}

#[uniffi::export]
impl Project {
    pub fn snapshots(&self) -> Vec<SnapshotInfo> {
        self.lock()
            .snapshots()
            .iter()
            .map(|s| SnapshotInfo {
                name: s.name.clone(),
                taken: s.taken,
                devices: s.device_count() as u32,
                cables: s.link_count() as u32,
            })
            .collect()
    }

    /// Saves the topology as it is now; returns the snapshot's index.
    pub fn take_snapshot(&self, name: String, taken: u64) -> u32 {
        self.lock().take_snapshot(&name, taken) as u32
    }

    /// Restores a snapshot; returns the state it replaced, for undo.
    pub fn restore_snapshot(&self, index: u32) -> Result<Arc<SavedState>, CoreError> {
        let previous = self
            .lock()
            .restore_snapshot(index as usize)
            .map_err(unknown)?;
        Ok(Arc::new(SavedState { state: previous }))
    }

    /// Puts a saved state back (undo of a restore).
    pub fn set_state(&self, state: Arc<SavedState>) -> Result<(), CoreError> {
        self.lock().set_state(state.state.clone()).map_err(unknown)
    }

    pub fn delete_snapshot(&self, index: u32) -> Result<Arc<SavedState>, CoreError> {
        let removed = self
            .lock()
            .delete_snapshot(index as usize)
            .map_err(unknown)?;
        Ok(Arc::new(SavedState { state: removed }))
    }

    /// Reverses `delete_snapshot`.
    pub fn insert_snapshot(&self, index: u32, snapshot: Arc<SavedState>) {
        self.lock()
            .insert_snapshot(index as usize, snapshot.state.clone());
    }

    pub fn rename_snapshot(&self, index: u32, name: String) -> Result<String, CoreError> {
        self.lock()
            .rename_snapshot(index as usize, &name)
            .map_err(unknown)
    }
}

#[uniffi::export]
impl Project {
    /// Starts, suspends or stops a device; returns its previous state.
    pub fn set_power(
        &self,
        id: u64,
        power: crate::DevicePower,
    ) -> Result<crate::DevicePower, CoreError> {
        self.lock()
            .set_power(doc::DeviceId(id), power.into())
            .map(Into::into)
            .map_err(unknown)
    }

    /// IOS `reload`: the running configuration becomes the startup one.
    pub fn reload_device(&self, id: u64) -> Result<(), CoreError> {
        self.lock()
            .reload_device(doc::DeviceId(id))
            .map_err(unknown)
    }
}

#[uniffi::export]
impl Project {
    /// Sets a cable's delay, jitter, loss and bandwidth; returns the previous ones.
    pub fn set_link_conditioning(
        &self,
        id: u64,
        conditioning: crate::LinkConditioning,
    ) -> Result<crate::LinkConditioning, CoreError> {
        self.lock()
            .set_link_conditioning(doc::LinkId(id), conditioning.into())
            .map(Into::into)
            .map_err(unknown)
    }
}

/// A server's DHCP service, as the inspector edits it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ServerDhcp {
    /// "192.168.1.0".
    pub network: String,
    pub prefix_len: u8,
    pub gateway: Option<String>,
    pub dns: Option<String>,
    /// The first address handed out; the ones before it are kept back.
    pub start: String,
}

fn parse_ip(text: &str) -> Result<std::net::Ipv4Addr, CoreError> {
    text.trim()
        .parse()
        .map_err(|_| CoreError::InvalidOperation {
            message: format!("\"{text}\" isn't an IPv4 address"),
        })
}

#[uniffi::export]
impl Project {
    pub fn server_dhcp(&self, id: u64) -> Option<ServerDhcp> {
        let project = self.lock();
        let d = project.device(doc::DeviceId(id))?;
        let pool = d.config.dhcp_pools.first()?;
        let (network, prefix_len) = pool.network?;
        let start = d
            .config
            .dhcp_excluded
            .iter()
            .map(|(_, high)| u32::from(*high) + 1)
            .max()
            .unwrap_or(u32::from(network) + 1);
        Some(ServerDhcp {
            network: network.to_string(),
            prefix_len,
            gateway: pool.default_router.map(|g| g.to_string()),
            dns: pool.dns_server.map(|g| g.to_string()),
            start: std::net::Ipv4Addr::from(start).to_string(),
        })
    }

    /// Turns a server's DHCP service on (with these settings) or off.
    pub fn set_server_dhcp(
        &self,
        id: u64,
        service: Option<ServerDhcp>,
    ) -> Result<Option<ServerDhcp>, CoreError> {
        let previous = self.server_dhcp(id);
        let converted = match service {
            None => None,
            Some(s) => {
                let network = parse_ip(&s.network)?;
                let mask = u32::from(doc::mask(s.prefix_len));
                let network = std::net::Ipv4Addr::from(u32::from(network) & mask);
                let start = parse_ip(&s.start)?;
                if u32::from(start) & mask != u32::from(network) {
                    return Err(CoreError::InvalidOperation {
                        message: format!("{start} isn't in {network}/{}", s.prefix_len),
                    });
                }
                let mut pool = doc::DhcpPool::new("serverPool");
                pool.network = Some((network, s.prefix_len));
                pool.default_router = s
                    .gateway
                    .as_deref()
                    .filter(|g| !g.trim().is_empty())
                    .map(parse_ip)
                    .transpose()?;
                pool.dns_server = s
                    .dns
                    .as_deref()
                    .filter(|g| !g.trim().is_empty())
                    .map(parse_ip)
                    .transpose()?;
                let first = std::net::Ipv4Addr::from(u32::from(network) + 1);
                let excluded = if u32::from(start) > u32::from(first) {
                    vec![(first, std::net::Ipv4Addr::from(u32::from(start) - 1))]
                } else {
                    Vec::new()
                };
                Some((pool, excluded))
            }
        };
        self.lock()
            .set_server_dhcp(doc::DeviceId(id), converted)
            .map_err(unknown)?;
        Ok(previous)
    }
}
