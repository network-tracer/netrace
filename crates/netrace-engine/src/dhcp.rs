//! DHCP: clients on end devices, servers (`ip dhcp pool`) and relays
//! (`ip helper-address`) on routers. Every step is a traced frame, so the
//! Discover → Offer → Request → ACK exchange can be watched hop by hop.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::time::Duration;

use netrace_doc::{DeviceId, InterfaceAddress, mask};
use netrace_sim::SimTime;

use crate::network::{Arrival, Event};
use crate::stack::{Iface, Route, RouteSource, Stack, StackKind};
use crate::{
    DHCP_CLIENT_PORT, DHCP_SERVER_PORT, Dhcp, DhcpOp, Frame, Ipv4Packet, L4, LayerRole, MacAddr,
    Network, Outcome, Payload, Udp, abbreviate_port, explain,
};

/// How long a client waits for its exchange to finish before falling back
/// to an automatic private address (Windows waits longer; this keeps traces short).
pub(crate) const DHCP_TIMEOUT: Duration = Duration::from_secs(6);
/// A client with no offer yet broadcasts its Discover again after this.
pub(crate) const DHCP_RETRY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientState {
    Selecting,
    Requesting,
    Bound,
}

#[derive(Debug, Clone)]
pub(crate) struct DhcpClient {
    pub xid: u32,
    pub state: ClientState,
}

/// A lease a server has handed out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DhcpBinding {
    pub address: Ipv4Addr,
    pub mac: MacAddr,
    pub pool: String,
}

impl Network {
    /// A client starts (or restarts) its exchange: broadcast Discover.
    pub(crate) fn dhcp_start(&mut self, now: SimTime, host: DeviceId) {
        let Some(stack) = self.stack(host).cloned() else {
            return;
        };
        let Some(iface) = stack.ifaces.first().cloned() else {
            return;
        };
        let name = self.name(host);
        let xid = 0x3903_f326 ^ (host.0 as u32).wrapping_mul(0x9e37_79b9) ^ (now.as_nanos() as u32);
        self.dhcp_clients.insert(
            host,
            DhcpClient {
                xid,
                state: ClientState::Selecting,
            },
        );
        // A fresh exchange starts from no address.
        if let Some(s) = self.stack_mut(host) {
            s.ifaces[0].address = None;
            s.routes.clear();
        }
        let dhcp = Dhcp {
            op: DhcpOp::Discover,
            xid,
            client_mac: iface.mac,
            your_ip: Ipv4Addr::UNSPECIFIED,
            relay: Ipv4Addr::UNSPECIFIED,
            server: Ipv4Addr::UNSPECIFIED,
            requested: None,
            prefix_len: 0,
            router: None,
            dns: None,
        };
        let frame = client_frame(iface.mac, dhcp);
        let egress = self.send_raw(host, &iface, frame);
        self.scheduler
            .schedule_in(DHCP_TIMEOUT, Event::DhcpTimeout { host, xid });
        self.scheduler
            .schedule_in(DHCP_RETRY, Event::DhcpRetry { host, xid });
        let layers = vec![
            explain::physical(format!("Sent out {}.", abbreviate_port(&iface.name))),
            explain::data_link(
                LayerRole::Changed,
                "Broadcast: the client doesn't know who the server is.",
                &frame,
            ),
            explain::ipv4(
                LayerRole::Changed,
                "From 0.0.0.0 (no address yet) to 255.255.255.255 (everyone on this network).",
                ip_of(&frame),
            ),
            explain::message(
                LayerRole::Changed,
                "DHCP Discover: \"Is there a DHCP server? I need an address.\"",
                ip_of(&frame),
            ),
        ];
        let summary = if egress.is_empty() {
            format!(
                "{name} wants an address from DHCP, but nothing is plugged into {}.",
                abbreviate_port(&iface.name)
            )
        } else {
            format!(
                "{name} gets its address from DHCP. It has no address yet, so it broadcasts a DHCP Discover asking \
                 any DHCP server on its network for one."
            )
        };
        self.record(
            now,
            host,
            None,
            Some(frame),
            Outcome::Created,
            summary,
            layers,
            egress,
        );
    }

