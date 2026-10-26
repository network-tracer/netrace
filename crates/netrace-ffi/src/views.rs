//! The VLAN view and route paths, for the canvas and the VLAN pane.

use std::collections::BTreeSet;

use netrace_doc as doc;
use netrace_engine as engine;

use crate::Simulation;

/// A port (or interface) that belongs to a VLAN.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct VlanMember {
    pub device_id: u64,
    /// The physical port, for the canvas.
    pub port: String,
    /// "access", "trunk", "subinterface" or "svi".
    pub mode: String,
    /// Spanning tree lets the VLAN's frames through.
    pub forwarding: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VlanInfo {
    pub vlan: u16,
    pub name: String,
    pub members: Vec<VlanMember>,
    /// End devices on the VLAN's access ports.
    pub hosts: Vec<u64>,
    /// Router subinterfaces and SVIs that route for the VLAN:
    /// "R1 Gi0/0.10 192.168.10.1/24".
    pub gateways: Vec<String>,
    pub root: Option<u64>,
}

/// Where a router's routes leave: one entry per exit port.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RoutePath {
    pub port: String,
    /// "C 192.168.1.0/24", "O 10.0.3.0/24 via 10.0.12.2".
    pub routes: Vec<String>,
}

#[uniffi::export]
impl Simulation {
    /// Every VLAN on the switches, with the ports, hosts and gateways in it.
    pub fn vlan_view(&self) -> Vec<VlanInfo> {
        let network = self.lock();
        let project = network.project();
        let l2 = network.layer2();
        let peer = |id: doc::DeviceId, port: &str| {
            project.links().iter().find_map(|l| {
                if l.a.device == id && l.a.port == port {
                    Some(l.b.device)
                } else if l.b.device == id && l.b.port == port {
                    Some(l.a.device)
                } else {
                    None
                }
            })
        };
        let vlans: BTreeSet<u16> = project
            .devices()
            .iter()
            .filter(|d| d.kind.is_switch())
            .flat_map(|d| d.config.vlan_ids())
            .collect();
        vlans
            .into_iter()
            .map(|vlan| {
                let mut members = Vec::new();
                let mut hosts = BTreeSet::new();
                let mut gateways = Vec::new();
                let mut name = String::new();
                for d in project.devices() {
                    if d.kind.is_switch() {
                        if let Some(v) = d.config.vlans.get(&vlan)
                            && name.is_empty()
                        {
                            name = v.name.clone().unwrap_or_default();
                        }
                        for port in d.kind.port_names() {
                            let sp = d.config.interface(d.kind, &port).switchport;
                            let Some(other) = peer(d.id, &port) else {
                                continue;
                            };
                            if sp.routed || !sp.carries(vlan) {
                                continue;
                            }
                            if !sp.is_trunk()
                                && project.device(other).is_some_and(|o| o.kind.is_host())
                            {
                                hosts.insert(other.0);
                            }
                            members.push(VlanMember {
                                device_id: d.id.0,
                                forwarding: l2.forwards(d.id, &port, vlan),
                                mode: if sp.is_trunk() { "trunk" } else { "access" }.into(),
                                port,
                            });
                        }
                    }
                    // Gateways: router subinterfaces and switch SVIs for the VLAN.
                    for (iface, cfg) in &d.config.interfaces {
                        let routes_vlan = cfg.encapsulation == Some(vlan)
                            || (iface == &format!("Vlan{vlan}")
                                && d.kind == doc::DeviceKind::L3Switch);
                        if !routes_vlan {
                            continue;
                        }
                        let address = cfg
                            .address
                            .map(|a| format!(" {}/{}", a.address, a.prefix_len))
                            .unwrap_or_default();
                        gateways.push(format!(
                            "{} {}{address}",
                            d.name,
                            engine::abbreviate_port(iface)
                        ));
                        if let Some(port) = iface.split('.').next().filter(|_| iface.contains('.'))
                        {
                            members.push(VlanMember {
                                device_id: d.id.0,
                                port: port.to_string(),
                                mode: "subinterface".into(),
                                forwarding: true,
                            });
                        }
                    }
                }
                if name.is_empty() {
                    name = if vlan == 1 {
                        "default".into()
                    } else {
                        format!("VLAN{vlan:04}")
                    };
                }
                VlanInfo {
                    vlan,
                    name,
                    members,
                    hosts: hosts.into_iter().collect(),
                    gateways,
                    root: l2
                        .trees
                        .get(&vlan)
                        .and_then(|t| t.island_roots().first().copied())
                        .map(|r| r.0),
                }
            })
            .collect()
    }

    /// A router's routes grouped by the port they leave from (routes out
    /// of loopbacks or SVIs have no cable to draw, so they're left out).
    pub fn route_paths(&self, device: u64) -> Vec<RoutePath> {
        let network = self.lock();
        let id = doc::DeviceId(device);
        let Some(d) = network.project().device(id) else {
            return Vec::new();
        };
        let mut paths: Vec<RoutePath> = Vec::new();
        for r in engine::routing_table(network.project(), id) {
            if r.source == engine::RouteSource::Local {
                continue;
            }
            let port = r.iface.split('.').next().unwrap_or(&r.iface).to_string();
            if !d.kind.has_port(&port) {
                continue;
            }
            let star = if r.is_default() { "*" } else { "" };
            let via = r.next_hop.map(|n| format!(" via {n}")).unwrap_or_default();
            let line = format!(
                "{}{star} {}/{}{via}",
                r.source.code(),
                r.network,
                r.prefix_len
            );
            match paths.iter_mut().find(|p| p.port == port) {
                Some(p) => {
                    if !p.routes.contains(&line) {
                        p.routes.push(line)
                    }
                }
                None => paths.push(RoutePath {
                    port,
                    routes: vec![line],
                }),
            }
        }
        paths
    }
}
