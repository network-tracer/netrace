//! OSPF commands and their `show` output.

use std::fmt::Write;
use std::net::Ipv4Addr;

use netrace_doc::{Device, DeviceKind, OspfNetwork, OspfProcess, Project};
use netrace_engine::{
    LsaKind, NeighborState, Network, OspfIfState, OspfNetType, OspfRouter, abbreviate_port,
};

use crate::grammar::Value;
use crate::{Mode, Session};

impl Session {
    /// Runs an OSPF command; `None` if `id` isn't one.
    pub(crate) fn ospf(
        &mut self,
        id: &str,
        values: &[Value],
        kind: DeviceKind,
        project: &mut Project,
        network: &Network,
    ) -> Option<String> {
        let ips: Vec<Ipv4Addr> = values
            .iter()
            .filter_map(|v| match v {
                Value::Ip(ip) => Some(*ip),
                _ => None,
            })
            .collect();
        let nums: Vec<u32> = values
            .iter()
            .filter_map(|v| match v {
                Value::Num(n) => Some(*n),
                _ => None,
            })
            .collect();
        let line = values.iter().find_map(|v| match v {
            Value::Line(w) | Value::Word(w) => Some(w.clone()),
            _ => None,
        });
        let iface = values.iter().find_map(|v| match v {
            Value::Iface(n) => Some(n.clone()),
            _ => None,
        });
        let device = project.device(self.device).cloned()?;
        let pid = match self.mode {
            Mode::RouterOspf(pid) => Some(pid),
            _ => None,
        };
        // Edits the process the session is in.
        let process = |s: &Session, project: &mut Project, edit: &dyn Fn(&mut OspfProcess)| {
            let Some(pid) = pid else { return };
            s.update(project, |c| {
                if let Some(p) = c.ospf.iter_mut().find(|p| p.process_id == pid) {
                    edit(p);
                }
            });
        };
        let out = match id {
            "router-ospf" => {
                let pid = nums[0] as u16;
                self.update(project, |c| {
                    if !c.ospf.iter().any(|p| p.process_id == pid) {
                        c.ospf.push(OspfProcess::new(pid));
                    }
                });
                self.mode = Mode::RouterOspf(pid);
                String::new()
            }
            "no-router-ospf" => {
                let pid = nums[0] as u16;
                self.update(project, |c| c.ospf.retain(|p| p.process_id != pid));
                String::new()
            }
            "ospf-network" | "no-ospf-network" => {
                let (address, wildcard, area) = (ips[0], ips[1], nums[0]);
                let add = id == "ospf-network";
                process(self, project, &|p| {
                    p.networks
                        .retain(|n| !(n.address == address && n.wildcard == wildcard));
                    if add {
                        p.networks.push(OspfNetwork {
                            address,
                            wildcard,
                            area,
                        });
                    }
                });
                String::new()
            }
            "ospf-router-id" => {
                let rid = ips[0];
                process(self, project, &|p| p.router_id = Some(rid));
                String::new()
            }
            "no-ospf-router-id" => {
                process(self, project, &|p| p.router_id = None);
                String::new()
            }
            "ospf-passive-default" | "no-ospf-passive-default" => {
                let on = id == "ospf-passive-default";
                process(self, project, &|p| {
                    p.passive_default = on;
                    p.passive.clear();
                    p.active.clear();
                });
                String::new()
            }
            "ospf-passive" | "no-ospf-passive" => {
                let name = iface?;
                let passive = id == "ospf-passive";
                process(self, project, &|p| {
                    p.passive.retain(|x| *x != name);
                    p.active.retain(|x| *x != name);
                    match (p.passive_default, passive) {
                        (false, true) => p.passive.push(name.clone()),
                        (true, false) => p.active.push(name.clone()),
                        _ => {}
                    }
                });
                String::new()
            }
            "ospf-default-originate" | "no-ospf-default-originate" => {
                let on = id == "ospf-default-originate";
                process(self, project, &|p| p.default_originate = on);
                String::new()
            }
            "ospf-ref-bw" | "no-ospf-ref-bw" => {
                let bw = if id == "ospf-ref-bw" { nums[0] } else { 100 };
                process(self, project, &|p| p.reference_bandwidth = bw);
                if id == "ospf-ref-bw" {
                    "% OSPF: Reference bandwidth is changed.\n        Please ensure reference bandwidth is consistent across all routers.".into()
                } else {
                    String::new()
                }
            }
            "if-ospf-cost" | "if-ospf-priority" | "if-ospf-hello" | "if-ospf-dead"
            | "if-ospf-area" | "if-ospf-p2p" | "if-ospf-broadcast" | "no-if-ospf" => {
                let names = self.interface_names();
                let words: Vec<String> = line
                    .clone()
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_lowercase)
                    .collect();
                self.update(project, |c| {
                    for n in &names {
                        let o = &mut c.interface_mut(kind, n).ospf;
                        match id {
                            "if-ospf-cost" => o.cost = Some(nums[0]),
                            "if-ospf-priority" => o.priority = Some(nums[0] as u8),
                            "if-ospf-hello" => o.hello = Some(nums[0] as u16),
                            "if-ospf-dead" => o.dead = Some(nums[0] as u16),
                            "if-ospf-area" => o.area = Some((nums[0] as u16, nums[1])),
                            "if-ospf-p2p" => o.point_to_point = true,
                            "if-ospf-broadcast" => o.point_to_point = false,
                            _ => match words.first().map(String::as_str) {
                                Some("cost") => o.cost = None,
                                Some("priority") => o.priority = None,
                                Some("hello-interval") => o.hello = None,
                                Some("dead-interval") => o.dead = None,
                                Some("network") => o.point_to_point = false,
                                Some(w) if w.parse::<u16>().is_ok() => o.area = None,
                                _ => {}
                            },
                        }
                    }
                });
                String::new()
            }
            "show-ospf-neighbor" => neighbors(network, &device),
            "show-ospf-interface-brief" => interface_brief(network, &device),
            "show-ospf-interface" => interfaces(network, &device, iface.as_deref()),
            "show-ospf-database" => database(network, &device),
            "show-ospf" => process_summary(network, &device),
            "show-ip-protocols" => protocols(network, &device),
            "show-ip-route-ospf" => {
                let routes: Vec<_> = netrace_engine::routing_table(project, self.device)
                    .into_iter()
                    .filter(|r| r.distance == netrace_engine::OSPF_DISTANCE)
                    .collect();
                crate::render::ip_route(kind, &device.config, &routes)
            }
            _ => return None,
        };
        Some(out)
    }
}

