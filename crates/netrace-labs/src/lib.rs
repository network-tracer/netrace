//! netrace labs: checking a student's work against a lab's checks, and the
//! bundled CCNA labs.
//!
//! Checks look at the simulated network, never at how it was configured:
//! "PC1 can ping PC4" runs a ping in a fresh simulation. A failing check
//! says why in the student's terms, using the simulation's own explanation
//! where there is one ("R1 has no route to 192.168.2.10 ...").

use std::net::Ipv4Addr;
use std::time::Duration;

use netrace_doc::{Device, DeviceId, LabCheck, PortMode, Project};
use netrace_engine::{Layer2, NeighborState, Network, Outcome, RouteSource, abbreviate_port};
use netrace_sim::SimTime;

mod bundled;

pub use bundled::{BundledLab, bundled_labs};

/// What one check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub passed: bool,
    /// The check in words: "PC1 can ping 192.168.2.10".
    pub label: String,
    /// Why it fails (empty when it passes).
    pub feedback: String,
}

/// A task is done when every check passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskResult {
    pub done: bool,
    pub checks: Vec<CheckResult>,
}

/// Checks every task of the project's lab (empty if it has none).
pub fn evaluate(project: &Project) -> Vec<TaskResult> {
    let Some(lab) = project.lab() else {
        return Vec::new();
    };
    let mut checker = Checker {
        project,
        network: None,
        next_ping: SimTime::ZERO,
    };
    lab.tasks
        .iter()
        .map(|task| {
            let checks: Vec<CheckResult> = task.checks.iter().map(|c| checker.check(c)).collect();
            TaskResult {
                done: checks.iter().all(|c| c.passed),
                checks,
            }
        })
        .collect()
}

/// The words a check stands for, without evaluating it.
pub fn describe(check: &LabCheck) -> String {
    match check {
        LabCheck::Cabled {
            a,
            b,
            a_port,
            b_port,
        } => match (a_port, b_port) {
            (Some(pa), Some(pb)) => format!("{a} {pa} is cabled to {b} {pb}"),
            _ => format!("{a} is cabled to {b}"),
        },
        LabCheck::InterfaceUp { device, interface } => format!("{device} {interface} is up"),
        LabCheck::Address {
            device,
            interface,
            address,
            prefix_len,
        } => match interface {
            Some(i) => format!("{device} {i} is {address}/{prefix_len}"),
            None => format!("{device} is {address}/{prefix_len}"),
        },
        LabCheck::Gateway { device, gateway } => format!("{device}'s default gateway is {gateway}"),
        LabCheck::Ping { from, to, succeeds } => {
            if *succeeds {
                format!("{from} can ping {to}")
            } else {
                format!("{from} can't ping {to}")
            }
        }
        LabCheck::Route {
            device,
            network,
            prefix_len,
            source,
        } => match source {
            Some(s) => format!("{device} has a {s} route to {network}/{prefix_len}"),
            None => format!("{device} has a route to {network}/{prefix_len}"),
        },
        LabCheck::Vlan { device, vlan, name } => match name {
            Some(n) => format!("{device} has VLAN {vlan} named {n}"),
            None => format!("{device} has VLAN {vlan}"),
        },
        LabCheck::AccessPort { device, port, vlan } => format!("{device} {port} is in VLAN {vlan}"),
        LabCheck::Trunk { device, port } => format!("{device} {port} is a trunk"),
        LabCheck::OspfNeighbors { device, count } => {
            let s = if *count == 1 { "" } else { "s" };
            format!("{device} has {count} full OSPF neighbour{s}")
        }
        LabCheck::StpRoot { device, vlan } => {
            format!("{device} is the root bridge for VLAN {vlan}")
        }
        LabCheck::DhcpLease { device } => format!("{device} gets its address from DHCP"),
        LabCheck::Config { device, contains } => {
            format!("{device}'s configuration has \"{contains}\"")
        }
    }
}

struct Checker<'a> {
    project: &'a Project,
    /// Built on the first check that needs it, then shared by all of them.
    network: Option<Network>,
    /// Pings run one after another so their traffic doesn't overlap.
    next_ping: SimTime,
}

