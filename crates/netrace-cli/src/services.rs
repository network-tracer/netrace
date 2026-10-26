//! DHCP and access-list commands, and their `show` output.

use std::fmt::Write;
use std::net::Ipv4Addr;

use netrace_doc::{
    AccessList, AclAction, AclAddress, AclEntry, AclKind, AclProtocol, Device, DeviceKind,
    DhcpPool, IcmpMatch, PortMatch, Project,
};
use netrace_engine::Network;

use crate::grammar::Value;
use crate::render::cisco_mac;
use crate::{Mode, Session};

impl Session {
    /// Runs a DHCP or ACL command; `None` if `id` isn't one.
    pub(crate) fn services(
        &mut self,
        id: &str,
        values: &[Value],
        line: &str,
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
        let words: Vec<String> = values
            .iter()
            .filter_map(|v| match v {
                Value::Word(w) | Value::Line(w) => Some(w.clone()),
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
        let keywords: Vec<&str> = values
            .iter()
            .filter_map(|v| match v {
                Value::Keyword(k) => Some(*k),
                _ => None,
            })
            .collect();
        let device = project.device(self.device).cloned()?;
        let out = match id {
            // DHCP server
            "dhcp-pool" => {
                let name = words.first()?.clone();
                self.update(project, |c| {
                    if !c.dhcp_pools.iter().any(|p| p.name == name) {
                        c.dhcp_pools.push(DhcpPool::new(&name));
                    }
                });
                self.mode = Mode::DhcpPool(name);
                String::new()
            }
            "no-dhcp-pool" => {
                let name = words.first()?.clone();
                self.update(project, |c| c.dhcp_pools.retain(|p| p.name != name));
                String::new()
            }
            "dhcp-network" | "dhcp-router" | "dhcp-dns" | "dhcp-domain" => {
                let Mode::DhcpPool(name) = self.mode.clone() else {
                    return Some(String::new());
                };
                if id == "dhcp-network" {
                    let Some(prefix) = netrace_doc::prefix_from_mask(ips[1]) else {
                        return Some(format!("% Bad mask {} for network {}", ips[1], ips[0]));
                    };
                    let network = Ipv4Addr::from(u32::from(ips[0]) & u32::from(ips[1]));
                    self.update(project, |c| {
                        if let Some(p) = c.dhcp_pools.iter_mut().find(|p| p.name == name) {
                            p.network = Some((network, prefix));
                        }
                    });
                } else {
                    let word = words.first().cloned();
                    let ip = ips.first().copied();
                    self.update(project, |c| {
                        if let Some(p) = c.dhcp_pools.iter_mut().find(|p| p.name == name) {
                            match id {
                                "dhcp-router" => p.default_router = ip,
                                "dhcp-dns" => p.dns_server = ip,
                                _ => p.domain_name = word.clone(),
                            }
                        }
                    });
                }
                String::new()
            }
            "dhcp-excluded" | "no-dhcp-excluded" => {
                let range = (ips[0], ips.get(1).copied().unwrap_or(ips[0]));
                if u32::from(range.1) < u32::from(range.0) {
                    return Some(
                        "% Invalid address range: the high address is below the low one.".into(),
                    );
                }
                self.update(project, |c| {
                    c.dhcp_excluded.retain(|r| *r != range);
                    if id == "dhcp-excluded" {
                        c.dhcp_excluded.push(range);
                    }
                });
                String::new()
            }
            "helper" | "no-helper" => {
                let names = self.interface_names();
                let ip = ips.first().copied();
                self.update(project, |c| {
                    for n in &names {
                        let i = c.interface_mut(kind, n);
                        match ip {
                            Some(ip) if id == "helper" => {
                                if !i.helper_addresses.contains(&ip) {
                                    i.helper_addresses.push(ip)
                                }
                            }
                            Some(ip) => i.helper_addresses.retain(|h| *h != ip),
                            None => i.helper_addresses.clear(),
                        }
                    }
                });
                String::new()
            }

            // Access lists
            "acl-numbered" => {
                let number = nums[0];
                let Some(acl_kind) = AccessList::kind_for_number(number) else {
                    return Some("% Invalid access list number".into());
                };
                let action = if keywords.contains(&"permit") {
                    AclAction::Permit
                } else {
                    AclAction::Deny
                };
                let rest = words.first().cloned().unwrap_or_default();
                let entry = match parse_entry(acl_kind, action, &rest) {
                    Ok(e) => e,
                    Err(at) => return Some(self.caret(project, line, &rest, at)),
                };
                let name = number.to_string();
                self.update(project, |c| {
                    let list = upsert(c, &name, acl_kind);
                    if list.has_rule(&entry) {
                        return;
                    }
                    let sequence = list.next_sequence();
                    list.entries.push(AclEntry {
                        sequence,
                        ..entry.clone()
                    });
                });
                String::new()
            }
            "acl-remark" => {
                let name = nums[0].to_string();
                let kind_for = AccessList::kind_for_number(nums[0])?;
                let text = words.first().cloned().unwrap_or_default();
                self.update(project, |c| {
                    upsert(c, &name, kind_for).remarks.push(text.clone())
                });
                String::new()
            }
            "no-acl" => {
                let name = nums[0].to_string();
                self.update(project, |c| c.access_lists.retain(|a| a.name != name));
                String::new()
            }
            "acl-named" => {
                let name = words.first()?.clone();
                let acl_kind = if keywords.contains(&"standard") {
                    AclKind::Standard
                } else {
                    AclKind::Extended
                };
                if let Some(existing) = device.config.access_list(&name)
                    && existing.kind != acl_kind
                {
                    return Some(
                        format!(
                            "% A {:?} access list named {name} already exists.",
                            existing.kind
                        )
                        .to_lowercase(),
                    );
                }
                self.update(project, |c| {
                    upsert(c, &name, acl_kind);
                });
                self.mode = Mode::Acl(name, acl_kind);
                String::new()
            }
            "no-acl-named" => {
                let name = words.first()?.clone();
                self.update(project, |c| {
                    c.access_lists
                        .retain(|a| !a.name.eq_ignore_ascii_case(&name))
                });
                String::new()
            }
            "nacl-permit" | "nacl-deny" | "nacl-seq-permit" | "nacl-seq-deny" => {
                let Mode::Acl(name, acl_kind) = self.mode.clone() else {
                    return Some(String::new());
                };
                let action = if id.ends_with("permit") {
                    AclAction::Permit
                } else {
                    AclAction::Deny
                };
                let rest = words.first().cloned().unwrap_or_default();
                let entry = match parse_entry(acl_kind, action, &rest) {
                    Ok(e) => e,
                    Err(at) => return Some(self.caret(project, line, &rest, at)),
                };
                let sequence = nums.first().copied();
                self.update(project, |c| {
                    let list = upsert(c, &name, acl_kind);
                    if list.has_rule(&entry) {
                        return;
                    }
                    let sequence = sequence.unwrap_or_else(|| list.next_sequence());
                    list.entries.retain(|e| e.sequence != sequence);
                    list.entries.push(AclEntry {
                        sequence,
                        ..entry.clone()
                    });
                    list.entries.sort_by_key(|e| e.sequence);
                });
                String::new()
            }
            "nacl-no-seq" => {
                let Mode::Acl(name, _) = self.mode.clone() else {
                    return Some(String::new());
                };
                let seq = nums[0];
                self.update(project, |c| {
                    if let Some(list) = c.access_lists.iter_mut().find(|a| a.name == name) {
                        list.entries.retain(|e| e.sequence != seq);
                    }
                });
                String::new()
            }
            "nacl-remark" => {
                let Mode::Acl(name, acl_kind) = self.mode.clone() else {
                    return Some(String::new());
                };
                let text = words.first().cloned().unwrap_or_default();
                self.update(project, |c| {
                    upsert(c, &name, acl_kind).remarks.push(text.clone())
                });
                String::new()
            }
            "access-group" | "no-access-group" => {
                let name = words.first()?.clone();
                let inbound = keywords.contains(&"in");
                let names = self.interface_names();
                let set = id == "access-group";
                self.update(project, |c| {
                    for n in &names {
                        let i = c.interface_mut(kind, n);
                        let slot = if inbound {
                            &mut i.acl_in
                        } else {
                            &mut i.acl_out
                        };
                        if set {
                            *slot = Some(name.clone());
                        } else if slot.as_deref() == Some(name.as_str()) {
                            *slot = None;
                        }
                    }
                });
                String::new()
            }

            // show
            "show-dhcp-binding" => dhcp_bindings(network, &device),
            "show-dhcp-pool" => dhcp_pools(network, &device),
            "show-acls" | "show-ip-acls" => {
                access_lists(network, &device, words.first().map(String::as_str))
            }
            _ => return None,
        };
        Some(out)
    }

    /// `% Invalid input detected at '^' marker.` pointing into the rest of the line.
    fn caret(&self, project: &Project, line: &str, rest: &str, token: usize) -> String {
        let rest_start = line.len() - line.trim_start().len()
            + line.trim_start().find(rest.trim_start()).unwrap_or(0);
        let tokens = crate::grammar::tokenize(rest);
        let offset = tokens.get(token).map(|t| t.start).unwrap_or(rest.len());
        let prompt = self.prompt(project).chars().count();
        format!(
            "{}^\n% Invalid input detected at '^' marker.",
            " ".repeat(prompt + rest_start + offset)
        )
    }
}

fn upsert<'a>(
    config: &'a mut netrace_doc::DeviceConfig,
    name: &str,
    kind: AclKind,
) -> &'a mut AccessList {
    if let Some(i) = config
        .access_lists
        .iter()
        .position(|a| a.name.eq_ignore_ascii_case(name))
    {
        return &mut config.access_lists[i];
    }
    config.access_lists.push(AccessList {
        name: name.to_string(),
        kind,
        entries: Vec::new(),
        remarks: Vec::new(),
    });
    config.access_lists.last_mut().expect("just pushed")
}