fn router<'a>(network: &'a Network, device: &Device) -> Option<&'a OspfRouter> {
    network.ospf().router(device.id)
}

/// "00:00:35": the dead timer, a few seconds after the last hello.
fn dead_time(dead: u16) -> String {
    let left = dead.saturating_sub(5);
    format!("00:{:02}:{:02}", left / 60, left % 60)
}

/// Stand-in LSA checksum: stable for the same LSA.
fn checksum(text: &str) -> u16 {
    text.bytes()
        .fold(0x4f1du16, |a, b| a.rotate_left(3) ^ u16::from(b))
}

/// `show ip ospf neighbor`.
fn neighbors(network: &Network, device: &Device) -> String {
    let Some(r) = router(network, device) else {
        return String::new();
    };
    let mut out = String::from(
        "\nNeighbor ID     Pri   State           Dead Time   Address         Interface\n",
    );
    for n in r
        .neighbors
        .iter()
        .filter(|n| n.state != NeighborState::Down)
    {
        let role = if n.role == "-" { "  -" } else { n.role };
        let state = format!("{}/{role}", n.state.ios());
        let _ = writeln!(
            out,
            "{:<15} {:>3}   {:<15} {:<11} {:<15} {}",
            n.router_id.to_string(),
            n.priority,
            state,
            dead_time(n.dead),
            n.address.to_string(),
            n.iface
        );
    }
    out
}

/// `show ip ospf interface brief`.
fn interface_brief(network: &Network, device: &Device) -> String {
    let Some(r) = router(network, device) else {
        return String::new();
    };
    let mut out = String::from(
        "Interface    PID   Area            IP Address/Mask    Cost  State Nbrs F/C\n",
    );
    for i in &r.ifaces {
        let (full, count) = counts(r, &i.name);
        let _ = writeln!(
            out,
            "{:<12} {:<5} {:<15} {:<18} {:<5} {:<5} {full}/{count}",
            abbreviate_port(&i.name),
            r.process_id,
            i.area,
            format!("{}/{}", i.address, i.prefix_len),
            i.cost,
            i.state.ios()
        );
    }
    out
}

/// (full, total) neighbours heard on an interface.
fn counts(r: &OspfRouter, iface: &str) -> (usize, usize) {
    let heard: Vec<_> = r
        .neighbors
        .iter()
        .filter(|n| n.iface == iface && n.state != NeighborState::Down)
        .collect();
    (
        heard
            .iter()
            .filter(|n| n.state == NeighborState::Full)
            .count(),
        heard.len(),
    )
}

