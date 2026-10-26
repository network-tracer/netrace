use std::net::Ipv4Addr;

use netrace_cli::Session;
use netrace_doc::{Endpoint, HostIp, Medium, Project};
use netrace_engine::Network;
use netrace_sim::SimTime;

use crate::{bundled_labs, device_id, evaluate};

/// Plays a lab's solution: `DEVICE: command` lines go to that device's
/// console (looked up by its current name, so `hostname` is followed);
/// `@ip`, `@dhcp` and `@cable` do what the student does in the GUI.
fn solve(project: &mut Project, solution: &str) {
    let mut sessions: Vec<(netrace_doc::DeviceId, Session)> = Vec::new();
    let mut network = Network::new(project.clone());
    for line in solution.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words[0] {
            "@ip" => {
                let id = device_id(project, words[1]).expect(line);
                let (address, prefix) = words[2].split_once('/').expect(line);
                let gateway = words.get(3).map(|g| g.parse::<Ipv4Addr>().expect(line));
                let ip = HostIp::new(
                    address.parse().expect(line),
                    prefix.parse().expect(line),
                    gateway,
                )
                .expect(line);
                project.set_host_ip(id, Some(ip)).expect(line);
            }
            "@dhcp" => {
                let id = device_id(project, words[1]).expect(line);
                project.set_host_dhcp(id, true).expect(line);
            }
            "@cable" => {
                let end = |name: &str, port: &str| Endpoint {
                    device: device_id(project, name).expect(line),
                    port: port.into(),
                };
                let (a, b) = (end(words[1], words[2]), end(words[3], words[4]));
                project.connect(a, b, Medium::Copper).expect(line);
            }
            _ => {
                let (name, command) = line.split_once(':').expect(line);
                let id =
                    device_id(project, name.trim()).unwrap_or_else(|| panic!("no {name}: {line}"));
                let at = sessions
                    .iter()
                    .position(|(d, _)| *d == id)
                    .unwrap_or_else(|| {
                        sessions.push((id, Session::new(id)));
                        sessions.len() - 1
                    });
                let now = network.end_time() + std::time::Duration::from_secs(1);
                let out = sessions[at]
                    .1
                    .execute(command.trim(), project, &mut network, now)
                    .output;
                assert!(
                    !out.contains("% Invalid")
                        && !out.contains("% Incomplete")
                        && !out.contains("% Ambiguous"),
                    "{line}\n{out}"
                );
            }
        }
    }
}

#[test]
fn every_bundled_lab_starts_unsolved_and_its_solution_passes() {
    let labs = bundled_labs();
    assert_eq!(labs.len(), 20);
    for bundled in labs {
        let mut project = bundled.project();
        let lab = bundled.lab();
        assert!(
            !lab.title.is_empty() && !lab.tasks.is_empty(),
            "{}",
            bundled.id
        );
        let before = evaluate(&project);
        assert!(
            before.iter().any(|t| !t.done),
            "{}: already solved before starting",
            bundled.id
        );
        solve(&mut project, bundled.solution);
        let after = evaluate(&project);
        for (task, result) in lab.tasks.iter().zip(&after) {
            if let Some(check) = result.checks.iter().find(|c| !c.passed) {
                panic!(
                    "{}: \"{}\" fails after the solution: {} — {}",
                    bundled.id, task.title, check.label, check.feedback
                );
            }
        }
    }
}

#[test]
fn failing_checks_say_why() {
    let bundled = bundled_labs()
        .into_iter()
        .find(|l| l.id == "03-default-gateway")
        .unwrap();
    let results = evaluate(&bundled.project());
    let first = &results[0].checks;
    assert!(
        first[0].feedback.contains("R1 Gi0/0 has no IP address yet"),
        "{first:?}"
    );
    assert!(first[1].feedback.contains("shut down"), "{first:?}");
    assert_eq!(first[0].label, "R1 Gi0/0 is 192.168.1.1/24");
    let ping = &results[3].checks[0];
    assert_eq!(ping.label, "PC1 can ping PC2");
    assert!(
        ping.feedback.starts_with("PC1 gets no reply from PC2."),
        "{}",
        ping.feedback
    );
    let _ = SimTime::ZERO;
}

