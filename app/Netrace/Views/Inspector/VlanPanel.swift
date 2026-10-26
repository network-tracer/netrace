import SwiftUI

/// What the canvas shows in the VLAN view: the chosen VLAN's cables in its
/// colour, and the devices on it; everything else fades back.
struct VlanDisplay {
    let vlan: UInt16
    /// (device, port) of every member port.
    let ports: Set<LinksLayer.PortKey>
    /// Switches, routers and hosts on the VLAN.
    let devices: Set<UInt64>

    func carries(_ link: LinkInfo) -> Bool {
        ports.contains(LinksLayer.PortKey(device: link.a.deviceId, port: link.a.port))
            || ports.contains(LinksLayer.PortKey(device: link.b.deviceId, port: link.b.port))
    }

    @MainActor
    static func make(simulation: SimulationController, workspace: WorkspaceModel, links: [LinkInfo]) -> VlanDisplay? {
        guard workspace.showsVlans else { return nil }
        let vlans = simulation.vlanView()
        guard let info = vlans.first(where: { $0.vlan == workspace.selectedVlan }) ?? vlans.first else { return nil }
        let ports = Set(info.members.map { LinksLayer.PortKey(device: $0.deviceId, port: $0.port) })
        var devices = Set(info.members.map(\.deviceId)).union(info.hosts)
        // Whatever sits at the far end of a lit cable is on the VLAN too
        // (a router on an access port, say).
        for link in links {
            if ports.contains(LinksLayer.PortKey(device: link.a.deviceId, port: link.a.port)) { devices.insert(link.b.deviceId) }
            if ports.contains(LinksLayer.PortKey(device: link.b.deviceId, port: link.b.port)) { devices.insert(link.a.deviceId) }
        }
        return VlanDisplay(vlan: info.vlan, ports: ports, devices: devices)
    }
}

/// Right pane in the VLAN view: every VLAN on the switches; the chosen one
/// is lit on the canvas, with its hosts, trunks, gateways and root bridge.
struct VlanPanel: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController

    var body: some View {
        let vlans = simulation.vlanView()
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Spacing.xl) {
                if vlans.isEmpty {
                    EmptyState(glyph: .system("square.stack.3d.down.right"), title: "No switches",
                               message: "VLANs live on switches. Add one and create a VLAN to see who's in it.")
                        .frame(maxWidth: .infinity, minHeight: 240)
                } else {
                    let current = vlans.first { $0.vlan == workspace.selectedVlan } ?? vlans[0]
                    list(vlans, current: current.vlan)
                    detail(current)
                }
            }
            .padding(Theme.Spacing.l)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func name(_ id: UInt64) -> String { document.device(id)?.name ?? "?" }

    @ViewBuilder
    private func list(_ vlans: [VlanInfo], current: UInt16) -> some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
            SectionHeader("VLANs")
            ForEach(vlans, id: \.vlan) { v in
                let selected = v.vlan == current
                Button {
                    workspace.selectedVlan = v.vlan
                } label: {
                    HStack(spacing: Theme.Spacing.s) {
                        RoundedRectangle(cornerRadius: 2)
                            .fill(v.vlan == 1 ? Theme.Palette.link : LinksLayer.vlanColor(v.vlan))
                            .frame(width: 8, height: 8)
                        Text("\(v.vlan)").font(Theme.Font.mono).frame(width: 34, alignment: .leading)
                        Text(v.name).font(Theme.Font.label)
                        Spacer(minLength: Theme.Spacing.s)
                        Text("\(v.hosts.count) host\(v.hosts.count == 1 ? "" : "s")")
                            .font(Theme.Font.caption.monospacedDigit())
                            .foregroundStyle(Theme.Palette.textSecondary)
                    }
                    .padding(.vertical, 5)
                    .padding(.horizontal, 6)
                    .background(selected ? Theme.Palette.accent.opacity(0.14) : .clear,
                                in: RoundedRectangle(cornerRadius: Theme.Radius.s))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .padding(.horizontal, -6)
            }
        }
    }

    @ViewBuilder
    private func detail(_ v: VlanInfo) -> some View {
        let access = v.members.filter { $0.mode == "access" }
        let trunks = v.members.filter { $0.mode == "trunk" }
        let blocked = v.members.filter { !$0.forwarding }
        VStack(alignment: .leading, spacing: Theme.Spacing.l) {
            VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                Text("VLAN \(v.vlan) · \(v.name)").font(Theme.Font.title)
                Text(summary(v))
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            group("Hosts", v.hosts.map(name).sorted())
            group("Access ports", access.map { "\(name($0.deviceId)) \($0.port.abbreviatedInterface)" })
            group("Trunks carrying it", trunks.map { "\(name($0.deviceId)) \($0.port.abbreviatedInterface)" })
            group("Gateways", v.gateways)
            if !blocked.isEmpty {
                group("Blocked by spanning tree", blocked.map { "\(name($0.deviceId)) \($0.port.abbreviatedInterface)" })
            }
            if let root = v.root {
                HStack(spacing: Theme.Spacing.s) {
                    RootBadge()
                    Text("\(name(root)) is the root bridge for VLAN \(v.vlan)").font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.textSecondary)
                }
            }
        }
    }

    /// One sentence on what the VLAN means for its hosts.
    private func summary(_ v: VlanInfo) -> String {
        let hosts = v.hosts.map(name).sorted()
        let reach = switch hosts.count {
        case 0: "No end devices are on it yet."
        case 1: "\(hosts[0]) is the only end device on it."
        default: "\(hosts.dropLast().joined(separator: ", ")) and \(hosts.last!) share this broadcast domain: they reach each other directly."
        }
        let routed = v.gateways.isEmpty
            ? " Nothing routes for it, so its hosts can't reach other VLANs."
            : " \(v.gateways.count == 1 ? "A gateway routes" : "Gateways route") it to other networks."
        return reach + routed
    }

    @ViewBuilder
    private func group(_ title: String, _ items: [String]) -> some View {
        if !items.isEmpty {
            VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                SectionHeader(title)
                Text(items.joined(separator: "\n"))
                    .font(Theme.Font.mono)
                    .foregroundStyle(Theme.Palette.textPrimary)
                    .textSelection(.enabled)
            }
        }
    }
}