/// `show ip ospf interface [X]`.
fn interfaces(network: &Network, device: &Device, only: Option<&str>) -> String {
    let Some(r) = router(network, device) else {
        return String::new();
    };
    let rid = r.router_id.map(|x| x.to_string()).unwrap_or_default();
    let mut out = String::new();
    for i in r.ifaces.iter().filter(|i| only.is_none_or(|o| o == i.name)) {
        let attached = if device
            .config
            .interface(device.kind, &i.name)
            .ospf
            .area
            .is_some()
        {
            "Interface Enable"
        } else {
            "Network Statement"
        };
        let _ = writeln!(out, "{} is up, line protocol is up ", i.name);
        let _ = writeln!(
            out,
            "  Internet Address {}/{}, Area {}, Attached via {attached}",
            i.address, i.prefix_len, i.area
        );
        let _ = writeln!(
            out,
            "  Process ID {}, Router ID {rid}, Network Type {}, Cost: {}",
            r.process_id,
            i.net_type.ios(),
            i.cost
        );
        if i.net_type == OspfNetType::Loopback {
            out += "  Loopback interface is treated as a stub Host\n";
            continue;
        }
        let state = match i.state {
            OspfIfState::DrOther => "DROTHER",
            OspfIfState::PointToPoint => "POINT_TO_POINT",
            OspfIfState::Waiting => "WAITING",
            other => other.ios(),
        };
        let _ = writeln!(
            out,
            "  Transmit Delay is 1 sec, State {state}, Priority {}",
            i.priority
        );
        if let Some((id, addr)) = i.dr {
            let _ = writeln!(
                out,
                "  Designated Router (ID) {id}, Interface address {addr}"
            );
        }
        if let Some((id, addr)) = i.bdr {
            let _ = writeln!(
                out,
                "  Backup Designated router (ID) {id}, Interface address {addr}"
            );
        }
        let _ = writeln!(
            out,
            "  Timer intervals configured, Hello {}, Dead {}, Wait {}, Retransmit 5",
            i.hello, i.dead, i.dead
        );
        if i.passive {
            out += "    No Hellos (Passive interface) \n";
        } else {
            out += "    Hello due in 00:00:03\n";
        }
        let (full, count) = counts(r, &i.name);
        let _ = writeln!(
            out,
            "  Neighbor Count is {count}, Adjacent neighbor count is {full} "
        );
        for n in r
            .neighbors
            .iter()
            .filter(|n| n.iface == i.name && n.state == NeighborState::Full)
        {
            let role = match n.role {
                "DR" => "  (Designated Router)",
                "BDR" => "  (Backup Designated Router)",
                _ => "",
            };
            let _ = writeln!(out, "    Adjacent with neighbor {}{role}", n.router_id);
        }
        out += "  Suppress hello for 0 neighbor(s)\n";
    }
    out
}

/// `show ip ospf database`.
fn database(network: &Network, device: &Device) -> String {
    let Some(r) = router(network, device) else {
        return String::new();
    };
    let Some(rid) = r.router_id else {
        return String::new();
    };
    let ospf = network.ospf();
    let mut out = format!(
        "\n            OSPF Router with ID ({rid}) (Process ID {})\n",
        r.process_id
    );
    let row = |lsa: &netrace_engine::Lsa, count: bool, age: usize| {
        let mut line = format!(
            "{:<15} {:<15} {:<11} 0x80000002 0x{:06X}",
            lsa.link_id.to_string(),
            lsa.adv_router.to_string(),
            age,
            checksum(&lsa.describe)
        );
        if count {
            let _ = write!(line, " {}", lsa.count);
        }
        line
    };
    for area in r.areas() {
        let lsas = ospf.lsdb.get(&area).cloned().unwrap_or_default();
        for (kind, title, count) in [
            (LsaKind::Router, "Router Link States", true),
            (LsaKind::Network, "Net Link States", false),
            (LsaKind::Summary, "Summary Net Link States", false),
        ] {
            let list: Vec<_> = lsas.iter().filter(|l| l.kind == kind).collect();
            if list.is_empty() {
                continue;
            }
            let _ = writeln!(out, "\n                {title} (Area {area})\n");
            out += if count {
                "Link ID         ADV Router      Age         Seq#       Checksum Link count\n"
            } else {
                "Link ID         ADV Router      Age         Seq#       Checksum\n"
            };
            for (n, lsa) in list.iter().enumerate() {
                let _ = writeln!(out, "{}", row(lsa, count, 30 + n * 7));
            }
        }
    }
    if !ospf.external.is_empty() {
        out += "\n                Type-5 AS External Link States\n\n";
        out += "Link ID         ADV Router      Age         Seq#       Checksum Tag\n";
        for (n, lsa) in ospf.external.iter().enumerate() {
            let _ = writeln!(out, "{} 1", row(lsa, false, 30 + n * 7));
        }
    }
    out
}

