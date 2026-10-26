use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    Annotation, AnnotationId, AnnotationKind, DeviceConfig, DeviceKind, DocError, FORMAT_VERSION,
    GroupId, HostIp, LinePattern, Medium, Rect, RemovedAnnotation, ShapeStyle, StartupConfig,
    Swatch,
};

const FORMAT_TAG: &str = "netrace";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LinkId(pub u64);

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl fmt::Display for LinkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Canvas position in points. Non-finite values are stored as 0 so the
/// project always serialises to valid JSON.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
        Point {
            x: finite(x),
            y: finite(y),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: DeviceId,
    pub kind: DeviceKind,
    pub name: String,
    pub position: Point,
    /// IPv4 settings for end devices; absent until configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<HostIp>,
    /// An end device that gets its address from DHCP (then `ip` is unused).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dhcp: bool,
    /// Tint of the device symbol; `None` is the default ink.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Swatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupId>,
    /// Running configuration (routers and switches).
    #[serde(default, skip_serializing_if = "DeviceConfig::is_default")]
    pub config: DeviceConfig,
    /// Saved by `copy running-config startup-config`; `reload` restores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<StartupConfig>,
}

/// A device's name and settings, without its place on the canvas. What a
/// command line change can touch, so undo can put it back in one step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceSettings {
    pub name: String,
    #[serde(default)]
    pub ip: Option<HostIp>,
    #[serde(default)]
    pub dhcp: bool,
    #[serde(default)]
    pub config: DeviceConfig,
    #[serde(default)]
    pub startup: Option<StartupConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Endpoint {
    pub device: DeviceId,
    pub port: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub id: LinkId,
    pub a: Endpoint,
    pub b: Endpoint,
    pub medium: Medium,
    /// `None` uses the default cable colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Swatch>,
    /// `None` uses the medium's own look (copper solid, fibre double line).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<LinePattern>,
}

/// A device or annotation, for operations that apply to both (grouping).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CanvasItem {
    Device(DeviceId),
    Annotation(AnnotationId),
}

/// Everything needed to put a removed device back exactly as it was,
/// including its cables and its stacking order. Used for undo.
#[derive(Debug, Clone, PartialEq)]
pub struct RemovedDevice {
    pub device: Device,
    pub links: Vec<Link>,
    pub index: usize,
}

/// A network topology: devices on the canvas, the cables between them and
/// annotations (notes, areas) documenting it.
///
/// Device, link and annotation ids come from one shared counter and are never
/// reused, so undo/redo can restore an item under its original id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    format: String,
    version: u32,
    next_id: u64,
    devices: Vec<Device>,
    links: Vec<Link>,
    /// Added after the first format release; absent in older files.
    #[serde(default)]
    annotations: Vec<Annotation>,
}

