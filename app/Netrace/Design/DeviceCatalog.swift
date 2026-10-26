import Foundation

/// How devices are grouped in the library and the topology outline.
enum DeviceCategory: String, CaseIterable, Identifiable {
    case network = "Network"
    case endDevices = "End Devices"

    var id: Self { self }

    var kinds: [DeviceKind] { DeviceKind.allCases.filter { $0.category == self } }
}

extension DeviceKind {
    var category: DeviceCategory {
        switch self {
        case .router, .l2Switch, .l3Switch, .hub, .firewall, .accessPoint, .cloud: .network
        case .pc, .laptop, .server, .printer, .ipPhone, .smartphone: .endDevices
        }
    }

    /// Routers and switches: an IOS command line and a configuration.
    var hasCli: Bool {
        switch self {
        case .router, .l2Switch, .l3Switch: true
        default: false
        }
    }

    /// End devices with a wired NIC: they take an IP configuration and can ping.
    var isHost: Bool {
        switch self {
        case .pc, .laptop, .server, .printer, .ipPhone: true
        default: false
        }
    }

    var displayName: String {
        switch self {
        case .router: "Router"
        case .l2Switch: "Switch"
        case .l3Switch: "L3 Switch"
        case .hub: "Hub"
        case .firewall: "Firewall"
        case .accessPoint: "Access Point"
        case .cloud: "Internet"
        case .pc: "PC"
        case .laptop: "Laptop"
        case .server: "Server"
        case .printer: "Printer"
        case .ipPhone: "IP Phone"
        case .smartphone: "Smartphone"
        }
    }

    var summary: String {
        switch self {
        case .router: "4 × Gigabit Ethernet, routing"
        case .l2Switch: "24 × Fast Ethernet, 2 × Gigabit"
        case .l3Switch: "24 × Gigabit, inter-VLAN routing"
        case .hub: "8 ports, one collision domain"
        case .firewall: "8 × Gigabit, ACLs and NAT"
        case .accessPoint: "Wireless LAN, Gigabit uplink"
        case .cloud: "ISP / internet attachment"
        case .pc: "Desktop, 1 × Fast Ethernet"
        case .laptop: "Portable, Ethernet + Wi-Fi"
        case .server: "DHCP, DNS and web services"
        case .printer: "Network printer"
        case .ipPhone: "Voice VLAN, PC pass-through"
        case .smartphone: "Wi-Fi only"
        }
    }

    /// Lowercase search terms beyond the display name.
    var keywords: [String] {
        switch self {
        case .router: ["gateway", "routing", "isr"]
        case .l2Switch: ["switch", "vlan", "layer 2", "access"]
        case .l3Switch: ["multilayer", "mls", "core", "distribution", "svi"]
        case .hub: ["repeater"]
        case .firewall: ["asa", "security", "acl", "nat"]
        case .accessPoint: ["wifi", "wireless", "wlan", "ap"]
        case .cloud: ["isp", "wan", "internet", "cloud"]
        case .pc: ["host", "desktop", "computer", "end device"]
        case .laptop: ["host", "notebook"]
        case .server: ["dhcp", "dns", "http", "host"]
        case .printer: ["host"]
        case .ipPhone: ["voip", "voice", "phone"]
        case .smartphone: ["mobile", "phone", "wireless"]
        }
    }

    func matches(_ query: String) -> Bool {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return true }
        return displayName.lowercased().contains(q) || keywords.contains { $0.contains(q) }
    }
}
