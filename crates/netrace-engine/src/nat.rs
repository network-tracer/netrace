//! Network address translation on routers: static NAT, dynamic NAT from a
//! pool, and PAT (overload). Every translation records the addresses
//! before and after, the rule used and whether a table entry was created,
//! so the trace can show the rewrite side by side.

use std::net::Ipv4Addr;

use netrace_doc::{DeviceId, NatRule, NatSide, NatTarget};
use netrace_sim::SimTime;

use crate::acl::AclDirection;
use crate::stack::{Iface, Stack};
use crate::{Ipv4Packet, L4, Network, abbreviate_port};

/// The first port PAT hands out when a host's own port is taken.
const PAT_FIRST_PORT: u16 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NatProto {
    Icmp,
    Udp,
}

impl NatProto {
    pub fn ios(self) -> &'static str {
        match self {
            NatProto::Icmp => "icmp",
            NatProto::Udp => "udp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatKind {
    Static,
    Dynamic,
    /// PAT: one public address shared by many hosts, told apart by port.
    Overload,
}

/// A row in the translation table. Address-only entries (`proto` none) are
/// the `---` rows IOS prints for static and one-to-one dynamic mappings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatEntry {
    pub proto: Option<NatProto>,
    pub local: Ipv4Addr,
    pub local_port: u16,
    pub global: Ipv4Addr,
    pub global_port: u16,
    pub outside: Ipv4Addr,
    pub outside_port: u16,
    pub kind: NatKind,
    /// The rule as the running-config writes it.
    pub rule: String,
    pub created: SimTime,
}

impl NatEntry {
    fn endpoint(&self, ip: Ipv4Addr, port: u16) -> String {
        match self.proto {
            Some(_) => format!("{ip}:{port}"),
            None => ip.to_string(),
        }
    }

    pub fn inside_local(&self) -> String {
        self.endpoint(self.local, self.local_port)
    }

    pub fn inside_global(&self) -> String {
        self.endpoint(self.global, self.global_port)
    }

