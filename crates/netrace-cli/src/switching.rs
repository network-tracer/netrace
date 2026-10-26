//! Switching commands: VLANs, access and trunk ports, spanning tree, port
//! security, CDP / LLDP, and their `show` output.

use std::fmt::Write;

use netrace_doc::{
    Device, DeviceKind, PortMode, PortSecurity, Project, StpMode, SwitchportConfig, Violation,
    VlanConfig, VlanSet,
};
use netrace_engine::{
    Layer2, Network, abbreviate_port, discovery_neighbors, interface_rows, natural_key,
};

use crate::grammar::Value;
use crate::render::cisco_mac;
use crate::{Mode, Session};

impl Session {
    /// Runs a switching command; `None` if `id` isn't one.
    pub(crate) fn switching(
        &mut self,
        id: &str,
        values: &[Value],
        kind: DeviceKind,
        project: &mut Project,
        network: &Network,
    ) -> Option<String> {
        let word = values.iter().find_map(|v| match v {
            Value::Word(w) | Value::Line(w) => Some(w.clone()),
            _ => None,
        });
        let num = values.iter().find_map(|v| match v {
            Value::Num(n) => Some(*n as u16),
            _ => None,
        });
        let device = project.device(self.device).cloned()?;
        let out = match id {
            // Interface: switchport
            "sw-mode-access" => self.ports(project, kind, |sp| sp.mode = PortMode::Access),
            "sw-mode-trunk" => self.ports(project, kind, |sp| sp.mode = PortMode::Trunk),
            "sw-mode-dynamic" => self.ports(project, kind, |sp| sp.mode = PortMode::DynamicAuto),
            "sw-mode-desirable" => {
                self.ports(project, kind, |sp| sp.mode = PortMode::DynamicAuto);
                "% netrace doesn't simulate DTP: the port acts as an access port until you set `switchport mode trunk`."
                    .into()
            }
            "sw-access-vlan" => {
                let vlan = num?;
                let mut note = String::new();
                if !device.config.vlan_exists(vlan) {
                    note = format!("% Access VLAN does not exist. Creating vlan {vlan}");
                    self.update(project, |c| {
                        c.vlans.insert(vlan, VlanConfig::default());
                    });
                }
                self.ports(project, kind, |sp| sp.access_vlan = vlan);
                note
            }
            "no-sw-access-vlan" => self.ports(project, kind, |sp| sp.access_vlan = 1),
            "sw-native-vlan" => self.ports(project, kind, |sp| sp.native_vlan = num.unwrap_or(1)),
            "no-sw-native-vlan" => self.ports(project, kind, |sp| sp.native_vlan = 1),
            "sw-allowed" | "sw-allowed-add" | "sw-allowed-remove" => {
                let text = word.unwrap_or_default();
                let set = match text.to_ascii_lowercase().as_str() {
                    "all" => Some(VlanSet::all()),
                    "none" => Some(VlanSet::none()),
                    _ => VlanSet::parse(&text),
                };
                let Some(set) = set else {
                    return Some("% Invalid VLAN list. Use VLAN IDs 1-4094, e.g. 10,20-30.".into());
                };
                self.ports(project, kind, |sp| {
                    sp.allowed_vlans = match id {
                        "sw-allowed-add" => sp.allowed_vlans.union(&set),
                        "sw-allowed-remove" => sp.allowed_vlans.minus(&set),
                        _ => set.clone(),
                    }
                })
            }
            "no-sw-allowed" => self.ports(project, kind, |sp| sp.allowed_vlans = VlanSet::all()),
            "sw-encapsulation" => String::new(),
            "sw-nonegotiate" => self.ports(project, kind, |sp| sp.nonegotiate = true),
            "no-sw-nonegotiate" => self.ports(project, kind, |sp| sp.nonegotiate = false),
            "psec" => {
                for name in self.interface_names() {
                    let sp = device.config.interface(kind, &name).switchport;
                    match sp.mode {
                        PortMode::Access => {}
                        PortMode::Trunk => {
                            return Some(format!("Command rejected: {name} is a trunk port."));
                        }
                        PortMode::DynamicAuto => {
                            return Some(format!("Command rejected: {name} is a dynamic port."));
                        }
                    }
                }
                self.ports(project, kind, |sp| {
                    sp.port_security
                        .get_or_insert_with(PortSecurity::default)
                        .enabled = true
                })
            }
            "no-psec" => self.ports(project, kind, |sp| {
                if let Some(ps) = sp.port_security.as_mut() {
                    ps.enabled = false;
                }
            }),
            "psec-max" => self.ports(project, kind, |sp| {
                sp.port_security
                    .get_or_insert_with(PortSecurity::default)
                    .maximum = num.unwrap_or(1)
            }),
            "psec-sticky" | "no-psec-sticky" => self.ports(project, kind, |sp| {
                sp.port_security
                    .get_or_insert_with(PortSecurity::default)
                    .sticky = id == "psec-sticky"
            }),
            "psec-mac" => {
                let text = word.unwrap_or_default();
                let Some(mac) = netrace_engine::MacAddr::parse_cisco(&text) else {
                    return Some(format!(
                        "% Invalid MAC address {text}. Write it as H.H.H, e.g. 0001.4321.abcd."
                    ));
                };
                self.ports(project, kind, |sp| {
                    let ps = sp.port_security.get_or_insert_with(PortSecurity::default);
                    let mac = cisco_mac(mac);
                    if !ps.static_macs.contains(&mac) {
                        ps.static_macs.push(mac);
                    }
                })
            }
            "psec-protect" | "psec-restrict" | "psec-shutdown" => {
                let violation = match id {
                    "psec-protect" => Violation::Protect,
                    "psec-restrict" => Violation::Restrict,
                    _ => Violation::Shutdown,
                };
                self.ports(project, kind, |sp| {
                    sp.port_security
                        .get_or_insert_with(PortSecurity::default)
                        .violation = violation
                })
            }
            "portfast" => {
                self.ports(project, kind, |sp| sp.portfast = true);
                let name = self.interface_names().join(", ");
                format!(
                    "%Warning: portfast should only be enabled on ports connected to a single\n host. Connecting hubs, \
                     concentrators, switches, bridges, etc... to this\n interface  when portfast is enabled, can cause \
                     temporary bridging loops.\n Use with CAUTION\n\n%Portfast has been configured on {name} but will \
                     only\n have effect when the interface is in a non-trunking mode."
                )
            }
            "no-portfast" => self.ports(project, kind, |sp| sp.portfast = false),
            "bpduguard" => self.ports(project, kind, |sp| sp.bpdu_guard = true),
            "no-bpduguard" => self.ports(project, kind, |sp| sp.bpdu_guard = false),
            "cdp-enable" | "no-cdp-enable" => {
                self.ports(project, kind, |sp| sp.cdp_disabled = id == "no-cdp-enable")
            }
            "no-switchport" | "switchport-l2" => {
                let routed = id == "no-switchport";
                self.ports(project, kind, |sp| sp.routed = routed)
            }

            // VLANs, ranges, spanning tree, discovery
            "vlan" | "no-vlan" => {
                let Some(set) = VlanSet::parse(&word.unwrap_or_default()) else {
                    return Some(
                        "% Invalid VLAN list. Use VLAN IDs 1-4094, e.g. 10 or 10,20-30.".into(),
                    );
                };
                let ids: Vec<u16> = (1..=4094).filter(|v| set.contains(*v)).collect();
                if id == "no-vlan" {
                    if ids.contains(&1) {
                        return Some("%Default VLAN 1 may not be deleted.".into());
                    }
                    self.update(project, |c| {
                        for v in &ids {
                            c.vlans.remove(v);
                        }
                    });
                    String::new()
                } else {
                    self.update(project, |c| {
                        for &v in ids.iter().filter(|v| **v != 1) {
                            c.vlans.entry(v).or_default();
                        }
                    });
                    self.mode = Mode::Vlan(ids);
                    String::new()
                }
            }
            "vlan-name" | "no-vlan-name" => {
                let Mode::Vlan(ids) = self.mode.clone() else {
                    return Some(String::new());
                };
                if ids.contains(&1) {
                    return Some("%Default VLAN 1 may not have its name changed.".into());
                }
                let name = if id == "vlan-name" { word } else { None };
                self.update(project, |c| {
                    for v in &ids {
                        c.vlans.entry(*v).or_default().name = name.clone();
                    }
                });
                String::new()
            }
            "interface-range" => {
                let Some(names) = parse_range(&word.unwrap_or_default(), kind) else {
                    return Some(
                        "% Invalid interface range. Example: interface range f0/1 - 12, f0/15"
                            .into(),
                    );
                };
                self.mode = Mode::Range(names);
                String::new()
            }
            "stp-pvst" | "stp-rapid" => {
                let mode = if id == "stp-rapid" {
                    StpMode::RapidPvst
                } else {
                    StpMode::Pvst
                };
                self.update(project, |c| c.spanning_tree.mode = mode);
                String::new()
            }
            "stp-priority" | "no-stp-priority" | "stp-root-primary" | "stp-root-secondary" => {
                let Some(set) = VlanSet::parse(&word.unwrap_or_default()) else {
                    return Some("% Invalid VLAN list.".into());
                };
                let ids: Vec<u16> = (1..=4094).filter(|v| set.contains(*v)).collect();
                let priority = match id {
                    "stp-priority" => {
                        let p = num.unwrap_or(32768);
                        if p % 4096 != 0 {
                            return Some(
                                "% Bridge Priority must be in increments of 4096.\n% Allowed values are:\n  \
                                 0     4096  8192  12288 16384 20480 24576 28672\n  \
                                 32768 36864 40960 45056 49152 53248 57344 61440"
                                    .into(),
                            );
                        }
                        Some(p)
                    }
                    "stp-root-primary" => Some(24576),
                    "stp-root-secondary" => Some(28672),
                    _ => None,
                };
                self.update(project, |c| {
                    for v in &ids {
                        match priority {
                            Some(p) => {
                                c.spanning_tree.priorities.insert(*v, p);
                            }
                            None => {
                                c.spanning_tree.priorities.remove(v);
                            }
                        }
                    }
                });
                String::new()
            }
            "cdp-run" | "no-cdp-run" => {
                self.update(project, |c| c.cdp_run = id == "cdp-run");
                String::new()
            }
            "lldp-run" | "no-lldp-run" => {
                self.update(project, |c| c.lldp_run = id == "lldp-run");
                String::new()
            }

            // show
            "show-vlan-brief" => vlan_brief(&device, false),
            "show-vlan" => vlan_brief(&device, true),
            "show-trunk" => trunks(project, &device, network.layer2()),
            "show-status" => interface_status(project, &device, network),
            "show-switchport" => {
                let name = values.iter().find_map(|v| match v {
                    Value::Iface(n) => Some(n.clone()),
                    _ => None,
                })?;
                switchport_detail(&device, &name)
            }
            "show-stp" => spanning_tree(&device, network.layer2(), None),
            "show-stp-vlan" => spanning_tree(&device, network.layer2(), num),
            "show-cdp" | "show-cdp-detail" => cdp(project, &device, id == "show-cdp-detail"),
            "show-lldp" => lldp(project, &device),
            "show-psec" => port_security_summary(&device, network),
            "show-psec-if" => {
                let name = values.iter().find_map(|v| match v {
                    Value::Iface(n) => Some(n.clone()),
                    _ => None,
                })?;
                port_security_interface(&device, network, &name)
            }
            "show-psec-address" => port_security_addresses(&device, network),
            _ => return None,
        };
        Some(out)
    }

