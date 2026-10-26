//! The netrace project model and its on-disk JSON format.
//!
//! A `.netrace` document is a package (directory) whose `project.json` holds a
//! serialised [`Project`]. This crate owns that format: loading validates every
//! reference so the rest of the app can trust the model it gets.

mod annotation;
mod config;
mod device;
mod error;
mod ip;
mod lab;
mod ospf;
mod project;
mod services;
mod style;
mod switching;

pub use annotation::{Annotation, AnnotationId, AnnotationKind, Rect, RemovedAnnotation};
pub use config::{
    DeviceConfig, InterfaceAddress, InterfaceConfig, LineConfig, RouteTarget, StartupConfig,
    StaticRoute, mask,
};
pub use device::{DeviceKind, Medium};
pub use error::DocError;
pub use ip::{HostIp, prefix_from_mask};
pub use lab::{Lab, LabCheck, LabTask};
pub use ospf::{OspfInterface, OspfNetwork, OspfProcess};
pub use project::{
    CanvasItem, Device, DeviceId, DeviceSettings, Endpoint, Link, LinkId, Point, Project,
    RemovedDevice,
};
pub use services::{
    AccessList, AclAction, AclAddress, AclEntry, AclKind, AclProtocol, DhcpPool, IcmpMatch,
    NatPool, NatRule, NatSide, NatTarget, PortMatch, port_name,
};
pub use style::{GroupId, LinePattern, ShapeKind, ShapeStyle, Swatch};
pub use switching::{
    PortMode, PortSecurity, StpConfig, StpMode, SwitchportConfig, Violation, VlanConfig, VlanSet,
};

/// File inside the `.netrace` package that holds the project JSON.
pub const PROJECT_FILE_NAME: &str = "project.json";

/// Current on-disk format version. Bump when the JSON shape changes.
///
/// 2: colours on devices and cables, styled shapes, locking and groups;
///    router and switch configurations (running and startup).
/// Version 1 files still load (the new fields have defaults) and are saved as 2.
pub const FORMAT_VERSION: u32 = 2;
