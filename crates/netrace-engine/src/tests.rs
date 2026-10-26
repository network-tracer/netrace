use std::net::Ipv4Addr;
use std::time::Duration;

use netrace_doc::{DeviceId, DeviceKind, Endpoint, HostIp, Medium, Point, Project};
use netrace_sim::SimTime;

use crate::{Network, Outcome, PingFailure, TraceEvent};

fn ip(s: &str) -> Ipv4Addr {
    s.parse().unwrap()
}

fn end(device: DeviceId, port: &str) -> Endpoint {
    Endpoint {
        device,
        port: port.into(),
    }
}

/// SW1 with PC1 (Fa0/1), PC2 (Fa0/2) and PC3 (Fa0/3), all in 192.168.1.0/24.
fn office() -> (Project, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut p = Project::new();
    let sw = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let mut pcs = Vec::new();
    for n in 1..=3u8 {
        let pc = p.add_device(DeviceKind::Pc, Point::default()).id;
        let cfg = HostIp::new(
            ip(&format!("192.168.1.{}", 9 + n)),
            24,
            Some(ip("192.168.1.1")),
        )
        .unwrap();
        p.set_host_ip(pc, Some(cfg)).unwrap();
        p.connect(
            end(sw, &format!("FastEthernet0/{n}")),
            end(pc, "FastEthernet0"),
            Medium::Copper,
        )
        .unwrap();
        pcs.push(pc);
    }
    (p, sw, pcs[0], pcs[1], pcs[2])
}

fn outcomes_at(trace: &[TraceEvent], device: DeviceId) -> Vec<Outcome> {
    trace
        .iter()
        .filter(|e| e.device == device)
        .map(|e| e.outcome)
        .collect()
}

#[test]
fn first_ping_resolves_arp_then_echoes() {
    let (p, sw, pc1, pc2, pc3) = office();
    let mut net = Network::new(p);
    let s = net.ping(pc1, ip("192.168.1.11"), 1, SimTime::ZERO).unwrap();
    let session = net.session(s).unwrap();
    assert_eq!(session.replies.len(), 1, "{:#?}", net.trace());
    // Two hops each way (4 ms), and this first ping also waited 4 ms for ARP,
    // just as a real first ping includes ARP resolution time.
    let (rtt, ttl) = session.replies[&1];
    assert_eq!(rtt, Duration::from_millis(8));
    assert_eq!(ttl, 128);

    let trace = net.trace();
    // PC1: ARP request, learns reply, sends echo, gets reply.
    assert_eq!(
        outcomes_at(trace, pc1),
        [
            Outcome::Created,
            Outcome::Accepted,
            Outcome::Created,
            Outcome::Completed
        ]
    );
    // SW1 floods the broadcast, then forwards three known unicasts.
    assert_eq!(
        outcomes_at(trace, sw),
        [
            Outcome::Flooded,
            Outcome::Forwarded,
            Outcome::Forwarded,
            Outcome::Forwarded
        ]
    );
    assert_eq!(
        outcomes_at(trace, pc2),
        [Outcome::Replied, Outcome::Replied]
    );
    // PC3 only sees the broadcast, and ignores it.
    assert_eq!(outcomes_at(trace, pc3), [Outcome::Ignored]);
    // Every step of this ping is tagged with its session.
    assert!(trace.iter().all(|e| e.session == Some(s)), "{trace:#?}");
}

#[test]
fn explanations_are_plain_and_specific() {
    let (p, sw, pc1, _, pc3) = office();
    let mut net = Network::new(p);
    net.ping(pc1, ip("192.168.1.11"), 1, SimTime::ZERO).unwrap();
    let trace = net.trace();
    let flood = trace.iter().find(|e| e.device == sw).unwrap();
    assert!(flood.summary.contains("learns that"), "{}", flood.summary);
    assert!(flood.summary.contains("broadcast"), "{}", flood.summary);
    assert!(
        flood.summary.contains("Fa0/2 and Fa0/3"),
        "{}",
        flood.summary
    );
    let ignored = trace.iter().find(|e| e.device == pc3).unwrap();
    assert!(
        ignored.summary.contains("not PC3's address 192.168.1.12"),
        "{}",
        ignored.summary
    );
    // A switch never reads past layer 2.
    assert!(
        flood
            .layers
            .iter()
            .any(|l| l.layer == crate::Layer::Network && l.role == crate::LayerRole::NotUsed)
    );
}

#[test]
fn second_ping_reuses_the_arp_cache() {
    let (p, _, pc1, _, _) = office();
    let mut net = Network::new(p);
    let s = net.ping(pc1, ip("192.168.1.11"), 2, SimTime::ZERO).unwrap();
    let session = net.session(s).unwrap();
    assert_eq!(session.replies.len(), 2);
    let arp_frames = net
        .trace()
        .iter()
        .filter(|e| e.device == pc1 && e.outcome == Outcome::Created)
        .filter(|e| matches!(e.frame.map(|f| f.payload), Some(crate::Payload::Arp(_))))
        .count();
    assert_eq!(arp_frames, 1);
    let report = net.ping_report(s).unwrap();
    assert!(
        report.contains("Reply from 192.168.1.11: bytes=32 time=4ms TTL=128"),
        "{report}"
    );
    assert!(
        report.contains("Sent = 2, Received = 2, Lost = 0 (0% loss)"),
        "{report}"
    );
}

