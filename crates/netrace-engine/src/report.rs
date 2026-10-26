//! Text output: end-device commands (`ping`, `tracert`, `ipconfig`,
//! `arp -a`) formatted like a Windows command prompt, and ping / traceroute
//! reports in Windows or IOS style.

use std::net::Ipv4Addr;
use std::time::Duration;

use netrace_doc::DeviceId;
use netrace_sim::SimTime;

use crate::frame::PING_DATA_BYTES;
use crate::probe::{MAX_HOPS, PROBES_PER_HOP, PingSession, ProbeResult, ReportStyle, SessionKind};
use crate::{Network, Unreachable, abbreviate_port};

/// Bytes in an IOS ping, as `ping` on a router reports.
const IOS_PING_BYTES: usize = 100;

impl Network {
    /// Runs one command typed at an end device's prompt and returns its output.
    /// `ping` and `tracert` are scheduled at `at` and the network replayed.
    pub fn host_command(&mut self, device: DeviceId, line: &str, at: SimTime) -> String {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            [] => String::new(),
            [cmd] if cmd.eq_ignore_ascii_case("ipconfig") => self.ipconfig(device),
            [cmd, flag]
                if cmd.eq_ignore_ascii_case("ipconfig") && flag.eq_ignore_ascii_case("/renew") =>
            {
                if !self.project.device(device).is_some_and(|d| d.dhcp) {
                    return "An error occurred while renewing interface FastEthernet0: DHCP is not enabled on this \
                            adapter. Switch its IP configuration to DHCP first."
                        .into();
                }
                self.renew(device, at);
                self.ipconfig(device)
            }
            [cmd, flag] if cmd.eq_ignore_ascii_case("arp") && flag.eq_ignore_ascii_case("-a") => {
                self.arp_table(device)
            }
            [cmd] if cmd.eq_ignore_ascii_case("arp") => self.arp_table(device),
            [cmd, rest @ ..] if cmd.eq_ignore_ascii_case("ping") => {
                let (count, target) = match rest {
                    [flag, n, target] if flag.eq_ignore_ascii_case("-n") => {
                        (n.parse().unwrap_or(4), *target)
                    }
                    [target] => (4, *target),
                    _ => return "Usage: ping [-n count] <IPv4 address>".into(),
                };
                let Ok(destination) = target.parse::<Ipv4Addr>() else {
                    return format!(
                        "Ping request could not find host {target}. Please check the name and try again."
                    );
                };
                match self.ping(device, destination, count, at) {
                    Ok(session) => self.session_report(session).unwrap_or_default(),
                    Err(e) => e.to_string(),
                }
            }
            [cmd, target] if cmd.eq_ignore_ascii_case("tracert") => {
                let Ok(destination) = target.parse::<Ipv4Addr>() else {
                    return format!("Unable to resolve target system name {target}.");
                };
                match self.traceroute(device, destination, at) {
                    Ok(session) => self.session_report(session).unwrap_or_default(),
                    Err(e) => e.to_string(),
                }
            }
            [cmd, target] if cmd.eq_ignore_ascii_case("curl") => {
                // curl [http://]ADDRESS[:PORT][/path]
                let rest = target
                    .strip_prefix("http://")
                    .or_else(|| target.strip_prefix("HTTP://"))
                    .unwrap_or(target);
                let host_port = rest.split('/').next().unwrap_or(rest);
                let (host, port) = match host_port.split_once(':') {
                    Some((h, p)) => (h, p.parse().unwrap_or(80)),
                    None => (host_port, 80),
                };
                let Ok(destination) = host.parse::<Ipv4Addr>() else {
                    return format!("curl: (6) Could not resolve host: {host}");
                };
                match self.connect(device, destination, port, crate::TcpApp::Http, at) {
                    Ok(conn) => self.connection_report(conn).unwrap_or_default(),
                    Err(e) => e.to_string(),
                }
            }
            [cmd, target, rest @ ..] if cmd.eq_ignore_ascii_case("telnet") && rest.len() <= 1 => {
                let Ok(destination) = target.parse::<Ipv4Addr>() else {
                    return format!(
                        "Connecting To {target}...Could not open connection to the host"
                    );
                };
                let port = rest.first().and_then(|p| p.parse().ok()).unwrap_or(23);
                match self.connect(device, destination, port, crate::TcpApp::Connect, at) {
                    Ok(conn) => self.connection_report(conn).unwrap_or_default(),
                    Err(e) => e.to_string(),
                }
            }
            [cmd] if cmd.eq_ignore_ascii_case("help") || *cmd == "?" => "Commands:\n  \
                 ping [-n count] <address>   Send echo requests and show the replies\n  \
                 tracert <address>           List the routers on the way to an address\n  \
                 ipconfig                    Show this device's IP configuration\n  \
                 ipconfig /renew             Ask DHCP for an address again\n  \
                 arp -a                      Show the ARP cache (IP-to-MAC entries)\n  \
                 curl <address>[:port]       Fetch a web page over TCP (port 80)\n  \
                 telnet <address> [port]     Open a TCP connection (port 23)"
                .into(),
            [cmd, ..] => format!(
                "'{cmd}' is not recognized as a command.\nType help to see what this device understands."
            ),
        }
    }

    /// A session's output, in the style of the device that ran it.
    pub fn session_report(&self, session: u64) -> Option<String> {
        let s = self.session(session)?;
        Some(match (s.kind, s.style) {
            (SessionKind::Ping, ReportStyle::Windows) => windows_ping(s),
            (SessionKind::Ping, ReportStyle::Ios) => ios_ping(s),
            (SessionKind::Traceroute, ReportStyle::Windows) => windows_tracert(s),
            (SessionKind::Traceroute, ReportStyle::Ios) => ios_traceroute(s),
        })
    }

    /// Kept for callers that only know about pings.
    pub fn ping_report(&self, session: u64) -> Option<String> {
        self.session_report(session)
    }

    pub fn ipconfig(&self, device: DeviceId) -> String {
        let Some(iface) = self.stack(device).and_then(|s| s.ifaces.first()) else {
            return format!("{} has no IP configuration of its own.", self.name(device));
        };
        let dhcp = self.project.device(device).is_some_and(|d| d.dhcp);
        let mut out = format!(
            "{} Connection:\n\n   Physical Address................: {}\n   DHCP Enabled....................: {}\n",
            abbreviate_port(&iface.name),
            iface.mac,
            if dhcp { "Yes" } else { "No" }
        );
        match self.current_address(device) {
            Some((address, prefix, gateway)) => {
                let label = if address.octets()[..2] == [169, 254] {
                    "Autoconfiguration IPv4 Address.."
                } else {
                    "IPv4 Address...................."
                };
                out += &format!(
                    "   {label}: {address}\n   Subnet Mask.....................: {}\n   \
                     Default Gateway.................: {}\n",
                    netrace_doc::mask(prefix),
                    gateway
                        .map(|g| g.to_string())
                        .unwrap_or_else(|| "0.0.0.0".into())
                );
                if let Some(lease) = self.held_lease(device) {
                    out += &format!("   DHCP Server.....................: {}\n", lease.server);
                    if let Some(dns) = lease.dns {
                        out += &format!("   DNS Servers.....................: {dns}\n");
                    }
                }
            }
            None if dhcp => out += "   IPv4 Address....................: (waiting for DHCP)\n",
            None => out += "   IPv4 Address....................: not configured\n",
        }
        out
    }

    /// The ARP cache as it stands after everything run so far.
    pub fn arp_table(&self, device: DeviceId) -> String {
        let Some(stack) = self.stack(device) else {
            return format!("{} has no ARP cache of its own.", self.name(device));
        };
        if stack.arp.is_empty() {
            return "No ARP Entries Found.".into();
        }
        let own = stack
            .ifaces
            .first()
            .and_then(|i| i.address)
            .map(|a| a.address.to_string())
            .unwrap_or_default();
        let mut out =
            format!("Interface: {own}\n  Internet Address      Physical Address      Type\n");
        for (addr, entry) in &stack.arp {
            let dashed = entry.mac.to_string().replace(':', "-");
            out += &format!("  {:<22}{:<22}dynamic\n", addr.to_string(), dashed);
        }
        out
    }
}