/// Parses the part of an ACL line after permit / deny. Errors give the
/// index of the offending word.
pub(crate) fn parse_entry(kind: AclKind, action: AclAction, text: &str) -> Result<AclEntry, usize> {
    let tokens: Vec<String> = text
        .split_whitespace()
        .map(|t| t.to_ascii_lowercase())
        .collect();
    let mut i = 0;
    let address = |i: &mut usize| -> Result<AclAddress, usize> {
        match tokens.get(*i).map(String::as_str) {
            Some("any") => {
                *i += 1;
                Ok(AclAddress::ANY)
            }
            Some("host") => {
                let ip = tokens
                    .get(*i + 1)
                    .and_then(|t| t.parse().ok())
                    .ok_or(*i + 1)?;
                *i += 2;
                Ok(AclAddress::host(ip))
            }
            Some(t) => {
                let address: Ipv4Addr = t.parse().map_err(|_| *i)?;
                // A wildcard is optional on standard lists (then it's a host).
                match tokens.get(*i + 1).and_then(|w| w.parse::<Ipv4Addr>().ok()) {
                    Some(wildcard) => {
                        *i += 2;
                        Ok(AclAddress { address, wildcard })
                    }
                    None => {
                        *i += 1;
                        Ok(AclAddress::host(address))
                    }
                }
            }
            None => Err(*i),
        }
    };
    if kind == AclKind::Standard {
        let source = address(&mut i)?;
        if i < tokens.len() {
            return Err(i);
        }
        return Ok(AclEntry {
            sequence: 0,
            action,
            protocol: AclProtocol::Ip,
            source,
            source_port: None,
            destination: AclAddress::ANY,
            destination_port: None,
            icmp: None,
        });
    }
    let protocol = match tokens.first().map(String::as_str) {
        Some("ip") => AclProtocol::Ip,
        Some("icmp") => AclProtocol::Icmp,
        Some("tcp") => AclProtocol::Tcp,
        Some("udp") => AclProtocol::Udp,
        _ => return Err(0),
    };
    i = 1;
    let ports = protocol == AclProtocol::Tcp || protocol == AclProtocol::Udp;
    let port_match = |i: &mut usize| -> Result<Option<PortMatch>, usize> {
        if !ports {
            return Ok(None);
        }
        let op = match tokens.get(*i).map(String::as_str) {
            Some(op @ ("eq" | "gt" | "lt" | "range")) => op.to_string(),
            _ => return Ok(None),
        };
        let port = |t: Option<&String>, at: usize| -> Result<u16, usize> {
            let t = t.ok_or(at)?;
            Ok(match t.as_str() {
                "bootps" => 67,
                "bootpc" => 68,
                "www" | "http" => 80,
                "domain" => 53,
                "telnet" => 23,
                "ftp" => 21,
                "smtp" => 25,
                "tftp" => 69,
                n => n.parse().map_err(|_| at)?,
            })
        };
        let m = match op.as_str() {
            "eq" => PortMatch::Eq(port(tokens.get(*i + 1), *i + 1)?),
            "gt" => PortMatch::Gt(port(tokens.get(*i + 1), *i + 1)?),
            "lt" => PortMatch::Lt(port(tokens.get(*i + 1), *i + 1)?),
            _ => {
                let m = PortMatch::Range(
                    port(tokens.get(*i + 1), *i + 1)?,
                    port(tokens.get(*i + 2), *i + 2)?,
                );
                *i += 1;
                m
            }
        };
        *i += 2;
        Ok(Some(m))
    };
    let source = address(&mut i)?;
    let source_port = port_match(&mut i)?;
    let destination = address(&mut i)?;
    let destination_port = port_match(&mut i)?;
    let icmp = if protocol == AclProtocol::Icmp {
        match tokens.get(i).map(String::as_str) {
            Some("echo") => Some(IcmpMatch::Echo),
            Some("echo-reply") => Some(IcmpMatch::EchoReply),
            Some("unreachable") => Some(IcmpMatch::Unreachable),
            Some("time-exceeded") => Some(IcmpMatch::TimeExceeded),
            _ => None,
        }
    } else {
        None
    };
    if icmp.is_some() {
        i += 1;
    }
    if i < tokens.len() {
        return Err(i);
    }
    Ok(AclEntry {
        sequence: 0,
        action,
        protocol,
        source,
        source_port,
        destination,
        destination_port,
        icmp,
    })
}

