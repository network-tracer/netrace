//! Minimal TCP: enough for extended ACLs and PAT to have real connections
//! to act on, and for students to see a handshake. A connection is the
//! three-way handshake, one request and one response (an HTTP GET, or
//! nothing for a Telnet test), and the close. Every segment is traced with
//! what it does and why.

use std::net::Ipv4Addr;
use std::time::Duration;

use netrace_doc::{DeviceId, DeviceKind};
use netrace_sim::SimTime;

use crate::explain;
use crate::network::{Arrival, Event};
use crate::stack::{Iface, Stack};
use crate::{Frame, Ipv4Packet, L4, LayerRole, Network, Outcome, Tcp, TcpFlags, abbreviate_port};

/// SYNs go at 0 s, 1 s and 3 s; with no answer by 7 s the connection fails,
/// a shortened version of what Windows does.
const SYN_RETRIES: [u64; 2] = [1, 3];
const GIVE_UP: Duration = Duration::from_secs(7);
/// The first port a client picks for its side, as Windows does.
const EPHEMERAL_BASE: u16 = 49152;

const HTTP_GET: &str = "GET / HTTP/1.1";
const HTTP_OK: &str = "HTTP/1.1 200 OK";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpApp {
    /// `curl`: GET / and read the page.
    Http,
    /// `telnet HOST PORT`: open the connection and close it again.
    Connect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcpResult {
    /// The exchange finished; for HTTP, the server's response line.
    Completed { response: Option<String> },
    /// The server answered RST: nothing listens on that port.
    Refused,
    /// No answer to any SYN.
    TimedOut,
    /// A router sent ICMP back (no route, or an access list stopped it).
    Unreachable { from: Ipv4Addr, prohibited: bool },
}

/// One connection a device opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpConn {
    pub id: u64,
    pub source: DeviceId,
    pub destination: Ipv4Addr,
    pub dst_port: u16,
    pub src_port: u16,
    pub app: TcpApp,
    pub started: SimTime,
    pub established: bool,
    pub result: Option<TcpResult>,
}

impl TcpConn {
    fn isn(&self) -> u32 {
        (self.id as u32 + 1) * 1000
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TcpCommand {
    pub at: SimTime,
    pub source: DeviceId,
    pub destination: Ipv4Addr,
    pub port: u16,
    pub app: TcpApp,
}

/// Well-known port names for explanations.
pub fn service(port: u16) -> &'static str {
    match port {
        21 => "FTP",
        22 => "SSH",
        23 => "Telnet",
        25 => "SMTP",
        53 => "DNS",
        80 => "HTTP",
        443 => "HTTPS",
        _ => "",
    }
}

fn port_text(port: u16) -> String {
    match service(port) {
        "" => format!("port {port}"),
        s => format!("port {port} ({s})"),
    }
}

impl Network {
    /// Opens a TCP connection from an end device at `at`, then replays
    /// everything. Returns the connection's id.
    pub fn connect(
        &mut self,
        source: DeviceId,
        destination: Ipv4Addr,
        port: u16,
        app: TcpApp,
        at: SimTime,
    ) -> Result<u64, crate::EngineError> {
        let device = self
            .project
            .device(source)
            .ok_or(crate::EngineError::UnknownDevice(source))?;
        if !device.kind.is_host() {
            return Err(crate::EngineError::CantSend(device.name.clone()));
        }
        self.tcp_commands.push(TcpCommand {
            at,
            source,
            destination,
            port,
            app,
        });
        self.rebuild();
        Ok(self.tcp_commands.len() as u64 - 1)
    }

    pub fn connection(&self, id: u64) -> Option<&TcpConn> {
        self.tcp_conns.get(usize::try_from(id).ok()?)
    }

    /// The connections, reset and their opening scheduled, for a replay.
    pub(crate) fn schedule_tcp(&mut self) {
        self.tcp_conns = self
            .tcp_commands
            .iter()
            .enumerate()
            .map(|(i, c)| TcpConn {
                id: i as u64,
                source: c.source,
                destination: c.destination,
                dst_port: c.port,
                src_port: EPHEMERAL_BASE + i as u16,
                app: c.app,
                started: c.at,
                established: false,
                result: None,
            })
            .collect();
        for (i, c) in self.tcp_commands.iter().enumerate() {
            let conn = i as u64;
            let _ = self
                .scheduler
                .schedule_at(c.at, Event::TcpSyn { conn, attempt: 0 });
            let _ = self
                .scheduler
                .schedule_at(c.at + GIVE_UP, Event::TcpGiveUp { conn });
        }
    }

