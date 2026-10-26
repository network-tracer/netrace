use netrace_doc::{DeviceId, DeviceKind, Endpoint, HostIp, Medium, Point, Project};
use netrace_engine::Network;
use netrace_sim::SimTime;

use crate::{Ctx, Session};

/// R1 Gi0/0 — PC1 (192.168.1.10), R1 Gi0/1 — PC2 (192.168.2.10).
fn lab() -> (Project, DeviceId, DeviceId) {
    let mut p = Project::new();
    let r1 = p.add_device(DeviceKind::Router, Point::default()).id;
    let pc1 = p.add_device(DeviceKind::Pc, Point::default()).id;
    let pc2 = p.add_device(DeviceKind::Pc, Point::default()).id;
    for (pc, port, address, gateway) in [
        (pc1, "GigabitEthernet0/0", "192.168.1.10", "192.168.1.1"),
        (pc2, "GigabitEthernet0/1", "192.168.2.10", "192.168.2.1"),
    ] {
        p.set_host_ip(
            pc,
            Some(
                HostIp::new(address.parse().unwrap(), 24, Some(gateway.parse().unwrap())).unwrap(),
            ),
        )
        .unwrap();
        p.connect(
            Endpoint {
                device: r1,
                port: port.into(),
            },
            Endpoint {
                device: pc,
                port: "FastEthernet0".into(),
            },
            Medium::Copper,
        )
        .unwrap();
    }
    (p, r1, pc1)
}

struct Console {
    session: Session,
    project: Project,
    network: Network,
}

impl Console {
    fn new(project: Project, device: DeviceId) -> Self {
        let network = Network::new(project.clone());
        Console {
            session: Session::new(device),
            project,
            network,
        }
    }

    fn run(&mut self, line: &str) -> String {
        let now = self.network.end_time() + std::time::Duration::from_secs(1);
        self.session
            .execute(line, &mut self.project, &mut self.network, now)
            .output
    }

    fn lines(&mut self, lines: &[&str]) {
        for line in lines {
            let out = self.run(line);
            assert!(
                !out.contains("% Invalid") && !out.contains("% Incomplete"),
                "{line}: {out}"
            );
        }
    }

    fn prompt(&self) -> String {
        self.session.prompt(&self.project)
    }
}

#[test]
fn modes_and_prompts() {
    let (p, r1, _) = lab();
    let mut c = Console::new(p, r1);
    assert_eq!(c.prompt(), "R1>");
    c.run("en");
    assert_eq!(c.prompt(), "R1#");
    assert!(c.run("conf t").contains("Enter configuration commands"));
    assert_eq!(c.prompt(), "R1(config)#");
    c.run("int g0/0");
    assert_eq!(c.prompt(), "R1(config-if)#");
    c.run("int gig 0/1.10");
    assert_eq!(c.prompt(), "R1(config-subif)#");
    c.run("exit");
    assert_eq!(c.prompt(), "R1(config)#");
    c.run("line con 0");
    assert_eq!(c.prompt(), "R1(config-line)#");
    assert!(c.run("end").contains("Configured from console"));
    assert_eq!(c.prompt(), "R1#");
    c.run("disable");
    assert_eq!(c.session.context(), Ctx::User);
}

#[test]
fn errors_look_like_ios() {
    let (p, r1, _) = lab();
    let mut c = Console::new(p, r1);
    // Privileged commands aren't available in user EXEC.
    let out = c.run("show running-config");
    assert!(
        out.contains("% Invalid input detected at '^' marker."),
        "{out}"
    );
    let caret = out.lines().next().unwrap();
    assert_eq!(caret.find('^'), Some("R1>show ".len()), "{out}");
    c.run("enable");
    assert_eq!(c.run("show ip"), "% Incomplete command.\n");
    assert!(
        c.run("co").contains("% Ambiguous command"),
        "configure / copy"
    );
    let out = c.run("blah");
    assert!(
        out.contains("Translating \"blah\"...domain server"),
        "{out}"
    );
    c.lines(&["conf t", "no ip domain-lookup", "end"]);
    let out = c.run("blah");
    assert!(!out.contains("Translating"), "{out}");
}

#[test]
fn help_and_completion() {
    let (p, r1, _) = lab();
    let mut c = Console::new(p, r1);
    c.run("enable");
    let help = c.session.help("show ", &c.project);
    assert!(
        help.contains("running-config") && help.contains("Current operating configuration"),
        "{help}"
    );
    assert!(c.session.help("sh", &c.project).contains("show"));
    let help = c.session.help("show ip interface ", &c.project);
    assert!(help.contains("brief"), "{help}");
    assert_eq!(
        c.session.complete("sh", &c.project).as_deref(),
        Some("show ")
    );
    assert_eq!(
        c.session.complete("show ip int", &c.project).as_deref(),
        Some("show ip interface ")
    );
    assert_eq!(c.session.complete("co", &c.project), None, "ambiguous");
    c.run("conf t");
    let help = c.session.help("interface ", &c.project);
    assert!(
        help.contains("GigabitEthernet") && help.contains("Loopback"),
        "{help}"
    );
    assert_eq!(
        c.session.complete("int gi", &c.project).as_deref(),
        Some("int GigabitEthernet ")
    );
    let help = c.session.help("ip route 10.0.0.0 255.0.0.0 ", &c.project);
    assert!(
        help.contains("A.B.C.D") && help.contains("GigabitEthernet"),
        "{help}"
    );
    // `do` reaches EXEC commands from config mode.
    assert!(c.session.help("do sh", &c.project).contains("show"));
}

