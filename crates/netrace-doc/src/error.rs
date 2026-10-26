use crate::{AnnotationId, DeviceId, DeviceKind, LinkId};

#[derive(Debug, thiserror::Error)]
pub enum DocError {
    #[error("not a valid netrace project: {0}")]
    Malformed(#[from] serde_json::Error),

    #[error("expected a netrace project file, found format \"{0}\"")]
    WrongFormat(String),

    #[error(
        "project was saved by a newer version of netrace (format {found}, this app reads up to {supported})"
    )]
    UnsupportedVersion { found: u32, supported: u32 },

    #[error("device {0} appears more than once")]
    DuplicateDevice(DeviceId),

    #[error("link {0} appears more than once")]
    DuplicateLink(LinkId),

    #[error("annotation {0} appears more than once")]
    DuplicateAnnotation(AnnotationId),

    #[error("annotation {0} does not exist")]
    UnknownAnnotation(AnnotationId),

    #[error("device {0} does not exist")]
    UnknownDevice(DeviceId),

    #[error("link {0} does not exist")]
    UnknownLink(LinkId),

    #[error("{kind:?} has no port named \"{port}\"")]
    UnknownPort { kind: DeviceKind, port: String },

    #[error("port {port} on device {device} is already connected")]
    PortInUse { device: DeviceId, port: String },

    #[error("a link cannot connect a port to itself")]
    SelfLoop,

    #[error("\"{0}\" is not a valid hostname: {1}")]
    InvalidName(String, &'static str),

    #[error("invalid IP settings: {0}")]
    InvalidIp(String),

    #[error("{0} has no Ethernet port to configure")]
    NotAHost(String),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("{0} has no command line")]
    NoCli(String),

    #[error("another device is already named \"{0}\"")]
    DuplicateName(String),

    #[error("next_id {next_id} is not above the highest id in use ({highest})")]
    StaleIdCounter { next_id: u64, highest: u64 },
}