#[derive(Deserialize)]
struct Header {
    format: String,
    version: u32,
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl Project {
    pub fn new() -> Self {
        Project {
            format: FORMAT_TAG.into(),
            version: FORMAT_VERSION,
            next_id: 1,
            devices: Vec::new(),
            links: Vec::new(),
            annotations: Vec::new(),
        }
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, DocError> {
        // Check the header first so a newer file reports a version error
        // rather than a confusing parse error on fields we don't know.
        let header: Header = serde_json::from_slice(bytes)?;
        if header.format != FORMAT_TAG {
            return Err(DocError::WrongFormat(header.format));
        }
        if header.version > FORMAT_VERSION {
            return Err(DocError::UnsupportedVersion {
                found: header.version,
                supported: FORMAT_VERSION,
            });
        }
        let mut project: Project = serde_json::from_slice(bytes)?;
        project.validate()?;
        // Older files are upgraded in memory and saved in the current format.
        project.version = FORMAT_VERSION;
        for a in &mut project.annotations {
            a.style = a.style.sanitised();
        }
        Ok(project)
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).expect("project model always serialises")
    }

    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    pub fn links(&self) -> &[Link] {
        &self.links
    }

    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    pub fn add_annotation(
        &mut self,
        kind: AnnotationKind,
        rect: Rect,
        text: &str,
        style: ShapeStyle,
    ) -> &Annotation {
        let annotation = Annotation {
            id: AnnotationId(self.allocate_id()),
            kind,
            rect: Rect::new(rect.x, rect.y, rect.width, rect.height),
            text: Annotation::clamp_text(text),
            style: style.sanitised(),
            locked: false,
            group: None,
        };
        self.annotations.push(annotation);
        self.annotations.last().expect("just pushed")
    }

    /// Replaces an annotation's frame and text; returns the previous version.
    pub fn update_annotation(
        &mut self,
        id: AnnotationId,
        rect: Rect,
        text: &str,
    ) -> Result<Annotation, DocError> {
        let annotation = self
            .annotations
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(DocError::UnknownAnnotation(id))?;
        let previous = annotation.clone();
        annotation.rect = Rect::new(rect.x, rect.y, rect.width, rect.height);
        annotation.text = Annotation::clamp_text(text);
        Ok(previous)
    }

    /// Replaces an annotation's look; returns the previous one.
    pub fn set_annotation_style(
        &mut self,
        id: AnnotationId,
        style: ShapeStyle,
    ) -> Result<ShapeStyle, DocError> {
        let annotation = self.annotation_mut(id)?;
        Ok(std::mem::replace(&mut annotation.style, style.sanitised()))
    }

    /// Locks or unlocks an annotation; returns the previous state.
    pub fn set_annotation_locked(
        &mut self,
        id: AnnotationId,
        locked: bool,
    ) -> Result<bool, DocError> {
        let annotation = self.annotation_mut(id)?;
        Ok(std::mem::replace(&mut annotation.locked, locked))
    }

    /// Moves an annotation to `index` in the stacking order (0 is the back,
    /// clamped to the front). Returns its previous index.
    pub fn reorder_annotation(
        &mut self,
        id: AnnotationId,
        index: usize,
    ) -> Result<usize, DocError> {
        let from = self
            .annotations
            .iter()
            .position(|a| a.id == id)
            .ok_or(DocError::UnknownAnnotation(id))?;
        let annotation = self.annotations.remove(from);
        let to = index.min(self.annotations.len());
        self.annotations.insert(to, annotation);
        Ok(from)
    }

    pub fn remove_annotation(&mut self, id: AnnotationId) -> Result<RemovedAnnotation, DocError> {
        let index = self
            .annotations
            .iter()
            .position(|a| a.id == id)
            .ok_or(DocError::UnknownAnnotation(id))?;
        Ok(RemovedAnnotation {
            annotation: self.annotations.remove(index),
            index,
        })
    }

    /// Reverses [`Project::remove_annotation`].
    pub fn restore_annotation(&mut self, removed: RemovedAnnotation) -> Result<(), DocError> {
        let mut candidate = self.clone();
        let index = removed.index.min(candidate.annotations.len());
        candidate.annotations.insert(index, removed.annotation);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub fn device(&self, id: DeviceId) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn add_device(&mut self, kind: DeviceKind, position: Point) -> &Device {
        let device = Device {
            id: DeviceId(self.allocate_id()),
            kind,
            name: self.next_free_name(kind),
            position: Point::new(position.x, position.y),
            ip: None,
            dhcp: false,
            color: None,
            group: None,
            config: DeviceConfig::default(),
            startup: None,
        };
        self.devices.push(device);
        self.devices.last().expect("just pushed")
    }

    pub fn move_device(&mut self, id: DeviceId, position: Point) -> Result<(), DocError> {
        let device = self.device_mut(id)?;
        device.position = Point::new(position.x, position.y);
        Ok(())
    }

    /// Renames a device. Names follow IOS hostname rules (1–63 characters,
    /// letters, digits and hyphens, starting with a letter and not ending in a
    /// hyphen) and are unique ignoring case. Returns the previous name.
    pub fn rename_device(&mut self, id: DeviceId, name: &str) -> Result<String, DocError> {
        let name = name.trim();
        validate_hostname(name)?;
        let taken = self
            .devices
            .iter()
            .any(|d| d.id != id && d.name.eq_ignore_ascii_case(name));
        if taken {
            return Err(DocError::DuplicateName(name.to_string()));
        }
        let device = self.device_mut(id)?;
        Ok(std::mem::replace(&mut device.name, name.to_string()))
    }

    /// Sets or clears an end device's IP settings; returns the previous ones.
    pub fn set_host_ip(
        &mut self,
        id: DeviceId,
        ip: Option<HostIp>,
    ) -> Result<Option<HostIp>, DocError> {
        let device = self.device_mut(id)?;
        if !device.kind.is_host() {
            return Err(DocError::NotAHost(device.name.clone()));
        }
        Ok(std::mem::replace(&mut device.ip, ip))
    }

    /// Replaces a router's or switch's running configuration; returns the previous one.
    pub fn set_device_config(
        &mut self,
        id: DeviceId,
        config: DeviceConfig,
    ) -> Result<DeviceConfig, DocError> {
        let device = self.device_mut(id)?;
        if !device.kind.has_cli() {
            return Err(DocError::NoCli(device.name.clone()));
        }
        config.validate(device.kind)?;
        Ok(std::mem::replace(&mut device.config, config))
    }

    /// `copy running-config startup-config`.
    pub fn save_startup_config(&mut self, id: DeviceId) -> Result<(), DocError> {
        let device = self.device_mut(id)?;
        device.startup = Some(StartupConfig {
            hostname: device.name.clone(),
            config: device.config.clone(),
        });
        Ok(())
    }

    /// `reload`: the running configuration becomes the startup one (or the
    /// factory defaults if none was saved). The saved hostname comes back
    /// unless another device has taken it meanwhile.
    pub fn reload_device(&mut self, id: DeviceId) -> Result<(), DocError> {
        let startup = self
            .device(id)
            .ok_or(DocError::UnknownDevice(id))?
            .startup
            .clone();
        match startup {
            Some(saved) => {
                let _ = self.rename_device(id, &saved.hostname);
                self.device_mut(id)?.config = saved.config;
            }
            None => self.device_mut(id)?.config = DeviceConfig::default(),
        }
        Ok(())
    }

    /// Changes whenever anything the simulation depends on changes (device
    /// kinds, names, addresses, configurations and cables), but not when
    /// devices move or are restyled. Stable within one run of the app.
    pub fn simulation_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for d in &self.devices {
            d.id.hash(&mut hasher);
            d.kind.hash(&mut hasher);
            d.name.hash(&mut hasher);
            d.ip.hash(&mut hasher);
            d.dhcp.hash(&mut hasher);
            serde_json::to_string(&d.config)
                .unwrap_or_default()
                .hash(&mut hasher);
        }
        for l in &self.links {
            (l.id, &l.a, &l.b, l.medium).hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn device_settings(&self, id: DeviceId) -> Result<DeviceSettings, DocError> {
        let d = self.device(id).ok_or(DocError::UnknownDevice(id))?;
        Ok(DeviceSettings {
            name: d.name.clone(),
            ip: d.ip,
            dhcp: d.dhcp,
            config: d.config.clone(),
            startup: d.startup.clone(),
        })
    }

    /// Puts back settings from [`Project::device_settings`] (undo of a CLI
    /// change). All or nothing.
    pub fn set_device_settings(
        &mut self,
        id: DeviceId,
        settings: DeviceSettings,
    ) -> Result<DeviceSettings, DocError> {
        let mut candidate = self.clone();
        let previous = candidate.device_settings(id)?;
        if previous.name != settings.name {
            candidate.rename_device(id, &settings.name)?;
        }
        let device = candidate.device_mut(id)?;
        if device.kind.has_cli() {
            settings.config.validate(device.kind)?;
        }
        device.ip = settings.ip.filter(|_| device.kind.is_host());
        device.dhcp = settings.dhcp && device.kind.is_host();
        device.config = settings.config;
        device.startup = settings.startup;
        *self = candidate;
        Ok(previous)
    }

    /// Tints a device symbol; returns the previous colour.
    pub fn set_device_color(
        &mut self,
        id: DeviceId,
        color: Option<Swatch>,
    ) -> Result<Option<Swatch>, DocError> {
        let device = self.device_mut(id)?;
        Ok(std::mem::replace(&mut device.color, color))
    }

    /// Sets a cable's colour and line pattern; returns the previous ones.
    pub fn set_link_style(
        &mut self,
        id: LinkId,
        color: Option<Swatch>,
        line: Option<LinePattern>,
    ) -> Result<(Option<Swatch>, Option<LinePattern>), DocError> {
        let link = self
            .links
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or(DocError::UnknownLink(id))?;
        let previous = (link.color, link.line);
        link.color = color;
        link.line = line;
        Ok(previous)
    }

    /// A fresh id for [`Project::set_groups`].
    pub fn new_group_id(&mut self) -> GroupId {
        GroupId(self.allocate_id())
    }

    pub fn group_of(&self, item: CanvasItem) -> Option<GroupId> {
        match item {
            CanvasItem::Device(id) => self.device(id)?.group,
            CanvasItem::Annotation(id) => self.annotations.iter().find(|a| a.id == id)?.group,
        }
    }

    /// Assigns each item to a group (or none). All or nothing: an unknown item
    /// changes nothing. Returns the previous assignments, for undo.
    pub fn set_groups(
        &mut self,
        changes: &[(CanvasItem, Option<GroupId>)],
    ) -> Result<Vec<(CanvasItem, Option<GroupId>)>, DocError> {
        let mut previous = Vec::with_capacity(changes.len());
        for &(item, _) in changes {
            let group = match item {
                CanvasItem::Device(id) => self.device(id).ok_or(DocError::UnknownDevice(id))?.group,
                CanvasItem::Annotation(id) => {
                    self.annotations
                        .iter()
                        .find(|a| a.id == id)
                        .ok_or(DocError::UnknownAnnotation(id))?
                        .group
                }
            };
            previous.push((item, group));
        }
        for &(item, group) in changes {
            match item {
                CanvasItem::Device(id) => self.device_mut(id)?.group = group,
                CanvasItem::Annotation(id) => self.annotation_mut(id)?.group = group,
            }
        }
        Ok(previous)
    }

    /// Switches an end device between DHCP and its static settings;
    /// returns the previous mode.
    pub fn set_host_dhcp(&mut self, id: DeviceId, dhcp: bool) -> Result<bool, DocError> {
        let device = self.device_mut(id)?;
        if !device.kind.is_host() {
            return Err(DocError::NotAHost(device.name.clone()));
        }
        Ok(std::mem::replace(&mut device.dhcp, dhcp))
    }

    /// Removes a device and every link attached to it.
    pub fn remove_device(&mut self, id: DeviceId) -> Result<RemovedDevice, DocError> {
        let index = self
            .devices
            .iter()
            .position(|d| d.id == id)
            .ok_or(DocError::UnknownDevice(id))?;
        let device = self.devices.remove(index);
        let (links, kept) = std::mem::take(&mut self.links)
            .into_iter()
            .partition(|l| l.a.device == id || l.b.device == id);
        self.links = kept;
        Ok(RemovedDevice {
            device,
            links,
            index,
        })
    }

    /// Reverses [`Project::remove_device`].
    pub fn restore_device(&mut self, removed: RemovedDevice) -> Result<(), DocError> {
        let mut candidate = self.clone();
        let index = removed.index.min(candidate.devices.len());
        candidate.devices.insert(index, removed.device);
        candidate.links.extend(removed.links);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub fn connect(&mut self, a: Endpoint, b: Endpoint, medium: Medium) -> Result<&Link, DocError> {
        let link = Link {
            id: LinkId(self.next_id),
            a,
            b,
            medium,
            color: None,
            line: None,
        };
        self.check_link(&link, &self.port_usage())?;
        self.allocate_id();
        self.links.push(link);
        Ok(self.links.last().expect("just pushed"))
    }

    pub fn disconnect(&mut self, id: LinkId) -> Result<Link, DocError> {
        let index = self
            .links
            .iter()
            .position(|l| l.id == id)
            .ok_or(DocError::UnknownLink(id))?;
        Ok(self.links.remove(index))
    }

    /// Reverses [`Project::disconnect`], keeping the link's original id.
    pub fn restore_link(&mut self, link: Link) -> Result<(), DocError> {
        let mut candidate = self.clone();
        candidate.links.push(link);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    fn device_mut(&mut self, id: DeviceId) -> Result<&mut Device, DocError> {
        self.devices
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(DocError::UnknownDevice(id))
    }

    fn annotation_mut(&mut self, id: AnnotationId) -> Result<&mut Annotation, DocError> {
        self.annotations
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(DocError::UnknownAnnotation(id))
    }

    fn allocate_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn next_free_name(&self, kind: DeviceKind) -> String {
        let taken: HashSet<&str> = self.devices.iter().map(|d| d.name.as_str()).collect();
        (1..)
            .map(|n| format!("{}{n}", kind.name_prefix()))
            .find(|name| !taken.contains(name.as_str()))
            .expect("unbounded range always yields a free name")
    }

    fn port_usage(&self) -> HashSet<&Endpoint> {
        self.links.iter().flat_map(|l| [&l.a, &l.b]).collect()
    }

    fn check_link(&self, link: &Link, in_use: &HashSet<&Endpoint>) -> Result<(), DocError> {
        if link.a == link.b {
            return Err(DocError::SelfLoop);
        }
        for end in [&link.a, &link.b] {
            let device = self
                .device(end.device)
                .ok_or(DocError::UnknownDevice(end.device))?;
            if !device.kind.has_port(&end.port) {
                return Err(DocError::UnknownPort {
                    kind: device.kind,
                    port: end.port.clone(),
                });
            }
            if in_use.contains(end) {
                return Err(DocError::PortInUse {
                    device: end.device,
                    port: end.port.clone(),
                });
            }
        }
        Ok(())
    }

    /// Checks every invariant the rest of netrace relies on.
    fn validate(&self) -> Result<(), DocError> {
        let mut device_ids = HashSet::new();
        for d in &self.devices {
            if !device_ids.insert(d.id) {
                return Err(DocError::DuplicateDevice(d.id));
            }
            if !d.config.is_default() {
                if !d.kind.has_cli() {
                    return Err(DocError::NoCli(d.name.clone()));
                }
                d.config.validate(d.kind)?;
            }
        }

        let mut link_ids = HashSet::new();
        let mut in_use = HashSet::new();
        for link in &self.links {
            if !link_ids.insert(link.id) {
                return Err(DocError::DuplicateLink(link.id));
            }
            self.check_link(link, &in_use)?;
            in_use.insert(&link.a);
            in_use.insert(&link.b);
        }

        let mut annotation_ids = HashSet::new();
        for a in &self.annotations {
            if !annotation_ids.insert(a.id) {
                return Err(DocError::DuplicateAnnotation(a.id));
            }
        }

        let highest = device_ids
            .iter()
            .map(|d| d.0)
            .chain(link_ids.iter().map(|l| l.0))
            .chain(annotation_ids.iter().map(|a| a.0))
            .chain(self.devices.iter().filter_map(|d| d.group).map(|g| g.0))
            .chain(self.annotations.iter().filter_map(|a| a.group).map(|g| g.0))
            .max()
            .unwrap_or(0);
        if self.next_id <= highest {
            return Err(DocError::StaleIdCounter {
                next_id: self.next_id,
                highest,
            });
        }
        Ok(())
    }
}

fn validate_hostname(name: &str) -> Result<(), DocError> {
    let invalid = |reason| Err(DocError::InvalidName(name.to_string(), reason));
    if name.is_empty() {
        return invalid("it is empty");
    }
    if name.len() > 63 {
        return invalid("it is longer than 63 characters");
    }
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return invalid("it must start with a letter");
    }
    if name.ends_with('-') {
        return invalid("it must not end with a hyphen");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return invalid("use only letters, digits and hyphens");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end(device: DeviceId, port: &str) -> Endpoint {
        Endpoint {
            device,
            port: port.into(),
        }
    }

    /// R1 Gi0/0 — SW1 Gi0/1, SW1 Fa0/1 — PC1.
    fn sample() -> (Project, DeviceId, DeviceId, DeviceId) {
        let mut p = Project::new();
        let r1 = p.add_device(DeviceKind::Router, Point::new(0.0, 0.0)).id;
        let sw1 = p
            .add_device(DeviceKind::L2Switch, Point::new(100.0, 0.0))
            .id;
        let pc1 = p.add_device(DeviceKind::Pc, Point::new(200.0, 0.0)).id;
        p.connect(
            end(r1, "GigabitEthernet0/0"),
            end(sw1, "GigabitEthernet0/1"),
            Medium::Copper,
        )
        .unwrap();
        p.connect(
            end(sw1, "FastEthernet0/1"),
            end(pc1, "FastEthernet0"),
            Medium::Copper,
        )
        .unwrap();
        (p, r1, sw1, pc1)
    }

    #[test]
    fn names_follow_lab_conventions_and_fill_gaps() {
        let mut p = Project::new();
        let names: Vec<_> = [DeviceKind::Router, DeviceKind::Router, DeviceKind::L2Switch]
            .into_iter()
            .map(|k| p.add_device(k, Point::default()).name.clone())
            .collect();
        assert_eq!(names, ["R1", "R2", "SW1"]);

        let r1 = p.devices()[0].id;
        p.remove_device(r1).unwrap();
        assert_eq!(
            p.add_device(DeviceKind::Router, Point::default()).name,
            "R1"
        );
    }

    #[test]
    fn rename_follows_hostname_rules() {
        let (mut p, r1, sw1, _) = sample();
        assert_eq!(p.rename_device(r1, "  Core-1 ").unwrap(), "R1");
        assert_eq!(p.device(r1).unwrap().name, "Core-1");

        for bad in ["", "1R", "R 1", "R1-", "R_1", &"x".repeat(64)] {
            assert!(
                matches!(p.rename_device(sw1, bad), Err(DocError::InvalidName(..))),
                "{bad:?} should be rejected"
            );
        }
        assert!(matches!(
            p.rename_device(sw1, "core-1"),
            Err(DocError::DuplicateName(_))
        ));
        // Renaming to its own name (any case) is allowed.
        assert!(p.rename_device(r1, "CORE-1").is_ok());
        assert!(matches!(
            p.rename_device(DeviceId(999), "X"),
            Err(DocError::UnknownDevice(_))
        ));
    }

    #[test]
    fn annotations_round_trip_and_share_the_id_counter() {
        let (mut p, ..) = sample();
        let area = p
            .add_annotation(
                AnnotationKind::Area,
                Rect::new(10.0, 20.0, 300.0, 200.0),
                "VLAN 10",
                ShapeStyle::default(),
            )
            .id;
        let note = p
            .add_annotation(
                AnnotationKind::Note,
                Rect::new(0.0, 0.0, 1.0, f64::NAN),
                "Lab 1",
                ShapeStyle::default(),
            )
            .id;
        assert!(
            area.0 > p.links().last().unwrap().id.0,
            "ids come from the shared counter"
        );
        // Sanitised: minimum size, finite values.
        let n = p.annotations().iter().find(|a| a.id == note).unwrap();
        assert_eq!(n.rect, Rect::new(0.0, 0.0, Rect::MIN_SIDE, Rect::MIN_SIDE));

        let loaded = Project::from_json(&p.to_json()).unwrap();
        assert_eq!(loaded, p);
    }

    #[test]
    fn annotation_update_remove_restore() {
        let mut p = Project::new();
        let id = p
            .add_annotation(
                AnnotationKind::Note,
                Rect::new(0.0, 0.0, 100.0, 50.0),
                "a",
                ShapeStyle::default(),
            )
            .id;
        let previous = p
            .update_annotation(id, Rect::new(5.0, 5.0, 120.0, 60.0), "b")
            .unwrap();
        assert_eq!(previous.text, "a");
        assert_eq!(p.annotations()[0].text, "b");

        let removed = p.remove_annotation(id).unwrap();
        assert!(p.annotations().is_empty());
        p.restore_annotation(removed.clone()).unwrap();
        assert_eq!(p.annotations()[0].text, "b");
        assert!(matches!(
            p.restore_annotation(removed),
            Err(DocError::DuplicateAnnotation(_))
        ));
        assert!(matches!(
            p.remove_annotation(AnnotationId(999)),
            Err(DocError::UnknownAnnotation(_))
        ));
    }

    #[test]
    fn annotation_text_is_clamped() {
        let mut p = Project::new();
        let long = "é".repeat(Annotation::MAX_TEXT + 10);
        let a = p.add_annotation(
            AnnotationKind::Note,
            Rect::new(0.0, 0.0, 50.0, 50.0),
            &long,
            ShapeStyle::default(),
        );
        assert_eq!(a.text.chars().count(), Annotation::MAX_TEXT);
    }

    #[test]
    fn files_without_annotations_still_load() {
        let old =
            br#"{ "format": "netrace", "version": 1, "next_id": 1, "devices": [], "links": [] }"#;
        assert!(Project::from_json(old).unwrap().annotations().is_empty());
    }

    #[test]
    fn version_1_files_load_with_default_styles_and_save_as_current() {
        let old = br#"{ "format": "netrace", "version": 1, "next_id": 3,
            "devices": [{ "id": 1, "kind": "pc", "name": "PC1", "position": { "x": 0, "y": 0 } }],
            "links": [],
            "annotations": [{ "id": 2, "kind": "area", "text": "VLAN 10",
                              "rect": { "x": 0, "y": 0, "width": 100, "height": 100 } }] }"#;
        let p = Project::from_json(old).unwrap();
        assert_eq!(p.annotations()[0].style, ShapeStyle::default());
        assert_eq!(p.devices()[0].color, None);
        let saved: serde_json::Value = serde_json::from_slice(&p.to_json()).unwrap();
        assert_eq!(saved["version"], FORMAT_VERSION);
    }

    #[test]
    fn styles_colours_and_locks_round_trip() {
        use crate::{ShapeKind, Swatch};
        let (mut p, r1, ..) = sample();
        let hex = ShapeStyle {
            shape: ShapeKind::Polygon { sides: 6 },
            stroke: Some(Swatch::Purple),
            fill: Some(Swatch::Purple),
            fill_opacity: 0.2,
            line: LinePattern::Solid,
            label: None,
        };
        let area = p
            .add_annotation(
                AnnotationKind::Area,
                Rect::new(0.0, 0.0, 200.0, 200.0),
                "",
                hex,
            )
            .id;
        let previous = p
            .set_annotation_style(
                area,
                ShapeStyle {
                    shape: ShapeKind::Polygon { sides: 99 },
                    ..hex
                },
            )
            .unwrap();
        assert_eq!(previous, hex);
        assert_eq!(
            p.annotations()[0].style.shape,
            ShapeKind::Polygon { sides: 12 }
        );
        assert!(!p.set_annotation_locked(area, true).unwrap());
        assert_eq!(p.set_device_color(r1, Some(Swatch::Blue)).unwrap(), None);
        let link = p.links()[0].id;
        assert_eq!(
            p.set_link_style(link, Some(Swatch::Orange), Some(LinePattern::Dotted))
                .unwrap(),
            (None, None)
        );
        assert_eq!(Project::from_json(&p.to_json()).unwrap(), p);
    }

    #[test]
    fn reorder_moves_within_the_stack() {
        let mut p = Project::new();
        let ids: Vec<_> = (0..3)
            .map(|i| {
                p.add_annotation(
                    AnnotationKind::Area,
                    Rect::new(0.0, 0.0, 50.0, 50.0),
                    &i.to_string(),
                    ShapeStyle::default(),
                )
                .id
            })
            .collect();
        assert_eq!(p.reorder_annotation(ids[0], usize::MAX).unwrap(), 0);
        let order: Vec<_> = p.annotations().iter().map(|a| a.id).collect();
        assert_eq!(order, [ids[1], ids[2], ids[0]]);
        assert_eq!(p.reorder_annotation(ids[0], 0).unwrap(), 2);
        assert_eq!(p.annotations()[0].id, ids[0]);
    }

    #[test]
    fn groups_are_all_or_nothing_and_survive_reload() {
        let (mut p, r1, sw1, _) = sample();
        let note = p
            .add_annotation(
                AnnotationKind::Note,
                Rect::new(0.0, 0.0, 50.0, 50.0),
                "",
                ShapeStyle::default(),
            )
            .id;
        let g = p.new_group_id();
        let items = [
            CanvasItem::Device(r1),
            CanvasItem::Device(sw1),
            CanvasItem::Annotation(note),
        ];
        let changes: Vec<_> = items.iter().map(|&i| (i, Some(g))).collect();
        let previous = p.set_groups(&changes).unwrap();
        assert!(previous.iter().all(|(_, g)| g.is_none()));
        assert_eq!(p.group_of(CanvasItem::Annotation(note)), Some(g));

        let before = p.clone();
        let bad = [
            (CanvasItem::Device(sw1), None),
            (CanvasItem::Device(DeviceId(999)), None),
        ];
        assert!(p.set_groups(&bad).is_err());
        assert_eq!(p, before);

        // A group id counts toward the id counter check on load.
        assert_eq!(Project::from_json(&p.to_json()).unwrap(), p);
        assert!(p.add_device(DeviceKind::Pc, Point::default()).id.0 > g.0);
    }

    #[test]
    fn configs_save_reload_and_undo() {
        use crate::{InterfaceAddress, RouteTarget, StaticRoute};
        let (mut p, r1, _, pc1) = sample();
        let mut config = DeviceConfig::default();
        let gi0 = config.interface_mut(DeviceKind::Router, "GigabitEthernet0/0");
        gi0.address = Some(InterfaceAddress {
            address: "10.0.0.1".parse().unwrap(),
            prefix_len: 24,
        });
        gi0.shutdown = false;
        config.routes.push(StaticRoute {
            network: "10.9.0.0".parse().unwrap(),
            prefix_len: 16,
            target: RouteTarget::NextHop("10.0.0.2".parse().unwrap()),
            distance: 1,
        });
        let before = p.device_settings(r1).unwrap();
        let key = p.simulation_key();
        p.set_device_config(r1, config.clone()).unwrap();
        assert_ne!(p.simulation_key(), key);
        assert_eq!(Project::from_json(&p.to_json()).unwrap(), p);

        // Saved, changed, reloaded: back to what was saved.
        p.save_startup_config(r1).unwrap();
        p.rename_device(r1, "Edge").unwrap();
        p.set_device_config(r1, DeviceConfig::default()).unwrap();
        p.reload_device(r1).unwrap();
        assert_eq!(p.device(r1).unwrap().name, "R1");
        assert_eq!(p.device(r1).unwrap().config, config);

        // Undo puts everything back in one step.
        p.set_device_settings(r1, before.clone()).unwrap();
        assert_eq!(p.device_settings(r1).unwrap(), before);

        // Hosts have no CLI; bad interface names are refused.
        assert!(matches!(
            p.set_device_config(pc1, config),
            Err(DocError::NoCli(_))
        ));
        let mut bad = DeviceConfig::default();
        bad.interface_mut(DeviceKind::Router, "Vlan1");
        assert!(matches!(
            p.set_device_config(r1, bad),
            Err(DocError::InvalidConfig(_))
        ));
    }

    #[test]
    fn host_ip_round_trips_and_is_limited_to_hosts() {
        let (mut p, r1, _, pc1) = sample();
        let ip = HostIp::new(
            "192.168.1.10".parse().unwrap(),
            24,
            Some("192.168.1.1".parse().unwrap()),
        )
        .unwrap();
        assert_eq!(p.set_host_ip(pc1, Some(ip)).unwrap(), None);
        assert_eq!(p.device(pc1).unwrap().ip, Some(ip));
        assert_eq!(Project::from_json(&p.to_json()).unwrap(), p);
        assert!(matches!(
            p.set_host_ip(r1, Some(ip)),
            Err(DocError::NotAHost(_))
        ));
        assert_eq!(p.set_host_ip(pc1, None).unwrap(), Some(ip));
    }

    #[test]
    fn ids_are_never_reused() {
        let mut p = Project::new();
        let a = p.add_device(DeviceKind::Pc, Point::default()).id;
        p.remove_device(a).unwrap();
        let b = p.add_device(DeviceKind::Pc, Point::default()).id;
        assert_ne!(a, b);
    }

    #[test]
    fn json_round_trip_preserves_everything() {
        let (p, ..) = sample();
        let loaded = Project::from_json(&p.to_json()).unwrap();
        assert_eq!(loaded, p);
    }

    #[test]
    fn json_shape_is_stable() {
        let mut p = Project::new();
        p.add_device(DeviceKind::L3Switch, Point::new(1.5, -2.0));
        let value: serde_json::Value = serde_json::from_slice(&p.to_json()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "format": "netrace",
                "version": 2,
                "next_id": 2,
                "devices": [
                    { "id": 1, "kind": "l3_switch", "name": "MLS1", "position": { "x": 1.5, "y": -2.0 } }
                ],
                "links": [],
                "annotations": []
            })
        );
    }

    #[test]
    fn non_finite_positions_are_sanitised() {
        let mut p = Project::new();
        let id = p
            .add_device(DeviceKind::Pc, Point::new(f64::NAN, f64::INFINITY))
            .id;
        assert_eq!(p.device(id).unwrap().position, Point::new(0.0, 0.0));
        assert!(Project::from_json(&p.to_json()).is_ok());
    }

    #[test]
    fn removing_a_device_takes_its_links_and_restore_puts_them_back() {
        let (mut p, _, sw1, _) = sample();
        let before = p.clone();

        let removed = p.remove_device(sw1).unwrap();
        assert_eq!(removed.links.len(), 2);
        assert!(p.links().is_empty());

        p.restore_device(removed).unwrap();
        assert_eq!(p.devices(), before.devices());
        assert_eq!(p.links().len(), 2);
    }

    #[test]
    fn restore_rejects_conflicting_state() {
        let (mut p, r1, ..) = sample();
        let removed = p.remove_device(r1).unwrap();
        p.restore_device(removed.clone()).unwrap();
        // Restoring the same device twice would duplicate its id.
        assert!(matches!(
            p.restore_device(removed),
            Err(DocError::DuplicateDevice(_))
        ));
    }

    #[test]
    fn connect_validates_ports() {
        let (mut p, r1, sw1, pc1) = sample();
        assert!(matches!(
            p.connect(
                end(r1, "GigabitEthernet0/9"),
                end(sw1, "FastEthernet0/2"),
                Medium::Copper
            ),
            Err(DocError::UnknownPort { .. })
        ));
        assert!(matches!(
            p.connect(
                end(r1, "GigabitEthernet0/1"),
                end(pc1, "FastEthernet0"),
                Medium::Copper
            ),
            Err(DocError::PortInUse { .. })
        ));
        assert!(matches!(
            p.connect(
                end(r1, "GigabitEthernet0/1"),
                end(r1, "GigabitEthernet0/1"),
                Medium::Copper
            ),
            Err(DocError::SelfLoop)
        ));
        assert!(matches!(
            p.connect(
                end(DeviceId(999), "FastEthernet0"),
                end(r1, "GigabitEthernet0/1"),
                Medium::Copper
            ),
            Err(DocError::UnknownDevice(DeviceId(999)))
        ));
    }

    #[test]
    fn failed_connect_does_not_consume_an_id() {
        let (mut p, r1, ..) = sample();
        let before = p.clone();
        let _ = p.connect(
            end(r1, "nope"),
            end(r1, "GigabitEthernet0/1"),
            Medium::Copper,
        );
        assert_eq!(p, before);
    }

    #[test]
    fn disconnect_and_restore_link() {
        let (mut p, ..) = sample();
        let id = p.links()[0].id;
        let link = p.disconnect(id).unwrap();
        assert_eq!(p.links().len(), 1);
        p.restore_link(link).unwrap();
        assert!(p.links().iter().any(|l| l.id == id));
        assert!(matches!(
            p.disconnect(LinkId(999)),
            Err(DocError::UnknownLink(_))
        ));
    }

    #[test]
    fn bundled_examples_load() {
        // Samples are meant to be edited in the app, so only check they stay valid.
        for bytes in [
            &include_bytes!("../../../examples/starter.netrace/project.json")[..],
            &include_bytes!("../../../examples/campus.netrace/project.json")[..],
            &include_bytes!("../../../examples/office-lan.netrace/project.json")[..],
            &include_bytes!("../../../examples/two-lans.netrace/project.json")[..],
            &include_bytes!("../../../examples/vlans.netrace/project.json")[..],
            &include_bytes!("../../../examples/services.netrace/project.json")[..],
            &include_bytes!("../../../examples/nat.netrace/project.json")[..],
            &include_bytes!("../../../examples/ospf.netrace/project.json")[..],
        ] {
            let p = Project::from_json(bytes).unwrap();
            assert!(!p.devices().is_empty());
        }
    }

    #[test]
    fn rejects_foreign_and_future_files() {
        assert!(matches!(
            Project::from_json(br#"{"format":"pkt","version":1}"#),
            Err(DocError::WrongFormat(f)) if f == "pkt"
        ));
        assert!(matches!(
            Project::from_json(br#"{"format":"netrace","version":99,"whatever":true}"#),
            Err(DocError::UnsupportedVersion { found: 99, .. })
        ));
        assert!(matches!(
            Project::from_json(b"not json"),
            Err(DocError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_files_with_broken_references() {
        let dangling = br#"{
            "format": "netrace", "version": 1, "next_id": 3,
            "devices": [{ "id": 1, "kind": "pc", "name": "PC1", "position": { "x": 0, "y": 0 } }],
            "links": [{ "id": 2, "medium": "copper",
                        "a": { "device": 1, "port": "FastEthernet0" },
                        "b": { "device": 7, "port": "FastEthernet0" } }]
        }"#;
        assert!(matches!(
            Project::from_json(dangling),
            Err(DocError::UnknownDevice(DeviceId(7)))
        ));

        let stale_counter = br#"{
            "format": "netrace", "version": 1, "next_id": 1,
            "devices": [{ "id": 1, "kind": "pc", "name": "PC1", "position": { "x": 0, "y": 0 } }],
            "links": []
        }"#;
        assert!(matches!(
            Project::from_json(stale_counter),
            Err(DocError::StaleIdCounter { .. })
        ));
    }
}