fn dhcp_bindings(network: &Network, device: &Device) -> String {
    let mut out = String::from(
        "Bindings from all pools not associated with VRF:\n\
         IP address      Client-ID/              Lease expiration        Type\n                \
         Hardware address\n",
    );
    for b in network.dhcp_bindings(device.id) {
        let _ = writeln!(
            out,
            "{:<16}{:<24}{:<24}Automatic",
            b.address.to_string(),
            cisco_mac(b.mac),
            "Infinite"
        );
    }
    out
}

fn dhcp_pools(network: &Network, device: &Device) -> String {
    let bindings = network.dhcp_bindings(device.id);
    let mut out = String::new();
    for pool in &device.config.dhcp_pools {
        let leased = bindings.iter().filter(|b| b.pool == pool.name).count();
        let _ = writeln!(out, "\nPool {} :", pool.name);
        match pool.network {
            Some((net, len)) => {
                let total = (1u64 << (32 - u32::from(len))).saturating_sub(2);
                let _ = writeln!(
                    out,
                    " Utilization mark (high/low)    : 100 / 0\n Subnet size (first/next)       : 0 / 0\n \
                     Total addresses                : {total}\n Leased addresses               : {leased}\n \
                     Excluded addresses             : {}\n Pending event                  : none\n \
                     1 subnet is currently in the pool :\n Current index        IP address range                    Leased/Excluded/Total\n \
                     {:<21}{} - {:<20}{leased} / {} / {total}",
                    device.config.dhcp_excluded.len(),
                    Ipv4Addr::from(u32::from(net) + 1),
                    Ipv4Addr::from(u32::from(net) + 1),
                    Ipv4Addr::from((u32::from(net) | !u32::from(netrace_doc::mask(len))) - 1),
                    device.config.dhcp_excluded.len()
                );
            }
            None => out += " (no network configured)\n",
        }
    }
    if out.is_empty() {
        out = "% No DHCP pools configured".into();
    }
    out
}

fn access_lists(network: &Network, device: &Device, only: Option<&str>) -> String {
    let hits = network.acl_hits(device.id);
    let mut out = String::new();
    for acl in &device.config.access_lists {
        if only.is_some_and(|n| !n.eq_ignore_ascii_case(&acl.name)) {
            continue;
        }
        let kind = match acl.kind {
            AclKind::Standard => "Standard",
            AclKind::Extended => "Extended",
        };
        let _ = writeln!(out, "{kind} IP access list {}", acl.name);
        for e in &acl.entries {
            let matches = hits
                .get(&(acl.name.clone(), e.sequence))
                .map(|n| format!(" ({n} match{})", if *n == 1 { "" } else { "es" }))
                .unwrap_or_default();
            let _ = writeln!(out, "    {} {}{matches}", e.sequence, e.ios(acl.kind));
        }
    }
    out
}
