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