#[test]
fn locked_devices_can_be_looked_at_but_not_changed() {
    let bundled = bundled_labs()
        .into_iter()
        .find(|l| l.id == "16-nat-pat")
        .unwrap();
    let mut project = bundled.project();
    for name in &bundled.lab().locked {
        assert!(
            device_id(&project, name).is_some(),
            "{}: locks {name}, which doesn't exist",
            bundled.id
        );
    }
    let isp = device_id(&project, "ISP").unwrap();
    let r1 = device_id(&project, "R1").unwrap();
    assert!(project.is_locked(isp) && !project.is_locked(r1));
    let config = project.device(isp).unwrap().config.clone();
    assert!(matches!(
        project.set_device_config(isp, config),
        Err(netrace_doc::DocError::Locked(name)) if name == "ISP"
    ));
    assert!(project.rename_device(isp, "Other").is_err());
    assert!(project.remove_device(isp).is_err());
    let link = project
        .links()
        .iter()
        .find(|l| l.a.device == isp || l.b.device == isp)
        .unwrap()
        .id;
    assert!(project.disconnect(link).is_err());

    // The console still answers show commands, but won't enter configuration.
    let mut network = Network::new(project.clone());
    let mut session = Session::new(isp);
    let now = SimTime::ZERO;
    session.execute("enable", &mut project, &mut network, now);
    let out = session
        .execute("configure terminal", &mut project, &mut network, now)
        .output;
    assert!(out.contains("locked by the lab"), "{out}");
    let out = session
        .execute("show ip interface brief", &mut project, &mut network, now)
        .output;
    assert!(out.contains("GigabitEthernet0/0"), "{out}");
    // Every bundled lab locks only devices it has.
    for b in bundled_labs() {
        let p = b.project();
        for name in &b.lab().locked {
            assert!(device_id(&p, name).is_some(), "{}: locks {name}", b.id);
        }
    }
}

#[test]
fn the_tour_network_shows_every_feature_working() {
    use netrace_engine::{NeighborState, Outcome, RouteSource, TcpApp};
    let project = Project::from_json(crate::tour_json()).expect("tour loads");
    let id = |name: &str| device_id(&project, name).unwrap();
    let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
    let mut net = Network::new(project.clone());
    assert!(net.warnings().is_empty(), "{:?}", net.warnings());

    // DHCP: workstations in every agency get leases from the core.
    for (host, net_) in [
        ("SOC-1", 10),
        ("SECURE-LINK", 20),
        ("AUDITOR", 30),
        ("GPS-TRACK", 40),
    ] {
        let (a, ..) = net.current_address(id(host)).unwrap();
        assert_eq!(a.octets()[..3], [10, net_, net_], "{host}: {a}");
    }
    // Spanning tree: the core is root, and the SW-IA–SW-FIELD link closes
    // a loop, so one port blocks.
    let tree = &net.layer2().trees[&30];
    let blocked = tree.ports.values().filter(|p| !p.role.forwards()).count();
    assert_eq!(blocked, 1);
    // OSPF: the core and the firewall are adjacent, and the firewall's
    // default route reaches the core.
    for r in ["CORE", "FW"] {
        let full = net
            .ospf()
            .router(id(r))
            .unwrap()
            .neighbors
            .iter()
            .filter(|n| n.state == NeighborState::Full)
            .count();
        assert_eq!(full, 1, "{r}");
    }
    assert!(
        netrace_engine::routing_table(&project, id("CORE"))
            .iter()
            .any(|r| r.is_default() && r.source == RouteSource::OspfExternal2)
    );

    let mut t = net.end_time() + std::time::Duration::from_secs(1);
    let step = std::time::Duration::from_secs(30);
    let replies = |net: &mut Network, from: &str, to: &str, t| {
        let s = net.ping(id(from), ip(to), 6, t).unwrap();
        net.session_report(s)
            .unwrap()
            .contains(&format!("Reply from {to}"))
    };
    // PAT through the firewall: a web request across three cold ARP caches
    // (one SYN lost at each router), then a ping and another request.
    let cold = net
        .connect(id("TAC-TEAM"), ip("198.51.100.10"), 80, TcpApp::Http, t)
        .unwrap();
    let report = net.connection_report(cold).unwrap();
    assert!(report.contains("netrace web server"), "{report}");
    t = t + step;
    assert!(replies(&mut net, "GPS-TRACK", "198.51.100.10", t));
    t = t + step;
    let web = net
        .connect(id("SOC-1"), ip("198.51.100.10"), 80, TcpApp::Http, t)
        .unwrap();
    let report = net.connection_report(web).unwrap();
    assert!(report.contains("netrace web server"), "{report}");
    assert!(
        net.nat_translations(id("FW"))
            .iter()
            .any(|e| e.proto == Some(netrace_engine::NatProto::Tcp))
    );
    t = t + step;
    // Inter-VLAN routing on the core, and its access list: Field Ops can't
    // reach the personnel server; Counterterrorism can.
    assert!(replies(&mut net, "THREAT-MON", "10.30.30.20", t));
    t = t + step;
    assert!(!replies(&mut net, "GPS-TRACK", "10.30.30.20", t));
    t = t + step;
    // SPAN: the Watchers capture Internal Affairs' traffic to the vault.
    let before = net.trace().len();
    assert!(replies(&mut net, "AUDITOR", "10.50.50.10", t));
    let watcher = id("WATCHER");
    let captured = net.trace()[before..]
        .iter()
        .filter(|e| e.device == watcher && e.outcome == Outcome::Accepted)
        .count();
    assert!(captured >= 4, "{captured}");
    // Every kind of frame here (tagged, DHCP, OSPF, BPDUs, TCP) encodes
    // for pcap export to exactly its stated length.
    for e in net.trace() {
        if let Some(frame) = &e.frame {
            assert_eq!(frame.to_bytes().len(), frame.len(), "{}", frame.info());
        }
    }
}