fn ms(rtt: Duration) -> u128 {
    rtt.as_millis()
}

fn windows_ping(s: &PingSession) -> String {
    let mut out = format!(
        "Pinging {} with {PING_DATA_BYTES} bytes of data:\n\n",
        s.destination
    );
    let mut times = Vec::new();
    for seq in 1..=s.count {
        match s.probe(seq).and_then(|p| p.result.as_ref()) {
            Some(ProbeResult::Reply { rtt, ttl, from }) => {
                let t = ms(*rtt);
                times.push(t);
                let time = if t == 0 {
                    "time<1ms".to_string()
                } else {
                    format!("time={t}ms")
                };
                out += &format!("Reply from {from}: bytes={PING_DATA_BYTES} {time} TTL={ttl}\n");
            }
            Some(ProbeResult::TimeExceeded { from, .. }) => {
                out += &format!("Reply from {from}: TTL expired in transit.\n")
            }
            Some(ProbeResult::Unreachable { from, code, .. }) => {
                let what = match code {
                    Unreachable::Network => "net",
                    Unreachable::Host => "host",
                    Unreachable::Prohibited => "prohibited",
                };
                if what == "prohibited" {
                    out +=
                        &format!("Reply from {from}: Communication administratively prohibited.\n");
                } else {
                    out += &format!("Reply from {from}: Destination {what} unreachable.\n");
                }
            }
            Some(ProbeResult::Failed(reason)) if reason == "no ARP reply" => {
                out += "Destination host unreachable.\n"
            }
            Some(ProbeResult::Failed(reason)) => out += &format!("{reason}\n"),
            Some(ProbeResult::TimedOut) | None => out += "Request timed out.\n",
        }
    }
    let sent = usize::from(s.count);
    let received = times.len();
    let lost = sent - received;
    out += &format!(
        "\nPing statistics for {}:\n    Packets: Sent = {sent}, Received = {received}, Lost = {lost} ({}% loss),\n",
        s.destination,
        lost * 100 / sent.max(1)
    );
    if received > 0 {
        let min = times.iter().min().copied().unwrap_or(0);
        let max = times.iter().max().copied().unwrap_or(0);
        let avg = times.iter().sum::<u128>() / received as u128;
        out += &format!(
            "Approximate round trip times in milli-seconds:\n    Minimum = {min}ms, Maximum = {max}ms, Average = {avg}ms\n"
        );
    }
    out
}