    /// Still no offer: broadcast the Discover again (same transaction).
    pub(crate) fn dhcp_retry(&mut self, now: SimTime, host: DeviceId, xid: u32) {
        let Some(client) = self.dhcp_clients.get(&host) else {
            return;
        };
        if client.xid != xid || client.state != ClientState::Selecting {
            return;
        }
        let Some(iface) = self.stack(host).and_then(|s| s.ifaces.first().cloned()) else {
            return;
        };
        let frame = client_frame(
            iface.mac,
            Dhcp {
                op: DhcpOp::Discover,
                xid,
                client_mac: iface.mac,
                your_ip: Ipv4Addr::UNSPECIFIED,
                relay: Ipv4Addr::UNSPECIFIED,
                server: Ipv4Addr::UNSPECIFIED,
                requested: None,
                prefix_len: 0,
                router: None,
                dns: None,
            },
        );
        let egress = self.send_raw(host, &iface, frame);
        let name = self.name(host);
        let layers = vec![
            explain::physical(format!("Sent out {}.", abbreviate_port(&iface.name))),
            explain::data_link(LayerRole::Changed, "Broadcast again.", &frame),
            explain::message(
                LayerRole::Changed,
                "The same Discover, same transaction ID.",
                ip_of(&frame),
            ),
        ];
        self.record(
            now,
            host,
            None,
            Some(frame),
            Outcome::Created,
            format!(
                "{name} has had no offer after {} seconds, so it broadcasts its DHCP Discover again. (A router relaying \
                 it drops the first copy while it looks up the server's MAC address with ARP.)",
                DHCP_RETRY.as_secs()
            ),
            layers,
            egress,
        );
    }

    /// No lease in time: Windows gives itself an APIPA address (169.254.x.x).
    pub(crate) fn dhcp_timeout(&mut self, now: SimTime, host: DeviceId, xid: u32) {
        let Some(client) = self.dhcp_clients.get(&host) else {
            return;
        };
        if client.xid != xid || client.state == ClientState::Bound {
            return;
        }
        let name = self.name(host);
        let apipa = Ipv4Addr::new(
            169,
            254,
            (host.0 >> 8) as u8 | 1,
            (host.0 as u8).clamp(1, 254),
        );
        if let Some(s) = self.stack_mut(host) {
            s.ifaces[0].address = Some(InterfaceAddress {
                address: apipa,
                prefix_len: 16,
            });
            s.routes.clear();
            s.routes.push(Route {
                network: Ipv4Addr::new(169, 254, 0, 0),
                prefix_len: 16,
                source: RouteSource::Connected,
                next_hop: None,
                iface: s.ifaces[0].name.clone(),
                distance: 0,
            });
        }
        self.dhcp_clients.remove(&host);
        self.record(
            now,
            host,
            None,
            None,
            Outcome::Dropped,
            format!(
                "No DHCP server answered {name} within {} seconds. Like Windows, it gives itself an automatic \
                 private address, {apipa}/16 (APIPA), which only reaches other devices that also failed DHCP. \
                 A 169.254 address is the classic sign that DHCP isn't working: check for a pool on this network, \
                 or an `ip helper-address` on the router if the server is elsewhere.",
                DHCP_TIMEOUT.as_secs()
            ),
            Vec::new(),
            Vec::new(),
        );
    }

