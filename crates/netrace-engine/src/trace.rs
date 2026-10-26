use netrace_doc::{DeviceId, LinkId};
use netrace_sim::SimTime;

use crate::Frame;

/// Where in the stack a note applies. Ordered bottom-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Physical,
    DataLink,
    Network,
    /// The message carried inside IP (ICMP for now).
    Message,
}

impl Layer {
    pub fn number(self) -> u8 {
        match self {
            Layer::Physical => 1,
            Layer::DataLink => 2,
            Layer::Network => 3,
            Layer::Message => 4,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Layer::Physical => "Physical",
            Layer::DataLink => "Data link",
            Layer::Network => "Network",
            Layer::Message => "Message",
        }
    }
}

/// What the device did with a layer at this hop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerRole {
    /// Read and acted on.
    Used,
    /// Built or rewritten here.
    Changed,
    /// Not looked at by this device.
    NotUsed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub value: String,
    /// Set by this device (built or rewritten here).
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerNote {
    pub layer: Layer,
    pub role: LayerRole,
    /// One plain sentence about this layer at this hop.
    pub note: String,
    pub fields: Vec<Field>,
}

/// What happened to the frame at this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The device created and sent a new frame.
    Created,
    /// A switch sent it out one port.
    Forwarded,
    /// Sent out several ports (broadcast, unknown destination, or a hub).
    Flooded,
    /// Addressed to this device, which replied.
    Replied,
    /// Addressed to this device; it learned something and is done.
    Accepted,
    /// Received but not for this device.
    Ignored,
    /// Could not be delivered or processed.
    Dropped,
    /// A ping finished (reply received or timed out).
    Completed,
}

/// One step of the simulation, as shown in the event list and inspector.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceEvent {
    pub id: u64,
    /// When the device handled the frame.
    pub time: SimTime,
    /// When the frame left the previous device, for animating the hop.
    pub sent: Option<SimTime>,
    pub device: DeviceId,
    pub from: Option<DeviceId>,
    pub link: Option<LinkId>,
    /// Port the frame arrived on.
    pub ingress: Option<String>,
    /// Ports the device sent frames out of while handling this event.
    pub egress: Vec<String>,
    pub frame: Option<Frame>,
    pub outcome: Outcome,
    /// The plain-language explanation of this step.
    pub summary: String,
    pub layers: Vec<LayerNote>,
    /// The ping this step belongs to, if any.
    pub session: Option<u64>,
    /// The access list this device checked, and its verdict.
    pub acl: Option<crate::AclDecision>,
}