fn ios_ping(s: &PingSession) -> String {
    let mut out = format!(
        "Type escape sequence to abort.\nSending {}, {IOS_PING_BYTES}-byte ICMP Echos to {}, timeout is 2 seconds:\n",
        s.count, s.destination
    );
    let mut times = Vec::new();
    for seq in 1..=s.count {
        out.push(match s.probe(seq).and_then(|p| p.result.as_ref()) {
            Some(ProbeResult::Reply { rtt, .. }) => {
                times.push(ms(*rtt));
                '!'
            }
            Some(ProbeResult::Unreachable { .. }) => 'U',
            Some(ProbeResult::TimeExceeded { .. }) => '&',
            _ => '.',
        });
    }
    let received = times.len();
    let percent = received * 100 / usize::from(s.count).max(1);
    out += &format!(
        "\nSuccess rate is {percent} percent ({received}/{})",
        s.count
    );
    if received > 0 {
        let min = times.iter().min().copied().unwrap_or(0);
        let max = times.iter().max().copied().unwrap_or(0);
        let avg = times.iter().sum::<u128>() / received as u128;
        out += &format!(", round-trip min/avg/max = {min}/{avg}/{max} ms");
    }
    out.push('\n');
    out
}

/// Probes grouped by hop: (hop, results of its three probes).
fn hops(s: &PingSession) -> Vec<(u16, Vec<Option<&ProbeResult>>)> {
    let last = s.probes.iter().map(|p| p.seq).max().unwrap_or(0);
    let hop_count = last.div_ceil(PROBES_PER_HOP);
    (1..=hop_count)
        .map(|hop| {
            let first = (hop - 1) * PROBES_PER_HOP + 1;
            let results = (first..first + PROBES_PER_HOP)
                .map(|seq| s.probe(seq).and_then(|p| p.result.as_ref()))
                .collect();
            (hop, results)
        })
        .collect()
}

fn responder(results: &[Option<&ProbeResult>]) -> Option<Ipv4Addr> {
    results.iter().find_map(|r| match r {
        Some(ProbeResult::Reply { from, .. })
        | Some(ProbeResult::TimeExceeded { from, .. })
        | Some(ProbeResult::Unreachable { from, .. }) => Some(*from),
        _ => None,
    })
}

fn windows_tracert(s: &PingSession) -> String {
    let mut out = format!(
        "\nTracing route to {} over a maximum of {MAX_HOPS} hops\n\n",
        s.destination
    );
    for (hop, results) in hops(s) {
        if let Some(Some(ProbeResult::Failed(reason))) = results.first() {
            out += &format!("  {reason}\n");
            continue;
        }
        let mut line = format!("{hop:>3}");
        for r in &results {
            line += &match r {
                Some(ProbeResult::Reply { rtt, .. })
                | Some(ProbeResult::TimeExceeded { rtt, .. })
                | Some(ProbeResult::Unreachable { rtt, .. }) => {
                    if ms(*rtt) == 0 {
                        "    <1 ms".to_string()
                    } else {
                        format!("{:>6} ms", ms(*rtt))
                    }
                }
                _ => "        *".to_string(),
            };
        }
        match responder(&results) {
            Some(from) => {
                let unreachable = results
                    .iter()
                    .any(|r| matches!(r, Some(ProbeResult::Unreachable { .. })));
                if unreachable {
                    line += &format!("  {from} reports: Destination net unreachable.");
                } else {
                    line += &format!("  {from}");
                }
            }
            None => line += "     Request timed out.",
        }
        out += &line;
        out.push('\n');
    }
    if s.is_finished() {
        out += "\nTrace complete.\n";
    }
    out
}

fn ios_traceroute(s: &PingSession) -> String {
    let mut out = format!(
        "Type escape sequence to abort.\nTracing the route to {}\nVRF info: (vrf in name/id, vrf out name/id)\n",
        s.destination
    );
    for (hop, results) in hops(s) {
        if let Some(Some(ProbeResult::Failed(reason))) = results.first() {
            out += &format!("% {reason}\n");
            continue;
        }
        let mut line = format!("{hop:>3}");
        if let Some(from) = responder(&results) {
            line += &format!(" {from}");
        }
        for r in &results {
            line += &match r {
                Some(ProbeResult::Reply { rtt, .. })
                | Some(ProbeResult::TimeExceeded { rtt, .. }) => {
                    format!(" {} msec", ms(*rtt))
                }
                Some(ProbeResult::Unreachable { .. }) => " !N".to_string(),
                _ => "  *".to_string(),
            };
        }
        out += &line;
        out.push('\n');
    }
    out
}
