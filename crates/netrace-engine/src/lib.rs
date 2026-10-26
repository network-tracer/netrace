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

mod acl;
mod dhcp;
mod explain;
mod frame;
mod mac;
mod nat;
mod network;
mod ospf;
mod probe;
mod report;
mod routing;
mod stack;
mod stp;
mod switching;
mod tcp;
#[cfg(test)]
mod tests;
mod trace;
mod views;

pub use acl::{AclDecision, AclDirection};
pub use dhcp::DhcpBinding;
pub use frame::{
    ArpOp, ArpPacket, DHCP_CLIENT_PORT, DHCP_SERVER_PORT, Dhcp, DhcpOp, Frame, Icmp, IcmpKind,
    Ipv4Packet, L4, Payload, Protocol, Tcp, TcpFlags, Udp, Unreachable,
};
pub use mac::MacAddr;
pub use nat::{NatDecision, NatDirection, NatEntry, NatKind, NatProto};
pub use network::{EngineError, HOP_LATENCY, Network};
pub use ospf::{
    Lsa, LsaKind, NeighborState, OSPF_DISTANCE, Ospf, OspfChange, OspfIfState, OspfIface,
    OspfNeighbor, OspfNetType, OspfRoute, OspfRouteKind, OspfRouter, OspfSegment,
};
pub use probe::{
    MAX_HOPS, PROBES_PER_HOP, PingFailure, PingSession, Probe, ProbeResult, ReportStyle,
    SessionKind,
};
pub use stack::{Route, RouteSource};
pub use stp::{BridgeId, Layer2, PortRole, StpBridge, StpChange, StpPort, VlanTree, bridge_mac};
pub use tcp::{TcpApp, TcpConn, TcpResult, service};
pub use trace::{Field, Layer, LayerNote, LayerRole, Outcome, TraceEvent};
pub use views::{
    ArpRow, InterfaceRow, MacRow, Neighbor, discovery_neighbors, interface_rows, natural_key,
    routing_table,
};

/// Short interface name, as IOS prints it: "GigabitEthernet0/1" → "Gi0/1".
pub fn abbreviate_port(port: &str) -> String {
    for (long, short) in [
        ("GigabitEthernet", "Gi"),
        ("FastEthernet", "Fa"),
        ("Ethernet", "Eth"),
        ("Loopback", "Lo"),
    ] {
        if let Some(rest) = port.strip_prefix(long) {
            return format!("{short}{rest}");
        }
    }
    port.to_string()
}
