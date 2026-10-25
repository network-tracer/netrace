//! The netrace project model and its on-disk JSON format.
//!
//! A `.netrace` document is a package (directory) whose `project.json` holds a
//! serialised [`Project`]. This crate owns that format: loading validates every
//! reference so the rest of the app can trust the model it gets.

mod annotation;
mod device;
mod error;
mod project;

pub use annotation::{Annotation, AnnotationId, AnnotationKind, Rect, RemovedAnnotation};
pub use device::{DeviceKind, Medium};
pub use error::DocError;
pub use project::{Device, DeviceId, Endpoint, Link, LinkId, Point, Project, RemovedDevice};

/// File inside the `.netrace` package that holds the project JSON.
pub const PROJECT_FILE_NAME: &str = "project.json";

/// Current on-disk format version. Bump when the JSON shape changes.
pub const FORMAT_VERSION: u32 = 1;
