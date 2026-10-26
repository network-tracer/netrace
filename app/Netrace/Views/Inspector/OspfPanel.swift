import SwiftUI

/// What the canvas shows in the OSPF view: each OSPF interface's cost and
/// role on its segment, ports whose neighbour won't come up, and (while a
/// link failure replays) the failed cable.
struct OspfDisplay {
    struct Label: Equatable {
        var cost: UInt32
        /// "DR", "BDR", or empty.
        var role: String
        var problem: Bool
    }

    struct Adjacency: Equatable {
        var a: UInt64
        var b: UInt64
        /// "FULL", "2WAY" or "DOWN".
        var state: String
        var area: UInt32
    }

    var labels: [LinksLayer.PortKey: Label] = [:]
    var adjacencies: [Adjacency] = []
    /// More than one area: adjacencies are labelled with theirs.
    var multiArea = false
    var failedLink: UInt64?

    @MainActor
    static func make(simulation: SimulationController, workspace: WorkspaceModel) -> OspfDisplay? {
        guard workspace.showsOspf, let view = simulation.ospfView() else { return nil }
        var display = OspfDisplay()
        for router in view.routers {
            for iface in router.interfaces where !iface.port.isEmpty && iface.state != "LOOP" {
                let problem = router.neighbors.contains { $0.iface == iface.name && $0.problem != nil }
                let role = iface.state == "DR" || iface.state == "BDR" ? iface.state : ""
                display.labels[LinksLayer.PortKey(device: router.deviceId, port: iface.port)] =
                    Label(cost: iface.cost, role: role, problem: problem)
            }
        }
        // One line per pair of routers, the best state between them.
        let rank = ["FULL": 2, "2WAY": 1, "DOWN": 0]
        var seen: [String: Adjacency] = [:]
        for router in view.routers {
            for n in router.neighbors {
                let key = "\(min(router.deviceId, n.deviceId))-\(max(router.deviceId, n.deviceId))"
                let area = router.interfaces.first { $0.name == n.iface }?.area ?? 0
                let candidate = Adjacency(a: router.deviceId, b: n.deviceId, state: n.state, area: area)
                if let existing = seen[key], rank[existing.state, default: 0] >= rank[n.state, default: 0] { continue }
                seen[key] = candidate
            }
        }
        display.adjacencies = seen.values.sorted { ($0.a, $0.b) < ($1.a, $1.b) }
        display.multiArea = Set(view.routers.flatMap { $0.interfaces.map(\.area) }).count > 1
        display.failedLink = workspace.ospfFailedLink
        return display
    }
}