/// `show ip ospf`.
fn process_summary(network: &Network, device: &Device) -> String {
    let Some(r) = router(network, device) else {
        return String::new();
    };
    let Some(process) = device.config.ospf.first() else {
        return String::new();
    };
    let rid = r.router_id.map(|x| x.to_string()).unwrap_or_default();
    let mut out = format!(
        " Routing Process \"ospf {}\" with ID {rid}\n Supports only single TOS(TOS0) routes\n Supports opaque LSA\n",
        r.process_id
    );
    if r.abr {
        out += " It is an area border router\n";
    }
    if r.asbr {
        out += " It is an autonomous system boundary router\n";
    }
    let areas = r.areas();
    let _ = writeln!(
        out,
        " Number of areas in this router is {}. {} normal 0 stub 0 nssa",
        areas.len(),
        areas.len()
    );
    let _ = writeln!(
        out,
        " Reference bandwidth unit is {} mbps",
        process.reference_bandwidth
    );
    for area in areas {
        let name = if area == 0 {
            "BACKBONE(0)".to_string()
        } else {
            area.to_string()
        };
        let ifaces: Vec<_> = r.ifaces.iter().filter(|i| i.area == area).collect();
        let loopbacks = ifaces
            .iter()
            .filter(|i| i.net_type == OspfNetType::Loopback)
            .count();
        let _ = writeln!(out, "    Area {name}");
        let _ = writeln!(
            out,
            "        Number of interfaces in this area is {} ({loopbacks} loopback)",
            ifaces.len()
        );
        out += "        Area has no authentication\n";
    }
    out
}

/// `show ip protocols` (the OSPF part).
fn protocols(network: &Network, device: &Device) -> String {
    let Some(process) = device.config.ospf.first() else {
        return String::new();
    };
    let r = router(network, device);
    let mut out = format!(
        "*** IP Routing is NSF aware ***\n\nRouting Protocol is \"ospf {}\"\n  Outgoing update filter list for all interfaces is not set\n  Incoming update filter list for all interfaces is not set\n",
        process.process_id
    );
    if let Some(rid) = r.and_then(|r| r.router_id) {
        let _ = writeln!(out, "  Router ID {rid}");
    }
    let areas = r.map(|r| r.areas().len()).unwrap_or(0);
    let _ = writeln!(
        out,
        "  Number of areas in this router is {areas}. {areas} normal 0 stub 0 nssa\n  Maximum path: 4"
    );
    out += "  Routing for Networks:\n";
    for n in &process.networks {
        let _ = writeln!(out, "    {} {} area {}", n.address, n.wildcard, n.area);
    }
    let explicit: Vec<(String, u32)> = device
        .config
        .interfaces
        .iter()
        .filter_map(|(name, i)| {
            i.ospf
                .area
                .filter(|(pid, _)| *pid == process.process_id)
                .map(|(_, a)| (name.clone(), a))
        })
        .collect();
    for (name, area) in explicit {
        let _ = writeln!(
            out,
            "  Routing on Interfaces Configured Explicitly (Area {area}):\n    {name}"
        );
    }
    if let Some(r) = r {
        let passive: Vec<_> = r.ifaces.iter().filter(|i| i.passive).collect();
        if !passive.is_empty() {
            out += "  Passive Interface(s):\n";
            for i in passive {
                let _ = writeln!(out, "    {}", i.name);
            }
        }
        out += "  Routing Information Sources:\n    Gateway         Distance      Last Update\n";
        let mut sources: Vec<Ipv4Addr> = r
            .neighbors
            .iter()
            .filter(|n| n.state == NeighborState::Full)
            .map(|n| n.router_id)
            .collect();
        sources.sort();
        sources.dedup();
        for s in sources {
            let _ = writeln!(out, "    {:<15} {:>8}      00:00:12", s.to_string(), 110);
        }
    }
    out += "  Distance: (default is 110)\n";
    out
}
