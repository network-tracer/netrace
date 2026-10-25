import Foundation

/// Placeholder content for panels whose engines arrive in later milestones
/// (console M3, events and capture M2, labs M6). Everything here is derived
/// from the real topology so the preview reads plausibly, and every panel
/// that shows it labels it as a preview.
enum PreviewData {
    struct Event: Identifiable, Hashable {
        let id: Int
        let time: Double
        let lastDevice: String
        let atDevice: String
        let proto: NetProtocol
        let info: String
        let linkId: UInt64
        let fromId: UInt64
        let toId: UInt64
    }

    struct Packet: Identifiable, Hashable {
        let id: Int
        let time: Double
        let source: String
        let destination: String
        let proto: NetProtocol
        let length: Int
        let info: String
    }

    struct LabTask: Identifiable {
        let id: Int
        let title: String
        let detail: String
        let isDone: Bool
    }

    /// Stand-in addressing so tables have something realistic to show.
    static func address(of device: DeviceInfo, in snapshot: ProjectSnapshot) -> String {
        let index = (snapshot.devices.firstIndex { $0.id == device.id } ?? 0)
        switch device.kind.category {
        case .network: return "192.168.1.\(1 + index)"
        case .endDevices: return "192.168.1.\(10 + index)"
        }
    }

    static func mac(of device: DeviceInfo) -> String {
        let n = device.id &* 2_654_435_761
        return String(format: "00:1a:2b:%02x:%02x:%02x", (n >> 16) & 0xFF, (n >> 8) & 0xFF, n & 0xFF)
    }

    // MARK: Events

    static func events(for snapshot: ProjectSnapshot) -> [Event] {
        let byId = Dictionary(uniqueKeysWithValues: snapshot.devices.map { ($0.id, $0) })
        var events: [Event] = []
        var t = 0.0
        for link in snapshot.links.prefix(6) {
            guard let a = byId[link.a.deviceId], let b = byId[link.b.deviceId] else { continue }
            let (from, to) = a.kind.category == .endDevices ? (a, b) : (b, a)
            let steps: [(DeviceInfo, DeviceInfo, NetProtocol, String)] = [
                (from, to, .arp, "Who has \(address(of: to, in: snapshot))? Tell \(address(of: from, in: snapshot))"),
                (to, from, .arp, "\(address(of: to, in: snapshot)) is at \(mac(of: to))"),
                (from, to, .icmp, "Echo request, id 0x0001, seq 1, ttl 128"),
                (to, from, .icmp, "Echo reply, id 0x0001, seq 1, ttl 255"),
            ]
            for (src, dst, proto, info) in steps {
                t += 0.001 + Double(events.count % 3) * 0.0004
                events.append(Event(id: events.count, time: t, lastDevice: src.name, atDevice: dst.name,
                                    proto: proto, info: info, linkId: link.id, fromId: src.id, toId: dst.id))
            }
            if [a, b].contains(where: { $0.kind == .l2Switch || $0.kind == .l3Switch }) {
                t += 0.002
                let sw = [a, b].first { $0.kind == .l2Switch || $0.kind == .l3Switch }!
                let peer = sw.id == a.id ? b : a
                events.append(Event(id: events.count, time: t, lastDevice: sw.name, atDevice: peer.name,
                                    proto: .stp, info: "Config BPDU, root 32769/\(mac(of: sw)), cost 0",
                                    linkId: link.id, fromId: sw.id, toId: peer.id))
                events.append(Event(id: events.count, time: t + 0.0005, lastDevice: sw.name, atDevice: peer.name,
                                    proto: .cdp, info: "Device ID \(sw.name), port \(link.endpoint(for: sw.id)?.port.abbreviatedInterface ?? "")",
                                    linkId: link.id, fromId: sw.id, toId: peer.id))
            }
        }
        return events.sorted { $0.time < $1.time }
    }

    // MARK: Capture