    /// Applies a switchport change to every interface the mode covers.
    fn ports(
        &self,
        project: &mut Project,
        kind: DeviceKind,
        edit: impl Fn(&mut SwitchportConfig),
    ) -> String {
        let names = self.interface_names();
        self.update(project, |c| {
            for name in &names {
                edit(&mut c.interface_mut(kind, name).switchport);
            }
        });
        String::new()
    }
}

/// "f0/1 - 12, f0/15" → the full port names, or `None` if any is invalid.
fn parse_range(text: &str, kind: DeviceKind) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for part in text.split(',') {
        let part: String = part.chars().filter(|c| !c.is_whitespace()).collect();
        if part.is_empty() {
            return None;
        }
        match part.split_once('-') {
            Some((first, last)) => {
                let first = crate::iface::parse(first, kind)?;
                let (prefix, start) = first.rsplit_once('/').map(|(p, n)| (format!("{p}/"), n))?;
                let (start, end): (u32, u32) = (start.parse().ok()?, last.parse().ok()?);
                if end < start {
                    return None;
                }
                for n in start..=end {
                    let name = format!("{prefix}{n}");
                    if !kind.has_port(&name) {
                        return None;
                    }
                    names.push(name);
                }
            }
            None => names.push(crate::iface::parse(&part, kind).filter(|n| kind.has_port(n))?),
        }
    }
    names.dedup();
    Some(names)
}