    /// A DHCP message arriving at a device's IP stack.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dhcp_receive(
        &mut self,
        now: SimTime,
        id: DeviceId,
        stack: &Stack,
        iface: &Iface,
        arrival: Arrival,
        frame: Frame,
        packet: Ipv4Packet,
        udp: Udp,
    ) {
        let name = self.name(id);
        let d = udp.dhcp;
        let port = abbreviate_port(&arrival.port);
        let layers = |note: String| {
            vec![
                explain::physical(format!("Arrived on {port}.")),
                explain::data_link(LayerRole::Used, "Read and passed up.", &frame),
                explain::ipv4(
                    LayerRole::Used,
                    format!("From {} to {}.", packet.src, packet.dst),
                    &packet,
                ),
                explain::message(LayerRole::Used, note, &packet),
            ]
        };

        if udp.dst_port == DHCP_CLIENT_PORT {
            if stack.kind != StackKind::Host {
                // A relay forwarding the server's answer back to its client.
                return self.relay_to_client(now, id, stack, arrival, frame, packet, udp);
            }
            let mine = self
                .dhcp_clients
                .get(&id)
                .is_some_and(|c| c.xid == d.xid && d.client_mac == iface.mac);
            if !mine {
                let summary = format!(
                    "{name} receives a DHCP {} meant for another client (it carries a different transaction ID and MAC), so it ignores it.",
                    d.op.name()
                );
                return self.record(
                    now,
                    id,
                    Some(&arrival),
                    Some(frame),
                    Outcome::Ignored,
                    summary,
                    layers("Not this client's transaction.".into()),
                    Vec::new(),
                );
            }
            let state = self.dhcp_clients[&id].state;
            match (d.op, state) {
                (DhcpOp::Offer, ClientState::Selecting) => {
                    let request = Dhcp {
                        op: DhcpOp::Request,
                        requested: Some(d.your_ip),
                        your_ip: Ipv4Addr::UNSPECIFIED,
                        relay: Ipv4Addr::UNSPECIFIED,
                        ..d
                    };
                    let out = client_frame(iface.mac, request);
                    let egress = self.send_raw(id, iface, out);
                    if let Some(c) = self.dhcp_clients.get_mut(&id) {
                        c.state = ClientState::Requesting;
                    }
                    let summary = format!(
                        "{name} receives an offer of {}/{} from {}. It broadcasts a DHCP Request for that address \
                         (broadcast, so any other server that made an offer knows it wasn't chosen).",
                        d.your_ip,
                        d.prefix_len,
                        self.who_ip(d.server)
                    );
                    self.record(
                        now,
                        id,
                        Some(&arrival),
                        Some(frame),
                        Outcome::Replied,
                        summary,
                        layers(format!("DHCP Offer: {} is available.", d.your_ip)),
                        egress,
                    );
                }
                (DhcpOp::Ack, ClientState::Requesting) => {
                    if let Some(s) = self.stack_mut(id) {
                        s.ifaces[0].address = Some(InterfaceAddress {
                            address: d.your_ip,
                            prefix_len: d.prefix_len,
                        });
                        let iface_name = s.ifaces[0].name.clone();
                        let network =
                            Ipv4Addr::from(u32::from(d.your_ip) & u32::from(mask(d.prefix_len)));
                        s.routes = vec![Route {
                            network,
                            prefix_len: d.prefix_len,
                            source: RouteSource::Connected,
                            next_hop: None,
                            iface: iface_name.clone(),
                            distance: 0,
                        }];
                        if let Some(gw) = d.router {
                            s.routes.push(Route {
                                network: Ipv4Addr::UNSPECIFIED,
                                prefix_len: 0,
                                source: RouteSource::Gateway,
                                next_hop: Some(gw),
                                iface: iface_name,
                                distance: 1,
                            });
                        }
                    }
                    if let Some(c) = self.dhcp_clients.get_mut(&id) {
                        c.state = ClientState::Bound;
                    }
                    self.dhcp_leases_held.insert(id, d);
                    let gateway = d
                        .router
                        .map(|g| format!(", default gateway {g}"))
                        .unwrap_or_default();
                    let dns = d.dns.map(|g| format!(", DNS {g}")).unwrap_or_default();
                    let summary = format!(
                        "{name} receives the DHCP ACK and configures itself: {}/{}{gateway}{dns}. The exchange is \
                         complete (Discover, Offer, Request, ACK).",
                        d.your_ip, d.prefix_len
                    );
                    self.record(
                        now,
                        id,
                        Some(&arrival),
                        Some(frame),
                        Outcome::Accepted,
                        summary,
                        layers("DHCP ACK: the lease is confirmed.".into()),
                        Vec::new(),
                    );
                }
                (DhcpOp::Nak, _) => {
                    let summary = format!(
                        "{name} receives a DHCP NAK: the server refused the request. It starts over."
                    );
                    self.record(
                        now,
                        id,
                        Some(&arrival),
                        Some(frame),
                        Outcome::Accepted,
                        summary,
                        layers("DHCP NAK.".into()),
                        Vec::new(),
                    );
                    self.dhcp_start(now, id);
                }
                _ => {
                    let summary = format!(
                        "{name} already has what it needs, so it ignores the extra DHCP {}.",
                        d.op.name()
                    );
                    self.record(
                        now,
                        id,
                        Some(&arrival),
                        Some(frame),
                        Outcome::Ignored,
                        summary,
                        layers("Not needed any more.".into()),
                        Vec::new(),
                    );
                }
            }
            return;
        }

        // To a server (port 67).
        if stack.kind == StackKind::Host {
            let summary = format!(
                "{name} isn't a DHCP server, so it ignores the DHCP {}.",
                d.op.name()
            );
            return self.record(
                now,
                id,
                Some(&arrival),
                Some(frame),
                Outcome::Ignored,
                summary,
                layers("Only servers answer this.".into()),
                Vec::new(),
            );
        }
        let config = self
            .project
            .device(id)
            .map(|d| d.config.clone())
            .unwrap_or_default();
        // The network the client is on: where the relay heard it, or this interface.
        let client_net = if d.relay.is_unspecified() {
            iface.address.map(|a| a.address)
        } else {
            Some(d.relay)
        };
        let pool =
            client_net.and_then(|net| config.dhcp_pools.iter().find(|p| p.contains(net)).cloned());
        if let Some(pool) = pool {
            return self.serve(now, id, iface, arrival, frame, packet, d, pool, &config);
        }
        // Not a server for this network: relay if configured, else stop here.
        let helpers = config
            .interface(self.project.device(id).expect("exists").kind, &iface.name)
            .helper_addresses;
        if let Some(address) = iface.address
            && d.relay.is_unspecified()
            && !helpers.is_empty()
        {
            let relay_ip = address.address;
            let mut sent = Vec::new();
            for helper in &helpers {
                let relayed = Ipv4Packet {
                    src: relay_ip,
                    dst: *helper,
                    ttl: stack.ttl,
                    l4: L4::Udp(Udp {
                        src_port: DHCP_SERVER_PORT,
                        dst_port: DHCP_SERVER_PORT,
                        dhcp: Dhcp {
                            relay: relay_ip,
                            ..d
                        },
                    }),
                };
                if stack.resolve(*helper).is_some() {
                    self.send_from(now, id, relayed, None, false);
                    sent.push(helper.to_string());
                }
            }
            let summary = format!(
                "Routers don't forward broadcasts, but {name}'s {port} has `ip helper-address {}`, so it relays the DHCP \
                 {}: it puts its own address {relay_ip} in the relay field (so the server knows which network the \
                 client is on) and sends it as a normal packet to the server.",
                sent.join(", "),
                d.op.name()
            );
            return self.record(
                now,
                id,
                Some(&arrival),
                Some(frame),
                Outcome::Forwarded,
                summary,
                layers(format!("Relayed to {}.", sent.join(", "))),
                vec![arrival.port.clone()],
            );
        }
        let summary = if config.dhcp_pools.is_empty() {
            format!(
                "{name} receives the DHCP {}, but it isn't a DHCP server and routers don't forward broadcasts. Without an \
                 `ip dhcp pool` here or an `ip helper-address` on {port}, the request goes no further.",
                d.op.name()
            )
        } else {
            format!(
                "{name} has DHCP pools, but none covers the network of {port}, so it doesn't answer. Check the pool's \
                 `network` command."
            )
        };
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            Outcome::Dropped,
            summary,
            layers("No pool for this network.".into()),
            Vec::new(),
        );
    }

    /// A router with a matching pool answers: Offer for a Discover, ACK for
    /// a Request.
    #[allow(clippy::too_many_arguments)]
    fn serve(
        &mut self,
        now: SimTime,
        id: DeviceId,
        iface: &Iface,
        arrival: Arrival,
        frame: Frame,
        packet: Ipv4Packet,
        d: Dhcp,
        pool: netrace_doc::DhcpPool,
        config: &netrace_doc::DeviceConfig,
    ) {
        let name = self.name(id);
        let port = abbreviate_port(&arrival.port);
        let Some(server_ip) = iface.address.map(|a| a.address) else {
            return;
        };
        let (network, prefix) = pool.network.expect("pool matched a network");
        let layers = |note: String| {
            vec![
                explain::physical(format!("Arrived on {port}.")),
                explain::data_link(LayerRole::Used, "Read and passed up.", &frame),
                explain::ipv4(
                    LayerRole::Used,
                    format!("From {} to {}.", packet.src, packet.dst),
                    &packet,
                ),
                explain::message(LayerRole::Used, note, &packet),
            ]
        };
        let (op, address, summary) = match d.op {
            DhcpOp::Discover => {
                let Some(address) = self.allocate(id, &pool, config, d.client_mac) else {
                    let summary = format!(
                        "{name}'s pool {} has no free addresses left (after excluded addresses and existing leases), so it can't make an offer.",
                        pool.name
                    );
                    return self.record(
                        now,
                        id,
                        Some(&arrival),
                        Some(frame),
                        Outcome::Dropped,
                        summary,
                        layers("Pool exhausted.".into()),
                        Vec::new(),
                    );
                };
                self.dhcp_offers.insert((id, d.client_mac), address);
                let excluded = if config.dhcp_excluded.is_empty() {
                    String::new()
                } else {
                    " (skipping the excluded addresses)".to_string()
                };
                (
                    DhcpOp::Offer,
                    address,
                    format!(
                        "{name} is a DHCP server with pool {} for {network}/{prefix}. It picks the first free address{excluded}, \
                         {address}, and offers it.",
                        pool.name
                    ),
                )
            }
            DhcpOp::Request => {
                if !d.server.is_unspecified() && d.server != server_ip {
                    let summary = format!(
                        "{name} sees the client chose another server's offer ({}), so it withdraws its own.",
                        d.server
                    );
                    self.dhcp_offers.remove(&(id, d.client_mac));
                    return self.record(
                        now,
                        id,
                        Some(&arrival),
                        Some(frame),
                        Outcome::Ignored,
                        summary,
                        layers("Another server was chosen.".into()),
                        Vec::new(),
                    );
                }
                let offered = self.dhcp_offers.get(&(id, d.client_mac)).copied();
                match (offered, d.requested) {
                    (Some(a), Some(r)) if a == r => {
                        self.dhcp_bindings.entry(id).or_default().insert(
                            d.client_mac,
                            DhcpBinding {
                                address: a,
                                mac: d.client_mac,
                                pool: pool.name.clone(),
                            },
                        );
                        (
                            DhcpOp::Ack,
                            a,
                            format!(
                                "{name} confirms the lease of {a} to the client with a DHCP ACK and records the binding."
                            ),
                        )
                    }
                    _ => (
                        DhcpOp::Nak,
                        Ipv4Addr::UNSPECIFIED,
                        format!("{name} didn't offer that address, so it answers with a DHCP NAK."),
                    ),
                }
            }
            _ => return,
        };
        let reply = Dhcp {
            op,
            your_ip: address,
            server: server_ip,
            prefix_len: prefix,
            router: pool.default_router,
            dns: pool.dns_server,
            requested: None,
            ..d
        };
        let egress = if d.relay.is_unspecified() {
            // Same network as the client: broadcast back out the interface it came in on.
            let out = Frame {
                src: iface.mac,
                dst: MacAddr::BROADCAST,
                vlan: None,
                payload: Payload::Ipv4(Ipv4Packet {
                    src: server_ip,
                    dst: Ipv4Addr::BROADCAST,
                    ttl: 255,
                    l4: L4::Udp(Udp {
                        src_port: DHCP_SERVER_PORT,
                        dst_port: DHCP_CLIENT_PORT,
                        dhcp: reply,
                    }),
                }),
            };
            self.send_raw(id, iface, out)
        } else {
            // Relayed: answer the relay agent, which passes it on.
            let unicast = Ipv4Packet {
                src: server_ip,
                dst: d.relay,
                ttl: 255,
                l4: L4::Udp(Udp {
                    src_port: DHCP_SERVER_PORT,
                    dst_port: DHCP_CLIENT_PORT,
                    dhcp: reply,
                }),
            };
            self.send_from(now, id, unicast, None, false);
            vec![arrival.port.clone()]
        };
        let to = if d.relay.is_unspecified() {
            "broadcast back to the client".into()
        } else {
            format!("sent to the relay agent {}", d.relay)
        };
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            Outcome::Replied,
            format!("{summary} The reply is {to}."),
            layers(format!("DHCP {}", d.op.name())),
            egress,
        );
    }

    /// The first free address in a pool, or the one this client already has.
    fn allocate(
        &self,
        server: DeviceId,
        pool: &netrace_doc::DhcpPool,
        config: &netrace_doc::DeviceConfig,
        mac: MacAddr,
    ) -> Option<Ipv4Addr> {
        let empty = BTreeMap::new();
        let bindings = self.dhcp_bindings.get(&server).unwrap_or(&empty);
        if let Some(b) = bindings.get(&mac) {
            return Some(b.address);
        }
        let (network, prefix) = pool.network?;
        let first = u32::from(network) + 1;
        let last = (u32::from(network) | !u32::from(mask(prefix))).checked_sub(1)?;
        let own: Vec<Ipv4Addr> = config
            .interfaces
            .values()
            .filter_map(|i| i.address.map(|a| a.address))
            .collect();
        let taken = |ip: Ipv4Addr| {
            own.contains(&ip)
                || config.dhcp_excluded(ip)
                || pool.default_router == Some(ip)
                || bindings.values().any(|b| b.address == ip)
                || self
                    .dhcp_offers
                    .iter()
                    .any(|((s, m), a)| *s == server && *m != mac && *a == ip)
        };
        (first..=last).map(Ipv4Addr::from).find(|ip| !taken(*ip))
    }

    /// The relay agent passes the server's Offer / ACK to the client's network.
    #[allow(clippy::too_many_arguments)]
    fn relay_to_client(
        &mut self,
        now: SimTime,
        id: DeviceId,
        stack: &Stack,
        arrival: Arrival,
        frame: Frame,
        packet: Ipv4Packet,
        udp: Udp,
    ) {
        let name = self.name(id);
        let Some(out_iface) = stack.owner_of(udp.dhcp.relay).cloned() else {
            return;
        };
        let out = Frame {
            src: out_iface.mac,
            dst: MacAddr::BROADCAST,
            vlan: None,
            payload: Payload::Ipv4(Ipv4Packet {
                src: udp.dhcp.relay,
                dst: Ipv4Addr::BROADCAST,
                ttl: 255,
                l4: L4::Udp(Udp {
                    src_port: DHCP_SERVER_PORT,
                    dst_port: DHCP_CLIENT_PORT,
                    dhcp: udp.dhcp,
                }),
            }),
        };
        let egress = self.send_raw(id, &out_iface, out);
        let layers = vec![
            explain::physical(format!(
                "Arrived on {}; sent out {}.",
                abbreviate_port(&arrival.port),
                abbreviate_port(&out_iface.name)
            )),
            explain::data_link(
                LayerRole::Changed,
                "Broadcast on the client's network.",
                &out,
            ),
            explain::ipv4(
                LayerRole::Changed,
                format!("From {} to the client's network.", udp.dhcp.relay),
                &packet,
            ),
            explain::message(LayerRole::Used, "Passed on unchanged.", &packet),
        ];
        let summary = format!(
            "{name} is the relay agent for this client: it passes the server's DHCP {} on to the client's network on {}.",
            udp.dhcp.op.name(),
            abbreviate_port(&out_iface.name)
        );
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            Outcome::Forwarded,
            summary,
            layers,
            egress,
        );
    }

    /// Puts a ready-made frame on the wire from an interface.
    fn send_raw(&mut self, id: DeviceId, iface: &Iface, frame: Frame) -> Vec<String> {
        self.stack_transmit_frame(id, iface, frame)
    }

    /// Leases a server has handed out (`show ip dhcp binding`).
    pub fn dhcp_bindings(&self, server: DeviceId) -> Vec<DhcpBinding> {
        let mut rows: Vec<DhcpBinding> = self
            .dhcp_bindings
            .get(&server)
            .map(|b| b.values().cloned().collect())
            .unwrap_or_default();
        rows.sort_by_key(|b| u32::from(b.address));
        rows
    }

    /// The address an end device has right now (static, leased or APIPA).
    pub fn current_address(&self, host: DeviceId) -> Option<(Ipv4Addr, u8, Option<Ipv4Addr>)> {
        let stack = self.stack(host)?;
        let iface = stack.ifaces.first()?;
        let a = iface.address?;
        let gateway = stack
            .routes
            .iter()
            .find(|r| r.is_default())
            .and_then(|r| r.next_hop);
        Some((a.address, a.prefix_len, gateway))
    }

    /// The DHCP lease an end device holds, if any.
    pub fn held_lease(&self, host: DeviceId) -> Option<&Dhcp> {
        self.dhcp_leases_held.get(&host)
    }
}

fn client_frame(mac: MacAddr, dhcp: Dhcp) -> Frame {
    Frame {
        src: mac,
        dst: MacAddr::BROADCAST,
        vlan: None,
        payload: Payload::Ipv4(Ipv4Packet {
            src: Ipv4Addr::UNSPECIFIED,
            dst: Ipv4Addr::BROADCAST,
            ttl: 128,
            l4: L4::Udp(Udp {
                src_port: DHCP_CLIENT_PORT,
                dst_port: DHCP_SERVER_PORT,
                dhcp,
            }),
        }),
    }
}

fn ip_of(frame: &Frame) -> &Ipv4Packet {
    match &frame.payload {
        Payload::Ipv4(ip) => ip,
        Payload::Arp(_) => unreachable!("DHCP frames carry IPv4"),
    }
}