    /// Sends (or resends) the SYN that opens a connection.
    pub(crate) fn tcp_syn(&mut self, now: SimTime, conn: u64, attempt: usize) {
        let Some(c) = self.tcp_conns.get(conn as usize).cloned() else {
            return;
        };
        if c.established || c.result.is_some() {
            return;
        }
        let Some(src) = self
            .stack(c.source)
            .and_then(|s| s.ifaces.first())
            .and_then(|i| i.address)
            .map(|a| a.address)
        else {
            let name = self.name(c.source);
            self.record(
                now,
                c.source,
                None,
                None,
                Outcome::Dropped,
                format!("{name} can't open a connection: it has no IP address."),
                Vec::new(),
                Vec::new(),
            );
            self.settle_tcp(conn, TcpResult::TimedOut);
            return;
        };
        let packet = Ipv4Packet {
            src,
            dst: c.destination,
            ttl: crate::stack::HOST_TTL,
            l4: L4::Tcp(Tcp {
                src_port: c.src_port,
                dst_port: c.dst_port,
                flags: TcpFlags::SYN,
                seq: c.isn(),
                ack: 0,
                data: None,
            }),
        };
        self.send_from(now, c.source, packet, None, true);
        if let Some(delay) = SYN_RETRIES.get(attempt) {
            let at = c.started + Duration::from_secs(*delay);
            let _ = self.scheduler.schedule_at(
                at,
                Event::TcpSyn {
                    conn,
                    attempt: attempt + 1,
                },
            );
        }
    }

    pub(crate) fn tcp_give_up(&mut self, now: SimTime, conn: u64) {
        let Some(c) = self.tcp_conns.get(conn as usize).cloned() else {
            return;
        };
        if c.established || c.result.is_some() {
            return;
        }
        let name = self.name(c.source);
        self.record(
            now,
            c.source,
            None,
            None,
            Outcome::Dropped,
            format!(
                "{name} gives up connecting to {} {}: none of its three SYNs got an answer.",
                c.destination,
                port_text(c.dst_port)
            ),
            Vec::new(),
            Vec::new(),
        );
        self.settle_tcp(conn, TcpResult::TimedOut);
    }

    fn settle_tcp(&mut self, conn: u64, result: TcpResult) {
        if let Some(c) = self.tcp_conns.get_mut(conn as usize)
            && c.result.is_none()
        {
            c.result = Some(result);
        }
    }

    /// The connection a segment arriving at `id` belongs to, from the
    /// client's side.
    fn client_conn(&self, id: DeviceId, local_port: u16) -> Option<u64> {
        self.tcp_conns
            .iter()
            .find(|c| c.source == id && c.src_port == local_port)
            .map(|c| c.id)
    }

    /// Whether a device accepts connections on a port: servers run the
    /// usual services, routers and switches answer Telnet.
    fn listens(&self, id: DeviceId, port: u16) -> bool {
        match self.project.device(id).map(|d| d.kind) {
            Some(DeviceKind::Server) => matches!(port, 21 | 22 | 23 | 25 | 80 | 443),
            Some(k) if k.has_cli() => port == 23,
            _ => false,
        }
    }

    /// A TCP segment addressed to this device.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tcp_receive(
        &mut self,
        now: SimTime,
        id: DeviceId,
        stack: &Stack,
        own: &Iface,
        arrival: Arrival,
        frame: Frame,
        packet: Ipv4Packet,
        tcp: Tcp,
    ) {
        let name = self.name(id);
        let port = abbreviate_port(&arrival.port);
        let layers = |note: String| {
            vec![
                explain::physical(format!("Arrived on {port}.")),
                explain::data_link(LayerRole::Used, format!("Addressed to {name}."), &frame),
                explain::ipv4(
                    LayerRole::Used,
                    format!("Destination {} is {name}'s address.", packet.dst),
                    &packet,
                ),
                explain::tcp(LayerRole::Used, note, &tcp),
            ]
        };
        let reply = |flags: TcpFlags, seq: u32, ack: u32, data: Option<&'static str>| Ipv4Packet {
            src: packet.dst,
            dst: packet.src,
            ttl: stack.ttl,
            l4: L4::Tcp(Tcp {
                src_port: tcp.dst_port,
                dst_port: tcp.src_port,
                flags,
                seq,
                ack,
                data,
            }),
        };
        let next = tcp.seq + tcp.data_len() + u32::from(tcp.flags.syn || tcp.flags.fin);
        let _ = own;

