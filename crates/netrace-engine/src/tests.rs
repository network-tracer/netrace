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
fn switching_loops_stop_with_a_warning() {
    let mut p = Project::new();
    let a = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let b = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let pc = p.add_device(DeviceKind::Pc, Point::default()).id;
    p.set_host_ip(pc, Some(HostIp::new(ip("10.0.0.1"), 24, None).unwrap()))
        .unwrap();
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
    let mut net = Network::new(p);
    net.ping(pc, ip("10.0.0.2"), 1, SimTime::ZERO).unwrap();
    assert!(!net.warnings().is_empty());
}

#[test]
fn routers_drop_frames_until_m3_and_say_so() {
    let mut p = Project::new();
    let r = p.add_device(DeviceKind::Router, Point::default()).id;
    let pc = p.add_device(DeviceKind::Pc, Point::default()).id;
    p.set_host_ip(
        pc,
        Some(HostIp::new(ip("10.0.0.2"), 24, Some(ip("10.0.0.1"))).unwrap()),
    )
    .unwrap();
    p.connect(
        end(r, "GigabitEthernet0/0"),
        end(pc, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();
    let mut net = Network::new(p);
    net.ping(pc, ip("10.0.0.1"), 1, SimTime::ZERO).unwrap();
    let at_router = net.trace().iter().find(|e| e.device == r).unwrap();
    assert_eq!(at_router.outcome, Outcome::Dropped);
    assert!(at_router.summary.contains("M3"));
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
            .contains("not recognized")
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
