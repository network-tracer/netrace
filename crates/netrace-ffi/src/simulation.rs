//! The simulation as seen from Swift: commands in, an explained trace out.

use std::sync::{Arc, Mutex, MutexGuard};

use netrace_doc as doc;
use netrace_engine as engine;
use netrace_sim::SimTime;

use crate::{CoreError, Project};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TraceOutcome {
    Created,
    Forwarded,
    Flooded,
    Replied,
    Accepted,
    Ignored,
    Dropped,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LayerRole {
    /// Read and acted on at this hop.
    Used,
    /// Built or rewritten at this hop.
    Changed,
    /// Not looked at by this device.
    NotUsed,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FieldInfo {
    pub name: String,
    pub value: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LayerNoteInfo {
    /// 1 = physical … 4 = message carried inside IP.
    pub number: u8,
    pub title: String,
    pub role: LayerRole,
    pub note: String,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TraceEventInfo {
    pub id: u64,
    pub time_ns: u64,
    /// When the frame left the previous device (for animating the hop).
    pub sent_ns: Option<u64>,
    pub device_id: u64,
    pub from_device_id: Option<u64>,
    pub link_id: Option<u64>,
    pub ingress: Option<String>,
    pub egress: Vec<String>,
    /// "ARP" or "ICMP"; absent for events without a frame (timeouts).
    pub protocol: Option<String>,
    pub outcome: TraceOutcome,
    /// The plain-language explanation.
    pub summary: String,
    /// One-line, Wireshark-style description of the frame.
    pub info: String,
    /// IP addresses for IP packets, MAC addresses for ARP.
    pub source: Option<String>,
    pub destination: Option<String>,
    pub length: Option<u32>,
    pub layers: Vec<LayerNoteInfo>,
    pub session: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PingSessionInfo {
    pub id: u64,
    pub source_id: u64,
    pub destination: String,
    pub count: u16,
    pub received: u16,
    pub failed: u16,
    pub finished: bool,
    /// Windows-style ping output.
    pub report: String,
}

/// A simulation of one project snapshot. Rebuild it when the topology
/// changes; commands added to it are replayed deterministically.
#[derive(uniffi::Object)]
pub struct Simulation {
    inner: Mutex<engine::Network>,
}

#[uniffi::export]
impl Simulation {
    #[uniffi::constructor]
    pub fn new(project: Arc<Project>) -> Arc<Self> {
        let snapshot = project.lock().clone();
        Arc::new(Simulation {
            inner: Mutex::new(engine::Network::new(snapshot)),
        })
    }

    /// Schedules `count` pings from an end device at `at_ns`; returns the session id.
    pub fn ping(
        &self,
        source: u64,
        destination: String,
        count: u16,
        at_ns: u64,
    ) -> Result<u64, CoreError> {
        let destination = destination
            .trim()
            .parse()
            .map_err(|_| CoreError::InvalidOperation {
                message: format!("\"{}\" is not a valid IPv4 address.", destination.trim()),
            })?;
        self.lock()
            .ping(
                doc::DeviceId(source),
                destination,
                count,
                SimTime::from_nanos(at_ns),
            )
            .map_err(|e| CoreError::InvalidOperation {
                message: e.to_string(),
            })
    }

    /// Runs a command typed at an end device's prompt; returns the output.
    pub fn host_command(&self, device: u64, line: String, at_ns: u64) -> String {
        self.lock()
            .host_command(doc::DeviceId(device), &line, SimTime::from_nanos(at_ns))
    }

    pub fn trace(&self) -> Vec<TraceEventInfo> {
        self.lock()
            .trace()
            .iter()
            .map(TraceEventInfo::from)
            .collect()
    }

    pub fn sessions(&self) -> Vec<PingSessionInfo> {
        let network = self.lock();
        network
            .sessions()
            .iter()
            .map(|s| PingSessionInfo {
                id: s.id,
                source_id: s.source.0,
                destination: s.destination.to_string(),
                count: s.count,
                received: s.replies.len() as u16,
                failed: s.failures.len() as u16,
                finished: s.is_finished(),
                report: network.ping_report(s.id).unwrap_or_default(),
            })
            .collect()
    }

    pub fn warnings(&self) -> Vec<String> {
        self.lock().warnings().to_vec()
    }

    pub fn end_time_ns(&self) -> u64 {
        self.lock().end_time().as_nanos()
    }

    /// The MAC a device uses on a port, e.g. for the inspector.
    pub fn mac_address(&self, device: u64, port: String) -> Option<String> {
        self.lock()
            .mac(doc::DeviceId(device), &port)
            .map(|m| m.to_string())
    }
}

impl Simulation {
    fn lock(&self) -> MutexGuard<'_, engine::Network> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Time a frame takes to cross one cable, in nanoseconds.
#[uniffi::export]
pub fn hop_latency_ns() -> u64 {
    engine::HOP_LATENCY.as_nanos() as u64
}

impl From<&engine::TraceEvent> for TraceEventInfo {
    fn from(e: &engine::TraceEvent) -> Self {
        let (source, destination) = match e.frame.map(|f| f.payload) {
            Some(engine::Payload::Ipv4(ip)) => (Some(ip.src.to_string()), Some(ip.dst.to_string())),
            Some(engine::Payload::Arp(_)) => (
                e.frame.map(|f| f.src.to_string()),
                e.frame.map(|f| f.dst.to_string()),
            ),
            None => (None, None),
        };
        TraceEventInfo {
            id: e.id,
            time_ns: e.time.as_nanos(),
            sent_ns: e.sent.map(SimTime::as_nanos),
            device_id: e.device.0,
            from_device_id: e.from.map(|d| d.0),
            link_id: e.link.map(|l| l.0),
            ingress: e.ingress.clone(),
            egress: e.egress.clone(),
            protocol: e.frame.map(|f| f.protocol().name().to_string()),
            outcome: match e.outcome {
                engine::Outcome::Created => TraceOutcome::Created,
                engine::Outcome::Forwarded => TraceOutcome::Forwarded,
                engine::Outcome::Flooded => TraceOutcome::Flooded,
                engine::Outcome::Replied => TraceOutcome::Replied,
                engine::Outcome::Accepted => TraceOutcome::Accepted,
                engine::Outcome::Ignored => TraceOutcome::Ignored,
                engine::Outcome::Dropped => TraceOutcome::Dropped,
                engine::Outcome::Completed => TraceOutcome::Completed,
            },
            summary: e.summary.clone(),
            info: e
                .frame
                .map(|f| f.info())
                .unwrap_or_else(|| e.summary.clone()),
            source,
            destination,
            length: e.frame.map(|f| f.len() as u32),
            layers: e
                .layers
                .iter()
                .map(|l| LayerNoteInfo {
                    number: l.layer.number(),
                    title: l.layer.title().to_string(),
                    role: match l.role {
                        engine::LayerRole::Used => LayerRole::Used,
                        engine::LayerRole::Changed => LayerRole::Changed,
                        engine::LayerRole::NotUsed => LayerRole::NotUsed,
                    },
                    note: l.note.clone(),
                    fields: l
                        .fields
                        .iter()
                        .map(|f| FieldInfo {
                            name: f.name.clone(),
                            value: f.value.clone(),
                            changed: f.changed,
                        })
                        .collect(),
                })
                .collect(),
            session: e.session,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceKind, EndpointInfo, Medium, parse_host_ip};

    #[test]
    fn ping_through_the_bridge() {
        let project = Project::new();
        let sw = project.add_device(DeviceKind::L2Switch, 0.0, 0.0);
        let a = project.add_device(DeviceKind::Pc, 0.0, 0.0);
        let b = project.add_device(DeviceKind::Pc, 0.0, 0.0);
        for (pc, port, addr) in [
            (&a, "FastEthernet0/1", "10.0.0.1"),
            (&b, "FastEthernet0/2", "10.0.0.2"),
        ] {
            project
                .connect(
                    EndpointInfo {
                        device_id: sw.id,
                        port: port.into(),
                    },
                    EndpointInfo {
                        device_id: pc.id,
                        port: "FastEthernet0".into(),
                    },
                    Medium::Copper,
                )
                .unwrap();
            let ip = parse_host_ip(addr.into(), "255.255.255.0".into(), "".into()).unwrap();
            project.set_host_ip(pc.id, Some(ip)).unwrap();
        }
        let sim = Simulation::new(project.clone());
        let session = sim.ping(a.id, "10.0.0.2".into(), 1, 0).unwrap();
        let info = &sim.sessions()[session as usize];
        assert_eq!(info.received, 1);
        assert!(info.report.contains("Reply from 10.0.0.2"));
        let trace = sim.trace();
        assert!(trace.iter().any(|e| e.protocol.as_deref() == Some("ARP")));
        assert!(trace.iter().all(|e| !e.summary.is_empty()));
        assert!(
            sim.host_command(a.id, "arp -a".into(), sim.end_time_ns())
                .contains("10.0.0.2")
        );
        assert!(sim.mac_address(a.id, "FastEthernet0".into()).is_some());
    }

    #[test]
    fn form_parsing() {
        let ip = parse_host_ip("192.168.1.10".into(), "/24".into(), "192.168.1.1".into()).unwrap();
        assert_eq!((ip.prefix_len, ip.mask.as_str()), (24, "255.255.255.0"));
        assert!(
            parse_host_ip("192.168.1.10".into(), "255.255.255.0".into(), "".into())
                .unwrap()
                .gateway
                .is_none()
        );
        assert!(parse_host_ip("192.168.1.300".into(), "24".into(), "".into()).is_err());
        assert!(parse_host_ip("192.168.1.10".into(), "255.0.255.0".into(), "".into()).is_err());
        assert!(parse_host_ip("192.168.1.10".into(), "24".into(), "10.0.0.1".into()).is_err());
    }
}