    static func packets(on link: LinkInfo?, in snapshot: ProjectSnapshot) -> [Packet] {
        guard let link,
              let a = snapshot.devices.first(where: { $0.id == link.a.deviceId }),
              let b = snapshot.devices.first(where: { $0.id == link.b.deviceId }) else { return [] }
        let ipA = address(of: a, in: snapshot), ipB = address(of: b, in: snapshot)
        let rows: [(String, String, NetProtocol, Int, String)] = [
            (mac(of: a), "ff:ff:ff:ff:ff:ff", .arp, 42, "Who has \(ipB)? Tell \(ipA)"),
            (mac(of: b), mac(of: a), .arp, 42, "\(ipB) is at \(mac(of: b))"),
            (ipA, ipB, .icmp, 98, "Echo (ping) request id=0x0001, seq=1/256, ttl=128"),
            (ipB, ipA, .icmp, 98, "Echo (ping) reply id=0x0001, seq=1/256, ttl=255"),
            (ipA, ipB, .icmp, 98, "Echo (ping) request id=0x0001, seq=2/512, ttl=128"),
            (ipB, ipA, .icmp, 98, "Echo (ping) reply id=0x0001, seq=2/512, ttl=255"),
            (mac(of: b), "01:80:c2:00:00:00", .stp, 60, "Conf. Root = 32768/1/\(mac(of: b)) Cost = 0 Port = 0x8001"),
            (mac(of: b), "01:00:0c:cc:cc:cc", .cdp, 351, "Device ID: \(b.name)  Port ID: \(link.b.port)"),
            ("0.0.0.0", "255.255.255.255", .dhcp, 342, "DHCP Discover - Transaction ID 0x3903f326"),
            (ipB, "255.255.255.255", .dhcp, 342, "DHCP Offer - Transaction ID 0x3903f326"),
            (ipA, ipB, .dns, 74, "Standard query 0x1a2b A netrace.lab"),
            (ipA, ipB, .tcp, 66, "49152 → 80 [SYN] Seq=0 Win=64240 Len=0 MSS=1460"),
        ]
        return rows.enumerated().map { i, r in
            Packet(id: i + 1, time: Double(i) * 0.137 + 0.0042, source: r.0, destination: r.1, proto: r.2, length: r.3, info: r.4)
        }
    }

    /// Wireshark-style detail tree for one packet.
    static func detail(of packet: Packet) -> [(String, [String])] {
        var tree: [(String, [String])] = [
            ("Frame \(packet.id): \(packet.length) bytes on wire (\(packet.length * 8) bits)",
             ["Arrival time: \(String(format: "%.6f", packet.time)) s", "Frame length: \(packet.length) bytes", "Protocols in frame: eth:\(packet.proto.rawValue.lowercased())"]),
            ("Ethernet II",
             ["Destination: \(packet.destination.contains(":") ? packet.destination : "00:1a:2b:00:00:02")",
              "Source: \(packet.source.contains(":") ? packet.source : "00:1a:2b:00:00:01")",
              "Type: \(packet.proto == .arp ? "ARP (0x0806)" : "IPv4 (0x0800)")"]),
        ]
        switch packet.proto {
        case .arp:
            tree.append(("Address Resolution Protocol", ["Hardware type: Ethernet (1)", "Protocol type: IPv4 (0x0800)", "Opcode: \(packet.info.hasPrefix("Who") ? "request (1)" : "reply (2)")"]))
        case .icmp:
            tree.append(("Internet Protocol Version 4, Src: \(packet.source), Dst: \(packet.destination)", ["Version: 4", "Header length: 20 bytes", "Time to live: \(packet.info.contains("reply") ? 255 : 128)", "Protocol: ICMP (1)"]))
            tree.append(("Internet Control Message Protocol", ["Type: \(packet.info.contains("reply") ? "0 (Echo reply)" : "8 (Echo request)")", "Code: 0", "Identifier: 0x0001", "Sequence number: \(packet.info.contains("seq=2") ? 2 : 1)"]))
        default:
            tree.append(("\(packet.proto.rawValue)", [packet.info]))
        }
        return tree
    }

    // MARK: Console