fn short(port: &str) -> String {
    abbreviate_port(port)
}

/// Access ports of each VLAN, as `show vlan brief` groups them.
fn vlan_members(device: &Device, vlan: u16) -> Vec<String> {
    device
        .kind
        .port_names()
        .into_iter()
        .filter(|p| {
            let sp = device.config.interface(device.kind, p).switchport;
            !sp.is_trunk() && !sp.routed && sp.access_vlan == vlan
        })
        .collect()
}

fn vlan_brief(device: &Device, full: bool) -> String {
    let mut out = String::from(
        "\nVLAN Name                             Status    Ports\n\
         ---- -------------------------------- --------- -------------------------------\n",
    );
    for vlan in device.config.vlan_ids() {
        let ports: Vec<String> = vlan_members(device, vlan)
            .iter()
            .map(|p| short(p))
            .collect();
        let mut chunks = ports.chunks(4).map(|c| c.join(", "));
        let first = chunks.next().unwrap_or_default();
        let _ = writeln!(
            out,
            "{:<5}{:<33}{:<10}{}",
            vlan,
            device.config.vlan_name(vlan),
            "active",
            first
        );
        for chunk in chunks {
            let _ = writeln!(out, "{:48}{}", "", chunk);
        }
    }
    for (id, name) in [
        (1002, "fddi-default"),
        (1003, "token-ring-default"),
        (1004, "fddinet-default"),
        (1005, "trnet-default"),
    ] {
        let _ = writeln!(out, "{id:<5}{name:<33}act/unsup");
    }
    if full {
        out += "\nVLAN Type  SAID       MTU   Parent RingNo BridgeNo Stp  BrdgMode Trans1 Trans2\n\
                ---- ----- ---------- ----- ------ ------ -------- ---- -------- ------ ------\n";
        for vlan in device.config.vlan_ids() {
            let _ = writeln!(
                out,
                "{vlan:<5}enet  {:<11}1500  -      -      -        -    -        0      0",
                100000 + u32::from(vlan)
            );
        }
    }
    out
}