#[test]
fn configuring_a_router_makes_it_route() {
    let (p, r1, pc1) = lab();
    let mut c = Console::new(p, r1);
    c.lines(&[
        "enable",
        "configure terminal",
        "hostname Edge",
        "interface g0/0",
        "description LAN A",
        "ip address 192.168.1.1 255.255.255.0",
    ]);
    let out = c.run("no shutdown");
    assert!(
        out.contains("Line protocol on Interface GigabitEthernet0/0, changed state to up"),
        "{out}"
    );
    c.lines(&[
        "int g0/1",
        "ip add 192.168.2.1 255.255.255.0",
        "no shut",
        "end",
    ]);
    assert_eq!(c.prompt(), "Edge#");

    let brief = c.run("sh ip int br");
    assert!(
        brief
            .contains("GigabitEthernet0/0     192.168.1.1     YES manual up                    up"),
        "{brief}"
    );
    assert!(
        brief.contains(
            "GigabitEthernet0/2     unassigned      YES unset  administratively down down"
        ),
        "{brief}"
    );

    let route = c.run("show ip route");
    assert!(
        route.contains("Gateway of last resort is not set"),
        "{route}"
    );
    assert!(
        route.contains("C        192.168.1.0/24 is directly connected, GigabitEthernet0/0"),
        "{route}"
    );
    assert!(
        route.contains("L        192.168.1.1/32 is directly connected, GigabitEthernet0/0"),
        "{route}"
    );

    let run = c.run("show run");
    assert!(run.contains("hostname Edge"), "{run}");
    assert!(run.contains("interface GigabitEthernet0/0\n description LAN A\n ip address 192.168.1.1 255.255.255.0"), "{run}");
    assert!(
        run.contains(
            "interface GigabitEthernet0/2\n no ip address\n duplex auto\n speed auto\n shutdown"
        ),
        "{run}"
    );

    // The PCs can now reach each other through the router.
    let mut net = Network::new(c.project.clone());
    let s = net
        .ping(pc1, "192.168.2.10".parse().unwrap(), 4, SimTime::ZERO)
        .unwrap();
    assert!(net.session(s).unwrap().replies.len() >= 3);

    // And so can the router itself.
    let out = c.run("ping 192.168.2.10");
    assert!(out.contains("Success rate is"), "{out}");
    let again = c.run("ping 192.168.2.10");
    assert!(again.contains("!!!!!"), "{again}");
    let arp = c.run("show arp");
    assert!(
        arp.contains("192.168.2.10") && arp.contains("ARPA   GigabitEthernet0/1"),
        "{arp}"
    );
}

#[test]
fn bad_addresses_are_refused() {
    let (p, r1, _) = lab();
    let mut c = Console::new(p, r1);
    c.lines(&[
        "en",
        "conf t",
        "int g0/0",
        "ip address 10.0.0.1 255.255.255.0",
    ]);
    assert!(
        c.run("ip address 10.0.0.0 255.255.255.0")
            .contains("Bad mask /24")
    );
    assert!(
        c.run("ip address 10.0.0.1 255.0.255.0")
            .contains("Bad mask")
    );
    c.run("int g0/1");
    assert!(
        c.run("ip address 10.0.0.2 255.255.255.0")
            .contains("overlaps with GigabitEthernet0/0")
    );
    c.run("exit");
    assert!(
        c.run("ip route 10.1.1.1 255.255.0.0 10.0.0.2")
            .contains("Inconsistent address and mask")
    );
    let out = c.run("int g0/9");
    assert!(out.contains("% Invalid input"), "{out}");
}

