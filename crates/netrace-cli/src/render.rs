//! `show` output, laid out like IOS so it reads the way students will see
//! it on real equipment.

use std::fmt::Write;
use std::net::Ipv4Addr;

use netrace_doc::{Device, DeviceConfig, DeviceKind, RouteTarget, mask};
use netrace_engine::{
    InterfaceRow, MacAddr, Network, Route, RouteSource, abbreviate_port, natural_key,
};

/// "024e.0000.0100": how IOS writes MAC addresses.
pub(crate) fn cisco_mac(mac: MacAddr) -> String {
    let b = mac.0;
    format!(
        "{:02x}{:02x}.{:02x}{:02x}.{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5]
    )
}

/// Cisco "type 7" password obfuscation, as `service password-encryption`
/// shows line passwords. (Reversible; not real security, which is the lesson.)
pub(crate) fn type7(password: &str) -> String {
    const KEY: &[u8] = b"dsfd;kfoA,.iyewrkldJKDHSUBsgvca69834ncxv9873254k;fg87";
    let seed = 2usize;
    let mut out = format!("{seed:02}");
    for (i, byte) in password.bytes().enumerate() {
        let _ = write!(out, "{:02X}", byte ^ KEY[(seed + i) % KEY.len()]);
    }
    out
}

/// Stand-in for the MD5 hash IOS shows for `enable secret`: deterministic,
/// looks the part, and never shows the secret.
pub(crate) fn secret_hash(secret: &str) -> String {
    const ALPHABET: &[u8] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    let mut next = |byte: u8| {
        state ^= u64::from(byte);
        state = state.wrapping_mul(0x0100_0000_01b3);
        ALPHABET[(state >> 58) as usize % ALPHABET.len()] as char
    };
    let salt: String = secret
        .bytes()
        .chain(*b"salt")
        .take(4)
        .map(&mut next)
        .collect();
    let body: String = secret.bytes().cycle().take(22).map(&mut next).collect();
    format!("$1${salt}${body}")
}

fn line_password(config: &DeviceConfig, password: &str) -> String {
    if config.password_encryption {
        format!("password 7 {}", type7(password))
    } else {
        format!("password {password}")
    }
}

fn interface_names(device: &Device) -> Vec<String> {
    let mut names = device.kind.port_names();
    let mut extra: Vec<String> = device
        .config
        .interfaces
        .keys()
        .filter(|n| !device.kind.has_port(n))
        .cloned()
        .collect();
    if device.kind.is_switch() && !extra.iter().any(|n| n == "Vlan1") {
        extra.push("Vlan1".into());
    }
    extra.sort_by_key(|n| natural_key(n));
    // Loopbacks first, as IOS lists them; then ports, subinterfaces after their parent.
    let (loopbacks, rest): (Vec<String>, Vec<String>) =
        extra.into_iter().partition(|n| n.starts_with("Loopback"));
    let (vlans, subifs): (Vec<String>, Vec<String>) =
        rest.into_iter().partition(|n| n.starts_with("Vlan"));
    let mut ordered = loopbacks;
    for port in names.drain(..) {
        let prefix = format!("{port}.");
        ordered.push(port);
        ordered.extend(subifs.iter().filter(|s| s.starts_with(&prefix)).cloned());
    }
    ordered.extend(vlans);
    ordered
}