fn trunk_ports(project: &Project, device: &Device) -> Vec<String> {
    let rows = interface_rows(project, device.id);
    device
        .kind
        .port_names()
        .into_iter()
        .filter(|p| {
            device
                .config
                .interface(device.kind, p)
                .switchport
                .is_trunk()
                && rows.iter().any(|r| r.name == *p && r.is_up())
        })
        .collect()
}

fn trunks(project: &Project, device: &Device, l2: &Layer2) -> String {
    let ports = trunk_ports(project, device);
    if ports.is_empty() {
        return String::new();
    }
    let sp = |p: &str| device.config.interface(device.kind, p).switchport;
    let mut out =
        String::from("\nPort        Mode             Encapsulation  Status        Native vlan\n");
    for p in &ports {
        let _ = writeln!(
            out,
            "{:<12}{:<17}{:<15}{:<14}{}",
            short(p),
            "on",
            "802.1q",
            "trunking",
            sp(p).native_vlan
        );
    }
    out += "\nPort        Vlans allowed on trunk\n";
    for p in &ports {
        let _ = writeln!(out, "{:<12}{}", short(p), sp(p).allowed_vlans.to_ios());
    }
    let active = |p: &str| -> Vec<u16> {
        device
            .config
            .vlan_ids()
            .into_iter()
            .filter(|v| sp(p).allowed_vlans.contains(*v))
            .collect()
    };
    let join = |ids: Vec<u16>| {
        if ids.is_empty() {
            "none".to_string()
        } else {
            VlanSet(ids.iter().map(|v| (*v, *v)).collect())
                .union(&VlanSet::none())
                .to_ios()
        }
    };
    out += "\nPort        Vlans allowed and active in management domain\n";
    for p in &ports {
        let _ = writeln!(out, "{:<12}{}", short(p), join(active(p)));
    }
    out += "\nPort        Vlans in spanning tree forwarding state and not pruned\n";
    for p in &ports {
        let forwarding: Vec<u16> = active(p)
            .into_iter()
            .filter(|v| l2.forwards(device.id, p, *v))
            .collect();
        let _ = writeln!(out, "{:<12}{}", short(p), join(forwarding));
    }
    out
}