    static func transcript(for device: DeviceInfo, in snapshot: ProjectSnapshot) -> String {
        let connected = Set(snapshot.links.compactMap { $0.endpoint(for: device.id)?.port })
        let name = device.name
        switch device.kind {
        case .router, .l3Switch, .firewall:
            let rows = device.kind.ports.map { port in
                let up = connected.contains(port)
                return port.padding(toLength: 23, withPad: " ", startingAt: 0)
                    + "unassigned      YES unset  "
                    + (up ? "up                    up" : "administratively down down")
            }
            return """
            \(name)>enable
            \(name)#show ip interface brief
            Interface              IP-Address      OK? Method Status                Protocol
            \(rows.joined(separator: "\n"))
            \(name)#
            """
        case .l2Switch, .hub:
            let access = device.kind.ports.prefix(8).map(\.abbreviatedInterface).joined(separator: ", ")
            return """
            \(name)>enable
            \(name)#show vlan brief

            VLAN Name                             Status    Ports
            ---- -------------------------------- --------- -------------------------------
            1    default                          active    \(access)
            1002 fddi-default                     act/unsup
            1003 token-ring-default               act/unsup
            \(name)#
            """
        case .cloud:
            return "\(name) is an ISP attachment point. It has no console."
        default:
            let ip = address(of: device, in: snapshot)
            return """
            C:\\> ipconfig

            \(device.kind.ports.first ?? "Wireless") Connection:

               Connection-specific DNS Suffix..: netrace.lab
               IPv4 Address....................: \(ip)
               Subnet Mask.....................: 255.255.255.0
               Default Gateway.................: 192.168.1.1

            C:\\> ping 192.168.1.1

            Pinging 192.168.1.1 with 32 bytes of data:
            Reply from 192.168.1.1: bytes=32 time<1ms TTL=255
            Reply from 192.168.1.1: bytes=32 time<1ms TTL=255

            C:\\>
            """
        }
    }

    static func prompt(for device: DeviceInfo) -> String {
        switch device.kind {
        case .router, .l2Switch, .l3Switch, .firewall, .hub: "\(device.name)#"
        default: "C:\\>"
        }
    }

    static func runningConfig(for device: DeviceInfo, in snapshot: ProjectSnapshot) -> String {
        let connected = Set(snapshot.links.compactMap { $0.endpoint(for: device.id)?.port })
        var lines = ["!", "version 15.2", "hostname \(device.name)", "!", "no ip domain-lookup", "!"]
        for port in device.kind.ports.prefix(10) {
            lines.append("interface \(port)")
            lines.append(device.kind.category == .network && device.kind != .l2Switch && device.kind != .hub ? " no ip address" : " switchport mode access")
            if !connected.contains(port) { lines.append(" shutdown") }
            lines.append("!")
        }
        if device.kind.ports.count > 10 { lines.append("! … \(device.kind.ports.count - 10) more interfaces") }
        lines += ["line con 0", " logging synchronous", "line vty 0 4", " login", "!", "end"]
        return lines.joined(separator: "\n")
    }

    // MARK: Lab

    static let labTitle = "Lab 1 · Build a small office LAN"
    static let labBrief = """
    Connect two PCs to a switch and the switch to a router. Give each PC an address in \
    192.168.1.0/24 with the router as default gateway, then prove both PCs can reach it.
    """
    static let labTasks: [LabTask] = [
        LabTask(id: 1, title: "Place a router, a switch and two PCs", detail: "Any names", isDone: true),
        LabTask(id: 2, title: "Cable both PCs to the switch", detail: "Copper, access ports", isDone: true),
        LabTask(id: 3, title: "Uplink the switch to the router", detail: "Router Gi0/0", isDone: false),
        LabTask(id: 4, title: "Address the router interface", detail: "192.168.1.1/24, no shutdown", isDone: false),
        LabTask(id: 5, title: "Address both PCs", detail: "Gateway 192.168.1.1", isDone: false),
        LabTask(id: 6, title: "PC1 can ping the gateway", detail: "Checked by simulation", isDone: false),
    ]
}