/// The configuration as `show running-config` / `show startup-config` print it.
pub(crate) fn config_text(device: &Device, hostname: &str, config: &DeviceConfig) -> String {
    let kind = device.kind;
    let mut out = String::new();
    let router = kind == DeviceKind::Router;
    out += "!\n";
    out += if router {
        "version 15.4\n"
    } else {
        "version 15.0\n"
    };
    if !router {
        out += "no service pad\n";
    }
    out += "service timestamps debug datetime msec\nservice timestamps log datetime msec\n";
    out += if config.password_encryption {
        "service password-encryption\n"
    } else {
        "no service password-encryption\n"
    };
    let _ = write!(
        out,
        "!\nhostname {hostname}\n!\nboot-start-marker\nboot-end-marker\n!\n"
    );
    if let Some(secret) = &config.enable_secret {
        let _ = writeln!(out, "enable secret 5 {}", secret_hash(secret));
    }
    if let Some(password) = &config.enable_password {
        if config.password_encryption {
            let _ = writeln!(out, "enable password 7 {}", type7(password));
        } else {
            let _ = writeln!(out, "enable password {password}");
        }
    }
    if config.enable_secret.is_some() || config.enable_password.is_some() {
        out += "!\n";
    }
    out += "no aaa new-model\n!\n";
    for (low, high) in &config.dhcp_excluded {
        if low == high {
            let _ = writeln!(out, "ip dhcp excluded-address {low}");
        } else {
            let _ = writeln!(out, "ip dhcp excluded-address {low} {high}");
        }
    }
    if !config.dhcp_excluded.is_empty() {
        out += "!\n";
    }
    for pool in &config.dhcp_pools {
        let _ = writeln!(out, "ip dhcp pool {}", pool.name);
        if let Some((net, len)) = pool.network {
            let _ = writeln!(out, " network {net} {}", netrace_doc::mask(len));
        }
        if let Some(r) = pool.default_router {
            let _ = writeln!(out, " default-router {r}");
        }
        if let Some(d) = pool.dns_server {
            let _ = writeln!(out, " dns-server {d}");
        }
        if let Some(d) = &pool.domain_name {
            let _ = writeln!(out, " domain-name {d}");
        }
        out += "!\n";
    }
    if !config.domain_lookup {
        out += "no ip domain lookup\n";
    }
    if kind == DeviceKind::L3Switch && config.ip_routing {
        out += "ip routing\n";
    }
    if router {
        out += "ip cef\nno ipv6 cef\n";
    } else {
        let _ = writeln!(
            out,
            "spanning-tree mode {}\nspanning-tree extend system-id",
            config.spanning_tree.mode.ios()
        );
        for (vlan, priority) in &config.spanning_tree.priorities {
            let _ = writeln!(out, "spanning-tree vlan {vlan} priority {priority}");
        }
    }
    if !config.cdp_run {
        out += "no cdp run\n";
    }
    if config.lldp_run {
        out += "lldp run\n";
    }
    out += "!\n";
    // The VLAN database, as a switch in VTP transparent mode keeps it in its configuration.
    for (id, vlan) in &config.vlans {
        let _ = writeln!(out, "vlan {id}");
        if let Some(name) = &vlan.name {
            let _ = writeln!(out, " name {name}");
        }
        out += "!\n";
    }
    for name in interface_names(device) {
        let iface = config.interface(kind, &name);
        let _ = writeln!(out, "interface {name}");
        if let Some(d) = &iface.description {
            let _ = writeln!(out, " description {d}");
        }
        if let Some(vlan) = iface.encapsulation {
            let native = if iface.native { " native" } else { "" };
            let _ = writeln!(out, " encapsulation dot1Q {vlan}{native}");
        }
        let sp = &iface.switchport;
        let switch_port = kind.is_switch() && kind.has_port(&name);
        if switch_port && sp.routed {
            out += " no switchport\n";
        } else if switch_port {
            out += &switchport_lines(sp);
        }
        let layer3 = router
            || name.starts_with("Vlan")
            || name.starts_with("Loopback")
            || (switch_port && sp.routed);
        if layer3 {
            match iface.address {
                Some(a) => {
                    let _ = writeln!(out, " ip address {} {}", a.address, a.mask());
                }
                None => out += " no ip address\n",
            }
        }
        for helper in &iface.helper_addresses {
            let _ = writeln!(out, " ip helper-address {helper}");
        }
        if let Some(acl) = &iface.acl_in {
            let _ = writeln!(out, " ip access-group {acl} in");
        }
        if let Some(acl) = &iface.acl_out {
            let _ = writeln!(out, " ip access-group {acl} out");
        }
        if let Some(side) = iface.nat {
            let _ = writeln!(out, " ip nat {}", side.ios());
        }
        let ospf = &iface.ospf;
        if let Some((pid, area)) = ospf.area {
            let _ = writeln!(out, " ip ospf {pid} area {area}");
        }
        if let Some(cost) = ospf.cost {
            let _ = writeln!(out, " ip ospf cost {cost}");
        }
        if let Some(priority) = ospf.priority {
            let _ = writeln!(out, " ip ospf priority {priority}");
        }
        if let Some(hello) = ospf.hello {
            let _ = writeln!(out, " ip ospf hello-interval {hello}");
        }
        if let Some(dead) = ospf.dead {
            let _ = writeln!(out, " ip ospf dead-interval {dead}");
        }
        if ospf.point_to_point {
            out += " ip ospf network point-to-point\n";
        }
        if router && kind.has_port(&name) {
            out += " duplex auto\n speed auto\n";
        }
        if iface.shutdown {
            out += " shutdown\n";
        }
        out += "!\n";
    }
    for p in &config.ospf {
        let _ = writeln!(out, "router ospf {}", p.process_id);
        if let Some(rid) = p.router_id {
            let _ = writeln!(out, " router-id {rid}");
        }
        if p.reference_bandwidth != 100 {
            let _ = writeln!(
                out,
                " auto-cost reference-bandwidth {}",
                p.reference_bandwidth
            );
        }
        if p.passive_default {
            out += " passive-interface default\n";
            for a in &p.active {
                let _ = writeln!(out, " no passive-interface {a}");
            }
        }
        for name in &p.passive {
            let _ = writeln!(out, " passive-interface {name}");
        }
        for n in &p.networks {
            let _ = writeln!(out, " network {} {} area {}", n.address, n.wildcard, n.area);
        }
        if p.default_originate {
            out += " default-information originate\n";
        }
        out += "!\n";
    }
    if let Some(gw) = config.default_gateway {
        let _ = writeln!(out, "ip default-gateway {gw}");
    }
    if router {
        out += "ip forward-protocol nd\n!\nno ip http server\n!\n";
    } else {
        out += "ip http server\nip http secure-server\n!\n";
    }
    for pool in &config.nat_pools {
        let _ = writeln!(out, "{}", pool.config_line());
    }
    for rule in &config.nat_rules {
        let _ = writeln!(out, "{}", rule.config_line());
    }
    for route in &config.routes {
        let target = match &route.target {
            RouteTarget::NextHop(nh) => nh.to_string(),
            RouteTarget::Interface(name) => name.clone(),
        };
        let distance = if route.distance != 1 {
            format!(" {}", route.distance)
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "ip route {} {} {target}{distance}",
            route.network,
            route.mask()
        );
    }
    if !config.routes.is_empty() {
        out += "!\n";
    }
    for acl in &config.access_lists {
        if acl.is_numbered() {
            for remark in &acl.remarks {
                let _ = writeln!(out, "access-list {} remark {remark}", acl.name);
            }
            for e in &acl.entries {
                let _ = writeln!(out, "access-list {} {}", acl.name, e.config_line(acl.kind));
            }
        } else {
            let kind = match acl.kind {
                netrace_doc::AclKind::Standard => "standard",
                netrace_doc::AclKind::Extended => "extended",
            };
            let _ = writeln!(out, "ip access-list {kind} {}", acl.name);
            for remark in &acl.remarks {
                let _ = writeln!(out, " remark {remark}");
            }
            for e in &acl.entries {
                let _ = writeln!(out, " {}", e.config_line(acl.kind));
            }
        }
    }
    if !config.access_lists.is_empty() {
        out += "!\n";
    }
    if let Some(banner) = &config.banner_motd {
        let _ = writeln!(out, "banner motd ^C{banner}^C");
        out += "!\n";
    }
    for (key, label) in [
        ("con 0", "line con 0"),
        ("aux 0", "line aux 0"),
        ("vty 0 4", "line vty 0 4"),
    ] {
        if !router && key == "aux 0" {
            continue;
        }
        let _ = writeln!(out, "{label}");
        if let Some(line) = config.lines.get(key) {
            if let Some((m, s)) = line.exec_timeout {
                let _ = writeln!(out, " exec-timeout {m} {s}");
            }
            if let Some(p) = &line.password {
                let _ = writeln!(out, " {}", line_password(config, p));
            }
            if line.logging_synchronous {
                out += " logging synchronous\n";
            }
            if line.login {
                out += " login\n";
            }
        } else if key.starts_with("vty") {
            out += " login\n";
        }
    }
    out += "!\nend\n";
    out
}