fn interface_status(project: &Project, device: &Device, network: &Network) -> String {
    let rows = interface_rows(project, device.id);
    let mut out =
        String::from("\nPort      Name               Status       Vlan       Duplex  Speed Type\n");
    for p in device.kind.port_names() {
        let config = device.config.interface(device.kind, &p);
        let row = rows.iter().find(|r| r.name == p);
        let status = if network.err_disabled_reason(device.id, &p).is_some() {
            "err-disabled"
        } else if config.shutdown {
            "disabled"
        } else if row.is_some_and(|r| r.is_up()) {
            "connected"
        } else {
            "notconnect"
        };
        let vlan = if config.switchport.routed {
            "routed".to_string()
        } else if config.switchport.is_trunk() {
            "trunk".to_string()
        } else {
            config.switchport.access_vlan.to_string()
        };
        let fast = p.starts_with("Fast");
        let (duplex, speed) = if status == "connected" {
            ("a-full", if fast { "a-100" } else { "a-1000" })
        } else {
            ("auto", "auto")
        };
        let kind = if fast {
            "10/100BaseTX"
        } else {
            "10/100/1000BaseTX"
        };
        let name: String = config
            .description
            .unwrap_or_default()
            .chars()
            .take(18)
            .collect();
        let _ = writeln!(
            out,
            "{:<10}{:<19}{:<13}{:<11}{:>6} {:>6} {kind}",
            short(&p),
            name,
            status,
            vlan,
            duplex,
            speed
        );
    }
    out
}

fn switchport_detail(device: &Device, name: &str) -> String {
    let config = device.config.interface(device.kind, name);
    let sp = config.switchport;
    if sp.routed {
        return format!("Name: {}\nSwitchport: Disabled\n", short(name));
    }
    let admin = match sp.mode {
        PortMode::Access => "static access",
        PortMode::Trunk => "trunk",
        PortMode::DynamicAuto => "dynamic auto",
    };
    let oper = if sp.is_trunk() {
        "trunk"
    } else {
        "static access"
    };
    format!(
        "Name: {}\nSwitchport: Enabled\nAdministrative Mode: {admin}\nOperational Mode: {oper}\n\
         Administrative Trunking Encapsulation: dot1q\nOperational Trunking Encapsulation: {}\n\
         Negotiation of Trunking: {}\nAccess Mode VLAN: {} ({})\nTrunking Native Mode VLAN: {} ({})\n\
         Trunking VLANs Enabled: {}\nProtected: false\n",
        short(name),
        if sp.is_trunk() { "dot1q" } else { "native" },
        if sp.nonegotiate || sp.mode == PortMode::Access {
            "Off"
        } else {
            "On"
        },
        sp.access_vlan,
        device.config.vlan_name(sp.access_vlan),
        sp.native_vlan,
        device.config.vlan_name(sp.native_vlan),
        if sp.allowed_vlans.is_all() {
            "ALL".to_string()
        } else {
            sp.allowed_vlans.to_ios()
        },
    )
}

fn spanning_tree(device: &Device, l2: &Layer2, only: Option<u16>) -> String {
    let protocol = match device.config.spanning_tree.mode {
        StpMode::Pvst => "ieee",
        StpMode::RapidPvst => "rstp",
    };
    let mut out = String::new();
    for (vlan, tree) in &l2.trees {
        if only.is_some_and(|v| v != *vlan) {
            continue;
        }
        let Some(me) = tree.bridges.get(&device.id) else {
            continue;
        };
        let _ = writeln!(
            out,
            "\nVLAN{vlan:04}\n  Spanning tree enabled protocol {protocol}"
        );
        let _ = writeln!(
            out,
            "  Root ID    Priority    {}\n             Address     {}",
            tree.root.priority,
            cisco_mac(tree.root.mac)
        );
        if tree.root_device == device.id {
            out += "             This bridge is the root\n";
        } else if let Some(port) = &me.root_port {
            let number = tree
                .ports
                .get(&(device.id, port.clone()))
                .map(|p| p.number)
                .unwrap_or(0);
            let _ = writeln!(
                out,
                "             Cost        {}\n             Port        {number} ({port})",
                me.root_cost
            );
        }
        out += "             Hello Time   2 sec  Max Age 20 sec  Forward Delay 15 sec\n\n";
        let configured = me.id.priority - u32::from(*vlan);
        let _ = writeln!(
            out,
            "  Bridge ID  Priority    {}  (priority {configured} sys-id-ext {vlan})\n             Address     {}\n             \
             Hello Time   2 sec  Max Age 20 sec  Forward Delay 15 sec\n             Aging Time  300 sec\n",
            me.id.priority,
            cisco_mac(me.id.mac)
        );
        out += "Interface           Role Sts Cost      Prio.Nbr Type\n\
                ------------------- ---- --- --------- -------- --------------------------------\n";
        let mut ports: Vec<_> = tree
            .ports
            .iter()
            .filter(|((d, _), _)| *d == device.id)
            .collect();
        ports.sort_by_key(|((_, p), _)| natural_key(p));
        for ((_, port), info) in ports {
            let state = if info.role.forwards() { "FWD" } else { "BLK" };
            let edge = if device
                .config
                .interface(device.kind, port)
                .switchport
                .portfast
            {
                "P2p Edge"
            } else {
                "P2p"
            };
            let _ = writeln!(
                out,
                "{:<20}{:<5}{:<4}{:<10}{:<9}{edge}",
                short(port),
                info.role.ios(),
                state,
                info.cost,
                format!("128.{}", info.number)
            );
        }
    }
    if out.is_empty() {
        return match only {
            Some(v) => format!("Spanning tree instance(s) for vlan {v} does not exist."),
            None => "No spanning tree instance exists.".into(),
        };
    }
    out
}

