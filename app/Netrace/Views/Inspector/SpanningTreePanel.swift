import SwiftUI

/// What the canvas shows in the STP view: each port's role, the root, and
/// (while a link failure replays) which ports just changed. Built from the
/// engine's computed tree and its list of changes, step by step.
struct StpDisplay {
    var roles: [LinksLayer.PortKey: StpRole] = [:]
    var root: UInt64?
    var failedLink: UInt64?
    var changed: Set<LinksLayer.PortKey> = []

    @MainActor
    static func make(simulation: SimulationController, workspace: WorkspaceModel) -> StpDisplay? {
        guard workspace.rightPane == .spanningTree, workspace.showsInspector,
              let view = simulation.stpView(vlan: workspace.stpVlan) else { return nil }
        var display = StpDisplay()
        for port in view.ports {
            display.roles[LinksLayer.PortKey(device: port.deviceId, port: port.port)] = port.role
        }
        display.root = view.bridges.first(where: \.isRoot)?.deviceId
        guard let link = workspace.stpFailedLink,
              let failure = simulation.stpLinkFailure(link: link, vlan: workspace.stpVlan) else { return display }
        display.failedLink = link
        // Apply the first `stpStep` changes, as the replay has reached them.
        for change in failure.changes.prefix(workspace.stpStep) {
            guard let port = change.port else {
                display.root = failure.after.bridges.first(where: \.isRoot)?.deviceId
                continue
            }
            let key = LinksLayer.PortKey(device: change.deviceId, port: port)
            display.roles[key] = change.after
            display.changed = [key]
        }
        return display
    }
}

extension StpRole {
    var title: String {
        switch self {
        case .root: "Root port"
        case .designated: "Designated"
        case .alternate: "Alternate (blocked)"
        case .backup: "Backup (blocked)"
        }
    }

    var letter: String {
        switch self {
        case .root: "R"
        case .designated: "D"
        case .alternate, .backup: "✕"
        }
    }

    var forwards: Bool { self == .root || self == .designated }

    /// Gold for root ports (each switch's way to the gold root bridge),
    /// neutral ink for designated ports, red for blocked ones.
    var color: Color {
        switch self {
        case .root: Theme.Palette.root
        case .designated: Theme.Palette.symbolInk
        case .alternate, .backup: Theme.Palette.statusDown
        }
    }

    var letterColor: Color {
        switch self {
        case .root: Theme.Palette.onRoot
        case .designated: Theme.Palette.symbolKnockout
        case .alternate, .backup: .white
        }
    }
}

