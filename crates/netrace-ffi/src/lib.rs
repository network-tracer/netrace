//! Swift-facing API of the netrace core, exported with UniFFI.
//!
//! The surface is deliberately coarse: the app sends commands and reads back
//! plain value snapshots. No Rust references or callbacks cross the boundary,
//! so the Swift side never shares mutable state with the core.

use std::sync::{Arc, Mutex, MutexGuard};

use netrace_doc as doc;

uniffi::setup_scaffolding!();

mod simulation;
pub use simulation::*;

/// Version of the Rust core, shown in the app's About panel and status bar.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Name of the JSON file inside a `.netrace` package.
#[uniffi::export]
pub fn project_file_name() -> String {
    doc::PROJECT_FILE_NAME.to_string()
}

/// Interface names of a device kind's physical ports, in panel order.
#[uniffi::export]
pub fn port_names(kind: DeviceKind) -> Vec<String> {
    doc::DeviceKind::from(kind).port_names()
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    /// A file could not be opened as a netrace project.
    #[error("{message}")]
    InvalidDocument { message: String },
    /// A command referred to something that doesn't exist or broke a rule.
    #[error("{message}")]
    InvalidOperation { message: String },
}

fn invalid_operation(e: doc::DocError) -> CoreError {
    CoreError::InvalidOperation {
        message: e.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DeviceKind {
    Router,
    L2Switch,
    L3Switch,
    Hub,
    Firewall,
    AccessPoint,
    Cloud,
    Pc,
    Laptop,
    Server,
    Printer,
    IpPhone,
    Smartphone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Medium {
    Copper,
    Fiber,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeviceInfo {
    pub id: u64,
    pub kind: DeviceKind,
    pub name: String,
    pub x: f64,
    pub y: f64,
    /// IPv4 settings (end devices only).
    pub ip: Option<HostIpInfo>,
}

/// An end device's IPv4 settings. `mask` is derived from `prefix_len` and
/// ignored on input.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HostIpInfo {
    pub address: String,
    pub prefix_len: u8,
    pub mask: String,
    pub gateway: Option<String>,
}

impl From<doc::HostIp> for HostIpInfo {
    fn from(ip: doc::HostIp) -> Self {
        HostIpInfo {
            address: ip.address.to_string(),
            prefix_len: ip.prefix_len,
            mask: ip.mask().to_string(),
            gateway: ip.gateway.map(|g| g.to_string()),
        }
    }
}

impl TryFrom<&HostIpInfo> for doc::HostIp {
    type Error = CoreError;

    fn try_from(info: &HostIpInfo) -> Result<Self, CoreError> {
        let address = parse_ipv4(&info.address, "IP address")?;
        let gateway = info
            .gateway
            .as_deref()
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(|g| parse_ipv4(g, "default gateway"))
            .transpose()?;
        doc::HostIp::new(address, info.prefix_len, gateway).map_err(invalid_operation)
    }
}

fn parse_ipv4(text: &str, what: &str) -> Result<std::net::Ipv4Addr, CoreError> {
    text.trim()
        .parse()
        .map_err(|_| CoreError::InvalidOperation {
            message: format!(
                "\"{}\" is not a valid {what}. Use four numbers 0–255, like 192.168.1.10.",
                text.trim()
            ),
        })
}

/// Validates IP settings typed into a form. `mask` may be dotted
/// (255.255.255.0) or a prefix (/24 or 24); an empty gateway means none.
#[uniffi::export]
pub fn parse_host_ip(
    address: String,
    mask: String,
    gateway: String,
) -> Result<HostIpInfo, CoreError> {
    let mask = mask.trim();
    let prefix_len = if let Some(p) = mask
        .strip_prefix('/')
        .or(mask.parse::<u8>().ok().map(|_| mask))
    {
        p.parse::<u8>().map_err(|_| CoreError::InvalidOperation {
            message: format!("\"{mask}\" is not a valid prefix length."),
        })?
    } else {
        let dotted = parse_ipv4(mask, "subnet mask")?;
        doc::prefix_from_mask(dotted).ok_or_else(|| CoreError::InvalidOperation {
            message: format!(
                "{mask} is not a valid subnet mask: the 1s must all come first, like 255.255.255.0."
            ),
        })?
    };
    let draft = HostIpInfo {
        address,
        prefix_len,
        mask: String::new(),
        gateway: Some(gateway),
    };
    let ip = doc::HostIp::try_from(&draft)?;
    Ok(ip.into())
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EndpointInfo {
    pub device_id: u64,
    pub port: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LinkInfo {
    pub id: u64,
    pub a: EndpointInfo,
    pub b: EndpointInfo,
    pub medium: Medium,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationKind {
    Note,
    Area,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AnnotationInfo {
    pub id: u64,
    pub kind: AnnotationKind,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub text: String,
}

/// Lets undo put a deleted annotation back in its stacking slot.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RemovedAnnotationInfo {
    pub annotation: AnnotationInfo,
    pub index: u64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ProjectSnapshot {
    pub devices: Vec<DeviceInfo>,
    pub links: Vec<LinkInfo>,
    pub annotations: Vec<AnnotationInfo>,
}

/// Opaque-to-the-user record that lets undo put a deleted device back.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RemovedDeviceInfo {
    pub device: DeviceInfo,
    pub links: Vec<LinkInfo>,
    pub index: u64,
}

/// A live, mutable netrace project owned by one document window.
#[derive(uniffi::Object)]
pub struct Project {
    inner: Mutex<doc::Project>,
}

#[uniffi::export]
impl Project {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Project {
            inner: Mutex::new(doc::Project::new()),
        })
    }

    #[uniffi::constructor]
    pub fn from_json(data: Vec<u8>) -> Result<Arc<Self>, CoreError> {
        let project = doc::Project::from_json(&data).map_err(|e| CoreError::InvalidDocument {
            message: e.to_string(),
        })?;
        Ok(Arc::new(Project {
            inner: Mutex::new(project),
        }))
    }

    pub fn to_json(&self) -> Vec<u8> {
        self.lock().to_json()
    }

    pub fn snapshot(&self) -> ProjectSnapshot {
        let project = self.lock();
        ProjectSnapshot {
            devices: project.devices().iter().map(DeviceInfo::from).collect(),
            links: project.links().iter().map(LinkInfo::from).collect(),
            annotations: project
                .annotations()
                .iter()
                .map(AnnotationInfo::from)
                .collect(),
        }
    }

    pub fn add_device(&self, kind: DeviceKind, x: f64, y: f64) -> DeviceInfo {
        DeviceInfo::from(self.lock().add_device(kind.into(), doc::Point::new(x, y)))
    }

    pub fn move_device(&self, id: u64, x: f64, y: f64) -> Result<(), CoreError> {
        self.lock()
            .move_device(doc::DeviceId(id), doc::Point::new(x, y))
            .map_err(invalid_operation)
    }

    /// Sets or clears an end device's IP settings; returns the previous ones (for undo).
    pub fn set_host_ip(
        &self,
        id: u64,
        ip: Option<HostIpInfo>,
    ) -> Result<Option<HostIpInfo>, CoreError> {
        let ip = ip.as_ref().map(doc::HostIp::try_from).transpose()?;
        let previous = self
            .lock()
            .set_host_ip(doc::DeviceId(id), ip)
            .map_err(invalid_operation)?;
        Ok(previous.map(HostIpInfo::from))
    }

    /// Renames a device; returns the previous name (for undo).
    pub fn rename_device(&self, id: u64, name: String) -> Result<String, CoreError> {
        self.lock()
            .rename_device(doc::DeviceId(id), &name)
            .map_err(invalid_operation)
    }

    pub fn remove_device(&self, id: u64) -> Result<RemovedDeviceInfo, CoreError> {
        let removed = self
            .lock()
            .remove_device(doc::DeviceId(id))
            .map_err(invalid_operation)?;
        Ok(RemovedDeviceInfo {
            device: DeviceInfo::from(&removed.device),
            links: removed.links.iter().map(LinkInfo::from).collect(),
            index: removed.index as u64,
        })
    }

    pub fn restore_device(&self, removed: RemovedDeviceInfo) -> Result<(), CoreError> {
        let removed = doc::RemovedDevice {
            device: removed.device.into(),
            links: removed.links.into_iter().map(Into::into).collect(),
            index: usize::try_from(removed.index).unwrap_or(usize::MAX),
        };
        self.lock()
            .restore_device(removed)
            .map_err(invalid_operation)
    }

    pub fn add_annotation(
        &self,
        kind: AnnotationKind,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: String,
    ) -> AnnotationInfo {
        let rect = doc::Rect::new(x, y, width, height);
        AnnotationInfo::from(self.lock().add_annotation(kind.into(), rect, &text))
    }

    /// Replaces an annotation's frame and text; returns the previous version (for undo).
    pub fn update_annotation(
        &self,
        id: u64,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: String,
    ) -> Result<AnnotationInfo, CoreError> {
        let previous = self
            .lock()
            .update_annotation(
                doc::AnnotationId(id),
                doc::Rect::new(x, y, width, height),
                &text,
            )
            .map_err(invalid_operation)?;
        Ok(AnnotationInfo::from(&previous))
    }

    pub fn remove_annotation(&self, id: u64) -> Result<RemovedAnnotationInfo, CoreError> {
        let removed = self
            .lock()
            .remove_annotation(doc::AnnotationId(id))
            .map_err(invalid_operation)?;
        Ok(RemovedAnnotationInfo {
            annotation: AnnotationInfo::from(&removed.annotation),
            index: removed.index as u64,
        })
    }

    pub fn restore_annotation(&self, removed: RemovedAnnotationInfo) -> Result<(), CoreError> {
        let a = removed.annotation;
        let removed = doc::RemovedAnnotation {
            annotation: doc::Annotation {
                id: doc::AnnotationId(a.id),
                kind: a.kind.into(),
                rect: doc::Rect::new(a.x, a.y, a.width, a.height),
                text: a.text,
            },
            index: usize::try_from(removed.index).unwrap_or(usize::MAX),
        };
        self.lock()
            .restore_annotation(removed)
            .map_err(invalid_operation)
    }

    pub fn connect(
        &self,
        a: EndpointInfo,
        b: EndpointInfo,
        medium: Medium,
    ) -> Result<LinkInfo, CoreError> {
        let mut project = self.lock();
        let link = project
            .connect(a.into(), b.into(), medium.into())
            .map_err(invalid_operation)?;
        Ok(LinkInfo::from(link))
    }

    pub fn disconnect(&self, link_id: u64) -> Result<LinkInfo, CoreError> {
        let link = self
            .lock()
            .disconnect(doc::LinkId(link_id))
            .map_err(invalid_operation)?;
        Ok(LinkInfo::from(&link))
    }

    pub fn restore_link(&self, link: LinkInfo) -> Result<(), CoreError> {
        self.lock()
            .restore_link(link.into())
            .map_err(invalid_operation)
    }
}

impl Project {
    pub(crate) fn lock(&self) -> MutexGuard<'_, doc::Project> {
        // A panic mid-command can't leave the model half-written (every
        // mutation validates first), so a poisoned lock is safe to reuse.
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl From<DeviceKind> for doc::DeviceKind {
    fn from(kind: DeviceKind) -> Self {
        match kind {
            DeviceKind::Router => doc::DeviceKind::Router,
            DeviceKind::L2Switch => doc::DeviceKind::L2Switch,
            DeviceKind::L3Switch => doc::DeviceKind::L3Switch,
            DeviceKind::Hub => doc::DeviceKind::Hub,
            DeviceKind::Firewall => doc::DeviceKind::Firewall,
            DeviceKind::AccessPoint => doc::DeviceKind::AccessPoint,
            DeviceKind::Cloud => doc::DeviceKind::Cloud,
            DeviceKind::Pc => doc::DeviceKind::Pc,
            DeviceKind::Laptop => doc::DeviceKind::Laptop,
            DeviceKind::Server => doc::DeviceKind::Server,
            DeviceKind::Printer => doc::DeviceKind::Printer,
            DeviceKind::IpPhone => doc::DeviceKind::IpPhone,
            DeviceKind::Smartphone => doc::DeviceKind::Smartphone,
        }
    }
}

impl From<doc::DeviceKind> for DeviceKind {
    fn from(kind: doc::DeviceKind) -> Self {
        match kind {
            doc::DeviceKind::Router => DeviceKind::Router,
            doc::DeviceKind::L2Switch => DeviceKind::L2Switch,
            doc::DeviceKind::L3Switch => DeviceKind::L3Switch,
            doc::DeviceKind::Hub => DeviceKind::Hub,
            doc::DeviceKind::Firewall => DeviceKind::Firewall,
            doc::DeviceKind::AccessPoint => DeviceKind::AccessPoint,
            doc::DeviceKind::Cloud => DeviceKind::Cloud,
            doc::DeviceKind::Pc => DeviceKind::Pc,
            doc::DeviceKind::Laptop => DeviceKind::Laptop,
            doc::DeviceKind::Server => DeviceKind::Server,
            doc::DeviceKind::Printer => DeviceKind::Printer,
            doc::DeviceKind::IpPhone => DeviceKind::IpPhone,
            doc::DeviceKind::Smartphone => DeviceKind::Smartphone,
        }
    }
}

impl From<Medium> for doc::Medium {
    fn from(medium: Medium) -> Self {
        match medium {
            Medium::Copper => doc::Medium::Copper,
            Medium::Fiber => doc::Medium::Fiber,
        }
    }
}

impl From<doc::Medium> for Medium {
    fn from(medium: doc::Medium) -> Self {
        match medium {
            doc::Medium::Copper => Medium::Copper,
            doc::Medium::Fiber => Medium::Fiber,
        }
    }
}

impl From<AnnotationKind> for doc::AnnotationKind {
    fn from(kind: AnnotationKind) -> Self {
        match kind {
            AnnotationKind::Note => doc::AnnotationKind::Note,
            AnnotationKind::Area => doc::AnnotationKind::Area,
        }
    }
}

impl From<doc::AnnotationKind> for AnnotationKind {
    fn from(kind: doc::AnnotationKind) -> Self {
        match kind {
            doc::AnnotationKind::Note => AnnotationKind::Note,
            doc::AnnotationKind::Area => AnnotationKind::Area,
        }
    }
}

impl From<&doc::Annotation> for AnnotationInfo {
    fn from(a: &doc::Annotation) -> Self {
        AnnotationInfo {
            id: a.id.0,
            kind: a.kind.into(),
            x: a.rect.x,
            y: a.rect.y,
            width: a.rect.width,
            height: a.rect.height,
            text: a.text.clone(),
        }
    }
}

impl From<&doc::Device> for DeviceInfo {
    fn from(d: &doc::Device) -> Self {
        DeviceInfo {
            id: d.id.0,
            kind: d.kind.into(),
            name: d.name.clone(),
            x: d.position.x,
            y: d.position.y,
            ip: d.ip.map(HostIpInfo::from),
        }
    }
}

impl From<DeviceInfo> for doc::Device {
    fn from(d: DeviceInfo) -> Self {
        doc::Device {
            id: doc::DeviceId(d.id),
            kind: d.kind.into(),
            name: d.name,
            position: doc::Point::new(d.x, d.y),
            ip: d.ip.as_ref().and_then(|ip| doc::HostIp::try_from(ip).ok()),
        }
    }
}

impl From<&doc::Endpoint> for EndpointInfo {
    fn from(e: &doc::Endpoint) -> Self {
        EndpointInfo {
            device_id: e.device.0,
            port: e.port.clone(),
        }
    }
}

impl From<EndpointInfo> for doc::Endpoint {
    fn from(e: EndpointInfo) -> Self {
        doc::Endpoint {
            device: doc::DeviceId(e.device_id),
            port: e.port,
        }
    }
}

impl From<&doc::Link> for LinkInfo {
    fn from(l: &doc::Link) -> Self {
        LinkInfo {
            id: l.id.0,
            a: (&l.a).into(),
            b: (&l.b).into(),
            medium: l.medium.into(),
        }
    }
}

impl From<LinkInfo> for doc::Link {
    fn from(l: LinkInfo) -> Self {
        doc::Link {
            id: doc::LinkId(l.id),
            a: l.a.into(),
            b: l.b.into(),
            medium: l.medium.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_through_doc() {
        for kind in doc::DeviceKind::ALL {
            assert_eq!(doc::DeviceKind::from(DeviceKind::from(kind)), kind);
        }
    }

    #[test]
    fn save_and_reopen() {
        let project = Project::new();
        let r1 = project.add_device(DeviceKind::Router, 10.0, 20.0);
        let pc = project.add_device(DeviceKind::Pc, 30.0, 40.0);
        project
            .connect(
                EndpointInfo {
                    device_id: r1.id,
                    port: "GigabitEthernet0/0".into(),
                },
                EndpointInfo {
                    device_id: pc.id,
                    port: "FastEthernet0".into(),
                },
                Medium::Copper,
            )
            .unwrap();

        let reopened = Project::from_json(project.to_json()).unwrap();
        assert_eq!(reopened.snapshot(), project.snapshot());
    }

    #[test]
    fn remove_then_restore_is_lossless() {
        let project = Project::new();
        let sw = project.add_device(DeviceKind::L2Switch, 0.0, 0.0);
        let pc = project.add_device(DeviceKind::Pc, 0.0, 0.0);
        project
            .connect(
                EndpointInfo {
                    device_id: sw.id,
                    port: "FastEthernet0/1".into(),
                },
                EndpointInfo {
                    device_id: pc.id,
                    port: "FastEthernet0".into(),
                },
                Medium::Copper,
            )
            .unwrap();
        let before = project.snapshot();

        let removed = project.remove_device(sw.id).unwrap();
        assert!(project.snapshot().links.is_empty());
        project.restore_device(removed).unwrap();
        assert_eq!(project.snapshot(), before);
    }

    #[test]
    fn annotation_lifecycle_through_the_bridge() {
        let project = Project::new();
        let note =
            project.add_annotation(AnnotationKind::Note, 10.0, 10.0, 160.0, 64.0, "Core".into());
        let previous = project
            .update_annotation(note.id, 20.0, 20.0, 200.0, 80.0, "Core layer".into())
            .unwrap();
        assert_eq!(previous, note);
        let removed = project.remove_annotation(note.id).unwrap();
        assert!(project.snapshot().annotations.is_empty());
        project.restore_annotation(removed).unwrap();
        assert_eq!(project.snapshot().annotations[0].text, "Core layer");
        let reopened = Project::from_json(project.to_json()).unwrap();
        assert_eq!(reopened.snapshot(), project.snapshot());
    }

    #[test]
    fn bad_input_becomes_typed_errors() {
        assert!(matches!(
            Project::from_json(b"{}".to_vec()),
            Err(CoreError::InvalidDocument { .. })
        ));
        assert!(matches!(
            Project::new().move_device(42, 0.0, 0.0),
            Err(CoreError::InvalidOperation { .. })
        ));
    }
}