#[test]
fn static_routes_are_saved_and_shown() {
    let (p, r1, _) = lab();
    let mut c = Console::new(p, r1);
    c.lines(&[
        "en",
        "conf t",
        "int g0/1",
        "ip address 10.0.0.1 255.255.255.252",
        "no shut",
        "exit",
        "ip route 172.16.0.0 255.255.0.0 10.0.0.2",
        "ip route 0.0.0.0 0.0.0.0 10.0.0.2",
        "ip route 172.17.0.0 255.255.0.0 10.0.0.2 200",
        "end",
    ]);
    let config = &c.project.device(r1).unwrap().config;
    assert_eq!(config.routes.len(), 3);
    let route = c.run("show ip route");
    assert!(
        route.contains("Gateway of last resort is 10.0.0.2 to network 0.0.0.0"),
        "{route}"
    );
    assert!(
        route.contains("S*    0.0.0.0/0 [1/0] via 10.0.0.2"),
        "{route}"
    );
    assert!(
        route.contains("S     172.16.0.0/16 [1/0] via 10.0.0.2"),
        "{route}"
    );
    assert!(route.contains("[200/0] via 10.0.0.2"), "{route}");
    let run = c.run("show run");
    assert!(
        run.contains("ip route 172.17.0.0 255.255.0.0 10.0.0.2 200"),
        "{run}"
    );
    c.lines(&["conf t", "no ip route 172.16.0.0 255.255.0.0", "end"]);
    assert_eq!(c.project.device(r1).unwrap().config.routes.len(), 2);
}

#[test]
fn save_reload_and_startup_config() {
    let (p, r1, _) = lab();
    let mut c = Console::new(p, r1);
    c.lines(&["en"]);
    assert_eq!(
        c.run("show startup-config"),
        "startup-config is not present\n"
    );
    c.lines(&["conf t", "hostname Core", "end"]);
    let r = c.session.execute(
        "copy run start",
        &mut c.project,
        &mut c.network,
        SimTime::ZERO,
    );
    assert_eq!(r.prompt, "Destination filename [startup-config]? ");
    let r = c
        .session
        .execute("", &mut c.project, &mut c.network, SimTime::ZERO);
    assert!(r.output.contains("[OK]") && r.changed);
    assert!(c.run("show start").contains("hostname Core"));

    c.lines(&["conf t", "hostname Scratch", "end"]);
    let r = c
        .session
        .execute("reload", &mut c.project, &mut c.network, SimTime::ZERO);
    assert!(r.prompt.contains("Save? [yes/no]"));
    let r = c
        .session
        .execute("no", &mut c.project, &mut c.network, SimTime::ZERO);
    assert_eq!(r.prompt, "Proceed with reload? [confirm]");
    let r = c
        .session
        .execute("", &mut c.project, &mut c.network, SimTime::ZERO);
    assert!(r.output.contains("Press RETURN"));
    assert_eq!(c.project.device(r1).unwrap().name, "Core");
    assert_eq!(c.prompt(), "Core>");
}

#[test]
fn passwords_and_banner() {
    let (p, r1, _) = lab();
    let mut c = Console::new(p, r1);
    c.lines(&[
        "en",
        "conf t",
        "enable secret class",
        "banner motd #Authorised access only#",
        "line con 0",
        "password cisco",
        "login",
        "logging synchronous",
        "exit",
        "service password-encryption",
        "end",
    ]);
    let run = c.run("show run");
    assert!(run.contains("enable secret 5 $1$"), "{run}");
    assert!(!run.contains("class"), "the secret never shows");
    assert!(run.contains(" password 7 02050D480809"), "{run}");
    assert!(
        run.contains("banner motd ^CAuthorised access only^C"),
        "{run}"
    );

    // A new console session asks for the line password, then enable asks for the secret.
    let mut s = Session::new(r1);
    let start = s.start(&c.project);
    assert!(start.output.contains("Authorised access only"));
    assert_eq!((start.prompt.as_str(), start.secret), ("Password: ", true));
    let r = s.execute("wrong", &mut c.project, &mut c.network, SimTime::ZERO);
    assert!(r.secret);
    let r = s.execute("cisco", &mut c.project, &mut c.network, SimTime::ZERO);
    assert_eq!(r.prompt, "R1>");
    let r = s.execute("enable", &mut c.project, &mut c.network, SimTime::ZERO);
    assert!(r.secret);
    let r = s.execute("class", &mut c.project, &mut c.network, SimTime::ZERO);
    assert_eq!(r.prompt, "R1#");
}

#[test]
fn multi_line_banner() {
    let (p, r1, _) = lab();
    let mut c = Console::new(p, r1);
    c.lines(&["en", "conf t"]);
    assert!(
        c.run("banner motd $")
            .contains("End with the character '$'")
    );
    c.run("Line one");
    c.run("Line two$");
    assert_eq!(
        c.project.device(r1).unwrap().config.banner_motd.as_deref(),
        Some("Line one\nLine two")
    );
    assert_eq!(c.prompt(), "R1(config)#");
}

#[test]
fn do_runs_exec_commands_from_config() {
    let (p, r1, _) = lab();
    let mut c = Console::new(p, r1);
    c.lines(&[
        "en",
        "conf t",
        "int g0/0",
        "ip address 192.168.1.1 255.255.255.0",
        "no shut",
    ]);
    let out = c.run("do show ip interface brief");
    assert!(out.contains("192.168.1.1"), "{out}");
    assert_eq!(c.prompt(), "R1(config-if)#");
}

