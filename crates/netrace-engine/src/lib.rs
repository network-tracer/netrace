//! netrace's network simulation.
//!
//! A [`Network`] is built from a [`netrace_doc::Project`] and runs on the
//! deterministic virtual clock from `netrace-sim`. Commands (such as a ping)
//! are scheduled at a point in virtual time; running the network produces a
//! [`TraceEvent`] for every frame a device handles, each with a plain-language
//! explanation and per-layer notes for the packet inspector.
//!
//! The network is rebuilt from scratch and every command replayed whenever a
//! command is added, so the trace is always a pure function of the topology
//! and the commands. That keeps simulation mode freely scrubbable.

mod explain;
mod frame;
mod mac;
mod network;
mod report;
#[cfg(test)]
mod tests;
mod trace;

pub use frame::{ArpOp, ArpPacket, Frame, Icmp, IcmpKind, Ipv4Packet, Payload, Protocol};
pub use mac::MacAddr;
pub use network::{EngineError, HOP_LATENCY, Network, PingFailure, PingSession};
pub use trace::{Field, Layer, LayerNote, LayerRole, Outcome, TraceEvent};

/// Short interface name, as IOS prints it: "GigabitEthernet0/1" → "Gi0/1".
pub fn abbreviate_port(port: &str) -> String {
    for (long, short) in [
        ("GigabitEthernet", "Gi"),
        ("FastEthernet", "Fa"),
        ("Ethernet", "Eth"),
    ] {
        if let Some(rest) = port.strip_prefix(long) {
            return format!("{short}{rest}");
        }
    }
    port.to_string()
}