/// "Gig 0/1", as CDP lists interfaces.
fn cdp_port(port: &str) -> String {
    let at = port
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(port.len());
    format!("{} {}", &port[..at.min(3)], &port[at..])
}

fn cdp(project: &Project, device: &Device, detail: bool) -> String {
    if !device.config.cdp_run {
        return "% CDP is not enabled".into();
    }
    let neighbors = discovery_neighbors(project, device.id, false);
    if detail {
        let mut out = String::new();
        for n in &neighbors {
            let caps = n
                .capability
                .split(' ')
                .map(|c| match c {
                    "R" => "Router",
                    "S" => "Switch",
                    "I" => "IGMP",
                    "H" => "Host",
                    "P" => "Phone",
                    _ => "Trans-Bridge",
                })
                .collect::<Vec<_>>()
                .join(" ");
            let _ = write!(
                out,
                "-------------------------\nDevice ID: {}\nEntry address(es): \n",
                n.name
            );
            if let Some(a) = n.address {
                let _ = writeln!(out, "  IP address: {a}");
            }
            let _ = writeln!(
                out,
                "Platform: cisco {},  Capabilities: {caps}\nInterface: {},  Port ID (outgoing port): {}\nHoldtime : 165 sec\n\n\
                 Version :\nnetrace IOS-style software, Version 15.4 (simulated)\n\nadvertisement version: 2\nDuplex: full\n",
                n.platform, n.local_port, n.remote_port
            );
        }
        let _ = writeln!(out, "\nTotal cdp entries displayed : {}", neighbors.len());
        return out;
    }
    let mut out = String::from(
        "Capability Codes: R - Router, T - Trans Bridge, B - Source Route Bridge\n                  \
         S - Switch, H - Host, I - IGMP, r - Repeater, P - Phone,\n                  \
         D - Remote, C - CVTA, M - Two-port Mac Relay\n\n\
         Device ID        Local Intrfce     Holdtme    Capability  Platform  Port ID\n",
    );
    for n in &neighbors {
        let _ = writeln!(
            out,
            "{:<17}{:<18}{:<11}{:>10}  {:<10}{}",
            n.name,
            cdp_port(&n.local_port),
            165,
            n.capability,
            n.platform,
            cdp_port(&n.remote_port)
        );
    }
    let _ = writeln!(out, "\nTotal cdp entries displayed : {}", neighbors.len());
    out
}

fn lldp(project: &Project, device: &Device) -> String {
    if !device.config.lldp_run {
        return "% LLDP is not enabled".into();
    }
    let neighbors = discovery_neighbors(project, device.id, true);
    let mut out = String::from(
        "Capability codes:\n    (R) Router, (B) Bridge, (T) Telephone, (C) DOCSIS Cable Device\n    \
         (W) WLAN Access Point, (P) Repeater, (S) Station, (O) Other\n\n\
         Device ID           Local Intf     Hold-time  Capability      Port ID\n",
    );
    for n in &neighbors {
        let caps = n
            .capability
            .replace("S I", "B")
            .replace('I', "")
            .replace('S', "B")
            .trim()
            .replace(' ', ",");
        let _ = writeln!(
            out,
            "{:<20}{:<15}{:<11}{:<16}{}",
            n.name,
            short(&n.local_port),
            120,
            caps,
            short(&n.remote_port)
        );
    }
    let _ = writeln!(out, "\nTotal entries displayed: {}", neighbors.len());
    out
}