#[test]
fn switches_get_a_management_address() {
    let mut p = Project::new();
    let sw = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let pc = p.add_device(DeviceKind::Pc, Point::default()).id;
    p.set_host_ip(
        pc,
        Some(HostIp::new("192.168.1.10".parse().unwrap(), 24, None).unwrap()),
    )
    .unwrap();
    p.connect(
        Endpoint {
            device: sw,
            port: "FastEthernet0/1".into(),
        },
        Endpoint {
            device: pc,
            port: "FastEthernet0".into(),
        },
        Medium::Copper,
    )
    .unwrap();
    let mut c = Console::new(p, sw);
    c.lines(&["en", "conf t", "int f0/1"]);
    assert!(
        c.run("ip address 192.168.1.2 255.255.255.0")
            .contains("layer 2")
    );
    assert!(c.run("switchport mode access").is_empty());
    c.lines(&[
        "int vlan 1",
        "ip address 192.168.1.2 255.255.255.0",
        "no shutdown",
        "exit",
        "ip default-gateway 192.168.1.1",
        "end",
    ]);
    let brief = c.run("show ip interface brief");
    assert!(
        brief
            .contains("Vlan1                  192.168.1.2     YES manual up                    up"),
        "{brief}"
    );
    let out = c.run("ping 192.168.1.10");
    assert!(out.contains("Success rate"), "{out}");
    assert!(c.run("show mac address-table").contains("Fa0/1"));
    assert!(
        c.run("show ip route")
            .contains("Default gateway is 192.168.1.1")
    );
    let run = c.run("show run");
    assert!(
        run.contains("interface FastEthernet0/1\n switchport mode access\n!"),
        "{run}"
    );
    assert!(run.contains("interface FastEthernet0/2\n!"), "{run}");
    assert!(run.contains("ip default-gateway 192.168.1.1"), "{run}");
}

// MARK: Switching (M4)

/// R1 Gi0/0 — Gi0/1 SW1; SW1 Fa0/1 PC10 (192.168.10.10), Fa0/2 PC20 (192.168.20.10).
fn vlan_lab() -> (Project, DeviceId, DeviceId, DeviceId) {
    let mut p = Project::new();
    let r1 = p.add_device(DeviceKind::Router, Point::default()).id;
    let sw = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let pc10 = p.add_device(DeviceKind::Pc, Point::default()).id;
    let pc20 = p.add_device(DeviceKind::Pc, Point::default()).id;
    for (pc, address, gateway, port) in [
        (pc10, "192.168.10.10", "192.168.10.1", "FastEthernet0/1"),
        (pc20, "192.168.20.10", "192.168.20.1", "FastEthernet0/2"),
    ] {
        p.set_host_ip(
            pc,
            Some(
                HostIp::new(address.parse().unwrap(), 24, Some(gateway.parse().unwrap())).unwrap(),
            ),
        )
        .unwrap();
        p.connect(
            Endpoint {
                device: sw,
                port: port.into(),
            },
            Endpoint {
                device: pc,
                port: "FastEthernet0".into(),
            },
            Medium::Copper,
        )
        .unwrap();
    }
    p.connect(
        Endpoint {
            device: r1,
            port: "GigabitEthernet0/0".into(),
        },
        Endpoint {
            device: sw,
            port: "GigabitEthernet0/1".into(),
        },
        Medium::Copper,
    )
    .unwrap();
    (p, r1, sw, pc10)
}

