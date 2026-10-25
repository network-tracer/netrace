//! Text output for end-device commands (`ping`, `ipconfig`, `arp -a`),
//! formatted like a Windows command prompt.

use std::net::Ipv4Addr;

use netrace_doc::DeviceId;
use netrace_sim::SimTime;

use crate::frame::PING_DATA_BYTES;
use crate::{Network, PingFailure, abbreviate_port};

impl Network {
    /// Runs one command typed at an end device's prompt and returns its output.
    /// `ping` is scheduled at `at` and the network replayed.
    pub fn host_command(&mut self, device: DeviceId, line: &str, at: SimTime) -> String {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            [] => String::new(),
            [cmd] if cmd.eq_ignore_ascii_case("ipconfig") => self.ipconfig(device),
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
                    Ok(session) => self.ping_report(session).unwrap_or_default(),
                    Err(e) => e.to_string(),
                }
            }
            [cmd] if cmd.eq_ignore_ascii_case("help") || *cmd == "?" => "Commands:\n  \
                 ping [-n count] <address>   Send echo requests and show the replies\n  \
                 ipconfig                    Show this device's IP configuration\n  \
                 arp -a                      Show the ARP cache (IP-to-MAC entries)\n\
                 The full IOS command line for routers and switches arrives in milestone M3."
                .into(),
            [cmd, ..] => format!(
                "'{cmd}' is not recognized as a command.\nType help to see what this device understands."
            ),
        }
    }

    /// Windows-style output for a ping session.
    pub fn ping_report(&self, session: u64) -> Option<String> {
        let s = self.session(session)?;
        let mut out = format!(
            "Pinging {} with {PING_DATA_BYTES} bytes of data:\n\n",
            s.destination
        );
        let mut times = Vec::new();
        for seq in 1..=s.count {
            if let Some((rtt, ttl)) = s.replies.get(&seq) {
                let ms = rtt.as_millis();
                times.push(ms);
                let time = if ms == 0 {
                    "time<1ms".to_string()
                } else {
                    format!("time={ms}ms")
                };
                out += &format!(
                    "Reply from {}: bytes={PING_DATA_BYTES} {time} TTL={ttl}\n",
                    s.destination
                );
            } else {
                match s.failures.get(&seq) {
                    Some(PingFailure::TimedOut) => out += "Request timed out.\n",
                    Some(PingFailure::Unreachable(reason)) if reason == "no ARP reply" => {
                        out += "Destination host unreachable.\n"
                    }
                    Some(PingFailure::Unreachable(reason)) => out += &format!("{reason}\n"),
                    None => out += "Request timed out.\n",
                }
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
        Some(out)
    }

    pub fn ipconfig(&self, device: DeviceId) -> String {
        let Some(host) = self.host_view(device) else {
            return format!("{} has no IP configuration of its own.", self.name(device));
        };
        let port = host
            .port
            .map(abbreviate_port)
            .unwrap_or_else(|| "Ethernet".into());
        let mut out = format!(
            "{port} Connection:\n\n   Physical Address................: {}\n",
            host.mac
        );
        match host.ip {
            Some(ip) => {
                out += &format!(
                    "   IPv4 Address....................: {}\n   Subnet Mask.....................: {}\n   \
                     Default Gateway.................: {}\n",
                    ip.address,
                    ip.mask(),
                    ip.gateway
                        .map(|g| g.to_string())
                        .unwrap_or_else(|| "0.0.0.0".into())
                );
            }
            None => out += "   IPv4 Address....................: not configured\n",
        }
        out
    }

    /// The ARP cache as it stands after everything run so far.
    pub fn arp_table(&self, device: DeviceId) -> String {
        let Some(host) = self.host_view(device) else {
            return format!("{} has no ARP cache of its own.", self.name(device));
        };
        if host.arp.is_empty() {
            return "No ARP Entries Found.".into();
        }
        let mut out = format!(
            "Interface: {}\n  Internet Address      Physical Address      Type\n",
            host.ip.map(|i| i.address.to_string()).unwrap_or_default()
        );
        for (addr, mac) in host.arp {
            let dashed = mac.to_string().replace(':', "-");
            out += &format!("  {:<22}{:<22}dynamic\n", addr.to_string(), dashed);
        }
        out
    }
}
