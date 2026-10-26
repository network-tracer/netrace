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

    var color: Color {
        switch self {
        case .root: Theme.Palette.statusUp
        case .designated: Color(light: 0x1F7AE0, dark: 0x4C9EFF)
        case .alternate, .backup: Theme.Palette.statusDown
        }
    }
}

/// Right pane in the STP view: the root and why, every switch's path to
/// it, every port's role and the reason, and the link-failure replay.
struct SpanningTreePanel: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController
    @State private var playing = false

    var body: some View {
        let view = simulation.stpView(vlan: workspace.stpVlan)
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Spacing.l) {
                if let view, !view.bridges.isEmpty {
                    header(view)
                    if let link = workspace.stpFailedLink {
                        failure(link: link)
                    } else {
                        Text("Right-click a cable and choose Fail Link to watch the tree reconverge.")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textTertiary)
                    }
                    bridges(view)
                    ports(view)
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

    @ViewBuilder
    private func header(_ view: StpView) -> some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            HStack {
                Text("Spanning Tree").font(Theme.Font.title)
                Spacer()
                Picker("VLAN", selection: $workspace.stpVlan) {
                    ForEach(view.vlans, id: \.self) { Text("VLAN \($0)").tag($0) }
                }
                .labelsHidden()
                .fixedSize()
                .controlSize(.small)
            }
            if let root = view.bridges.first(where: \.isRoot) {
                Label {
                    Text("\(name(root.deviceId)) is the root bridge").font(Theme.Font.headline)
                } icon: {
                    Image(systemName: "crown.fill").foregroundStyle(Theme.Palette.statusWarning)
                }
                Text(root.reason)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack(spacing: Theme.Spacing.m) {
                ForEach([StpRole.root, .designated, .alternate], id: \.self) { role in
                    HStack(spacing: 4) {
                        RoleBadge(role: role)
                        Text(role == .alternate ? "Blocked" : role.title).font(Theme.Font.caption)
                    }
                }
            }
            .foregroundStyle(Theme.Palette.textSecondary)
        }
    }

    @ViewBuilder
    private func failure(link: UInt64) -> some View {
        let result = simulation.stpLinkFailure(link: link, vlan: workspace.stpVlan)
        let changes = result?.changes ?? []
        InspectorBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.s) {
                HStack {
                    Image(systemName: "bolt.horizontal.circle.fill").foregroundStyle(Theme.Palette.statusDown)
                    Text("Link failure: \(linkName(link))").font(Theme.Font.headline)
                }
                if changes.isEmpty {
                    Text("Nothing changes: this link wasn't part of the tree for VLAN \(workspace.stpVlan).")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.textSecondary)
                }
                ForEach(Array(changes.enumerated()), id: \.offset) { index, change in
                    HStack(alignment: .top, spacing: Theme.Spacing.s) {
                        Text("\(index + 1)")
                            .font(Theme.Font.captionMedium)
                            .foregroundStyle(index < workspace.stpStep ? Theme.Palette.onAccent : Theme.Palette.textTertiary)
                            .frame(width: 18, height: 18)
                            .background(Circle().fill(index < workspace.stpStep ? Theme.Palette.accent : Theme.Palette.hover))
                        Text(change.explanation)
                            .font(Theme.Font.caption)
                            .foregroundStyle(index < workspace.stpStep ? Theme.Palette.textPrimary : Theme.Palette.textTertiary)
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
                    Button("Restore Link") {
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
        guard let link = document.link(id) else { return "cable" }
        return "\(name(link.a.deviceId)) \(link.a.port.abbreviatedInterface) – \(name(link.b.deviceId)) \(link.b.port.abbreviatedInterface)"
    }

    @ViewBuilder
    private func bridges(_ view: StpView) -> some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            SectionHeader("Switches")
            ForEach(view.bridges.sorted { name($0.deviceId) < name($1.deviceId) }, id: \.deviceId) { bridge in
                VStack(alignment: .leading, spacing: 2) {
                    HStack {
                        Text(name(bridge.deviceId)).font(Theme.Font.labelSemibold)
                        if bridge.isRoot {
                            Image(systemName: "crown.fill").font(.system(size: 9)).foregroundStyle(Theme.Palette.statusWarning)
                        }
                        Spacer()
                        Text(bridge.isRoot ? "root" : "cost \(bridge.rootCost) via \(bridge.rootPort?.abbreviatedInterface ?? "–")")
                            .font(Theme.Font.monoSmall)
                            .foregroundStyle(Theme.Palette.textSecondary)
                    }
                    Text("Bridge ID \(bridge.bridgeId)")
                        .font(Theme.Font.monoSmall)
                        .foregroundStyle(Theme.Palette.textTertiary)
                }
                .padding(.vertical, 2)
                .contentShape(Rectangle())
                .onTapGesture { workspace.selectDevices([bridge.deviceId]) }
            }
        }
    }

    @ViewBuilder
    private func ports(_ view: StpView) -> some View {
        let selected = workspace.selection
        let ordered = view.ports.sorted {
            let a = (selected.contains($0.deviceId) ? 0 : 1, name($0.deviceId), $0.port)
            let b = (selected.contains($1.deviceId) ? 0 : 1, name($1.deviceId), $1.port)
            return a < b
        }
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            SectionHeader("Ports")
            ForEach(ordered, id: \.self) { port in
                HStack(alignment: .top, spacing: Theme.Spacing.s) {
                    RoleBadge(role: port.role)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(name(port.deviceId)) \(port.port.abbreviatedInterface) · \(port.role.title) · cost \(port.cost)")
                            .font(Theme.Font.label)
                        Text(port.reason)
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
        }
    }
}

struct RoleBadge: View {
    let role: StpRole

    var body: some View {
        Text(role.letter)
            .font(.system(size: 9, weight: .bold))
            .foregroundStyle(.white)
            .frame(width: 15, height: 15)
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