#[test]
fn vlans_trunks_and_router_on_a_stick_from_the_cli() {
    let (p, r1, sw, pc10) = vlan_lab();
    let mut c = Console::new(p, sw);
    c.lines(&["en", "conf t", "vlan 10"]);
    assert_eq!(c.prompt(), "SW1(config-vlan)#");
    c.lines(&[
        "name Sales",
        "vlan 20",
        "name Staff",
        "exit",
        "interface range f0/1 - 2",
    ]);
    assert_eq!(c.prompt(), "SW1(config-if-range)#");
    c.lines(&[
        "switchport mode access",
        "int f0/2",
        "switchport access vlan 20",
        "int f0/1",
        "switchport access vlan 10",
    ]);
    c.lines(&[
        "int g0/1",
        "switchport mode trunk",
        "switchport trunk allowed vlan 10,20",
        "switchport trunk allowed vlan add 1",
        "end",
    ]);
    let brief = c.run("show vlan brief");
    assert!(
        brief.contains("10   Sales                            active    Fa0/1"),
        "{brief}"
    );
    assert!(
        brief.contains("20   Staff                            active    Fa0/2"),
        "{brief}"
    );
    assert!(brief.contains("1002 fddi-default"), "{brief}");
    let run = c.run("show run");
    assert!(run.contains("vlan 10\n name Sales"), "{run}");
    assert!(
        run.contains(
            "interface FastEthernet0/2\n switchport access vlan 20\n switchport mode access"
        ),
        "{run}"
    );
    assert!(
        run.contains(" switchport trunk allowed vlan 1,10,20\n switchport mode trunk"),
        "{run}"
    );

    // Configure R1's subinterfaces through its own console.
    let mut r = Console::new(c.project.clone(), r1);
    r.lines(&[
        "en",
        "conf t",
        "int g0/0",
        "no shut",
        "int g0/0.10",
        "encapsulation dot1Q 10",
        "ip address 192.168.10.1 255.255.255.0",
        "int g0/0.20",
        "encapsulation dot1Q 20",
        "ip address 192.168.20.1 255.255.255.0",
        "end",
    ]);
    let route = r.run("show ip route");
    assert!(
        route.contains("C        192.168.20.0/24 is directly connected, GigabitEthernet0/0.20"),
        "{route}"
    );
    let run = r.run("show run");
    assert!(
        run.contains(
            "interface GigabitEthernet0/0.10\n encapsulation dot1Q 10\n ip address 192.168.10.1"
        ),
        "{run}"
    );

    let mut net = Network::new(r.project.clone());
    let s = net
        .ping(pc10, "192.168.20.10".parse().unwrap(), 4, SimTime::ZERO)
        .unwrap();
    assert!(
        net.session(s).unwrap().replies.len() >= 3,
        "{}",
        net.session_report(s).unwrap()
    );

    c.project = r.project.clone();
    c.network = Network::new(c.project.clone());
    let trunk = c.run("show interfaces trunk");
    assert!(
        trunk.contains("Gi0/1       on               802.1q         trunking      1"),
        "{trunk}"
    );
    assert!(trunk.contains("Gi0/1       1,10,20"), "{trunk}");
    let status = c.run("show interfaces status");
    assert!(
        status.contains("Fa0/1                        connected    10"),
        "{status}"
    );
    assert!(
        status.contains("Gi0/1                        connected    trunk"),
        "{status}"
    );
    let switchport = c.run("show interfaces f0/1 switchport");
    assert!(
        switchport.contains("Access Mode VLAN: 10 (Sales)"),
        "{switchport}"
    );
    let cdp = c.run("show cdp neighbors");
    assert!(cdp.contains("R1               Gig 0/1"), "{cdp}");
    assert!(
        c.run("show lldp neighbors")
            .contains("% LLDP is not enabled")
    );
}

#[test]
fn spanning_tree_commands_and_output() {
    let mut p = Project::new();
    let a = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let b = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    for port in ["FastEthernet0/1", "FastEthernet0/2"] {
        p.connect(
            Endpoint {
                device: a,
                port: port.into(),
            },
            Endpoint {
                device: b,
                port: port.into(),
            },
            Medium::Copper,
        )
        .unwrap();
    }
    let mut c = Console::new(p, b);
    c.lines(&["en"]);
    let stp = c.run("show spanning-tree");
    assert!(
        stp.contains("VLAN0001") && stp.contains("Spanning tree enabled protocol ieee"),
        "{stp}"
    );
    assert!(
        stp.contains("Fa0/2               Altn BLK 19        128.2"),
        "{stp}"
    );
    assert!(stp.contains("Port        1 (FastEthernet0/1)"), "{stp}");
    let out = c.run("conf t");
    assert!(out.contains("Enter configuration"));
    assert!(
        c.run("spanning-tree vlan 1 priority 1000")
            .contains("increments of 4096")
    );
    c.lines(&[
        "spanning-tree vlan 1 root primary",
        "spanning-tree mode rapid-pvst",
        "end",
    ]);
    let stp = c.run("show spanning-tree vlan 1");
    assert!(stp.contains("This bridge is the root"), "{stp}");
    assert!(stp.contains("Priority    24577"), "{stp}");
    assert!(stp.contains("protocol rstp"), "{stp}");
    assert!(c.run("show run").contains("spanning-tree mode rapid-pvst\nspanning-tree extend system-id\nspanning-tree vlan 1 priority 24576"));
}

#[test]
fn port_security_from_the_cli() {
    let (p, _, sw, pc10) = vlan_lab();
    let mut c = Console::new(p, sw);
    c.lines(&["en", "conf t", "int f0/1"]);
    assert!(
        c.run("switchport port-security")
            .contains("is a dynamic port")
    );
    c.lines(&[
        "switchport mode access",
        "switchport port-security",
        "switchport port-security maximum 2",
        "switchport port-security violation restrict",
        "switchport port-security mac-address sticky",
        "spanning-tree portfast",
        "spanning-tree bpduguard enable",
        "end",
    ]);
    let run = c.run("show run");
    assert!(run.contains(
        " switchport mode access\n switchport port-security maximum 2\n switchport port-security violation restrict\n \
         switchport port-security mac-address sticky\n switchport port-security\n spanning-tree portfast\n spanning-tree bpduguard enable"
    ), "{run}");
    c.network
        .ping(pc10, "192.168.10.1".parse().unwrap(), 1, SimTime::ZERO)
        .unwrap();
    let summary = c.run("show port-security");
    assert!(
        summary.contains("Fa0/1              2            1                  0         Restrict"),
        "{summary}"
    );
    let detail = c.run("show port-security interface f0/1");
    assert!(
        detail.contains("Port Status                : Secure-up")
            && detail.contains("Sticky MAC Addresses       : 1"),
        "{detail}"
    );
    assert!(c.run("show port-security address").contains("SecureSticky"));
}