        // The client's side of one of its own connections.
        if let Some(conn) = self.client_conn(id, tcp.dst_port) {
            let c = self.tcp_conns[conn as usize].clone();
            let peer = self.who_ip(packet.src);
            if tcp.flags.rst {
                self.record(
                    now,
                    id,
                    Some(&arrival),
                    Some(frame),
                    Outcome::Completed,
                    format!(
                        "{name} gets RST from {peer}: nothing listens on {}, so the connection is refused.",
                        port_text(c.dst_port)
                    ),
                    layers("RST: the other end refuses the connection.".into()),
                    Vec::new(),
                );
                return self.settle_tcp(conn, TcpResult::Refused);
            }
            if tcp.flags.syn && tcp.flags.ack {
                if c.established {
                    return;
                }
                self.tcp_conns[conn as usize].established = true;
                let (what, data, flags) = match c.app {
                    TcpApp::Http => (
                        format!(" and sends its request ({HTTP_GET})"),
                        Some(HTTP_GET),
                        TcpFlags::PSH_ACK,
                    ),
                    TcpApp::Connect => (
                        " and, having tested the connection, closes it".into(),
                        None,
                        TcpFlags::FIN_ACK,
                    ),
                };
                self.record(
                    now,
                    id,
                    Some(&arrival),
                    Some(frame),
                    Outcome::Accepted,
                    format!(
                        "{name} gets SYN-ACK from {peer}: the server accepts. {name} completes the three-way handshake \
                         with an ACK{what}."
                    ),
                    layers("SYN-ACK: the second step of the handshake.".into()),
                    vec![arrival.port.clone()],
                );
                self.send_from(
                    now,
                    id,
                    reply(TcpFlags::ACK, tcp.ack, next, None),
                    None,
                    false,
                );
                self.send_from(now, id, reply(flags, tcp.ack, next, data), None, false);
                return;
            }
            if let Some(response) = tcp.data {
                self.record(
                    now,
                    id,
                    Some(&arrival),
                    Some(frame),
                    Outcome::Accepted,
                    format!(
                        "{name} receives the server's response ({response}) and closes its side with FIN."
                    ),
                    layers(format!("Data: {response}.")),
                    vec![arrival.port.clone()],
                );
                self.settle_tcp(
                    conn,
                    TcpResult::Completed {
                        response: Some(response.to_string()),
                    },
                );
                self.send_from(
                    now,
                    id,
                    reply(TcpFlags::FIN_ACK, tcp.ack, next, None),
                    None,
                    false,
                );
                return;
            }
            if tcp.flags.fin {
                self.record(
                    now,
                    id,
                    Some(&arrival),
                    Some(frame),
                    Outcome::Completed,
                    format!("{name} gets the server's FIN and acknowledges it: the connection is closed."),
                    layers("FIN: the other end closes its side.".into()),
                    vec![arrival.port.clone()],
                );
                if c.app == TcpApp::Connect {
                    self.settle_tcp(conn, TcpResult::Completed { response: None });
                }
                self.send_from(
                    now,
                    id,
                    reply(TcpFlags::ACK, tcp.ack, next, None),
                    None,
                    false,
                );
                return;
            }
            // A bare ACK: nothing to do.
            return self.record(
                now,
                id,
                Some(&arrival),
                Some(frame),
                Outcome::Completed,
                format!("{name} gets an acknowledgement from {peer}."),
                layers("ACK: confirms what was received.".into()),
                Vec::new(),
            );
        }

