//! What switches do with frames: VLAN membership and 802.1Q tags,
//! spanning-tree port states, port security, MAC learning, forwarding and
//! flooding within a VLAN, and frames to and from their own `Vlan`
//! interfaces.

use std::time::Duration;

use netrace_doc::{DeviceId, Violation};
use netrace_sim::SimTime;

use crate::network::{Arrival, Event, Role, list};
use crate::stp::{PortKey, switchport};
use crate::{Frame, Layer, LayerRole, MacAddr, Network, Outcome, abbreviate_port, explain};

/// Why a switch didn't accept a frame on a port.
enum Refusal {
    Dropped(String),
}

impl Network {
    pub(crate) fn switch_receive(
        &mut self,
        now: SimTime,
        id: DeviceId,
        arrival: Arrival,
        frame: Frame,
    ) {
        let name = self.name(id);
        let ingress = arrival.port.clone();
        let p = abbreviate_port(&ingress);
        let key: PortKey = (id, ingress.clone());

        let vlan = match self.ingress_vlan(id, &ingress, &frame) {
            Ok(vlan) => vlan,
            Err(Refusal::Dropped(reason)) => {
                return self.switch_drop(
                    now,
                    id,
                    &arrival,
                    frame,
                    format!("{name} receives the frame on {p}, but {reason}"),
                );
            }
        };
        if let Err(Refusal::Dropped(reason)) = self.port_security(now, id, &key, &frame, &arrival) {
            return self.switch_drop(now, id, &arrival, frame, reason);
        }

        let src = self.who(frame.src);
        let dst = self.who(frame.dst);
        let candidates = self.vlan_ports(id, vlan);
        let svi_mac = self.svi_mac(id, vlan);
        let vlan_text = if self.vlans_in_use(id) {
            format!(" (VLAN {vlan})")
        } else {
            String::new()
        };
        let Some(Role::Switch { table, .. }) = self.nodes.get_mut(&id).map(|n| &mut n.role) else {
            return;
        };
        let previous = table.insert((vlan, frame.src), ingress.clone());
        let svi = format!("Vlan{vlan}");
        let known = if frame.dst.is_broadcast() {
            None
        } else if Some(frame.dst) == svi_mac {
            Some(svi.clone())
        } else {
            table.get(&(vlan, frame.dst)).cloned()
        };
        let learned = match previous {
            None => format!("{name} learns that {src} is on {p}{vlan_text}"),
            Some(old) if old == ingress => format!("{name} confirms {src} is still on {p}"),
            Some(old) => format!(
                "{name} sees {src} has moved from {} to {p}",
                abbreviate_port(&old)
            ),
        };
        let to_self = frame.dst.is_broadcast() && svi_mac.is_some();
        let deliver_to_self = to_self || known.as_deref() == Some(svi.as_str());

        let (outcome, egress, decision) = match known {
            Some(out) if out == svi => (
                Outcome::Forwarded,
                Vec::new(),
                format!(
                    "The frame is addressed to {name}'s own interface {svi}, so {name} reads it itself."
                ),
            ),
            Some(out) if out == ingress => (
                Outcome::Dropped,
                Vec::new(),
                format!(
                    "{dst} is on the same port the frame came in on, so there's nothing to forward and {name} drops it."
                ),
            ),
            Some(out) if candidates.contains(&out) => {
                let o = abbreviate_port(&out);
                (
                    Outcome::Forwarded,
                    vec![out],
                    format!(
                        "It already knows {dst} is on {o}, so it forwards the frame only out {o}."
                    ),
                )
            }
            _ => {
                let outs: Vec<String> = candidates
                    .iter()
                    .filter(|p| **p != ingress)
                    .cloned()
                    .collect();
                let reason = if frame.dst.is_broadcast() {
                    "The frame is a broadcast".to_string()
                } else {
                    format!("It doesn't know where {dst} is yet")
                };
                let scope = if vlan_text.is_empty() {
                    "every other connected port".to_string()
                } else {
                    format!("every other port in VLAN {vlan}")
                };
                let also = if to_self {
                    format!(" Its own {svi} interface gets a copy too.")
                } else {
                    String::new()
                };
                let decision = if outs.is_empty() {
                    format!(
                        "{reason}, but it has no other ports in this VLAN, so the frame goes nowhere.{also}"
                    )
                } else {
                    format!(
                        "{reason}, so it floods the frame out {scope}: {}.{also}",
                        self.describe_egress(id, &outs, vlan)
                    )
                };
                (Outcome::Flooded, outs, decision)
            }
        };
        let mut tagging = Vec::new();
        for port in &egress {
            let tag = self.egress_tag(id, port, vlan);
            if tag.is_some() != frame.vlan.is_some() {
                tagging.push((port.clone(), tag));
            }
            self.transmit(id, port, frame.tagged(tag), arrival.tag);
        }
        if deliver_to_self {
            self.deliver_internally(id, vlan, frame.tagged(None), arrival.tag);
        }
        let tag_note = match (frame.vlan, tagging.first()) {
            (None, Some((_, Some(t)))) => {
                format!(" An 802.1Q tag for VLAN {t} is added on the trunk.")
            }
            (Some(t), Some((_, None))) => {
                format!(" The VLAN {t} tag is removed for the access port.")
            }
            _ => String::new(),
        };
        let mut layers = vec![
            explain::physical(if egress.is_empty() {
                format!("Arrived on {p}.")
            } else {
                format!("Arrived on {p}; sent out {}.", list(&egress))
            }),
            explain::data_link(
                if tag_note.is_empty() {
                    LayerRole::Used
                } else {
                    LayerRole::Changed
                },
                format!(
                    "Reads the source MAC to learn where it is, and the destination MAC to decide where to send the \
                     frame.{}",
                    if tag_note.is_empty() {
                        " The frame itself isn't changed.".to_string()
                    } else {
                        tag_note.clone()
                    }
                ),
                &frame,
            ),
        ];
        layers.extend(explain::upper_not_used(&frame, "A switch"));
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            outcome,
            format!("{learned}. {decision}{tag_note}"),
            layers,
            egress,
        );
    }

    /// The VLAN a frame belongs to on arrival, or why it isn't accepted.
    fn ingress_vlan(&self, id: DeviceId, port: &str, frame: &Frame) -> Result<u16, Refusal> {
        let p = abbreviate_port(port);
        let key = (id, port.to_string());
        if self.l2.err_disabled.contains(&key) || self.psec_disabled.contains(&key) {
            return Err(Refusal::Dropped(format!(
                "{p} is err-disabled ({}), so the frame is lost. Fix the cause, then shutdown / no shutdown the port.",
                if self.psec_disabled.contains(&key) {
                    "port security violation"
                } else {
                    "BPDU guard"
                }
            )));
        }
        let Some(sp) = switchport(&self.project, id, port) else {
            return Ok(1);
        };
        let vlan = match (sp.is_trunk(), frame.vlan) {
            (true, Some(tag)) => tag,
            (true, None) => sp.native_vlan,
            (false, None) => sp.access_vlan,
            (false, Some(tag)) => {
                return Err(Refusal::Dropped(format!(
                    "the frame carries an 802.1Q tag (VLAN {tag}) and {p} is an access port, which only takes untagged \
                     frames. Is the other end a trunk while this one isn't?"
                )));
            }
        };
        if sp.is_trunk() && !sp.allowed_vlans.contains(vlan) {
            return Err(Refusal::Dropped(format!(
                "VLAN {vlan} isn't allowed on the trunk {p} (switchport trunk allowed vlan), so the frame is dropped."
            )));
        }
        let config = &self.project.device(id).expect("switch exists").config;
        if !config.vlan_exists(vlan) {
            return Err(Refusal::Dropped(format!(
                "VLAN {vlan} doesn't exist on this switch, so the frame is dropped. Create it with `vlan {vlan}`."
            )));
        }
        if !self.l2.forwards(id, port, vlan) {
            return Err(Refusal::Dropped(format!(
                "{p} is blocking for VLAN {vlan}: spanning tree turned it off to break a loop, so the frame is \
                 discarded. This is how a redundant link stays ready without flooding forever."
            )));
        }
        Ok(vlan)
    }

    /// Port security on an access port: learns secure MACs up to the
    /// maximum; a new MAC beyond it is a violation.
    fn port_security(
        &mut self,
        now: SimTime,
        id: DeviceId,
        key: &PortKey,
        frame: &Frame,
        arrival: &Arrival,
    ) -> Result<(), Refusal> {
        let Some(sp) = switchport(&self.project, id, &key.1) else {
            return Ok(());
        };
        let Some(ps) = sp.port_security.clone().filter(|ps| ps.enabled) else {
            return Ok(());
        };
        let statics: Vec<MacAddr> = ps
            .static_macs
            .iter()
            .filter_map(|m| MacAddr::parse_cisco(m))
            .collect();
        let learned = self.secure.entry(key.clone()).or_default();
        if statics.contains(&frame.src) || learned.contains(&frame.src) {
            return Ok(());
        }
        if statics.len() + learned.len() < usize::from(ps.maximum.max(1)) {
            learned.push(frame.src);
            return Ok(());
        }
        let name = self.name(id);
        let p = abbreviate_port(&key.1);
        let who = self.who(frame.src);
        *self.violations.entry(key.clone()).or_default() += 1;
        let reason = match ps.violation {
            Violation::Shutdown => {
                self.psec_disabled.insert(key.clone());
                let _ = (now, arrival);
                format!(
                    "%PORT_SECURITY-2-PSECURE_VIOLATION on {name} {p}: {who} is a new MAC address and the port allows \
                     only {} (port security). The violation mode is shutdown, so {p} goes err-disabled and drops \
                     everything from now on.",
                    ps.maximum
                )
            }
            Violation::Restrict => format!(
                "{name} drops the frame from {who} on {p}: port security allows only {} MAC address(es) there. \
                 Restrict mode drops and counts it, and logs a message.",
                ps.maximum
            ),
            Violation::Protect => format!(
                "{name} silently drops the frame from {who} on {p}: port security allows only {} MAC address(es) there \
                 (protect mode).",
                ps.maximum
            ),
        };
        Err(Refusal::Dropped(reason))
    }

    fn switch_drop(
        &mut self,
        now: SimTime,
        id: DeviceId,
        arrival: &Arrival,
        frame: Frame,
        summary: String,
    ) {
        let layers = vec![
            explain::physical(format!("Arrived on {}.", abbreviate_port(&arrival.port))),
            explain::data_link(
                LayerRole::Used,
                "Discarded before the switch learned or forwarded anything.",
                &frame,
            ),
            explain::not_used(Layer::Network, "Never reached."),
        ];
        self.record(
            now,
            id,
            Some(arrival),
            Some(frame),
            Outcome::Dropped,
            summary,
            layers,
            Vec::new(),
        );
    }

    /// Ports that pass frames of `vlan`: cabled, enabled, carrying it,
    /// forwarding in spanning tree.
    pub(crate) fn vlan_ports(&self, id: DeviceId, vlan: u16) -> Vec<String> {
        self.wired_ports(id)
            .into_iter()
            .filter(|port| {
                let key = (id, port.clone());
                !self.psec_disabled.contains(&key)
                    && switchport(&self.project, id, port).is_some_and(|sp| sp.carries(vlan))
                    && self.l2.forwards(id, port, vlan)
            })
            .collect()
    }

    /// The tag a frame of `vlan` leaves `port` with: none on access ports
    /// and for a trunk's native VLAN.
    fn egress_tag(&self, id: DeviceId, port: &str, vlan: u16) -> Option<u16> {
        switchport(&self.project, id, port)
            .filter(|sp| sp.is_trunk() && sp.native_vlan != vlan)
            .map(|_| vlan)
    }

    fn describe_egress(&self, id: DeviceId, ports: &[String], vlan: u16) -> String {
        if !self.vlans_in_use(id) {
            return list(ports);
        }
        let parts: Vec<String> = ports
            .iter()
            .map(|p| match self.egress_tag(id, p, vlan) {
                Some(t) => format!("{} (trunk, tagged {t})", abbreviate_port(p)),
                None if switchport(&self.project, id, p).is_some_and(|sp| sp.is_trunk()) => {
                    format!("{} (trunk, native)", abbreviate_port(p))
                }
                None => abbreviate_port(p),
            })
            .collect();
        match parts.as_slice() {
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
            [] => String::new(),
        }
    }

    /// Whether a switch uses anything beyond VLAN 1, so explanations only
    /// mention VLANs once they matter.
    pub(crate) fn vlans_in_use(&self, id: DeviceId) -> bool {
        self.project.device(id).is_some_and(|d| {
            !d.config.vlans.is_empty()
                || d.kind
                    .port_names()
                    .iter()
                    .any(|p| d.config.interface(d.kind, p).switchport.is_trunk())
        })
    }

    /// The MAC of the switch's `Vlan` interface for this VLAN, if it's up.
    fn svi_mac(&self, id: DeviceId, vlan: u16) -> Option<MacAddr> {
        self.stack(id)?
            .ifaces
            .iter()
            .find(|i| i.port.is_none() && i.vlan == Some(vlan) && i.up)
            .map(|i| i.mac)
    }

    /// A frame sent by a switch's own `Vlan` interface: learned on that
    /// interface, then switched like any other. Returns the ports it left by.
    pub(crate) fn switch_from_self(
        &mut self,
        id: DeviceId,
        vlan: u16,
        frame: Frame,
        tag: Option<u64>,
    ) -> Vec<String> {
        let candidates = self.vlan_ports(id, vlan);
        let Some(Role::Switch { table, .. }) = self.nodes.get_mut(&id).map(|n| &mut n.role) else {
            return Vec::new();
        };
        let svi = format!("Vlan{vlan}");
        table.insert((vlan, frame.src), svi.clone());
        let egress = match table.get(&(vlan, frame.dst)) {
            Some(port)
                if !frame.dst.is_broadcast() && *port != svi && candidates.contains(port) =>
            {
                vec![port.clone()]
            }
            _ => candidates,
        };
        for port in &egress {
            let out = frame.tagged(self.egress_tag(id, port, vlan));
            self.transmit(id, port, out, tag);
        }
        egress
    }

    fn deliver_internally(&mut self, id: DeviceId, vlan: u16, frame: Frame, tag: Option<u64>) {
        let sent = self.scheduler.now();
        self.scheduler.schedule_in(
            Duration::ZERO,
            Event::Deliver {
                to: id,
                port: format!("Vlan{vlan}"),
                frame,
                from: id,
                link: None,
                sent,
                tag,
            },
        );
    }
}

impl MacAddr {
    /// Parses "aaaa.bbbb.cccc" (IOS) or "aa:bb:cc:dd:ee:ff".
    pub fn parse_cisco(text: &str) -> Option<MacAddr> {
        let hex: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if hex.len() != 12
            || text
                .chars()
                .any(|c| !c.is_ascii_hexdigit() && c != '.' && c != ':' && c != '-')
        {
            return None;
        }
        let mut bytes = [0u8; 6];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(MacAddr(bytes))
    }
}