// MARK: Services (M5)

#[test]
fn dhcp_server_from_the_cli() {
    let mut p = Project::new();
    let r1 = p.add_device(DeviceKind::Router, Point::default()).id;
    let pc = p.add_device(DeviceKind::Pc, Point::default()).id;
    p.set_host_dhcp(pc, true).unwrap();
    p.connect(
        Endpoint {
            device: r1,
            port: "GigabitEthernet0/0".into(),
        },
        Endpoint {
            device: pc,
            port: "FastEthernet0".into(),
        },
        Medium::Copper,
    )
    .unwrap();
    let mut c = Console::new(p, r1);
    c.lines(&[
        "en",
        "conf t",
        "ip dhcp excluded-address 192.168.1.1 192.168.1.10",
        "ip dhcp pool LAN",
    ]);
    assert_eq!(c.prompt(), "R1(dhcp-config)#");
    c.lines(&[
        "network 192.168.1.0 255.255.255.0",
        "default-router 192.168.1.1",
        "dns-server 8.8.8.8",
        "int g0/0",
        "ip address 192.168.1.1 255.255.255.0",
        "no shut",
        "end",
    ]);
    let run = c.run("show run");
    assert!(run.contains("ip dhcp excluded-address 192.168.1.1 192.168.1.10\n!\nip dhcp pool LAN\n network 192.168.1.0 255.255.255.0\n default-router 192.168.1.1\n dns-server 8.8.8.8"), "{run}");
    c.network = Network::new(c.project.clone());
    let binding = c.run("show ip dhcp binding");
    assert!(binding.contains("192.168.1.11"), "{binding}");
    assert!(
        c.run("show ip dhcp pool")
            .contains("Leased addresses               : 1")
    );
}

#[test]
fn access_lists_from_the_cli() {
    let (p, r1, _) = lab();
    let mut c = Console::new(p, r1);
    c.lines(&[
        "en",
        "conf t",
        "access-list 101 remark No pings from PC1",
        "access-list 101 deny icmp host 192.168.1.10 any echo",
        "access-list 101 permit ip any any",
        // IOS ignores a repeated entry.
        "access-list 101 permit ip any any",
        "access-list 10 permit 192.168.1.0 0.0.0.255",
        "ip access-list extended WEB",
    ]);
    assert_eq!(c.prompt(), "R1(config-ext-nacl)#");
    c.lines(&[
        "permit udp any any eq bootps",
        "15 deny icmp any any",
        "exit",
    ]);
    let out = c.run("access-list 102 permit icmp any anywhere");
    assert!(
        out.contains("% Invalid input detected at '^' marker."),
        "{out}"
    );
    let caret = out.lines().next().unwrap().find('^').unwrap();
    assert_eq!(
        caret,
        "R1(config)#access-list 102 permit icmp any ".len(),
        "{out}"
    );
    c.lines(&[
        "int g0/0",
        "ip address 192.168.1.1 255.255.255.0",
        "no shut",
        "ip access-group 101 in",
        "int g0/1",
        "ip address 192.168.2.1 255.255.255.0",
        "no shut",
        "end",
    ]);
    let run = c.run("show run");
    assert!(
        run.contains(" ip address 192.168.1.1 255.255.255.0\n ip access-group 101 in"),
        "{run}"
    );
    assert!(run.contains("access-list 101 remark No pings from PC1\naccess-list 101 deny icmp host 192.168.1.10 any echo\naccess-list 101 permit ip any any"), "{run}");
    assert_eq!(
        run.matches("access-list 101 permit ip any any").count(),
        1,
        "{run}"
    );
    assert!(
        run.contains("access-list 10 permit 192.168.1.0 0.0.0.255"),
        "{run}"
    );
    assert!(
        run.contains(
            "ip access-list extended WEB\n permit udp any any eq bootps\n deny icmp any any"
        ),
        "{run}"
    );

    let pc1 = c
        .project
        .devices()
        .iter()
        .find(|d| d.name == "PC1")
        .unwrap()
        .id;
    c.network = Network::new(c.project.clone());
    c.network
        .ping(pc1, "192.168.2.10".parse().unwrap(), 2, SimTime::ZERO)
        .unwrap();
    let acls = c.run("show access-lists");
    assert!(
        acls.contains(
            "Extended IP access list 101\n    10 deny icmp host 192.168.1.10 any echo (2 matches)"
        ),
        "{acls}"
    );
    assert!(
        acls.contains(
            "Standard IP access list 10\n    10 permit 192.168.1.0, wildcard bits 0.0.0.255"
        ),
        "{acls}"
    );
    assert!(acls.contains("Extended IP access list WEB\n    10 permit udp any any eq bootps\n    15 deny icmp any any"), "{acls}");
}