/// Right pane in the OSPF view, built for scanning: one router at a time,
/// one line per neighbour, route and interface, with the reasoning a click
/// away. Problems (a neighbour that won't come up) are always shown.
struct OspfPanel: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController
    @State private var playing = false
    @State private var focus: UInt64?
    /// Rows showing their explanation.
    @State private var expanded: Set<String> = []

    var body: some View {
        let view = simulation.ospfView()
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Spacing.xl) {
                if let view, !view.routers.isEmpty, let router = focused(in: view) {
                    summary(view, router: router)
                    if let link = workspace.ospfFailedLink {
                        failure(link: link)
                    }
                    neighbors(router)
                    routes(router)
                    interfaces(router)
                    segments(view)
                    database(view)
                    if workspace.ospfFailedLink == nil {
                        Text("Right-click a cable and choose Fail Link (OSPF View) to watch OSPF reconverge.")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textTertiary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                } else {
                    EmptyState(glyph: .system("arrow.triangle.branch"), title: "No OSPF",
                               message: "Configure router ospf with network statements on two or more connected routers to see their neighbours and the routes OSPF chooses.")
                        .frame(maxWidth: .infinity, minHeight: 240)
                }
            }
            .padding(Theme.Spacing.l)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .task(id: playing) {
            guard playing else { return }
            while playing, let link = workspace.ospfFailedLink,
                  let failure = simulation.ospfLinkFailure(link: link),
                  workspace.ospfStep < failure.changes.count {
                try? await Task.sleep(for: .seconds(1.2))
                guard playing else { return }
                withAnimation(.easeOut(duration: 0.3)) { workspace.ospfStep += 1 }
            }
            playing = false
        }
    }

    private func name(_ id: UInt64) -> String { document.device(id)?.name ?? "?" }

    /// The router picked in the pane, else the selected device, else the first.
    private func focused(in view: OspfView) -> OspfRouterInfo? {
        for id in [focus, workspace.selection.first].compactMap({ $0 }) {
            if let r = view.routers.first(where: { $0.deviceId == id }) { return r }
        }
        return view.routers.min { name($0.deviceId) < name($1.deviceId) }
    }

    private func toggle(_ key: String) {
        withAnimation(.easeOut(duration: 0.15)) {
            if expanded.contains(key) { expanded.remove(key) } else { expanded.insert(key) }
        }
    }

    // MARK: Summary

    @ViewBuilder
    private func summary(_ view: OspfView, router: OspfRouterInfo) -> some View {
        let full = router.neighbors.filter { $0.state == "FULL" }.count
        let down = router.neighbors.filter { $0.state == "DOWN" }.count
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            HStack(alignment: .firstTextBaseline) {
                Menu {
                    ForEach(view.routers.sorted { name($0.deviceId) < name($1.deviceId) }, id: \.deviceId) { r in
                        Button(name(r.deviceId)) { focus = r.deviceId }
                    }
                } label: {
                    HStack(spacing: 4) {
                        Text(name(router.deviceId)).font(Theme.Font.title)
                        Image(systemName: "chevron.up.chevron.down")
                            .font(.system(size: 9, weight: .semibold))
                            .foregroundStyle(Theme.Palette.textTertiary)
                    }
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .help("Choose a router (or select one on the canvas)")
                Spacer()
                if router.abr { Tag(text: "ABR", help: "Area border router: it joins area 0 to another area") }
                if router.asbr { Tag(text: "ASBR", help: "Advertises a default route into OSPF") }
            }
            ExplainRow(expanded: expanded.contains("rid"), action: { toggle("rid") }) {
                HStack(spacing: Theme.Spacing.s) {
                    Text("Router ID").font(Theme.Font.caption).foregroundStyle(Theme.Palette.textSecondary)
                    Text(router.routerId ?? "none").font(Theme.Font.mono).foregroundStyle(Theme.Palette.textPrimary)
                }
            } detail: {
                Text(router.routerIdReason)
            }
            HStack(spacing: Theme.Spacing.m) {
                Stat(value: "\(full)", label: full == 1 ? "adjacency" : "adjacencies")
                Stat(value: "\(router.routes.count)", label: router.routes.count == 1 ? "route" : "routes")
                if down > 0 {
                    Stat(value: "\(down)", label: down == 1 ? "problem" : "problems", color: Theme.Palette.statusDown)
                }
            }
        }
    }

    // MARK: Failure replay

    @ViewBuilder
    private func failure(link: UInt64) -> some View {
        let changes = simulation.ospfLinkFailure(link: link)?.changes ?? []
        InspectorBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.s) {
                HStack(spacing: Theme.Spacing.s) {
                    Image(systemName: "bolt.horizontal.circle.fill").foregroundStyle(Theme.Palette.statusDown)
                    Text(linkName(link)).font(Theme.Font.headline)
                }
                if changes.isEmpty {
                    Text("Nothing changes: no adjacency or route used this link.")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.textSecondary)
                }
                ForEach(Array(changes.enumerated()), id: \.offset) { index, change in
                    let done = index < workspace.ospfStep
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
                    Button(playing ? "Pause" : (workspace.ospfStep == 0 ? "Play" : "Continue"),
                           systemImage: playing ? "pause.fill" : "play.fill") { playing.toggle() }
                        .disabled(workspace.ospfStep >= changes.count)
                    Button("Step", systemImage: "forward.frame.fill") {
                        withAnimation { workspace.ospfStep = min(changes.count, workspace.ospfStep + 1) }
                    }
                    .disabled(workspace.ospfStep >= changes.count)
                    Spacer()
                    Button("Restore") {
                        playing = false
                        workspace.ospfFailedLink = nil
                        workspace.ospfStep = 0
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

    // MARK: Neighbours

    @ViewBuilder
    private func neighbors(_ router: OspfRouterInfo) -> some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
            SectionHeader("Neighbours")
            if router.neighbors.isEmpty {
                Placeholder("No other OSPF router shares a network with \(name(router.deviceId)).")
            }
            ForEach(router.neighbors, id: \.self) { n in
                let key = "n\(n.deviceId)\(n.iface)"
                ExplainRow(expanded: expanded.contains(key), action: { toggle(key) }) {
                    HStack(spacing: Theme.Spacing.s) {
                        NeighborDot(state: n.state)
                        Text(name(n.deviceId)).font(Theme.Font.label)
                        Text(n.iface.abbreviatedInterface)
                            .font(Theme.Font.monoSmall)
                            .foregroundStyle(Theme.Palette.textTertiary)
                        Spacer(minLength: Theme.Spacing.s)
                        Text(stateText(n))
                            .font(Theme.Font.caption)
                            .foregroundStyle(n.state == "DOWN" ? Theme.Palette.statusDown : Theme.Palette.textSecondary)
                    }
                } detail: {
                    Text(n.reason + "  Router ID \(n.routerId).")
                }
                // What's wrong stays in view: it's the thing to fix.
                if let problem = n.problem, !expanded.contains(key) {
                    Text(problem.prefix(1).uppercased() + problem.dropFirst() + ".")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.statusDown)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.leading, 22)
                        .padding(.bottom, Theme.Spacing.xs)
                }
            }
        }
    }

    /// "Full · DR", "2-Way", "Down".
    private func stateText(_ n: OspfNeighborInfo) -> String {
        let state = switch n.state {
        case "FULL": "Full"
        case "2WAY": "2-Way"
        default: "Down"
        }
        guard n.state != "DOWN", n.role == "DR" || n.role == "BDR" else { return state }
        return "\(state) · \(n.role)"
    }

    // MARK: Routes

    @ViewBuilder
    private func routes(_ router: OspfRouterInfo) -> some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
            SectionHeader("Routes learned")
            if router.routes.isEmpty {
                Placeholder("None yet: \(name(router.deviceId)) needs a full adjacency to learn routes.")
            }
            ForEach(router.routes, id: \.self) { r in
                let key = "r\(r.prefix)"
                ExplainRow(expanded: expanded.contains(key), action: { toggle(key) }) {
                    VStack(alignment: .leading, spacing: 1) {
                        HStack(spacing: Theme.Spacing.s) {
                            Text(r.prefix).font(Theme.Font.mono).foregroundStyle(Theme.Palette.textPrimary)
                            if r.code != "O" {
                                Text(r.code == "O IA" ? "inter-area" : "external")
                                    .font(Theme.Font.caption)
                                    .foregroundStyle(Theme.Palette.textTertiary)
                            }
                            Spacer(minLength: Theme.Spacing.s)
                            Text("cost \(r.cost)")
                                .font(Theme.Font.caption.monospacedDigit())
                                .foregroundStyle(Theme.Palette.textSecondary)
                        }
                        Text(r.path.count > 1 ? r.path.joined(separator: " → ") : r.nextHops.joined(separator: ", "))
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textTertiary)
                            .lineLimit(1)
                    }
                } detail: {
                    VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                        Text(r.reason)
                        Text(r.nextHops.map { "Next hop \($0)" }.joined(separator: "\n"))
                            .font(Theme.Font.monoSmall)
                        if !r.alternatives.isEmpty {
                            Text("Not chosen: " + r.alternatives.joined(separator: "; ") + ".")
                                .foregroundStyle(Theme.Palette.textTertiary)
                        }
                    }
                }
            }
        }
    }

    // MARK: Interfaces

    @ViewBuilder
    private func interfaces(_ router: OspfRouterInfo) -> some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
            SectionHeader("Interfaces")
            ForEach(router.interfaces, id: \.self) { i in
                let key = "i\(i.name)"
                ExplainRow(expanded: expanded.contains(key), action: { toggle(key) }) {
                    HStack(spacing: Theme.Spacing.s) {
                        Text(i.name.abbreviatedInterface.replacingOccurrences(of: "Loopback", with: "Lo"))
                            .font(Theme.Font.label)
                            .lineLimit(1)
                            .fixedSize()
                            .frame(minWidth: 44, alignment: .leading)
                        Text(i.address)
                            .font(Theme.Font.monoSmall)
                            .foregroundStyle(Theme.Palette.textSecondary)
                            .lineLimit(1)
                        Spacer(minLength: Theme.Spacing.s)
                        Text(interfaceRole(i))
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textTertiary)
                    }
                } detail: {
                    Text("Area \(i.area), cost \(i.cost). " + i.why.prefix(1).uppercased() + i.why.dropFirst() + "."
                         + (i.state == "LOOP" ? " Loopbacks are advertised as /32 host routes." : "")
                         + (i.passive ? " Passive: the network is advertised, but no hellos are sent." : ""))
                }
            }
        }
    }

    private func interfaceRole(_ i: OspfIfaceInfo) -> String {
        if i.passive { return "passive" }
        return switch i.state {
        case "DR": "DR"
        case "BDR": "BDR"
        case "DROTH": "DROther"
        case "P2P": "point-to-point"
        case "LOOP": "loopback"
        default: "waiting"
        }
    }

    // MARK: Shared networks and database

    @ViewBuilder
    private func segments(_ view: OspfView) -> some View {
        if !view.segments.isEmpty {
            VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                SectionHeader("DR elections")
                ForEach(view.segments, id: \.network) { s in
                    let key = "s\(s.network)"
                    ExplainRow(expanded: expanded.contains(key), action: { toggle(key) }) {
                        HStack(spacing: Theme.Spacing.s) {
                            Text(s.network).font(Theme.Font.monoSmall).foregroundStyle(Theme.Palette.textSecondary)
                            Spacer(minLength: Theme.Spacing.s)
                            if let dr = s.dr {
                                Text("DR \(name(dr.deviceId))\(s.bdr.map { "  BDR \(name($0.deviceId))" } ?? "")")
                                    .font(Theme.Font.caption)
                                    .foregroundStyle(Theme.Palette.textPrimary)
                            } else {
                                Text("point-to-point")
                                    .font(Theme.Font.caption)
                                    .foregroundStyle(Theme.Palette.textTertiary)
                            }
                        }
                    } detail: {
                        Text(s.election)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func database(_ view: OspfView) -> some View {
        let count = view.areas.reduce(0) { $0 + $1.lsas.count } + view.external.count
        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
            ExplainRow(expanded: expanded.contains("db"), action: { toggle("db") }) {
                HStack {
                    SectionHeader("Link-state database")
                    Text("\(count) LSAs").font(Theme.Font.caption).foregroundStyle(Theme.Palette.textTertiary)
                }
            } detail: {
                VStack(alignment: .leading, spacing: Theme.Spacing.m) {
                    ForEach(view.areas, id: \.area) { area in
                        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                            Text("Area \(area.area)").font(Theme.Font.captionMedium).foregroundStyle(Theme.Palette.textPrimary)
                            ForEach(area.lsas, id: \.self) { LsaRow(lsa: $0) }
                        }
                    }
                    if !view.external.isEmpty {
                        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                            Text("External").font(Theme.Font.captionMedium).foregroundStyle(Theme.Palette.textPrimary)
                            ForEach(view.external, id: \.self) { LsaRow(lsa: $0) }
                        }
                    }
                }
            }
        }
    }
}

private struct LsaRow: View {
    let lsa: OspfLsaInfo

    var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            Text("\(lsa.kind)  \(lsa.linkId)")
                .font(Theme.Font.monoSmall)
                .foregroundStyle(Theme.Palette.textPrimary)
            Text(lsa.describe)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.Palette.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// Full is green, 2-Way (normal between two DROthers) a hollow ring, Down red.
private struct NeighborDot: View {
    let state: String

    var body: some View {
        Group {
            switch state {
            case "FULL": Circle().fill(Theme.Palette.statusUp)
            case "2WAY": Circle().strokeBorder(Theme.Palette.textSecondary, lineWidth: 1.5)
            default: Circle().fill(Theme.Palette.statusDown)
            }
        }
        .frame(width: 7, height: 7)
        .frame(width: 14)
        .accessibilityLabel(state)
    }
}

private struct Tag: View {
    let text: String
    let help: String

    var body: some View {
        Text(text)
            .font(.system(size: 9.5, weight: .semibold))
            .tracking(0.3)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(Theme.Palette.hover, in: Capsule())
            .foregroundStyle(Theme.Palette.textSecondary)
            .help(help)
    }
}