/// A switch port's settings, in the order IOS writes them.
fn switchport_lines(sp: &netrace_doc::SwitchportConfig) -> String {
    use netrace_doc::{PortMode, Violation};
    let mut out = String::new();
    if sp.native_vlan != 1 {
        let _ = writeln!(out, " switchport trunk native vlan {}", sp.native_vlan);
    }
    if !sp.allowed_vlans.is_all() {
        let _ = writeln!(
            out,
            " switchport trunk allowed vlan {}",
            sp.allowed_vlans.to_ios()
        );
    }
    if sp.access_vlan != 1 {
        let _ = writeln!(out, " switchport access vlan {}", sp.access_vlan);
    }
    match sp.mode {
        PortMode::Access => out += " switchport mode access\n",
        PortMode::Trunk => out += " switchport mode trunk\n",
        PortMode::DynamicAuto => {}
    }
    if sp.nonegotiate {
        out += " switchport nonegotiate\n";
    }
    if let Some(ps) = &sp.port_security {
        if ps.maximum != 1 {
            let _ = writeln!(out, " switchport port-security maximum {}", ps.maximum);
        }
        if ps.violation != Violation::Shutdown {
            let _ = writeln!(
                out,
                " switchport port-security violation {}",
                ps.violation.ios()
            );
        }
        if ps.sticky {
            out += " switchport port-security mac-address sticky\n";
        }
        for mac in &ps.static_macs {
            let _ = writeln!(out, " switchport port-security mac-address {mac}");
        }
        if ps.enabled {
            out += " switchport port-security\n";
        }
    }
    if sp.portfast {
        out += " spanning-tree portfast\n";
    }
    if sp.bpdu_guard {
        out += " spanning-tree bpduguard enable\n";
    }
    if sp.cdp_disabled {
        out += " no cdp enable\n";
    }
    out
}