    /// Outside local and outside global are the same: only inside source
    /// translation is simulated.
    pub fn outside_addr(&self) -> String {
        match self.proto {
            Some(_) => format!("{}:{}", self.outside, self.outside_port),
            None => "---".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatDirection {
    /// Leaving by an outside interface: the source is rewritten.
    InsideToOutside,
    /// Arriving on an outside interface: the destination is rewritten back.
    OutsideToInside,
}

/// What NAT did to one packet at one router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatDecision {
    pub device: DeviceId,
    pub direction: NatDirection,
    /// False when the packet crossed inside → outside but no rule covered it.
    pub translated: bool,
    pub before_src: String,
    pub before_dst: String,
    pub after_src: String,
    pub after_dst: String,
    pub rule: Option<String>,
    /// A new table entry was made for this packet.
    pub created: bool,
    /// Plain-language account of the decision.
    pub note: String,
}

/// What the inside → outside check decided.
pub(crate) enum NatOutcome {
    /// Not crossing from inside to outside: NAT isn't involved.
    Untouched,
    Translated(Ipv4Packet),
    /// Crossing, but no rule covers the source: sent as it is.
    NotCovered,
    /// A rule covers it but there's no address left to give it.
    Exhausted,
}

/// (protocol, source port, destination port). ICMP queries use their ID
/// as both ports, the way IOS shows them.
fn flow(packet: &Ipv4Packet) -> (NatProto, u16, u16) {
    match packet.l4 {
        L4::Icmp(icmp) => (NatProto::Icmp, icmp.id, icmp.id),
        L4::Udp(udp) => (NatProto::Udp, udp.src_port, udp.dst_port),
    }
}

fn rewrite_src(packet: &Ipv4Packet, ip: Ipv4Addr, port: u16) -> Ipv4Packet {
    let mut out = Ipv4Packet { src: ip, ..*packet };
    match &mut out.l4 {
        L4::Icmp(icmp) => icmp.id = port,
        L4::Udp(udp) => udp.src_port = port,
    }
    out
}

fn rewrite_dst(packet: &Ipv4Packet, ip: Ipv4Addr, port: u16) -> Ipv4Packet {
    let mut out = Ipv4Packet { dst: ip, ..*packet };
    match &mut out.l4 {
        L4::Icmp(icmp) => icmp.id = port,
        L4::Udp(udp) => udp.dst_port = port,
    }
    out
}

fn endpoints(packet: &Ipv4Packet) -> (String, String) {
    let (_, sport, dport) = flow(packet);
    (
        format!("{}:{sport}", packet.src),
        format!("{}:{dport}", packet.dst),
    )
}

/// What a rule maps an inside host to.
struct Mapping {
    global: Ipv4Addr,
    kind: NatKind,
    rule: String,
}

impl Network {
    fn nat_side(&self, id: DeviceId, iface: &str) -> Option<NatSide> {
        let d = self.project.device(id)?;
        d.config.interface(d.kind, iface).nat
    }

    /// Whether a router answers ARP for `ip` on an outside interface: IOS
    /// does for inside global addresses (static or pool) in that subnet.
    pub(crate) fn nat_alias(&self, id: DeviceId, iface: &Iface, ip: Ipv4Addr) -> bool {
        let Some(d) = self.project.device(id) else {
            return false;
        };
        let Some(a) = iface.address else {
            return false;
        };
        if d.config.interface(d.kind, &iface.name).nat != Some(NatSide::Outside) {
            return false;
        }
        let mask = u32::from(netrace_doc::mask(a.prefix_len));
        if u32::from(ip) & mask != u32::from(a.address) & mask {
            return false;
        }
        d.config.nat_rules.iter().any(|r| match r {
            NatRule::Static { global, .. } => *global == ip,
            NatRule::Dynamic {
                target: NatTarget::Pool(p),
                ..
            } => d.config.nat_pool(p).is_some_and(|pool| pool.contains(ip)),
            NatRule::Dynamic { .. } => false,
        })
    }

    fn nat_table(&mut self, id: DeviceId) -> &mut Vec<NatEntry> {
        self.nat_tables.entry(id).or_default()
    }

    /// A packet routed from an inside interface out an outside one: find or
    /// create its translation and rewrite the source.
    pub(crate) fn nat_inside_out(
        &mut self,
        now: SimTime,
        id: DeviceId,
        stack: &Stack,
        in_iface: &Iface,
        out_iface: &Iface,
        packet: &Ipv4Packet,
    ) -> NatOutcome {
        if self.nat_side(id, &in_iface.name) != Some(NatSide::Inside)
            || self.nat_side(id, &out_iface.name) != Some(NatSide::Outside)
        {
            return NatOutcome::Untouched;
        }
        let (proto, sport, dport) = flow(packet);
        let (before_src, before_dst) = endpoints(packet);
        let name = self.name(id);

        // A flow already in the table.
        let existing = self.nat_table(id).iter().find(|e| {
            e.proto == Some(proto)
                && e.local == packet.src
                && e.local_port == sport
                && e.outside == packet.dst
                && e.outside_port == dport
        });
        if let Some(e) = existing.cloned() {
            let out = rewrite_src(packet, e.global, e.global_port);
            self.nat_hits.entry(id).or_default().0 += 1;
            let after = endpoints(&out);
            self.pending_nat = Some(NatDecision {
                device: id,
                direction: NatDirection::InsideToOutside,
                translated: true,
                note: format!(
                    "{name} already has a translation for {before_src}, so it reuses it: the source becomes {}.",
                    after.0
                ),
                before_src,
                before_dst,
                after_src: after.0,
                after_dst: after.1,
                rule: Some(e.rule),
                created: false,
            });
            return NatOutcome::Translated(out);
        }

        let Some(d) = self.project.device(id) else {
            return NatOutcome::Untouched;
        };
        let config = d.config.clone();
        if config.nat_rules.is_empty() {
            return NatOutcome::Untouched;
        }
        let mut mapping = None;
        let mut why_not = Vec::new();
        // Static rules first, as IOS does.
        for rule in &config.nat_rules {
            if let NatRule::Static { local, global } = rule
                && *local == packet.src
            {
                mapping = Some(Mapping {
                    global: *global,
                    kind: NatKind::Static,
                    rule: rule.config_line(),
                });
                break;
            }
        }
        if mapping.is_none() {
            for rule in &config.nat_rules {
                let NatRule::Dynamic {
                    acl,
                    target,
                    overload,
                } = rule
                else {
                    continue;
                };
                let permitted = crate::acl::evaluate(
                    id,
                    &config,
                    acl,
                    AclDirection::In,
                    &in_iface.name,
                    packet,
                )
                .map(|d| d.permitted);
                match permitted {
                    None => {
                        why_not.push(format!("access list {acl} doesn't exist"));
                        continue;
                    }
                    Some(false) => {
                        why_not.push(format!("access list {acl} doesn't permit {}", packet.src));
                        continue;
                    }
                    Some(true) => {}
                }
                let candidates: Vec<Ipv4Addr> = match target {
                    NatTarget::Pool(p) => match config.nat_pool(p) {
                        Some(pool) => pool.addresses().collect(),
                        None => {
                            why_not.push(format!("pool {p} isn't defined"));
                            continue;
                        }
                    },
                    NatTarget::Interface(i) => match stack.iface(i).and_then(|i| i.address) {
                        Some(a) => vec![a.address],
                        None => {
                            why_not.push(format!("{} has no IP address", abbreviate_port(i)));
                            continue;
                        }
                    },
                };
                let rule_text = rule.config_line();
                if *overload {
                    mapping = candidates.first().map(|g| Mapping {
                        global: *g,
                        kind: NatKind::Overload,
                        rule: rule_text,
                    });
                    break;
                }
                // One-to-one: the host's existing address, or the first free one.
                let table = self.nat_table(id);
                let held = table
                    .iter()
                    .find(|e| e.proto.is_none() && e.local == packet.src)
                    .map(|e| e.global);
                let taken = |g: &Ipv4Addr| {
                    table.iter().any(|e| e.proto.is_none() && e.global == *g)
                        || config
                            .nat_rules
                            .iter()
                            .any(|r| matches!(r, NatRule::Static { global, .. } if global == g))
                };
                let free = held.or_else(|| candidates.iter().copied().find(|g| !taken(g)));
                let Some(global) = free else {
                    self.nat_hits.entry(id).or_default().1 += 1;
                    let what = match target {
                        NatTarget::Pool(p) => format!("pool {p}"),
                        NatTarget::Interface(i) => abbreviate_port(i),
                    };
                    self.pending_nat = Some(NatDecision {
                        device: id,
                        direction: NatDirection::InsideToOutside,
                        translated: false,
                        note: format!(
                            "{} is permitted by access list {acl}, but every address in {what} is already given \
                             to another host (one address per host without overload), so {name} drops the packet. \
                             Adding `overload` lets hosts share an address.",
                            packet.src
                        ),
                        after_src: before_src.clone(),
                        after_dst: before_dst.clone(),
                        before_src,
                        before_dst,
                        rule: Some(rule_text),
                        created: false,
                    });
                    return NatOutcome::Exhausted;
                };
                if held.is_none() {
                    table.push(NatEntry {
                        proto: None,
                        local: packet.src,
                        local_port: 0,
                        global,
                        global_port: 0,
                        outside: Ipv4Addr::UNSPECIFIED,
                        outside_port: 0,
                        kind: NatKind::Dynamic,
                        rule: rule_text.clone(),
                        created: now,
                    });
                }
                mapping = Some(Mapping {
                    global,
                    kind: NatKind::Dynamic,
                    rule: rule_text,
                });
                break;
            }
        }

        let Some(m) = mapping else {
            let reason = if why_not.is_empty() {
                "no rule covers it".to_string()
            } else {
                why_not.join(", and ")
            };
            self.pending_nat = Some(NatDecision {
                device: id,
                direction: NatDirection::InsideToOutside,
                translated: false,
                note: format!(
                    "The packet goes from an inside to an outside interface, but it isn't translated: {reason}. \
                     It leaves with its private source {}, so replies can't find their way back.",
                    packet.src
                ),
                after_src: before_src.clone(),
                after_dst: before_dst.clone(),
                before_src,
                before_dst,
                rule: None,
                created: false,
            });
            return NatOutcome::NotCovered;
        };

        // PAT keeps the host's port when it's free on that address.
        let table = self.nat_table(id);
        let global_port = if m.kind == NatKind::Overload {
            let used = |p: u16| {
                table
                    .iter()
                    .any(|e| e.proto == Some(proto) && e.global == m.global && e.global_port == p)
            };
            if used(sport) {
                (PAT_FIRST_PORT..=u16::MAX)
                    .find(|p| !used(*p))
                    .unwrap_or(sport)
            } else {
                sport
            }
        } else {
            sport
        };
        table.push(NatEntry {
            proto: Some(proto),
            local: packet.src,
            local_port: sport,
            global: m.global,
            global_port,
            outside: packet.dst,
            outside_port: dport,
            kind: m.kind,
            rule: m.rule.clone(),
            created: now,
        });
        self.nat_hits.entry(id).or_default().1 += 1;
        let out = rewrite_src(packet, m.global, global_port);
        let after = endpoints(&out);
        let how = match m.kind {
            NatKind::Static => format!(
                "the static mapping always turns {} into {}",
                packet.src, m.global
            ),
            NatKind::Dynamic => format!(
                "{} gets {} from the pool for as long as the translation lasts",
                packet.src, m.global
            ),
            NatKind::Overload if global_port == sport => format!(
                "with overload (PAT) inside hosts share {}, told apart by port; port {sport} was free, so it's kept",
                m.global
            ),
            NatKind::Overload => format!(
                "with overload (PAT) inside hosts share {}, told apart by port; port {sport} was already in use, so \
                 this flow gets port {global_port}",
                m.global
            ),
        };
        self.pending_nat = Some(NatDecision {
            device: id,
            direction: NatDirection::InsideToOutside,
            translated: true,
            note: format!(
                "No translation for {before_src} yet, so {name} creates one: {how}. The source is rewritten from \
                 {before_src} to {}; the destination stays {before_dst}.",
                after.0
            ),
            before_src,
            before_dst,
            after_src: after.0,
            after_dst: after.1,
            rule: Some(m.rule),
            created: true,
        });
        NatOutcome::Translated(out)
    }

    /// A packet arriving on an outside interface for an inside global
    /// address: rewrite the destination back to the inside host. `None`
    /// when no translation matches (the packet carries on as it is).
    pub(crate) fn nat_outside_in(
        &mut self,
        now: SimTime,
        id: DeviceId,
        in_iface: &Iface,
        packet: &Ipv4Packet,
    ) -> Option<Ipv4Packet> {
        if self.nat_side(id, &in_iface.name) != Some(NatSide::Outside) {
            return None;
        }
        let (proto, sport, dport) = flow(packet);
        let error = packet.is_icmp_error();
        let name = self.name(id);
        let (before_src, before_dst) = endpoints(packet);
        // ICMP errors come from whichever router noticed, so match on the
        // inside global side only.
        let existing = self
            .nat_table(id)
            .iter()
            .find(|e| {
                e.proto == Some(proto)
                    && e.global == packet.dst
                    && e.global_port == dport
                    && (error || (e.outside == packet.src && e.outside_port == sport))
            })
            .cloned();
        let (local, local_port, rule, created) = match existing {
            Some(e) => {
                self.nat_hits.entry(id).or_default().0 += 1;
                (e.local, e.local_port, e.rule, false)
            }
            None => {
                let d = self.project.device(id)?;
                let fixed = d.config.nat_rules.iter().find_map(|r| match r {
                    NatRule::Static { local, global } if *global == packet.dst => {
                        Some((*local, NatKind::Static, r.config_line()))
                    }
                    _ => None,
                });
                let one_to_one = || {
                    self.nat_tables.get(&id).and_then(|t| {
                        t.iter()
                            .find(|e| e.proto.is_none() && e.global == packet.dst)
                            .map(|e| (e.local, NatKind::Dynamic, e.rule.clone()))
                    })
                };
                let (local, kind, rule) = fixed.or_else(one_to_one)?;
                if error {
                    return None;
                }
                self.nat_table(id).push(NatEntry {
                    proto: Some(proto),
                    local,
                    local_port: dport,
                    global: packet.dst,
                    global_port: dport,
                    outside: packet.src,
                    outside_port: sport,
                    kind,
                    rule: rule.clone(),
                    created: now,
                });
                self.nat_hits.entry(id).or_default().1 += 1;
                (local, dport, rule, true)
            }
        };
        let out = rewrite_dst(packet, local, local_port);
        let after = endpoints(&out);
        let note = if created {
            format!(
                "{} is a static inside global address, so {name} lets the outside reach the inside host: it creates \
                 a translation and rewrites the destination from {before_dst} to {}.",
                packet.dst, after.1
            )
        } else {
            format!(
                "The packet is for inside global {before_dst}, which the translation table maps to inside local {}. \
                 {name} rewrites the destination back, then routes it inside.",
                after.1
            )
        };
        self.pending_nat = Some(NatDecision {
            device: id,
            direction: NatDirection::OutsideToInside,
            translated: true,
            before_src,
            before_dst,
            after_src: after.0,
            after_dst: after.1,
            rule: Some(rule),
            created,
            note,
        });
        Some(out)
    }
}