/// Right pane in the STP view, built for scanning: the root and why, the
/// blocked ports (what people look for), each switch's way to the root,
/// then the forwarding ports. Reasons open on click.
struct SpanningTreePanel: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController
    @State private var playing = false
    @State private var expanded: Set<String> = []

    var body: some View {
        let view = simulation.stpView(vlan: workspace.stpVlan)
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Spacing.xl) {
                if let view, !view.bridges.isEmpty {
                    summary(view)
                    if let link = workspace.stpFailedLink {
                        failure(link: link)
                    }
                    blocked(view)
                    paths(view)
                    forwarding(view)
                    footnote
                } else {
                    EmptyState(glyph: .system("point.3.connected.trianglepath.dotted"), title: "No spanning tree",
                               message: "Spanning tree runs between switches. Cable two or more switches together to see it.")
                        .frame(maxWidth: .infinity, minHeight: 240)
                }
            }
            .padding(Theme.Spacing.l)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .task(id: playing) {
            guard playing else { return }
            while playing, let link = workspace.stpFailedLink,
                  let failure = simulation.stpLinkFailure(link: link, vlan: workspace.stpVlan),
                  workspace.stpStep < failure.changes.count {
                try? await Task.sleep(for: .seconds(1.2))
                guard playing else { return }
                withAnimation(.easeOut(duration: 0.3)) { workspace.stpStep += 1 }
            }
            playing = false
        }
    }

    private func name(_ id: UInt64) -> String { document.device(id)?.name ?? "?" }

    private func toggle(_ key: String) {
        withAnimation(.easeOut(duration: 0.15)) {
            if expanded.contains(key) { expanded.remove(key) } else { expanded.insert(key) }
        }
    }

    // MARK: Summary

    @ViewBuilder
    private func summary(_ view: StpView) -> some View {
        let blocked = view.ports.filter { !$0.role.forwards }.count
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            if view.vlans.count > 1 {
                Menu {
                    ForEach(view.vlans, id: \.self) { vlan in
                        Button("VLAN \(vlan)") { workspace.stpVlan = vlan }
                    }
                } label: {
                    HStack(spacing: 4) {
                        Text("VLAN \(view.vlan)").font(Theme.Font.title)
                        Image(systemName: "chevron.up.chevron.down")
                            .font(.system(size: 9, weight: .semibold))
                            .foregroundStyle(Theme.Palette.textTertiary)
                    }
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .help("Each VLAN has its own spanning tree")
            } else {
                Text("VLAN \(view.vlan)").font(Theme.Font.title)
            }
            if let root = view.bridges.first(where: \.isRoot) {
                ExplainRow(expanded: expanded.contains("root"), action: { toggle("root") }) {
                    HStack(spacing: Theme.Spacing.s) {
                        RootBadge()
                        Text(name(root.deviceId)).font(Theme.Font.headline)
                        Spacer(minLength: Theme.Spacing.s)
                        Text("root bridge")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textTertiary)
                    }
                } detail: {
                    Text(root.reason + "  Bridge ID \(root.bridgeId).")
                }
            }
            HStack(spacing: Theme.Spacing.m) {
                Stat(value: "\(view.bridges.count)", label: view.bridges.count == 1 ? "switch" : "switches")
                Stat(value: "\(blocked)", label: blocked == 1 ? "blocked port" : "blocked ports",
                     color: blocked > 0 ? Theme.Palette.statusDown : Theme.Palette.textPrimary)
            }
        }
    }

    // MARK: Failure replay

    @ViewBuilder
    private func failure(link: UInt64) -> some View {
        let changes = simulation.stpLinkFailure(link: link, vlan: workspace.stpVlan)?.changes ?? []
        InspectorBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.s) {
                HStack(spacing: Theme.Spacing.s) {
                    Image(systemName: "bolt.horizontal.circle.fill").foregroundStyle(Theme.Palette.statusDown)
                    Text(linkName(link)).font(Theme.Font.headline)
                }
                if changes.isEmpty {
                    Text("Nothing changes: this link wasn't part of the tree for VLAN \(workspace.stpVlan).")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.textSecondary)
                }
                ForEach(Array(changes.enumerated()), id: \.offset) { index, change in
                    let done = index < workspace.stpStep
                    HStack(alignment: .firstTextBaseline, spacing: Theme.Spacing.s) {
                        Text("\(index + 1)")
                            .font(.system(size: 10, weight: .semibold).monospacedDigit())
                            .foregroundStyle(done ? Theme.Palette.onAccent : Theme.Palette.textTertiary)
                            .frame(width: 16, height: 16)
                            .background(Circle().fill(done ? Theme.Palette.accent : Theme.Palette.hover))
                        Text(change.explanation)
                            .font(Theme.Font.caption)
                            .foregroundStyle(done ? Theme.Palette.textPrimary : Theme.Palette.textTertiary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                HStack {
                    Button(playing ? "Pause" : (workspace.stpStep == 0 ? "Play" : "Continue"),
                           systemImage: playing ? "pause.fill" : "play.fill") { playing.toggle() }
                        .disabled(workspace.stpStep >= changes.count)
                    Button("Step", systemImage: "forward.frame.fill") {
                        withAnimation { workspace.stpStep = min(changes.count, workspace.stpStep + 1) }
                    }
                    .disabled(workspace.stpStep >= changes.count)
                    Spacer()
                    Button("Restore") {
                        playing = false
                        workspace.stpFailedLink = nil
                        workspace.stpStep = 0
                    }
                }
                .controlSize(.small)
            }
        }
    }

    private func linkName(_ id: UInt64) -> String {
        guard let link = document.link(id) else { return "Cable failed" }
        return "\(name(link.a.deviceId)) \(link.a.port.abbreviatedInterface) – \(name(link.b.deviceId)) \(link.b.port.abbreviatedInterface) failed"
    }

    // MARK: Ports and paths

    /// Ports of the selected switch first, then by switch and port.
    private func ordered(_ ports: [StpPortInfo]) -> [StpPortInfo] {
        let selected = workspace.selection
        return ports.sorted {
            (selected.contains($0.deviceId) ? 0 : 1, name($0.deviceId), $0.port)
                < (selected.contains($1.deviceId) ? 0 : 1, name($1.deviceId), $1.port)
        }
    }

    private func portRow(_ port: StpPortInfo) -> some View {
        let key = "p\(port.deviceId)\(port.port)"
        return ExplainRow(expanded: expanded.contains(key), action: { toggle(key) }) {
            HStack(spacing: Theme.Spacing.s) {
                RoleBadge(role: port.role)
                Text(name(port.deviceId)).font(Theme.Font.label)
                Text(port.port.abbreviatedInterface)
                    .font(Theme.Font.monoSmall)
                    .foregroundStyle(Theme.Palette.textTertiary)
                Spacer(minLength: Theme.Spacing.s)
                Text(port.role.forwards ? "\(port.role == .root ? "root" : "designated") · \(port.cost)" : (port.role == .backup ? "backup" : "alternate"))
                    .font(Theme.Font.caption.monospacedDigit())
                    .foregroundStyle(Theme.Palette.textSecondary)
            }
        } detail: {
            Text(port.reason)
        }
    }

    @ViewBuilder
    private func blocked(_ view: StpView) -> some View {
        let ports = ordered(view.ports.filter { !$0.role.forwards })
        if !ports.isEmpty {
            VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                SectionHeader("Blocked")
                ForEach(ports, id: \.self) { portRow($0) }
            }
        }
    }

    @ViewBuilder
    private func paths(_ view: StpView) -> some View {
        let bridges = view.bridges.filter { !$0.isRoot }.sorted { name($0.deviceId) < name($1.deviceId) }
        if !bridges.isEmpty {
            VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                SectionHeader("Path to the root")
                ForEach(bridges, id: \.deviceId) { bridge in
                    let key = "b\(bridge.deviceId)"
                    ExplainRow(expanded: expanded.contains(key), action: { toggle(key) }) {
                        HStack(spacing: Theme.Spacing.s) {
                            Text(name(bridge.deviceId)).font(Theme.Font.label)
                            Spacer(minLength: Theme.Spacing.s)
                            Text("via \(bridge.rootPort?.abbreviatedInterface ?? "–") · cost \(bridge.rootCost)")
                                .font(Theme.Font.caption.monospacedDigit())
                                .foregroundStyle(Theme.Palette.textSecondary)
                        }
                    } detail: {
                        Text(bridge.reason + "  Bridge ID \(bridge.bridgeId).")
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func forwarding(_ view: StpView) -> some View {
        let ports = ordered(view.ports.filter(\.role.forwards))
        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
            SectionHeader("Forwarding")
            ForEach(ports, id: \.self) { portRow($0) }
        }
    }

    private var footnote: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            HStack(spacing: Theme.Spacing.m) {
                ForEach([StpRole.root, .designated, .alternate], id: \.self) { role in
                    HStack(spacing: 4) {
                        RoleBadge(role: role, size: 13)
                        Text(role == .root ? "root port" : role == .designated ? "designated" : "blocked")
                    }
                }
            }
            if workspace.stpFailedLink == nil {
                Text("Right-click a cable between switches and choose Fail Link to watch the tree reconverge.")
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .font(Theme.Font.caption)
        .foregroundStyle(Theme.Palette.textTertiary)
    }
}

struct RoleBadge: View {
    let role: StpRole
    var size: CGFloat = 15

    var body: some View {
        Text(role.letter)
            .font(.system(size: size * 0.6, weight: .bold))
            .foregroundStyle(role.letterColor)
            .frame(width: size, height: size)
            .background(Circle().fill(role.color))
            .accessibilityLabel(role.title)
    }
}

/// A rounded sunken box for grouped content in narrow panes.
struct InspectorBox<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        content
            .padding(Theme.Spacing.m)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous))
    }
}