pub(crate) fn running_config(device: &Device) -> String {
    let body = config_text(device, &device.name, &device.config);
    format!(
        "Building configuration...\n\nCurrent configuration : {} bytes\n{body}",
        body.len()
    )
}

pub(crate) fn startup_config(device: &Device) -> String {
    match &device.startup {
        Some(saved) => {
            let body = config_text(device, &saved.hostname, &saved.config);
            format!("Using {} out of 262144 bytes\n{body}", body.len())
        }
        None => "startup-config is not present".into(),
    }
}

pub(crate) fn ip_interface_brief(rows: &[InterfaceRow]) -> String {
    let mut out = format!(
        "{:<23}{:<16}{:<4}{:<7}{:<22}{}\n",
        "Interface", "IP-Address", "OK?", "Method", "Status", "Protocol"
    );
    for row in rows {
        let (address, method) = match row.address {
            Some(a) => (a.to_string(), "manual"),
            None => ("unassigned".to_string(), "unset"),
        };
        let _ = writeln!(
            out,
            "{:<23}{:<16}{:<4}{:<7}{:<22}{}",
            row.name, address, "YES", method, row.status, row.protocol
        );
    }
    out
}

fn classful_prefix(ip: Ipv4Addr) -> u8 {
    match ip.octets()[0] {
        0..=127 => 8,
        128..=191 => 16,
        _ => 24,
    }
}

fn route_line(route: &Route, indent: bool) -> String {
    let star = if route.is_default() { "*" } else { "" };
    let code = match route.source {
        RouteSource::OspfInterArea => format!("O{star} IA"),
        RouteSource::OspfExternal2 => format!("O{star}E2"),
        source => format!("{}{star}", source.code()),
    };
    let pad = if indent { 9 } else { 6 };
    let target = match (route.source, route.next_hop) {
        (RouteSource::Connected | RouteSource::Local, _) | (_, None) => {
            format!("is directly connected, {}", route.iface)
        }
        (RouteSource::Ospf | RouteSource::OspfInterArea | RouteSource::OspfExternal2, Some(nh)) => {
            format!(
                "[{}/{}] via {nh}, 00:00:12, {}",
                route.distance, route.metric, route.iface
            )
        }
        (_, Some(nh)) => format!("[{}/{}] via {nh}", route.distance, route.metric),
    };
    format!(
        "{code:<pad$}{}/{} {target}",
        route.network, route.prefix_len
    )
}