impl Checker<'_> {
    fn network(&mut self) -> &mut Network {
        self.network
            .get_or_insert_with(|| Network::new(self.project.clone()))
    }

    fn device(&self, name: &str) -> Result<&Device, String> {
        self.project
            .devices()
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("There's no device called {name} (was it renamed?)."))
    }

    /// The full interface name a lab wrote short.
    fn interface(&self, d: &Device, text: &str) -> String {
        netrace_cli::interface_name(text, d.kind).unwrap_or_else(|| text.to_string())
    }

    /// The check with its interface names as IOS abbreviates them
    /// ("g0/0" → "Gi0/0"), so labels and feedback read consistently.
    fn normalized(&self, check: &LabCheck) -> LabCheck {
        let short = |device: &str, text: &str| {
            self.device(device)
                .map(|d| abbreviate_port(&self.interface(d, text)))
                .unwrap_or_else(|_| text.to_string())
        };
        let mut check = check.clone();
        match &mut check {
            LabCheck::Cabled {
                a,
                b,
                a_port,
                b_port,
            } => {
                if let Some(p) = a_port {
                    *p = short(a, p);
                }
                if let Some(p) = b_port {
                    *p = short(b, p);
                }
            }
            LabCheck::InterfaceUp { device, interface } => *interface = short(device, interface),
            LabCheck::Address {
                device,
                interface: Some(interface),
                ..
            } => *interface = short(device, interface),
            LabCheck::AccessPort { device, port, .. } | LabCheck::Trunk { device, port } => {
                *port = short(device, port)
            }
            _ => {}
        }
        check
    }

    fn check(&mut self, check: &LabCheck) -> CheckResult {
        let check = &self.normalized(check);
        let label = describe(check);
        match self.run(check) {
            Ok(()) => CheckResult {
                passed: true,
                label,
                feedback: String::new(),
            },
            Err(feedback) => CheckResult {
                passed: false,
                label,
                feedback,
            },
        }
    }

    fn run(&mut self, check: &LabCheck) -> Result<(), String> {
        match check {
            LabCheck::Cabled {
                a,
                b,
                a_port,
                b_port,
            } => {
                let (da, db) = (self.device(a)?.clone(), self.device(b)?.clone());
                let pa = a_port.as_deref().map(|p| self.interface(&da, p));
                let pb = b_port.as_deref().map(|p| self.interface(&db, p));
                let found = self.project.links().iter().find(|l| {
                    let fits = |x: &netrace_doc::Endpoint, d: &Device, p: &Option<String>| {
                        x.device == d.id && p.as_ref().is_none_or(|p| *p == x.port)
                    };
                    (fits(&l.a, &da, &pa) && fits(&l.b, &db, &pb))
                        || (fits(&l.b, &da, &pa) && fits(&l.a, &db, &pb))
                });
                if found.is_some() {
                    return Ok(());
                }
                let any = self.project.links().iter().any(|l| {
                    (l.a.device == da.id && l.b.device == db.id)
                        || (l.a.device == db.id && l.b.device == da.id)
                });
                Err(if any {
                    format!("{a} and {b} are cabled, but not on the ports this step asks for.")
                } else {
                    format!("No cable joins {a} and {b} yet.")
                })
            }
            LabCheck::InterfaceUp { device, interface } => {
                let d = self.device(device)?.clone();
                let name = self.interface(&d, interface);
                let rows = netrace_engine::interface_rows(self.project, d.id);
                let Some(row) = rows.iter().find(|r| r.name == name) else {
                    return Err(format!("{device} has no interface {interface}."));
                };
                if row.is_up() {
                    Ok(())
                } else if row.status == "administratively down" {
                    Err(format!(
                        "{device} {} is shut down: enter the interface and type no shutdown.",
                        abbreviate_port(&name)
                    ))
                } else {
                    Err(format!(
                        "{device} {} is {}/{}: is it cabled to something that's switched on?",
                        abbreviate_port(&name),
                        row.status,
                        row.protocol
                    ))
                }
            }
            LabCheck::Address {
                device,
                interface,
                address,
                prefix_len,
            } => {
                let d = self.device(device)?.clone();
                let actual = match interface {
                    None => d.ip.map(|ip| (ip.address, ip.prefix_len)),
                    Some(i) => {
                        let name = self.interface(&d, i);
                        d.config
                            .interface(d.kind, &name)
                            .address
                            .map(|a| (a.address, a.prefix_len))
                    }
                };
                let at = interface
                    .as_deref()
                    .map(|i| format!("{device} {i}"))
                    .unwrap_or_else(|| device.clone());
                match actual {
                    Some(a) if a == (*address, *prefix_len) => Ok(()),
                    Some((ip, len)) => {
                        Err(format!("{at} is {ip}/{len}, not {address}/{prefix_len}."))
                    }
                    None => Err(format!("{at} has no IP address yet.")),
                }
            }
            LabCheck::Gateway { device, gateway } => {
                let d = self.device(device)?;
                match d.ip.and_then(|ip| ip.gateway) {
                    Some(g) if g == *gateway => Ok(()),
                    Some(g) => Err(format!("{device}'s gateway is {g}, not {gateway}.")),
                    None => Err(format!("{device} has no default gateway.")),
                }
            }
            LabCheck::Ping { from, to, succeeds } => {
                let source = self.device(from)?.id;
                let target = self.address_of(to)?;
                let at = self.next_ping;
                self.next_ping = at + Duration::from_secs(30);
                let network = self.network();
                let session = network
                    .ping(source, target, 4, at)
                    .map_err(|e| format!("{from} can't send a ping: {e}"))?;
                let replied = network
                    .session(session)
                    .is_some_and(|s| !s.replies.is_empty());
                match (replied, *succeeds) {
                    (true, true) | (false, false) => Ok(()),
                    (true, false) => Err(format!(
                        "{from} still gets replies from {to}: nothing stops this traffic yet."
                    )),
                    (false, true) => Err(self.why_ping_failed(session, from, to)),
                }
            }
            LabCheck::Route {
                device,
                network,
                prefix_len,
                source,
            } => {
                let d = self.device(device)?.id;
                let routes = netrace_engine::routing_table(self.project, d);
                let matching: Vec<_> = routes
                    .iter()
                    .filter(|r| r.network == *network && r.prefix_len == *prefix_len)
                    .collect();
                if matching.is_empty() {
                    return Err(format!(
                        "{device}'s routing table has no route to {network}/{prefix_len}."
                    ));
                }
                let Some(wanted) = source else { return Ok(()) };
                let fits = |s: RouteSource| match wanted.as_str() {
                    "connected" => s == RouteSource::Connected,
                    "static" => s == RouteSource::Static,
                    "ospf" => matches!(
                        s,
                        RouteSource::Ospf | RouteSource::OspfInterArea | RouteSource::OspfExternal2
                    ),
                    _ => true,
                };
                if matching.iter().any(|r| fits(r.source)) {
                    Ok(())
                } else {
                    Err(format!(
                        "{device} reaches {network}/{prefix_len}, but not through a {wanted} route."
                    ))
                }
            }
            LabCheck::Vlan { device, vlan, name } => {
                let d = self.device(device)?;
                if !d.config.vlan_exists(*vlan) {
                    return Err(format!(
                        "{device} has no VLAN {vlan}: create it with vlan {vlan}."
                    ));
                }
                match (name, d.config.vlans.get(vlan).and_then(|v| v.name.clone())) {
                    (None, _) => Ok(()),
                    (Some(want), Some(have)) if have.eq_ignore_ascii_case(want) => Ok(()),
                    (Some(want), _) => Err(format!(
                        "VLAN {vlan} on {device} isn't named {want}: under vlan {vlan}, type name {want}."
                    )),
                }
            }
            LabCheck::AccessPort { device, port, vlan } => {
                let d = self.device(device)?.clone();
                let name = self.interface(&d, port);
                let sp = d.config.interface(d.kind, &name).switchport;
                if sp.mode == PortMode::Trunk {
                    return Err(format!("{device} {port} is a trunk, not an access port."));
                }
                if sp.access_vlan == *vlan {
                    Ok(())
                } else {
                    Err(format!(
                        "{device} {port} is in VLAN {}, not {vlan}.",
                        sp.access_vlan
                    ))
                }
            }
            LabCheck::Trunk { device, port } => {
                let d = self.device(device)?.clone();
                let name = self.interface(&d, port);
                if d.config.interface(d.kind, &name).switchport.mode == PortMode::Trunk {
                    Ok(())
                } else {
                    Err(format!(
                        "{device} {port} isn't a trunk: use switchport mode trunk."
                    ))
                }
            }
            LabCheck::OspfNeighbors { device, count } => {
                let d = self.device(device)?.id;
                let network = self.network();
                let Some(router) = network.ospf().router(d) else {
                    return Err(format!("{device} isn't running OSPF yet."));
                };
                let full = router
                    .neighbors
                    .iter()
                    .filter(|n| n.state == NeighborState::Full)
                    .count() as u32;
                if full == *count {
                    return Ok(());
                }
                let problem = router.neighbors.iter().find_map(|n| n.problem.clone());
                Err(match problem {
                    Some(p) => format!("{device} has {full} full neighbour(s), not {count}: {p}."),
                    None => format!("{device} has {full} full neighbour(s), not {count}."),
                })
            }
            LabCheck::StpRoot { device, vlan } => {
                let d = self.device(device)?.id;
                let l2 = Layer2::for_project(self.project);
                let Some(tree) = l2.trees.get(vlan) else {
                    return Err(format!("There's no spanning tree for VLAN {vlan} yet."));
                };
                if tree.root_of(d) == d {
                    return Ok(());
                }
                let root = self
                    .project
                    .device(tree.root_of(d))
                    .map(|r| r.name.clone())
                    .unwrap_or_default();
                Err(format!(
                    "{root} is the root for VLAN {vlan}: give {device} a lower priority \
                     (spanning-tree vlan {vlan} root primary)."
                ))
            }
            LabCheck::DhcpLease { device } => {
                let d = self.device(device)?.clone();
                if !d.dhcp {
                    return Err(format!(
                        "{device} uses a static address: switch it to DHCP."
                    ));
                }
                match self.network().current_address(d.id) {
                    Some((a, ..)) if a.octets()[..2] != [169, 254] => Ok(()),
                    _ => Err(format!(
                        "{device} got no answer from a DHCP server, so it gave itself a 169.254 address."
                    )),
                }
            }
            LabCheck::Config { device, contains } => {
                let d = self.device(device)?.id;
                let config = netrace_cli::running_config(self.project, d).unwrap_or_default();
                let wanted = contains.trim().to_lowercase();
                if config
                    .lines()
                    .any(|l| l.trim().to_lowercase().contains(&wanted))
                {
                    Ok(())
                } else {
                    Err(format!(
                        "{device}'s running-config has no \"{contains}\" yet."
                    ))
                }
            }
        }
    }

    /// A device name (its first address) or a literal address.
    fn address_of(&self, to: &str) -> Result<Ipv4Addr, String> {
        if let Ok(ip) = to.parse() {
            return Ok(ip);
        }
        let d = self.device(to)?;
        if let Some(ip) = d.ip {
            return Ok(ip.address);
        }
        d.config
            .interfaces
            .values()
            .find_map(|i| i.address.map(|a| a.address))
            .ok_or_else(|| format!("{to} has no IP address to ping."))
    }

    /// The simulation's own account of where the ping died.
    fn why_ping_failed(&mut self, session: u64, from: &str, to: &str) -> String {
        let network = self.network();
        let dropped = network
            .trace()
            .iter()
            .rev()
            .find(|e| e.session == Some(session) && e.outcome == Outcome::Dropped);
        match dropped {
            Some(e) => format!("{from} gets no reply from {to}. {}", e.summary),
            None => format!("{from} gets no reply from {to}."),
        }
    }
}

/// The checks as plain IDs that `evaluate` can be matched against, for
/// tests: (task, check) → passed.
pub fn passed(results: &[TaskResult]) -> Vec<Vec<bool>> {
    results
        .iter()
        .map(|t| t.checks.iter().map(|c| c.passed).collect())
        .collect()
}

/// A device's ID by name, for tests and tools.
pub fn device_id(project: &Project, name: &str) -> Option<DeviceId> {
    project
        .devices()
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(name))
        .map(|d| d.id)
}

#[cfg(test)]
mod tests;
