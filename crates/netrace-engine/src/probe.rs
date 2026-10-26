//! Pings and traceroutes: the probes a session sends and how each ended.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::time::Duration;

use netrace_doc::DeviceId;
use netrace_sim::SimTime;

use crate::Unreachable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Ping,
    Traceroute,
}

/// Whose output format a session's report uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportStyle {
    /// An end device's command prompt (`ping`, `tracert`).
    Windows,
    /// A router's or switch's command line.
    Ios,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PingFailure {
    TimedOut,
    Unreachable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeResult {
    /// Echo reply from the destination.
    Reply {
        rtt: Duration,
        ttl: u8,
        from: Ipv4Addr,
    },
    /// A router on the way discarded the probe when its TTL ran out.
    TimeExceeded {
        rtt: Duration,
        from: Ipv4Addr,
    },
    /// A router (or the source itself) had no way to deliver it.
    Unreachable {
        rtt: Duration,
        from: Ipv4Addr,
        code: Unreachable,
    },
    TimedOut,
    /// Never left the source: the reason, in plain words.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub seq: u16,
    pub ttl: u8,
    pub sent: Option<SimTime>,
    pub result: Option<ProbeResult>,
    /// Had to wait for ARP before leaving the source.
    pub waited_for_arp: bool,
}

/// One `ping` or `traceroute` and how each of its probes ended.
#[derive(Debug, Clone, PartialEq)]
pub struct PingSession {
    pub id: u64,
    pub source: DeviceId,
    pub destination: Ipv4Addr,
    pub kind: SessionKind,
    pub style: ReportStyle,
    /// Probes a ping sends (traceroutes stop on their own).
    pub count: u16,
    pub started: SimTime,
    pub probes: Vec<Probe>,
    /// Sequence number → (round-trip time, TTL of the reply).
    pub replies: BTreeMap<u16, (Duration, u8)>,
    pub failures: BTreeMap<u16, PingFailure>,
    pub(crate) done: bool,
}

/// Probes per hop in a traceroute, as both Windows and IOS send.
pub const PROBES_PER_HOP: u16 = 3;
pub const MAX_HOPS: u8 = 30;

impl PingSession {
    pub(crate) fn new(
        id: u64,
        source: DeviceId,
        destination: Ipv4Addr,
        kind: SessionKind,
        style: ReportStyle,
        count: u16,
        started: SimTime,
    ) -> Self {
        PingSession {
            id,
            source,
            destination,
            kind,
            style,
            count,
            started,
            probes: Vec::new(),
            replies: BTreeMap::new(),
            failures: BTreeMap::new(),
            done: false,
        }
    }

    pub fn is_finished(&self) -> bool {
        match self.kind {
            SessionKind::Ping => {
                self.replies.len() + self.failures.len() >= usize::from(self.count)
            }
            SessionKind::Traceroute => self.done,
        }
    }

    pub fn probe(&self, seq: u16) -> Option<&Probe> {
        self.probes.iter().find(|p| p.seq == seq)
    }

    pub(crate) fn probe_mut(&mut self, seq: u16) -> Option<&mut Probe> {
        self.probes.iter_mut().find(|p| p.seq == seq)
    }

    pub(crate) fn settled(&self, seq: u16) -> bool {
        self.probe(seq).is_some_and(|p| p.result.is_some())
    }

    /// Windows pings go out on a fixed one-second rhythm; IOS pings and all
    /// traceroutes send the next probe as soon as the last one is settled.
    pub(crate) fn is_sequential(&self) -> bool {
        self.kind == SessionKind::Traceroute || self.style == ReportStyle::Ios
    }

    /// TTL of a probe: the device's normal TTL for pings, the hop number
    /// for traceroutes.
    pub(crate) fn ttl_for(&self, seq: u16, normal: u8) -> u8 {
        match self.kind {
            SessionKind::Ping => normal,
            SessionKind::Traceroute => (((seq - 1) / PROBES_PER_HOP) + 1).min(255) as u8,
        }
    }

    /// Records a result; returns false if the probe had already settled.
    pub(crate) fn settle(&mut self, seq: u16, result: ProbeResult) -> bool {
        let Some(probe) = self.probe_mut(seq) else {
            return false;
        };
        if probe.result.is_some() {
            return false;
        }
        probe.result = Some(result.clone());
        match result {
            ProbeResult::Reply { rtt, ttl, .. } => {
                self.replies.insert(seq, (rtt, ttl));
            }
            ProbeResult::TimedOut => {
                self.failures.insert(seq, PingFailure::TimedOut);
            }
            ProbeResult::Failed(reason) => {
                self.failures.insert(seq, PingFailure::Unreachable(reason));
            }
            ProbeResult::Unreachable { from, code, .. } => {
                let what = match code {
                    Unreachable::Network => "net",
                    Unreachable::Host => "host",
                    Unreachable::Prohibited => "prohibited",
                };
                self.failures.insert(
                    seq,
                    PingFailure::Unreachable(format!(
                        "Reply from {from}: Destination {what} unreachable."
                    )),
                );
            }
            ProbeResult::TimeExceeded { from, .. } => {
                if self.kind == SessionKind::Ping {
                    self.failures.insert(
                        seq,
                        PingFailure::Unreachable(format!(
                            "Reply from {from}: TTL expired in transit."
                        )),
                    );
                }
            }
        }
        true
    }

    /// The next probe to send after `seq` settled, or `None` when done.
    pub(crate) fn next_after(&mut self, seq: u16) -> Option<u16> {
        match self.kind {
            SessionKind::Ping => (seq < self.count).then_some(seq + 1),
            SessionKind::Traceroute => {
                if !seq.is_multiple_of(PROBES_PER_HOP) {
                    return Some(seq + 1);
                }
                let hop = seq / PROBES_PER_HOP;
                let first = seq + 1 - PROBES_PER_HOP;
                let reached = (first..=seq).any(|s| {
                    matches!(
                        self.probe(s).and_then(|p| p.result.as_ref()),
                        Some(
                            ProbeResult::Reply { .. }
                                | ProbeResult::Unreachable { .. }
                                | ProbeResult::Failed(_)
                        )
                    )
                });
                if reached || hop >= u16::from(MAX_HOPS) {
                    self.done = true;
                    None
                } else {
                    Some(seq + 1)
                }
            }
        }
    }
}
