use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    Annotation, AnnotationId, AnnotationKind, DeviceKind, DocError, FORMAT_VERSION, Medium, Rect,
    RemovedAnnotation,
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
        let project: Project = serde_json::from_slice(bytes)?;
        project.validate()?;
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

    pub fn add_annotation(&mut self, kind: AnnotationKind, rect: Rect, text: &str) -> &Annotation {
        let annotation = Annotation {
            id: AnnotationId(self.allocate_id()),
            kind,
            rect: Rect::new(rect.x, rect.y, rect.width, rect.height),
            text: Annotation::clamp_text(text),
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
            )
            .id;
        let note = p
            .add_annotation(
                AnnotationKind::Note,
                Rect::new(0.0, 0.0, 1.0, f64::NAN),
                "Lab 1",
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
            .add_annotation(AnnotationKind::Note, Rect::new(0.0, 0.0, 100.0, 50.0), "a")
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
        let a = p.add_annotation(AnnotationKind::Note, Rect::new(0.0, 0.0, 50.0, 50.0), &long);
        assert_eq!(a.text.chars().count(), Annotation::MAX_TEXT);
    }

    #[test]
    fn files_without_annotations_still_load() {
        let old =
            br#"{ "format": "netrace", "version": 1, "next_id": 1, "devices": [], "links": [] }"#;
        assert!(Project::from_json(old).unwrap().annotations().is_empty());
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
                "version": 1,
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