fn secure_ports(device: &Device) -> Vec<(String, PortSecurity)> {
    device
        .kind
        .port_names()
        .into_iter()
        .filter_map(|p| {
            let ps = device
                .config
                .interface(device.kind, &p)
                .switchport
                .port_security?;
            ps.enabled.then_some((p, ps))
        })
        .collect()
}

fn port_security_summary(device: &Device, network: &Network) -> String {
    let mut out = String::from(
        "Secure Port  MaxSecureAddr  CurrentAddr  SecurityViolation  Security Action\n                \
         (Count)       (Count)          (Count)\n\
         ---------------------------------------------------------------------------\n",
    );
    for (port, ps) in secure_ports(device) {
        let (learned, violations) = network.port_security_state(device.id, &port);
        let current = learned.len() + ps.static_macs.len();
        let action = match ps.violation {
            Violation::Shutdown => "Shutdown",
            Violation::Restrict => "Restrict",
            Violation::Protect => "Protect",
        };
        let _ = writeln!(
            out,
            "{:>11}{:>15}{:>13}{:>19}{:>17}",
            short(&port),
            ps.maximum,
            current,
            violations,
            action
        );
    }
    out += "---------------------------------------------------------------------------\n\
            Total Addresses in System (excluding one mac per port)     : 0\n\
            Max Addresses limit in System (excluding one mac per port) : 4096\n";
    out
}

fn port_security_interface(device: &Device, network: &Network, port: &str) -> String {
    let sp = device.config.interface(device.kind, port).switchport;
    let ps = sp.port_security.clone().unwrap_or_default();
    let (learned, violations) = network.port_security_state(device.id, port);
    let status = if !ps.enabled {
        "Secure-down"
    } else if network.err_disabled_reason(device.id, port) == Some("psecure-violation") {
        "Secure-shutdown"
    } else {
        "Secure-up"
    };
    let last = learned
        .last()
        .map(|m| format!("{}:{}", cisco_mac(*m), sp.access_vlan))
        .unwrap_or_else(|| "0000.0000.0000:0".into());
    format!(
        "Port Security              : {}\nPort Status                : {status}\nViolation Mode             : {}\n\
         Aging Time                 : 0 mins\nAging Type                 : Absolute\n\
         SecureStatic Address Aging : Disabled\nMaximum MAC Addresses      : {}\nTotal MAC Addresses        : {}\n\
         Configured MAC Addresses   : {}\nSticky MAC Addresses       : {}\nLast Source Address:Vlan   : {last}\n\
         Security Violation Count   : {violations}\n",
        if ps.enabled { "Enabled" } else { "Disabled" },
        capitalised(ps.violation.ios()),
        ps.maximum,
        learned.len() + ps.static_macs.len(),
        ps.static_macs.len(),
        if ps.sticky { learned.len() } else { 0 },
    )
}

fn port_security_addresses(device: &Device, network: &Network) -> String {
    let mut out = String::from(
        "               Secure Mac Address Table\n\
         -----------------------------------------------------------------------------\n\
         Vlan    Mac Address       Type                          Ports   Remaining Age\n                                                                   \
         (mins)\n----    -----------       ----                          -----   -------------\n",
    );
    for (port, ps) in secure_ports(device) {
        let vlan = device
            .config
            .interface(device.kind, &port)
            .switchport
            .access_vlan;
        for mac in &ps.static_macs {
            let _ = writeln!(
                out,
                "{vlan:>4}    {mac:<18}{:<30}{:<8}-",
                "SecureConfigured",
                short(&port)
            );
        }
        let (learned, _) = network.port_security_state(device.id, &port);
        let kind = if ps.sticky {
            "SecureSticky"
        } else {
            "SecureDynamic"
        };
        for mac in learned {
            let _ = writeln!(
                out,
                "{vlan:>4}    {:<18}{kind:<30}{:<8}-",
                cisco_mac(mac),
                short(&port)
            );
        }
    }
    out += "-----------------------------------------------------------------------------\n\
            Total Addresses in System (excluding one mac per port)     : 0\n\
            Max Addresses limit in System (excluding one mac per port) : 4096\n";
    out
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}