        // The server's side.
        let client = self.who_ip(packet.src);
        if tcp.flags.syn && !tcp.flags.ack {
            if self.listens(id, tcp.dst_port) {
                self.record(
                    now,
                    id,
                    Some(&arrival),
                    Some(frame),
                    Outcome::Replied,
                    format!(
                        "{name} gets a SYN from {client} for {}. It's listening there, so it answers SYN-ACK: the \
                         second step of the three-way handshake.",
                        port_text(tcp.dst_port)
                    ),
                    layers("SYN: the client asks to open a connection.".into()),
                    vec![arrival.port.clone()],
                );
                let isn = tcp.seq.wrapping_mul(7).wrapping_add(5000);
                self.send_from(
                    now,
                    id,
                    reply(TcpFlags::SYN_ACK, isn, next, None),
                    None,
                    false,
                );
            } else {
                self.record(
                    now,
                    id,
                    Some(&arrival),
                    Some(frame),
                    Outcome::Replied,
                    format!(
                        "{name} gets a SYN from {client} for {}, but nothing on {name} listens there, so it refuses \
                         with RST.",
                        port_text(tcp.dst_port)
                    ),
                    layers("SYN for a closed port.".into()),
                    vec![arrival.port.clone()],
                );
                self.send_from(
                    now,
                    id,
                    reply(TcpFlags::RST_ACK, 0, next, None),
                    None,
                    false,
                );
            }
            return;
        }
        if tcp.flags.fin {
            self.record(
                now,
                id,
                Some(&arrival),
                Some(frame),
                Outcome::Replied,
                format!("{name} gets FIN from {client} and closes its side too (FIN, ACK)."),
                layers("FIN: the client is done.".into()),
                vec![arrival.port.clone()],
            );
            self.send_from(
                now,
                id,
                reply(TcpFlags::FIN_ACK, tcp.ack, next, None),
                None,
                false,
            );
            return;
        }
        if let Some(request) = tcp.data {
            self.record(
                now,
                id,
                Some(&arrival),
                Some(frame),
                Outcome::Replied,
                format!("{name} gets the request ({request}) from {client} and sends its response ({HTTP_OK})."),
                layers(format!("Data: {request}.")),
                vec![arrival.port.clone()],
            );
            self.send_from(
                now,
                id,
                reply(TcpFlags::PSH_ACK, tcp.ack, next, Some(HTTP_OK)),
                None,
                false,
            );
            return;
        }
        self.record(
            now,
            id,
            Some(&arrival),
            Some(frame),
            Outcome::Completed,
            format!(
                "{name} gets {client}'s ACK: the three-way handshake is complete and the connection is open."
            ),
            layers("ACK: the third step of the handshake.".into()),
            Vec::new(),
        );
    }

    /// An ICMP error about one of this device's TCP segments (they're
    /// quoted with ID 0 and the source port as the sequence number).
    pub(crate) fn tcp_error(
        &mut self,
        id: DeviceId,
        local_port: u16,
        from: Ipv4Addr,
        prohibited: bool,
    ) -> bool {
        let Some(conn) = self.client_conn(id, local_port) else {
            return false;
        };
        self.settle_tcp(conn, TcpResult::Unreachable { from, prohibited });
        true
    }

    /// What a PC's `curl` or `telnet` prints for a connection.
    pub fn connection_report(&self, id: u64) -> Option<String> {
        let c = self.connection(id)?;
        let server = self.who_ip(c.destination);
        Some(match (c.app, &c.result) {
            (TcpApp::Http, Some(TcpResult::Completed { .. })) if c.dst_port == 80 => format!(
                "<html><body><h1>{server}</h1><p>netrace web server: the TCP connection to {}:{} worked.</p></body></html>",
                c.destination, c.dst_port
            ),
            (TcpApp::Http, Some(TcpResult::Completed { .. })) => format!(
                "Connected to {}:{} ({server}): the TCP handshake worked, but it isn't a web server.",
                c.destination, c.dst_port
            ),
            (TcpApp::Connect, Some(TcpResult::Completed { .. })) => format!(
                "Connecting To {}...\nConnected to {} port {}. (netrace opens and closes the connection to show TCP; \
                 it doesn't run a remote session.)",
                c.destination, c.destination, c.dst_port
            ),
            (TcpApp::Http, result) => format!(
                "curl: (7) Failed to connect to {} port {}: {}",
                c.destination,
                c.dst_port,
                match result {
                    Some(TcpResult::Refused) => "Connection refused".to_string(),
                    Some(TcpResult::Unreachable {
                        prohibited: true, ..
                    }) => {
                        "Network is unreachable (an access list stopped it)".into()
                    }
                    Some(TcpResult::Unreachable { from, .. }) =>
                        format!("No route to host (reported by {from})"),
                    _ => "Timed out".into(),
                }
            ),
            (TcpApp::Connect, _) => format!(
                "Connecting To {}...Could not open connection to the host, on port {}: Connect failed",
                c.destination, c.dst_port
            ),
        })
    }
}