#[test]
fn replays_are_deterministic() {
    let (p, _, pc1, _, pc3) = office();
    let mut a = Network::new(p.clone());
    let mut b = Network::new(p);
    for net in [&mut a, &mut b] {
        net.ping(pc1, ip("192.168.1.12"), 1, SimTime::ZERO).unwrap();
        net.ping(pc3, ip("192.168.1.10"), 1, SimTime::from_millis(500))
            .unwrap();
    }
    assert_eq!(a.trace(), b.trace());
}

#[test]
fn failures_are_explained() {
    let (mut p, _, pc1, pc2, _) = office();
    p.set_host_ip(pc2, None).unwrap();
    let mut net = Network::new(p);

    // Nobody has this address: ARP goes unanswered.
    let s = net.ping(pc1, ip("192.168.1.50"), 1, SimTime::ZERO).unwrap();
    assert!(matches!(
        net.session(s).unwrap().failures[&1],
        PingFailure::Unreachable(_)
    ));
    assert!(
        net.trace()
            .iter()
            .any(|e| e.summary.contains("Nobody answered"))
    );

    // Another network, no route: PC1 has a gateway but nothing answers for it.
    let s = net
        .ping(pc1, ip("10.0.0.1"), 1, SimTime::from_secs(5))
        .unwrap();
    assert!(!net.session(s).unwrap().failures.is_empty());

    // The source has no IP address.
    let s = net
        .ping(pc2, ip("192.168.1.10"), 1, SimTime::from_secs(10))
        .unwrap();
    match &net.session(s).unwrap().failures[&1] {
        PingFailure::Unreachable(reason) => assert!(reason.contains("no IP address"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn no_gateway_means_no_off_network_pings() {
    let mut p = Project::new();
    let sw = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let pc = p.add_device(DeviceKind::Pc, Point::default()).id;
    p.set_host_ip(pc, Some(HostIp::new(ip("192.168.1.10"), 24, None).unwrap()))
        .unwrap();
    p.connect(
        end(sw, "FastEthernet0/1"),
        end(pc, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    let mut net = Network::new(p);
    let s = net.ping(pc, ip("8.8.8.8"), 1, SimTime::ZERO).unwrap();
    match &net.session(s).unwrap().failures[&1] {
        PingFailure::Unreachable(reason) => {
            assert!(reason.contains("no default gateway"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn hubs_repeat_everything_and_hosts_filter_by_mac() {
    let mut p = Project::new();
    let hub = p.add_device(DeviceKind::Hub, Point::default()).id;
    let mut pcs = Vec::new();
    for n in 1..=3u8 {
        let pc = p.add_device(DeviceKind::Pc, Point::default()).id;
        p.set_host_ip(
            pc,
            Some(HostIp::new(ip(&format!("10.0.0.{n}")), 24, None).unwrap()),
        )
        .unwrap();
        p.connect(
            end(hub, &format!("Port{n}")),
            end(pc, "FastEthernet0"),
            Medium::Copper,
        )
        .unwrap();
        pcs.push(pc);
    }
    let mut net = Network::new(p);
    let s = net.ping(pcs[0], ip("10.0.0.2"), 1, SimTime::ZERO).unwrap();
    assert_eq!(net.session(s).unwrap().replies.len(), 1);
    // Unicast frames still reach PC3 through the hub, which discards them at layer 2.
    let pc3_ignored = net
        .trace()
        .iter()
        .filter(|e| e.device == pcs[2] && e.outcome == Outcome::Ignored)
        .count();
    assert!(pc3_ignored >= 3, "{:#?}", net.trace());
    assert!(
        net.trace()
            .iter()
            .filter(|e| e.device == hub)
            .all(|e| e.outcome == Outcome::Flooded)
    );
}

#[test]
fn spanning_tree_breaks_switching_loops() {
    let mut p = Project::new();
    let a = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let b = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let pc = p.add_device(DeviceKind::Pc, Point::default()).id;
    let pc2 = p.add_device(DeviceKind::Pc, Point::default()).id;
    for (host, address) in [(pc, "10.0.0.1"), (pc2, "10.0.0.2")] {
        p.set_host_ip(host, Some(HostIp::new(ip(address), 24, None).unwrap()))
            .unwrap();
    }
    p.connect(
        end(a, "FastEthernet0/1"),
        end(b, "FastEthernet0/1"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(
        end(a, "FastEthernet0/2"),
        end(b, "FastEthernet0/2"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(
        end(a, "FastEthernet0/3"),
        end(pc, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(
        end(b, "FastEthernet0/3"),
        end(pc2, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    let l2 = crate::Layer2::for_project(&p);
    let tree = &l2.trees[&1];
    // The lower bridge ID (same priority, lower MAC) is root: switch a.
    assert_eq!(tree.root_device, a);
    let blocked: Vec<_> = tree
        .ports
        .iter()
        .filter(|(_, port)| !port.role.forwards())
        .map(|(k, _)| k.clone())
        .collect();
    assert_eq!(blocked, [(b, "FastEthernet0/2".to_string())]);
    assert_eq!(
        tree.bridges[&b].root_port.as_deref(),
        Some("FastEthernet0/1")
    );

    let mut net = Network::new(p);
    let s = net.ping(pc, ip("10.0.0.2"), 2, SimTime::ZERO).unwrap();
    assert!(net.warnings().is_empty());
    assert_eq!(net.session(s).unwrap().replies.len(), 2);
    assert!(
        net.trace()
            .iter()
            .any(|e| e.summary.contains("blocking for VLAN 1"))
    );
}

#[test]
fn pc_commands() {
    let (p, _, pc1, _, _) = office();
    let mut net = Network::new(p);
    let out = net.host_command(pc1, "ipconfig", SimTime::ZERO);
    assert!(
        out.contains("192.168.1.10")
            && out.contains("255.255.255.0")
            && out.contains("192.168.1.1"),
        "{out}"
    );
    assert_eq!(
        net.host_command(pc1, "arp -a", SimTime::ZERO),
        "No ARP Entries Found."
    );
    let out = net.host_command(pc1, "ping 192.168.1.12", SimTime::ZERO);
    assert!(out.contains("Received = 4"), "{out}");
    assert!(
        net.host_command(pc1, "arp -a", SimTime::ZERO)
            .contains("192.168.1.12")
    );
    assert!(
        net.host_command(pc1, "ping nowhere", SimTime::ZERO)
            .contains("could not find host")
    );
    assert!(
        net.host_command(pc1, "traceroute 1.2.3.4", SimTime::ZERO)
            .contains("not recognized"),
        "Windows calls it tracert"
    );
}

#[test]
fn hop_timing_is_recorded_for_animation() {
    let (p, sw, pc1, _, _) = office();
    let mut net = Network::new(p);
    net.ping(pc1, ip("192.168.1.11"), 1, SimTime::ZERO).unwrap();
    let first_at_switch = net.trace().iter().find(|e| e.device == sw).unwrap();
    assert_eq!(first_at_switch.from, Some(pc1));
    assert_eq!(first_at_switch.sent, Some(SimTime::ZERO));
    assert_eq!(first_at_switch.time, SimTime::from_millis(1));
}

/// Prints a full trace for reviewing the wording:
/// `cargo test -p netrace-engine show_trace -- --ignored --nocapture`
#[test]
#[ignore]
fn show_trace() {
    let (p, _, pc1, _, _) = office();
    let mut net = Network::new(p);
    net.ping(pc1, ip("192.168.1.11"), 1, SimTime::ZERO).unwrap();
    for e in net.trace() {
        println!(
            "[{}] {:?} @ {}\n  {}",
            e.time, e.outcome, e.device, e.summary
        );
        for l in &e.layers {
            println!("    L{} {:?}: {}", l.layer.number(), l.role, l.note);
        }
    }
}

// MARK: Routing (M3)

use netrace_doc::{DeviceConfig, InterfaceAddress, RouteTarget, StaticRoute};

fn addr(s: &str, prefix_len: u8) -> InterfaceAddress {
    InterfaceAddress {
        address: ip(s),
        prefix_len,
    }
}

/// Gives a router interface an address and brings it up.
fn router_port(p: &mut Project, r: DeviceId, port: &str, address: &str, prefix: u8) {
    let mut config = p.device(r).unwrap().config.clone();
    let iface = config.interface_mut(DeviceKind::Router, port);
    iface.address = Some(addr(address, prefix));
    iface.shutdown = false;
    p.set_device_config(r, config).unwrap();
}

fn add_route(
    p: &mut Project,
    r: DeviceId,
    network: &str,
    prefix: u8,
    next_hop: &str,
    distance: u8,
) {
    let mut config = p.device(r).unwrap().config.clone();
    config.routes.push(StaticRoute {
        network: ip(network),
        prefix_len: prefix,
        target: RouteTarget::NextHop(ip(next_hop)),
        distance,
    });
    p.set_device_config(r, config).unwrap();
}

fn host(p: &mut Project, address: &str, gateway: &str) -> DeviceId {
    let pc = p.add_device(DeviceKind::Pc, Point::default()).id;
    p.set_host_ip(
        pc,
        Some(HostIp::new(ip(address), 24, Some(ip(gateway))).unwrap()),
    )
    .unwrap();
    pc
}

/// PC1 (192.168.1.10) — Gi0/0 R1 Gi0/1 — PC2 (192.168.2.10), router ports
/// addressed but still shut down, as a new router's are.
fn two_lans() -> (Project, DeviceId, DeviceId, DeviceId) {
    let mut p = Project::new();
    let r1 = p.add_device(DeviceKind::Router, Point::default()).id;
    let pc1 = host(&mut p, "192.168.1.10", "192.168.1.1");
    let pc2 = host(&mut p, "192.168.2.10", "192.168.2.1");
    p.connect(
        end(r1, "GigabitEthernet0/0"),
        end(pc1, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(
        end(r1, "GigabitEthernet0/1"),
        end(pc2, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    let mut config = DeviceConfig::default();
    config
        .interface_mut(DeviceKind::Router, "GigabitEthernet0/0")
        .address = Some(addr("192.168.1.1", 24));
    config
        .interface_mut(DeviceKind::Router, "GigabitEthernet0/1")
        .address = Some(addr("192.168.2.1", 24));
    p.set_device_config(r1, config).unwrap();
    (p, r1, pc1, pc2)
}

fn no_shutdown(p: &mut Project, r: DeviceId) {
    let mut config = p.device(r).unwrap().config.clone();
    for iface in config.interfaces.values_mut() {
        iface.shutdown = false;
    }
    p.set_device_config(r, config).unwrap();
}

#[test]
fn shut_router_ports_explain_themselves() {
    let (p, r1, pc1, _) = two_lans();
    let mut net = Network::new(p);
    let s = net.ping(pc1, ip("192.168.2.10"), 1, SimTime::ZERO).unwrap();
    assert!(net.session(s).unwrap().replies.is_empty());
    let at_router = net.trace().iter().find(|e| e.device == r1).unwrap();
    assert_eq!(at_router.outcome, Outcome::Dropped);
    assert!(
        at_router.summary.contains("no shutdown"),
        "{}",
        at_router.summary
    );
}

#[test]
fn routing_between_two_lans() {
    let (mut p, r1, pc1, pc2) = two_lans();
    no_shutdown(&mut p, r1);
    let mut net = Network::new(p);
    let s = net.ping(pc1, ip("192.168.2.10"), 4, SimTime::ZERO).unwrap();
    let session = net.session(s).unwrap();
    // The router drops the first packet while it ARPs for PC2, as IOS does.
    assert!(
        matches!(session.failures.get(&1), Some(PingFailure::TimedOut)),
        "{:#?}",
        net.trace()
    );
    assert_eq!(session.replies.len(), 3);
    // One router on the way: the reply's TTL is one lower.
    assert_eq!(session.replies[&2].1, 127);
    let report = net.session_report(s).unwrap();
    assert!(report.contains("Request timed out."), "{report}");
    assert!(
        report.contains("Reply from 192.168.2.10: bytes=32"),
        "{report}"
    );

    let routed = net
        .trace()
        .iter()
        .find(|e| e.device == r1 && e.outcome == Outcome::Forwarded)
        .expect("R1 forwards");
    assert!(
        routed.summary.contains("directly connected on Gi0/1"),
        "{}",
        routed.summary
    );
    assert!(routed.summary.contains("TTL to 127"), "{}", routed.summary);
    assert!(
        routed
            .layers
            .iter()
            .any(|l| l.layer == crate::Layer::Network && l.role == crate::LayerRole::Changed)
    );
    let dropped_for_arp = net
        .trace()
        .iter()
        .find(|e| e.device == r1 && e.outcome == Outcome::Dropped)
        .unwrap();
    assert!(
        dropped_for_arp
            .summary
            .contains("first ping across a router")
    );
    let _ = pc2;
}

#[test]
fn routers_answer_pings_to_any_of_their_addresses() {
    let (mut p, r1, pc1, _) = two_lans();
    no_shutdown(&mut p, r1);
    let mut net = Network::new(p);
    let near = net.ping(pc1, ip("192.168.1.1"), 2, SimTime::ZERO).unwrap();
    assert_eq!(
        net.session(near).unwrap().replies.len(),
        2,
        "{:#?}",
        net.trace()
    );
    assert_eq!(net.session(near).unwrap().replies[&1].1, 255);
    let far = net
        .ping(pc1, ip("192.168.2.1"), 1, SimTime::from_secs(5))
        .unwrap();
    assert_eq!(net.session(far).unwrap().replies.len(), 1);
}

/// PC1 — R1 — (10.0.0.0/30) — R2 — PC2.
fn two_routers(with_routes: bool) -> (Project, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut p = Project::new();
    let r1 = p.add_device(DeviceKind::Router, Point::default()).id;
    let r2 = p.add_device(DeviceKind::Router, Point::default()).id;
    let pc1 = host(&mut p, "192.168.1.10", "192.168.1.1");
    let pc2 = host(&mut p, "192.168.2.10", "192.168.2.1");
    p.connect(
        end(r1, "GigabitEthernet0/0"),
        end(pc1, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(
        end(r1, "GigabitEthernet0/1"),
        end(r2, "GigabitEthernet0/1"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(
        end(r2, "GigabitEthernet0/0"),
        end(pc2, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    router_port(&mut p, r1, "GigabitEthernet0/0", "192.168.1.1", 24);
    router_port(&mut p, r1, "GigabitEthernet0/1", "10.0.0.1", 30);
    router_port(&mut p, r2, "GigabitEthernet0/1", "10.0.0.2", 30);
    router_port(&mut p, r2, "GigabitEthernet0/0", "192.168.2.1", 24);
    if with_routes {
        add_route(&mut p, r1, "192.168.2.0", 24, "10.0.0.2", 1);
        add_route(&mut p, r2, "0.0.0.0", 0, "10.0.0.1", 1);
    }
    (p, r1, r2, pc1, pc2)
}

#[test]
fn missing_routes_send_unreachable() {
    let (p, r1, _, pc1, _) = two_routers(false);
    let mut net = Network::new(p);
    let s = net.ping(pc1, ip("192.168.2.10"), 1, SimTime::ZERO).unwrap();
    let report = net.session_report(s).unwrap();
    assert!(
        report.contains("Reply from 192.168.1.1: Destination net unreachable."),
        "{report}\n{:#?}",
        net.trace()
    );
    assert!(
        net.trace()
            .iter()
            .any(|e| e.device == r1 && e.summary.contains("has no route to 192.168.2.10"))
    );
}

#[test]
fn static_and_default_routes_carry_traffic() {
    let (p, r1, r2, pc1, _) = two_routers(true);
    let table = crate::routing_table(&p, r2);
    assert!(
        table
            .iter()
            .any(|r| r.is_default() && r.next_hop == Some(ip("10.0.0.1")))
    );
    assert!(
        table
            .iter()
            .any(|r| r.source == crate::RouteSource::Local && r.network == ip("10.0.0.2"))
    );
    let mut net = Network::new(p);
    let s = net.ping(pc1, ip("192.168.2.10"), 5, SimTime::ZERO).unwrap();
    let session = net.session(s).unwrap();
    assert!(
        session.replies.len() >= 3,
        "{}",
        net.session_report(s).unwrap()
    );
    assert_eq!(session.replies.values().last().unwrap().1, 126);
    assert!(net.trace().iter().any(|e| {
        e.device == r1
            && e.summary
                .contains("a static route sends 192.168.2.0/24 to 10.0.0.2 via Gi0/1")
    }));
    assert!(
        net.trace()
            .iter()
            .any(|e| e.device == r2 && e.summary.contains("default route"))
    );
}

#[test]
fn tracert_lists_every_router() {
    let (p, _, _, pc1, _) = two_routers(true);
    let mut net = Network::new(p);
    // Warm up ARP everywhere so each hop answers all three probes.
    net.ping(pc1, ip("192.168.2.10"), 2, SimTime::ZERO).unwrap();
    let out = net.host_command(pc1, "tracert 192.168.2.10", SimTime::from_secs(10));
    let lines: Vec<&str> = out
        .lines()
        .filter(|l| l.trim_start().starts_with(char::is_numeric))
        .collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert!(lines[0].ends_with("192.168.1.1"), "{out}");
    assert!(lines[1].ends_with("10.0.0.2"), "{out}");
    assert!(lines[2].ends_with("192.168.2.10"), "{out}");
    assert!(out.contains("Trace complete."), "{out}");
    assert!(
        net.trace()
            .iter()
            .any(|e| e.summary.contains("Traceroute uses exactly this"))
    );
}

#[test]
fn router_pings_look_like_ios() {
    let (p, r1, _, _, _) = two_routers(true);
    let mut net = Network::new(p);
    let s = net.ping(r1, ip("192.168.2.10"), 5, SimTime::ZERO).unwrap();
    let out = net.session_report(s).unwrap();
    // R1 drops the first while it ARPs for R2, R2 drops the second while it ARPs for PC2.
    assert!(
        out.contains("Sending 5, 100-byte ICMP Echos to 192.168.2.10"),
        "{out}"
    );
    assert!(
        out.contains("..!!!") || out.contains(".!!!!"),
        "{out}\n{:#?}",
        net.trace()
    );
    assert!(out.contains("Success rate is"), "{out}");
    let again = net
        .ping(r1, ip("192.168.2.10"), 5, SimTime::from_secs(20))
        .unwrap();
    assert!(net.session_report(again).unwrap().contains("!!!!!"));
}

#[test]
fn floating_static_waits_in_reserve() {
    let (mut p, r1, ..) = two_routers(true);
    add_route(&mut p, r1, "192.168.2.0", 24, "192.168.1.99", 200);
    let table = crate::routing_table(&p, r1);
    let to_lan: Vec<_> = table
        .iter()
        .filter(|r| r.network == ip("192.168.2.0"))
        .collect();
    assert_eq!(to_lan.len(), 1);
    assert_eq!(to_lan[0].next_hop, Some(ip("10.0.0.2")));
}

#[test]
fn switch_management_interface_answers_pings() {
    let (mut p, sw, pc1, _, _) = office();
    let mut config = DeviceConfig::default();
    let vlan1 = config.interface_mut(DeviceKind::L2Switch, "Vlan1");
    vlan1.address = Some(addr("192.168.1.2", 24));
    vlan1.shutdown = false;
    p.set_device_config(sw, config).unwrap();
    let rows = crate::interface_rows(&p, sw);
    let vlan = rows.iter().find(|r| r.name == "Vlan1").unwrap();
    assert!(vlan.is_up(), "{vlan:?}");
    assert_eq!(
        rows.iter()
            .find(|r| r.name == "FastEthernet0/1")
            .unwrap()
            .status,
        "up"
    );
    assert_eq!(
        rows.iter()
            .find(|r| r.name == "FastEthernet0/9")
            .unwrap()
            .status,
        "down"
    );

    let mut net = Network::new(p);
    let s = net.ping(pc1, ip("192.168.1.2"), 2, SimTime::ZERO).unwrap();
    assert_eq!(
        net.session(s).unwrap().replies.len(),
        2,
        "{:#?}",
        net.trace()
    );
    assert!(net.mac_rows(sw).iter().any(|r| r.port == "FastEthernet0/1"));
}

#[test]
fn interface_status_follows_cables_and_shutdown() {
    let (mut p, r1, ..) = two_lans();
    let rows = crate::interface_rows(&p, r1);
    let gi0 = rows
        .iter()
        .find(|r| r.name == "GigabitEthernet0/0")
        .unwrap();
    assert_eq!(
        (gi0.status, gi0.protocol),
        ("administratively down", "down")
    );
    no_shutdown(&mut p, r1);
    let rows = crate::interface_rows(&p, r1);
    assert!(
        rows.iter()
            .find(|r| r.name == "GigabitEthernet0/0")
            .unwrap()
            .is_up()
    );
    let gi2 = rows
        .iter()
        .find(|r| r.name == "GigabitEthernet0/2")
        .unwrap();
    assert_eq!(gi2.status, "administratively down");
}

/// `cargo test -p netrace-engine show_routing -- --ignored --nocapture`
#[test]
#[ignore]
fn show_routing() {
    let (p, r1, _, pc1, _) = two_routers(true);
    let mut net = Network::new(p);
    net.ping(pc1, ip("192.168.2.10"), 2, SimTime::ZERO).unwrap();
    println!(
        "{}",
        net.host_command(pc1, "tracert 192.168.2.10", SimTime::from_secs(10))
    );
    let s = net
        .ping(r1, ip("192.168.2.10"), 5, SimTime::from_secs(30))
        .unwrap();
    println!("{}", net.session_report(s).unwrap());
    let t = net
        .traceroute(r1, ip("192.168.2.10"), SimTime::from_secs(40))
        .unwrap();
    println!("{}", net.session_report(t).unwrap());
    for e in net.trace().iter().filter(|e| e.device == r1).take(6) {
        println!("[{}] {:?}: {}", e.time, e.outcome, e.summary);
    }
}

// MARK: Switching (M4)

use netrace_doc::{PortMode, PortSecurity, SwitchportConfig, Violation, VlanConfig, VlanSet};

fn switchport(p: &mut Project, sw: DeviceId, port: &str, edit: impl FnOnce(&mut SwitchportConfig)) {
    let mut config = p.device(sw).unwrap().config.clone();
    let kind = p.device(sw).unwrap().kind;
    edit(&mut config.interface_mut(kind, port).switchport);
    p.set_device_config(sw, config).unwrap();
}

fn vlans(p: &mut Project, sw: DeviceId, ids: &[u16]) {
    let mut config = p.device(sw).unwrap().config.clone();
    for id in ids {
        config.vlans.insert(*id, VlanConfig::default());
    }
    p.set_device_config(sw, config).unwrap();
}

fn access(p: &mut Project, sw: DeviceId, port: &str, vlan: u16) {
    switchport(p, sw, port, |sp| {
        sp.mode = PortMode::Access;
        sp.access_vlan = vlan;
    });
}

fn trunk(p: &mut Project, sw: DeviceId, port: &str) {
    switchport(p, sw, port, |sp| sp.mode = PortMode::Trunk);
}

/// SW1 with PC1, PC2 (VLAN 10) and PC3 (VLAN 20), all in 10.0.0.0/24.
fn vlan_office() -> (Project, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut p = Project::new();
    let sw = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let mut pcs = Vec::new();
    for n in 1..=3u8 {
        let pc = p.add_device(DeviceKind::Pc, Point::default()).id;
        p.set_host_ip(
            pc,
            Some(HostIp::new(ip(&format!("10.0.0.{n}")), 24, None).unwrap()),
        )
        .unwrap();
        p.connect(
            end(sw, &format!("FastEthernet0/{n}")),
            end(pc, "FastEthernet0"),
            Medium::Copper,
        )
        .unwrap();
        pcs.push(pc);
    }
    vlans(&mut p, sw, &[10, 20]);
    access(&mut p, sw, "FastEthernet0/1", 10);
    access(&mut p, sw, "FastEthernet0/2", 10);
    access(&mut p, sw, "FastEthernet0/3", 20);
    (p, sw, pcs[0], pcs[1], pcs[2])
}

#[test]
fn vlans_split_one_switch_into_separate_networks() {
    let (p, sw, pc1, _, pc3) = vlan_office();
    let mut net = Network::new(p);
    let same = net.ping(pc1, ip("10.0.0.2"), 1, SimTime::ZERO).unwrap();
    assert_eq!(net.session(same).unwrap().replies.len(), 1);
    // PC3 is in VLAN 20: never sees PC1's ARP broadcast.
    assert!(net.trace().iter().all(|e| e.device != pc3));
    let other = net
        .ping(pc1, ip("10.0.0.3"), 1, SimTime::from_secs(5))
        .unwrap();
    assert!(net.session(other).unwrap().replies.is_empty());
    let flood = net
        .trace()
        .iter()
        .find(|e| e.device == sw && e.outcome == Outcome::Flooded)
        .unwrap();
    assert!(
        flood.summary.contains("every other port in VLAN 10"),
        "{}",
        flood.summary
    );
    assert!(net.mac_rows(sw).iter().any(|r| r.vlan == 10));
}

#[test]
fn trunks_carry_tagged_vlans_between_switches() {
    let (mut p, sw1, pc1, _, _) = vlan_office();
    let sw2 = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let pc4 = p.add_device(DeviceKind::Pc, Point::default()).id;
    p.set_host_ip(pc4, Some(HostIp::new(ip("10.0.0.4"), 24, None).unwrap()))
        .unwrap();
    p.connect(
        end(sw1, "GigabitEthernet0/1"),
        end(sw2, "GigabitEthernet0/1"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(
        end(sw2, "FastEthernet0/1"),
        end(pc4, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    vlans(&mut p, sw2, &[10]);
    access(&mut p, sw2, "FastEthernet0/1", 10);
    trunk(&mut p, sw1, "GigabitEthernet0/1");
    // Without a trunk on SW2 the tagged frames are refused.
    let mut net = Network::new(p.clone());
    let s = net.ping(pc1, ip("10.0.0.4"), 1, SimTime::ZERO).unwrap();
    assert!(net.session(s).unwrap().replies.is_empty());
    assert!(
        net.trace()
            .iter()
            .any(|e| e.device == sw2 && e.summary.contains("access port"))
    );

    trunk(&mut p, sw2, "GigabitEthernet0/1");
    let mut net = Network::new(p.clone());
    let s = net.ping(pc1, ip("10.0.0.4"), 2, SimTime::ZERO).unwrap();
    assert_eq!(
        net.session(s).unwrap().replies.len(),
        2,
        "{:#?}",
        net.trace()
    );
    let tagged = net
        .trace()
        .iter()
        .find(|e| e.device == sw2 && e.frame.is_some_and(|f| f.vlan == Some(10)))
        .expect("tagged on the trunk");
    assert!(
        tagged
            .layers
            .iter()
            .any(|l| l.fields.iter().any(|f| f.value == "VLAN 10"))
    );

    // A VLAN missing from the allowed list doesn't cross.
    switchport(&mut p, sw2, "GigabitEthernet0/1", |sp| {
        sp.allowed_vlans = VlanSet::parse("20").unwrap()
    });
    let mut net = Network::new(p);
    let s = net.ping(pc1, ip("10.0.0.4"), 1, SimTime::ZERO).unwrap();
    assert!(net.session(s).unwrap().replies.is_empty());
    assert!(
        net.trace()
            .iter()
            .any(|e| e.summary.contains("isn't allowed on the trunk"))
    );
}

#[test]
fn router_on_a_stick_routes_between_vlans() {
    let mut p = Project::new();
    let r1 = p.add_device(DeviceKind::Router, Point::default()).id;
    let sw = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let pc10 = host(&mut p, "192.168.10.10", "192.168.10.1");
    let pc20 = host(&mut p, "192.168.20.10", "192.168.20.1");
    p.connect(
        end(r1, "GigabitEthernet0/0"),
        end(sw, "GigabitEthernet0/1"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(
        end(sw, "FastEthernet0/1"),
        end(pc10, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(
        end(sw, "FastEthernet0/2"),
        end(pc20, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    vlans(&mut p, sw, &[10, 20]);
    access(&mut p, sw, "FastEthernet0/1", 10);
    access(&mut p, sw, "FastEthernet0/2", 20);
    trunk(&mut p, sw, "GigabitEthernet0/1");
    let mut config = DeviceConfig::default();
    config
        .interface_mut(DeviceKind::Router, "GigabitEthernet0/0")
        .shutdown = false;
    for (vlan, address) in [(10, "192.168.10.1"), (20, "192.168.20.1")] {
        let sub = config.interface_mut(DeviceKind::Router, &format!("GigabitEthernet0/0.{vlan}"));
        sub.encapsulation = Some(vlan);
        sub.address = Some(addr(address, 24));
    }
    p.set_device_config(r1, config).unwrap();
    let table = crate::routing_table(&p, r1);
    assert!(
        table.iter().any(
            |r| r.iface == "GigabitEthernet0/0.20" && r.source == crate::RouteSource::Connected
        )
    );

    let mut net = Network::new(p);
    let s = net
        .ping(pc10, ip("192.168.20.10"), 4, SimTime::ZERO)
        .unwrap();
    assert!(
        net.session(s).unwrap().replies.len() >= 3,
        "{}",
        net.session_report(s).unwrap()
    );
    // R1 receives tagged VLAN 10 and sends it back out tagged VLAN 20.
    assert!(net.trace().iter().any(|e| e.device == r1
        && e.outcome == Outcome::Forwarded
        && e.frame.is_some_and(|f| f.vlan == Some(10))));
}

#[test]
fn layer3_switch_routes_between_its_vlan_interfaces() {
    let mut p = Project::new();
    let mls = p.add_device(DeviceKind::L3Switch, Point::default()).id;
    let pc10 = host(&mut p, "192.168.10.10", "192.168.10.1");
    let pc20 = host(&mut p, "192.168.20.10", "192.168.20.1");
    p.connect(
        end(mls, "GigabitEthernet1/0/1"),
        end(pc10, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(
        end(mls, "GigabitEthernet1/0/2"),
        end(pc20, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    vlans(&mut p, mls, &[10, 20]);
    access(&mut p, mls, "GigabitEthernet1/0/1", 10);
    access(&mut p, mls, "GigabitEthernet1/0/2", 20);
    let mut config = p.device(mls).unwrap().config.clone();
    config.ip_routing = true;
    for (vlan, address) in [(10, "192.168.10.1"), (20, "192.168.20.1")] {
        let svi = config.interface_mut(DeviceKind::L3Switch, &format!("Vlan{vlan}"));
        svi.address = Some(addr(address, 24));
        svi.shutdown = false;
    }
    p.set_device_config(mls, config).unwrap();
    let mut net = Network::new(p);
    let s = net
        .ping(pc10, ip("192.168.20.10"), 4, SimTime::ZERO)
        .unwrap();
    assert!(
        net.session(s).unwrap().replies.len() >= 3,
        "{}\n{:#?}",
        net.session_report(s).unwrap(),
        net.trace()
    );
}

#[test]
fn port_security_shuts_a_port_on_a_new_mac() {
    let mut p = Project::new();
    let sw = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let hub = p.add_device(DeviceKind::Hub, Point::default()).id;
    let a = host(&mut p, "10.0.0.1", "10.0.0.254");
    let b = host(&mut p, "10.0.0.2", "10.0.0.254");
    let c = host(&mut p, "10.0.0.3", "10.0.0.254");
    p.connect(
        end(sw, "FastEthernet0/1"),
        end(hub, "Port1"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(end(hub, "Port2"), end(a, "FastEthernet0"), Medium::Copper)
        .unwrap();
    p.connect(end(hub, "Port3"), end(b, "FastEthernet0"), Medium::Copper)
        .unwrap();
    p.connect(
        end(sw, "FastEthernet0/2"),
        end(c, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    switchport(&mut p, sw, "FastEthernet0/1", |sp| {
        sp.mode = PortMode::Access;
        sp.port_security = Some(PortSecurity {
            enabled: true,
            ..PortSecurity::default()
        });
    });
    let mut net = Network::new(p.clone());
    let first = net.ping(a, ip("10.0.0.3"), 1, SimTime::ZERO).unwrap();
    assert_eq!(net.session(first).unwrap().replies.len(), 1);
    let second = net
        .ping(b, ip("10.0.0.3"), 1, SimTime::from_secs(5))
        .unwrap();
    assert!(net.session(second).unwrap().replies.is_empty());
    assert!(
        net.trace()
            .iter()
            .any(|e| e.summary.contains("PSECURE_VIOLATION"))
    );
    assert!(
        net.err_disabled_ports(sw)
            .contains(&"FastEthernet0/1".to_string())
    );

    switchport(&mut p, sw, "FastEthernet0/1", |sp| {
        sp.port_security.as_mut().unwrap().violation = Violation::Restrict;
    });
    let mut net = Network::new(p);
    net.ping(a, ip("10.0.0.3"), 1, SimTime::ZERO).unwrap();
    net.ping(b, ip("10.0.0.3"), 1, SimTime::from_secs(5))
        .unwrap();
    assert!(net.err_disabled_ports(sw).is_empty());
}

#[test]
fn bpdu_guard_disables_an_edge_port_facing_a_switch() {
    let mut p = Project::new();
    let a = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let b = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    p.connect(
        end(a, "FastEthernet0/5"),
        end(b, "FastEthernet0/5"),
        Medium::Copper,
    )
    .unwrap();
    switchport(&mut p, a, "FastEthernet0/5", |sp| {
        sp.portfast = true;
        sp.bpdu_guard = true;
    });
    let l2 = crate::Layer2::for_project(&p);
    assert!(
        l2.err_disabled
            .contains(&(a, "FastEthernet0/5".to_string()))
    );
    let rows = crate::interface_rows(&p, b);
    assert_eq!(
        rows.iter()
            .find(|r| r.name == "FastEthernet0/5")
            .unwrap()
            .status,
        "down"
    );
}

#[test]
fn bridge_priority_picks_the_root() {
    let mut p = Project::new();
    let a = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let b = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    p.connect(
        end(a, "GigabitEthernet0/1"),
        end(b, "GigabitEthernet0/1"),
        Medium::Copper,
    )
    .unwrap();
    let mut config = p.device(b).unwrap().config.clone();
    config.spanning_tree.priorities.insert(1, 4096);
    p.set_device_config(b, config).unwrap();
    let l2 = crate::Layer2::for_project(&p);
    assert_eq!(l2.trees[&1].root_device, b);
    assert_eq!(l2.trees[&1].root.priority, 4097);
}

#[test]
fn cdp_finds_cisco_neighbors_and_lldp_waits_for_lldp_run() {
    let (mut p, r1, r2, pc1, _) = two_routers(true);
    let neighbors = crate::discovery_neighbors(&p, r1, false);
    assert_eq!(neighbors.len(), 1, "PCs don't run CDP");
    assert_eq!(neighbors[0].device, r2);
    assert_eq!(neighbors[0].local_port, "GigabitEthernet0/1");
    assert_eq!(neighbors[0].address, Some(ip("10.0.0.2")));
    assert!(crate::discovery_neighbors(&p, r1, true).is_empty());
    for r in [r1, r2] {
        let mut config = p.device(r).unwrap().config.clone();
        config.lldp_run = true;
        p.set_device_config(r, config).unwrap();
    }
    assert_eq!(crate::discovery_neighbors(&p, r1, true).len(), 1);
    assert!(crate::discovery_neighbors(&p, pc1, false).is_empty());
}

#[test]
fn stp_explains_roles_and_reconverges_when_a_link_fails() {
    let mut p = Project::new();
    let a = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let b = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let first = p
        .connect(
            end(a, "FastEthernet0/1"),
            end(b, "FastEthernet0/1"),
            Medium::Copper,
        )
        .unwrap()
        .id;
    p.connect(
        end(a, "FastEthernet0/2"),
        end(b, "FastEthernet0/2"),
        Medium::Copper,
    )
    .unwrap();
    let l2 = crate::Layer2::for_project(&p);
    let tree = &l2.trees[&1];
    assert!(tree.bridges[&a].reason.contains("lowest bridge ID"));
    let blocked = &tree.ports[&(b, "FastEthernet0/2".to_string())];
    assert!(
        blocked.reason.contains("Blocked (alternate)"),
        "{}",
        blocked.reason
    );
    assert!(
        tree.ports[&(b, "FastEthernet0/1".to_string())]
            .reason
            .contains("Root port")
    );

    let (after, changes) = crate::Layer2::after_link_failure(&p, first, 1);
    assert_eq!(
        after.trees[&1].ports[&(b, "FastEthernet0/2".to_string())].role,
        crate::PortRole::Root
    );
    assert!(
        changes
            .iter()
            .any(|c| c.explanation.contains("goes down with the failed link"))
    );
    let unblocked = changes
        .iter()
        .find(|c| c.port.as_deref() == Some("FastEthernet0/2") && c.device == b)
        .unwrap();
    assert!(
        unblocked
            .explanation
            .contains("stops blocking and becomes the root port"),
        "{}",
        unblocked.explanation
    );
}