pub(crate) fn ip_route(kind: DeviceKind, config: &DeviceConfig, routes: &[Route]) -> String {
    if kind.is_switch() && !(kind == DeviceKind::L3Switch && config.ip_routing) {
        return match config.default_gateway {
            Some(gw) => format!(
                "Default gateway is {gw}\n\nHost               Gateway           Last Use    Total Uses  Interface\n\
                 ICMP redirect cache is empty\n"
            ),
            None => "Default gateway is not set\n\nHost               Gateway           Last Use    Total Uses  Interface\n\
                     ICMP redirect cache is empty\n"
                .into(),
        };
    }
    let mut out = String::from(
        "Codes: L - local, C - connected, S - static, R - RIP, M - mobile, B - BGP\n\
         \x20      D - EIGRP, EX - EIGRP external, O - OSPF, IA - OSPF inter area\n\
         \x20      N1 - OSPF NSSA external type 1, N2 - OSPF NSSA external type 2\n\
         \x20      E1 - OSPF external type 1, E2 - OSPF external type 2\n\
         \x20      i - IS-IS, su - IS-IS summary, L1 - IS-IS level-1, L2 - IS-IS level-2\n\
         \x20      ia - IS-IS inter area, * - candidate default, U - per-user static route\n\
         \x20      o - ODR, P - periodic downloaded static route, H - NHRP, l - LISP\n\
         \x20      + - replicated route, % - next hop override\n\n",
    );
    match routes.iter().find(|r| r.is_default()) {
        Some(Route {
            next_hop: Some(nh), ..
        }) => {
            let _ = writeln!(out, "Gateway of last resort is {nh} to network 0.0.0.0\n");
        }
        Some(r) => {
            let _ = writeln!(
                out,
                "Gateway of last resort is 0.0.0.0 to network 0.0.0.0 via {}\n",
                r.iface
            );
        }
        None => out += "Gateway of last resort is not set\n\n",
    }
    // Defaults first, then grouped by classful network as IOS does.
    let defaults: Vec<&Route> = routes.iter().filter(|r| r.is_default()).collect();
    out += &route_lines(&defaults, false);
    let mut groups: Vec<(Ipv4Addr, u8, Vec<&Route>)> = Vec::new();
    for r in routes.iter().filter(|r| !r.is_default()) {
        let len = classful_prefix(r.network);
        let major = Ipv4Addr::from(u32::from(r.network) & u32::from(mask(len)));
        match groups.iter_mut().find(|(m, l, _)| *m == major && *l == len) {
            Some((_, _, members)) => members.push(r),
            None => groups.push((major, len, vec![r])),
        }
    }
    for (major, len, members) in groups {
        let mut prefixes: Vec<(Ipv4Addr, u8)> =
            members.iter().map(|r| (r.network, r.prefix_len)).collect();
        prefixes.dedup();
        if prefixes.len() == 1 && members[0].prefix_len == len {
            out += &route_lines(&members, false);
            continue;
        }
        let mut masks: Vec<u8> = members.iter().map(|r| r.prefix_len).collect();
        masks.sort_unstable();
        masks.dedup();
        let subnets = prefixes.len();
        let plural = if subnets == 1 { "" } else { "s" };
        if masks.len() == 1 {
            let _ = writeln!(
                out,
                "      {major}/{len} is subnetted, {subnets} subnet{plural}"
            );
        } else {
            let _ = writeln!(
                out,
                "      {major}/{len} is variably subnetted, {subnets} subnet{plural}, {} masks",
                masks.len()
            );
        }
        out += &route_lines(&members, true);
    }
    out
}

/// Route lines; a second equal-cost path to the same prefix continues on
/// the next line under the first, as IOS prints it.
fn route_lines(routes: &[&Route], indent: bool) -> String {
    let mut out = String::new();
    let mut previous: Option<&Route> = None;
    for r in routes {
        let line = route_line(r, indent);
        match previous {
            Some(p)
                if (p.network, p.prefix_len, p.source) == (r.network, r.prefix_len, r.source) =>
            {
                let at = line.find('[').unwrap_or(0);
                let _ = writeln!(out, "{}{}", " ".repeat(at), &line[at..]);
            }
            _ => {
                let _ = writeln!(out, "{line}");
            }
        }
        previous = Some(r);
    }
    out
}