#[test]
fn nat_from_the_cli() {
    let (p, r1, pc1) = lab();
    let mut c = Console::new(p, r1);
    c.lines(&[
        "en",
        "conf t",
        "int g0/0",
        "ip address 192.168.1.1 255.255.255.0",
        "ip nat inside",
        "no shut",
        "int g0/1",
        "ip address 192.168.2.1 255.255.255.0",
        "ip nat outside",
        "no shut",
        "exit",
        "access-list 1 permit 192.168.1.0 0.0.0.255",
        "ip nat inside source list 1 interface g0/1 overload",
        "ip nat inside source static 192.168.1.50 192.168.2.50",
    ]);
    assert_eq!(
        c.run("ip nat pool BAD 10.0.0.9 10.0.0.1 netmask 255.255.255.0"),
        "%End address less than start address in pool BAD\n"
    );
    c.run("ip nat pool PUB 192.168.2.100 192.168.2.110 prefix-length 24");
    c.run("ip nat inside source list 2 pool PUB");
    assert_eq!(
        c.run("no ip nat pool PUB"),
        "%Pool PUB in use, cannot destroy\n"
    );
    c.run("end");
    let run = c.run("show run");
    for line in [
        " ip address 192.168.1.1 255.255.255.0\n ip nat inside",
        " ip nat outside",
        "ip nat pool PUB 192.168.2.100 192.168.2.110 netmask 255.255.255.0",
        "ip nat inside source list 1 interface GigabitEthernet0/1 overload",
        "ip nat inside source static 192.168.1.50 192.168.2.50",
        "ip nat inside source list 2 pool PUB",
    ] {
        assert!(run.contains(line), "{line:?} missing from\n{run}");
    }

    c.network = Network::new(c.project.clone());
    c.network
        .ping(pc1, "192.168.2.10".parse().unwrap(), 2, SimTime::ZERO)
        .unwrap();
    let table = c.run("show ip nat translations");
    assert!(
        table.starts_with("Pro  Inside global         Inside local"),
        "{table}"
    );
    assert!(
        table.contains("---  192.168.2.50          192.168.1.50          ---"),
        "{table}"
    );
    let flow = table
        .lines()
        .find(|l| l.starts_with("icmp"))
        .unwrap_or_else(|| panic!("{table}"));
    assert!(
        flow.contains("192.168.2.1:") && flow.contains("192.168.1.10:"),
        "{flow}"
    );
    let stats = c.run("show ip nat statistics");
    assert!(
        stats.contains("Total active translations: 2 (1 static, 1 dynamic; 1 extended)"),
        "{stats}"
    );
    assert!(
        stats.contains("Outside interfaces:\n  GigabitEthernet0/1"),
        "{stats}"
    );
    assert!(
        stats.contains("[Id: 1] access-list 1 interface GigabitEthernet0/1 overload refcount 1"),
        "{stats}"
    );
    assert!(
        stats.contains("total addresses 11, allocated 0 (0%)"),
        "{stats}"
    );
}

