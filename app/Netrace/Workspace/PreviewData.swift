import Foundation

/// Placeholder content for panels whose engines arrive in later milestones:
/// the IOS console and running-config of routers and switches (M3) and labs
/// (M6). Every panel showing it carries a preview badge.
enum PreviewData {
    struct LabTask: Identifiable {
        let id: Int
        let title: String
        let detail: String
        let isDone: Bool
    }

    // MARK: Console (network devices)

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
        default:
            return "\(name) has no command line."
        }
    }

    static func prompt(for device: DeviceInfo) -> String {
        "\(device.name)#"
    }

    static func runningConfig(for device: DeviceInfo, in snapshot: ProjectSnapshot) -> String {
        let connected = Set(snapshot.links.compactMap { $0.endpoint(for: device.id)?.port })
        var lines = ["!", "version 15.2", "hostname \(device.name)", "!", "no ip domain-lookup", "!"]
        for port in device.kind.ports.prefix(10) {
            lines.append("interface \(port)")
            if let ip = device.ip, port == device.kind.ports.first {
                lines.append(" ip address \(ip.address) \(ip.mask)")
            } else {
                lines.append(device.kind.category == .network && device.kind != .l2Switch && device.kind != .hub
                             ? " no ip address" : " switchport mode access")
            }
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
