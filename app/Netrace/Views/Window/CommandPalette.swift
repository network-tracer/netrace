import SwiftUI

/// ⌘K: search and run any action or add any device.
struct CommandPalette: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager
    @State private var query = ""
    @State private var highlighted = 0
    @FocusState private var fieldFocused: Bool

    struct Command: Identifiable {
        let id: String
        let title: String
        let group: String
        let glyph: Glyph
        var shortcut: String = ""
        let run: () -> Void
    }

    var body: some View {
        let results = filtered
        ZStack(alignment: .top) {
            Color.black.opacity(0.12)
                .ignoresSafeArea()
                .onTapGesture { close() }

            VStack(spacing: 0) {
                HStack(spacing: Theme.Spacing.s) {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(Theme.Palette.textTertiary)
                    TextField("Search commands and devices", text: $query)
                        .textFieldStyle(.plain)
                        .font(.system(size: 15))
                        .focused($fieldFocused)
                        .onSubmit { run(results) }
                    Text("esc")
                        .font(Theme.Font.monoSmall)
                        .foregroundStyle(Theme.Palette.textTertiary)
                }
                .padding(Theme.Spacing.m)
                PanelDivider(axis: .horizontal)

                ScrollViewReader { proxy in
                    ScrollView {
                        VStack(alignment: .leading, spacing: 1) {
                            if results.isEmpty {
                                Text("No matches")
                                    .font(Theme.Font.body)
                                    .foregroundStyle(Theme.Palette.textTertiary)
                                    .padding(Theme.Spacing.m)
                            }
                            ForEach(Array(results.enumerated()), id: \.element.id) { index, command in
                                row(command, isHighlighted: index == highlighted)
                                    .id(index)
                                    .onTapGesture {
                                        highlighted = index
                                        run(results)
                                    }
                            }
                        }
                        .padding(Theme.Spacing.xs + 2)
                    }
                    .frame(maxHeight: 340)
                    .onChange(of: highlighted) { _, index in proxy.scrollTo(index) }
                }
            }
            .frame(width: 560)
            .background(Theme.Palette.surface, in: RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous).strokeBorder(Theme.Palette.borderStrong))
            .shadow(color: .black.opacity(0.18), radius: 24, y: 8)
            .padding(.top, 72)
        }
        .onAppear { fieldFocused = true }
        .onChange(of: query) { _, _ in highlighted = 0 }
        .onKeyPress(.downArrow) { highlighted = min(highlighted + 1, max(results.count - 1, 0)); return .handled }
        .onKeyPress(.upArrow) { highlighted = max(highlighted - 1, 0); return .handled }
        .onKeyPress(.escape) { close(); return .handled }
    }

    private func row(_ command: Command, isHighlighted: Bool) -> some View {
        HStack(spacing: Theme.Spacing.s + 2) {
            IconView(command.glyph, size: 16)
                .foregroundStyle(isHighlighted ? Theme.Palette.onAccent : Theme.Palette.textSecondary)
                .frame(width: 22)
            Text(command.title)
                .font(Theme.Font.body)
                .foregroundStyle(isHighlighted ? Theme.Palette.onAccent : Theme.Palette.textPrimary)
            Spacer()
            Text(command.shortcut.isEmpty ? command.group : command.shortcut)
                .font(command.shortcut.isEmpty ? Theme.Font.caption : Theme.Font.monoSmall)
                .foregroundStyle(isHighlighted ? Theme.Palette.onAccent.opacity(0.8) : Theme.Palette.textTertiary)
        }
        .padding(.horizontal, Theme.Spacing.s)
        .frame(height: 30)
        .background(
            RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
                .fill(isHighlighted ? Theme.Palette.accent : .clear)
        )
        .contentShape(Rectangle())
    }

    private var filtered: [Command] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return commands }
        return commands.filter { $0.title.lowercased().contains(q) || $0.group.lowercased().contains(q) }
    }

    private func run(_ results: [Command]) {
        guard results.indices.contains(highlighted) else { return }
        let command = results[highlighted]
        close()
        command.run()
    }

    private func close() {
        workspace.showsCommandPalette = false
    }

    private var commands: [Command] {
        let ws = workspace
        let ids = document.state.devices.map(\.id)
        var list: [Command] = DeviceKind.allCases.map { kind in
            Command(id: "add.\(kind)", title: "Add \(kind.displayName)", group: "Devices",
                    glyph: kind.category == .network ? .custom(.topology) : .system("desktopcomputer")) {
                let device = document.addDevice(kind, at: CGPoint(x: 240, y: 200).snapped, undoManager: undoManager)
                ws.selectDevices([device.id])
            }
        }
        list += WorkspaceModel.Tool.allCases.map { tool in
            Command(id: "tool.\(tool)", title: "\(tool.title) Tool", group: "Tools", glyph: tool.glyph,
                    shortcut: tool.key.uppercased()) { ws.tool = tool }
        }
        list += [
            Command(id: "mode.realtime", title: "Realtime Mode", group: "Simulation", glyph: .system("clock"), shortcut: "⌃⌘R") { ws.mode = .realtime },
            Command(id: "mode.simulation", title: "Simulation Mode", group: "Simulation", glyph: .system("stopwatch"), shortcut: "⌃⌘S") { ws.mode = .simulation },
            Command(id: "sim.play", title: ws.isPlaying ? "Pause Simulation" : "Play Simulation", group: "Simulation", glyph: .system(ws.isPlaying ? "pause.fill" : "play.fill")) {
                ws.mode = .simulation
                ws.isPlaying.toggle()
            },
            Command(id: "sim.reset", title: "Reset Simulation", group: "Simulation", glyph: .system("backward.end.fill")) {
                ws.isPlaying = false
                ws.simulationTime = 0
            },
            Command(id: "power.start", title: "Start All Devices", group: "Devices", glyph: .system("power")) { ws.setPower(.running, for: ids) },
            Command(id: "power.stop", title: "Stop All Devices", group: "Devices", glyph: .system("power")) { ws.setPower(.stopped, for: ids) },
            Command(id: "panel.console", title: "Show Console", group: "Panels", glyph: .system("terminal")) {
                ws.bottomTab = .console
                ws.showsBottomPanel = true
            },
            Command(id: "panel.events", title: "Show Events", group: "Panels", glyph: .system("list.bullet.rectangle")) {
                ws.bottomTab = .events
                ws.showsBottomPanel = true
            },
            Command(id: "panel.capture", title: "Show Capture", group: "Panels", glyph: .custom(.capture)) {
                ws.bottomTab = .capture
                ws.showsBottomPanel = true
            },
            Command(id: "panel.lab", title: "Show Lab", group: "Panels", glyph: .custom(.lab), shortcut: "⌥⌘4") { ws.openLab() },
            Command(id: "panel.maximise", title: ws.bottomPanelMaximized ? "Restore Bottom Panel" : "Maximise Bottom Panel",
                    group: "Panels", glyph: .system("rectangle.expand.vertical"), shortcut: "⌥⌘↑") {
                ws.showsBottomPanel = true
                ws.bottomPanelMaximized.toggle()
            },
            Command(id: "panel.sidebar", title: ws.showsSidebar ? "Hide Sidebar" : "Show Sidebar", group: "Panels", glyph: .system("sidebar.left"), shortcut: "⌥⌘1") { ws.showsSidebar.toggle() },
            Command(id: "panel.inspector", title: ws.showsInspector ? "Hide Inspector" : "Show Inspector", group: "Panels", glyph: .system("sidebar.right"), shortcut: "⌥⌘2") { ws.showsInspector.toggle() },
            Command(id: "panel.bottom", title: ws.showsBottomPanel ? "Hide Bottom Panel" : "Show Bottom Panel", group: "Panels", glyph: .system("rectangle.bottomthird.inset.filled"), shortcut: "⌥⌘3") { ws.showsBottomPanel.toggle() },
            Command(id: "view.labels", title: ws.showsInterfaceLabels ? "Hide Interface Labels" : "Show Interface Labels", group: "View", glyph: .system("tag"), shortcut: "⇧⌘L") { ws.showsInterfaceLabels.toggle() },
            Command(id: "view.grid", title: ws.showsGrid ? "Hide Grid" : "Show Grid", group: "View", glyph: .system("circle.grid.3x3")) { ws.showsGrid.toggle() },
            Command(id: "view.minimap", title: ws.showsMinimap ? "Hide Minimap" : "Show Minimap", group: "View", glyph: .system("map")) { ws.showsMinimap.toggle() },
            Command(id: "view.zoomin", title: "Zoom In", group: "View", glyph: .system("plus.magnifyingglass"), shortcut: "⌘+") { ws.zoomIn() },
            Command(id: "view.zoomout", title: "Zoom Out", group: "View", glyph: .system("minus.magnifyingglass"), shortcut: "⌘−") { ws.zoomOut() },
            Command(id: "view.actual", title: "Actual Size", group: "View", glyph: .system("1.magnifyingglass"), shortcut: "⌘0") { ws.setZoom(1) },
        ]
        return list
    }
}