#[test]
fn ospf_from_the_cli() {
    // R1 (lab) gets a neighbour: R2 on R1 Gi0/1's network instead of PC2.
    let (mut p, r1, pc1) = lab();
    let pc2 = p.devices().iter().find(|d| d.name == "PC2").unwrap().id;
    let link = p
        .links()
        .iter()
        .find(|l| l.a.device == r1 && l.a.port == "GigabitEthernet0/1")
        .unwrap()
        .id;
    p.disconnect(link).unwrap();
    let r2 = p.add_device(DeviceKind::Router, Point::default()).id;
    let ep = |device, port: &str| Endpoint {
        device,
        port: port.into(),
    };
    p.connect(
        ep(r1, "GigabitEthernet0/1"),
        ep(r2, "GigabitEthernet0/1"),
        Medium::Copper,
    )
    .unwrap();
    p.connect(
        ep(r2, "GigabitEthernet0/0"),
        ep(pc2, "FastEthernet0"),
        Medium::Copper,
    )
    .unwrap();

    let mut c = Console::new(p, r1);
    c.lines(&[
        "en",
        "conf t",
        "int g0/0",
        "ip address 192.168.1.1 255.255.255.0",
        "no shut",
        "int g0/1",
        "ip address 10.0.12.1 255.255.255.0",
        "ip ospf cost 5",
        "no shut",
        "router ospf 1",
    ]);
    assert_eq!(c.prompt(), "R1(config-router)#");
    c.lines(&[
        "router-id 1.1.1.1",
        "network 192.168.1.0 0.0.0.255 area 0",
        "network 10.0.12.0 0.0.0.255 area 0",
        "passive-interface g0/0",
        "end",
    ]);
    let run = c.run("show run");
    assert!(run.contains(" ip ospf cost 5\n"), "{run}");
    assert!(
        run.contains(
            "router ospf 1\n router-id 1.1.1.1\n passive-interface GigabitEthernet0/0\n network 192.168.1.0 0.0.0.255 area 0\n network 10.0.12.0 0.0.0.255 area 0\n!"
        ),
        "{run}"
    );

    let mut c2 = Console::new(c.project.clone(), r2);
    c2.lines(&[
        "en",
        "conf t",
        "int g0/0",
        "ip address 192.168.2.1 255.255.255.0",
        "ip ospf 1 area 0",
        "no shut",
        "int g0/1",
        "ip address 10.0.12.2 255.255.255.0",
        "ip ospf 1 area 0",
        "no shut",
        "router ospf 1",
        "router-id 2.2.2.2",
        "end",
    ]);
    c.project = c2.project.clone();
    let nbr = c.run("show ip ospf neighbor");
    assert!(
        nbr.contains("Neighbor ID     Pri   State           Dead Time   Address         Interface"),
        "{nbr}"
    );
    assert!(
        nbr.contains(
            "2.2.2.2           1   FULL/DR         00:00:35    10.0.12.2       GigabitEthernet0/1"
        ),
        "{nbr}"
    );
    let brief = c.run("show ip ospf interface brief");
    assert!(
        brief.contains("Gi0/1        1     0               10.0.12.1/24       5     BDR   1/1"),
        "{brief}"
    );
    assert!(
        brief.contains("Gi0/0        1     0               192.168.1.1/24     1     WAIT  0/0"),
        "{brief}"
    );
    let route = c.run("show ip route");
    assert!(
        route.contains("O     192.168.2.0/24 [110/6] via 10.0.12.2, 00:00:12, GigabitEthernet0/1"),
        "{route}"
    );
    let only = c.run("show ip route ospf");
    assert!(
        only.contains("192.168.2.0/24") && !only.contains("192.168.1.0/24 is directly"),
        "{only}"
    );
    let db = c.run("show ip ospf database");
    assert!(
        db.contains("OSPF Router with ID (1.1.1.1) (Process ID 1)"),
        "{db}"
    );
    assert!(
        db.contains("Router Link States (Area 0)") && db.contains("Net Link States (Area 0)"),
        "{db}"
    );
    let protocols = c.run("show ip protocols");
    assert!(
        protocols.contains("Routing Protocol is \"ospf 1\""),
        "{protocols}"
    );
    assert!(
        protocols.contains("Passive Interface(s):\n    GigabitEthernet0/0"),
        "{protocols}"
    );
    let iface = c.run("show ip ospf interface g0/1");
    assert!(
        iface.contains("Designated Router (ID) 2.2.2.2, Interface address 10.0.12.2"),
        "{iface}"
    );
    assert!(
        iface.contains("Adjacent with neighbor 2.2.2.2  (Designated Router)"),
        "{iface}"
    );

    c.network = Network::new(c.project.clone());
    let s = c
        .network
        .ping(pc1, "192.168.2.10".parse().unwrap(), 4, SimTime::ZERO)
        .unwrap();
    let report = c.network.session_report(s).unwrap();
    assert!(report.contains("Reply from 192.168.2.10"), "{report}");
}

#[test]
fn span_sessions_are_configured_and_shown() {
    let mut p = Project::new();
    let sw = p.add_device(DeviceKind::L2Switch, Point::default()).id;
    let mut c = Console::new(p, sw);
    c.lines(&[
        "enable",
        "configure terminal",
        "monitor session 1 source interface g0/1",
        "monitor session 1 source interface f0/2 rx",
        "monitor session 1 destination interface f0/24",
    ]);
    assert!(
        c.run("monitor session 1 source interface f0/24")
            .contains("already the destination")
    );
    c.run("end");
    let config = c.run("show running-config");
    assert!(
        config.contains("monitor session 1 source interface Gi0/1\n"),
        "{config}"
    );
    assert!(
        config.contains("monitor session 1 source interface Fa0/2 rx"),
        "{config}"
    );
    assert!(
        config.contains("monitor session 1 destination interface Fa0/24"),
        "{config}"
    );
    let show = c.run("show monitor");
    assert!(show.contains("Both               : Gi0/1"), "{show}");
    assert!(show.contains("RX Only            : Fa0/2"), "{show}");
    assert!(show.contains("Destination Ports      : Fa0/24"), "{show}");
    c.lines(&["configure terminal", "no monitor session 1", "end"]);
    assert!(!c.run("show running-config").contains("monitor session"));
}