pub(crate) fn interfaces(rows: &[InterfaceRow], kind: DeviceKind) -> String {
    let mut out = String::new();
    for row in rows {
        let status = if row.status == "administratively down" {
            "administratively down".to_string()
        } else {
            row.status.to_string()
        };
        let _ = writeln!(
            out,
            "{} is {status}, line protocol is {}",
            row.name, row.protocol
        );
        let hardware = if row.name.starts_with("Loopback") {
            "Loopback".to_string()
        } else if row.name.starts_with("Vlan") {
            "EtherSVI".to_string()
        } else if row.name.starts_with("Fast") {
            "Fast Ethernet".to_string()
        } else {
            "Gigabit Ethernet".to_string()
        };
        match row.mac {
            Some(mac) => {
                let _ = writeln!(
                    out,
                    "  Hardware is {hardware}, address is {} (bia {})",
                    cisco_mac(mac),
                    cisco_mac(mac)
                );
            }
            None => {
                let _ = writeln!(out, "  Hardware is {hardware}");
            }
        }
        if let Some(d) = &row.description {
            let _ = writeln!(out, "  Description: {d}");
        }
        if let (Some(a), Some(p)) = (row.address, row.prefix_len) {
            let _ = writeln!(out, "  Internet address is {a}/{p}");
        }
        let bandwidth = if row.name.starts_with("Fast") {
            100_000
        } else {
            1_000_000
        };
        let _ = writeln!(
            out,
            "  MTU 1500 bytes, BW {bandwidth} Kbit/sec, DLY 10 usec,\n     reliability 255/255, txload 1/255, rxload 1/255\n  \
             Encapsulation ARPA, loopback not set"
        );
        if kind.has_port(&row.name) {
            let speed = if row.name.starts_with("Fast") {
                "100Mb/s"
            } else {
                "1000Mb/s"
            };
            let _ = writeln!(out, "  Full-duplex, {speed}, media type is RJ45");
        }
    }
    out
}

pub(crate) fn arp(network: &Network, device: netrace_doc::DeviceId) -> String {
    let mut out =
        String::from("Protocol  Address          Age (min)  Hardware Addr   Type   Interface\n");
    for row in network.arp_rows(device) {
        let age = row
            .age_minutes
            .map(|a| a.to_string())
            .unwrap_or_else(|| "-".into());
        let _ = writeln!(
            out,
            "Internet  {:<17}{:>9}   {:<16}ARPA   {}",
            row.address.to_string(),
            age,
            cisco_mac(row.mac),
            row.iface
        );
    }
    out
}

pub(crate) fn mac_table(network: &Network, device: netrace_doc::DeviceId) -> String {
    let rows = network.mac_rows(device);
    let mut out = String::from(
        "          Mac Address Table\n-------------------------------------------\n\n\
         Vlan    Mac Address       Type        Ports\n----    -----------       --------    -----\n",
    );
    for row in &rows {
        let _ = writeln!(
            out,
            "{:>4}    {:<18}DYNAMIC     {}",
            row.vlan,
            cisco_mac(row.mac),
            abbreviate_port(&row.port)
        );
    }
    let _ = writeln!(
        out,
        "Total Mac Addresses for this criterion: {}",
        rows.len()
    );
    out
}

pub(crate) fn version(device: &Device) -> String {
    let (platform, ports) = match device.kind {
        DeviceKind::Router => ("ISR-style router", "4 Gigabit Ethernet interfaces"),
        DeviceKind::L3Switch => ("multilayer switch", "24 Gigabit Ethernet interfaces"),
        _ => (
            "access switch",
            "24 FastEthernet and 2 Gigabit Ethernet interfaces",
        ),
    };
    format!(
        "netrace IOS-style software, Version 15.4 (simulated)\n\
         Technical support: this is a simulator; behaviour follows Cisco IOS for CCNA topics.\n\n\
         {} uptime is 0 minutes\nSystem image file is \"flash:netrace-universal.bin\"\n\n\
         netrace {platform} with 1048576K bytes of memory.\n{ports}\n\
         262144K bytes of non-volatile configuration memory.\n\n\
         Configuration register is 0x2102\n",
        device.name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type7_matches_known_values() {
        // "cisco" with seed 02 is a well-known example.
        assert_eq!(type7("cisco"), "02050D480809");
        assert!(secret_hash("class").starts_with("$1$"));
        assert_ne!(secret_hash("class"), secret_hash("cisco"));
    }

    #[test]
    fn macs_in_cisco_style() {
        assert_eq!(
            cisco_mac(MacAddr([0x02, 0x4e, 0, 0, 1, 0])),
            "024e.0000.0100"
        );
    }
}
