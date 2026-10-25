import SwiftUI

/// Tabbed panel under the canvas: Console, Events, Capture, Lab.
struct BottomPanel: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @State private var dragStartHeight: CGFloat?

    var body: some View {
        VStack(spacing: 0) {
            // Resize handle on the top edge.
            PanelDivider(axis: .horizontal)
                .overlay {
                    Color.clear
                        .frame(height: 8)
                        .contentShape(Rectangle())
                        .onHover { inside in
                            if inside { NSCursor.resizeUpDown.push() } else { NSCursor.pop() }
                        }
                        .gesture(
                            DragGesture(coordinateSpace: .global)
                                .onChanged { value in
                                    workspace.bottomPanelMaximized = false
                                    if dragStartHeight == nil { dragStartHeight = workspace.bottomPanelHeight }
                                    workspace.bottomPanelHeight = min(560, max(150, (dragStartHeight ?? 0) - value.translation.height))
                                }
                                .onEnded { _ in dragStartHeight = nil }
                        )
                }

            HStack(spacing: Theme.Spacing.m) {
                TabStrip(tabs: WorkspaceModel.BottomTab.allCases, selection: $workspace.bottomTab,
                         title: \.title, glyph: \.glyph)
                Spacer()
                IconButton(glyph: .system(workspace.bottomPanelMaximized ? "rectangle.compress.vertical" : "rectangle.expand.vertical"),
                           help: workspace.bottomPanelMaximized ? "Restore Panel (⌥⌘↑)" : "Maximise Panel (⌥⌘↑)",
                           size: 13, isOn: workspace.bottomPanelMaximized) {
                    workspace.bottomPanelMaximized.toggle()
                }
                IconButton(glyph: .system("chevron.down"), help: "Hide Bottom Panel (⌥⌘3)", size: 13) {
                    workspace.showsBottomPanel = false
                    workspace.bottomPanelMaximized = false
                }
            }
            .padding(.horizontal, Theme.Spacing.m)
            .background(Theme.Palette.surfaceRaised)
            PanelDivider(axis: .horizontal)

            Group {
                switch workspace.bottomTab {
                case .console: ConsoleTab(document: document, workspace: workspace)
                case .events: EventsTab(document: document, workspace: workspace)
                case .capture: CaptureTab(document: document, workspace: workspace)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .background(Theme.Palette.surface)
    }
}

// MARK: - Console

private struct ConsoleTab: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @State private var input = ""
    @State private var history: [UInt64: [String]] = [:]
    @FocusState private var inputFocused: Bool

    var body: some View {
        let sessions = workspace.consoleSessions.compactMap { document.device($0) }
        if sessions.isEmpty {
            VStack(spacing: Theme.Spacing.m) {
                EmptyState(glyph: .system("terminal"), title: "No console sessions",
                           message: "Right-click a device and choose Open Console, or pick one here.")
                Menu("Open Console") {
                    ForEach(document.state.devices) { device in
                        Button(device.name) { workspace.openConsole(for: device.id) }
                    }
                }
                .fixedSize()
                .disabled(document.state.devices.isEmpty)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            let active = sessions.first { $0.id == workspace.activeConsole } ?? sessions[0]
            VStack(spacing: 0) {
                HStack(spacing: Theme.Spacing.xs) {
                    ForEach(sessions) { device in
                        SessionChip(device: device, isActive: device.id == active.id) {
                            workspace.activeConsole = device.id
                            workspace.selectDevices([device.id])
                        } onClose: {
                            workspace.consoleSessions.removeAll { $0 == device.id }
                            if workspace.activeConsole == device.id { workspace.activeConsole = workspace.consoleSessions.first }
                        }
                    }
                    Menu {
                        ForEach(document.state.devices.filter { !workspace.consoleSessions.contains($0.id) }) { device in
                            Button(device.name) { workspace.openConsole(for: device.id) }
                        }
                    } label: {
                        Image(systemName: "plus")
                    }
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                    .help("Open another console")
                    Spacer()
                    PreviewBadge(milestone: "M3")
                }
                .padding(.horizontal, Theme.Spacing.m)
                .padding(.vertical, Theme.Spacing.xs + 2)
                PanelDivider(axis: .horizontal)

                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        Text(PreviewData.transcript(for: active, in: document.state))
                        ForEach(Array((history[active.id] ?? []).enumerated()), id: \.offset) { _, line in
                            Text(line)
                        }
                    }
                    .font(Theme.Font.mono)
                    .foregroundStyle(Theme.Palette.textPrimary)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(Theme.Spacing.m)
                }
                .defaultScrollAnchor(.bottom)
                .background(Theme.Palette.surfaceSunken)

                HStack(spacing: Theme.Spacing.xs) {
                    Text(PreviewData.prompt(for: active))
                        .font(Theme.Font.mono)
                        .foregroundStyle(Theme.Palette.textSecondary)
                    TextField("Type a command", text: $input)
                        .textFieldStyle(.plain)
                        .font(Theme.Font.mono)
                        .focused($inputFocused)
                        // Deferred one turn: focusing in the same pass as insertion is ignored.
                        .onAppear { DispatchQueue.main.async { inputFocused = true } }
                        .onChange(of: workspace.focusConsoleInput) { _, _ in
                            DispatchQueue.main.async { inputFocused = true }
                        }
                        .onSubmit {
                            guard !input.isEmpty else { return }
                            history[active.id, default: []] += [
                                "\(PreviewData.prompt(for: active)) \(input)",
                                "% The command line arrives with milestone M3.",
                            ]
                            input = ""
                        }
                }
                .padding(.horizontal, Theme.Spacing.m)
                .frame(height: 30)
                .background(Theme.Palette.surfaceSunken)
                .overlay(alignment: .top) { PanelDivider(axis: .horizontal) }
            }
        }
    }
}

private struct SessionChip: View {
    let device: DeviceInfo
    let isActive: Bool
    let onSelect: () -> Void
    let onClose: () -> Void

    var body: some View {
        let size = DeviceSymbol.size(for: device.kind)
        HStack(spacing: Theme.Spacing.xs + 1) {
            DeviceSymbolView(kind: device.kind, scale: min(16 / size.width, 12 / size.height))
            Text(device.name)
                .font(Theme.Font.captionMedium)
            Button(action: onClose) {
                Image(systemName: "xmark")
                    .font(.system(size: 8, weight: .bold))
                    .frame(width: 20, height: 20)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .foregroundStyle(Theme.Palette.textTertiary)
            .accessibilityLabel("Close \(device.name) console")
        }
        .foregroundStyle(isActive ? Theme.Palette.textPrimary : Theme.Palette.textSecondary)
        .padding(.leading, Theme.Spacing.s)
        .padding(.trailing, Theme.Spacing.xxs)
        .padding(.vertical, 1)
        .background(
            RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
                .fill(isActive ? Theme.Palette.hover : .clear)
        )
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
                .strokeBorder(isActive ? Theme.Palette.borderStrong : .clear)
        )
        .contentShape(Rectangle())
        .onTapGesture(perform: onSelect)
    }
}

// MARK: - Events

private struct EventsTab: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel

    var body: some View {
        let events = PreviewData.events(for: document.state).filter { workspace.visibleProtocols.contains($0.proto) }
        let current = events.last { $0.time <= workspace.simulationTime }?.id
        VStack(spacing: 0) {
            HStack(spacing: Theme.Spacing.m) {
                Text(workspace.mode == .simulation ? "Simulation events" : "Switch to Simulation to step through events")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
                Spacer()
                ForEach(NetProtocol.allCases.filter { p in events.contains { $0.proto == p } }) { proto in
                    ProtocolTag(proto: proto)
                }
                PreviewBadge(milestone: "M2")
            }
            .padding(.horizontal, Theme.Spacing.m)
            .padding(.vertical, Theme.Spacing.xs + 2)
            PanelDivider(axis: .horizontal)

            if events.isEmpty {
                EmptyState(glyph: .system("list.bullet.rectangle"), title: "No events",
                           message: "Cable some devices together. Their ARP, ICMP and spanning-tree traffic shows up here.")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                Table(events, selection: Binding(
                    get: { current },
                    set: { id in
                        if let event = events.first(where: { $0.id == id }) {
                            workspace.mode = .simulation
                            workspace.isPlaying = false
                            workspace.simulationTime = event.time
                        }
                    }
                )) {
                    TableColumn("Time (s)") { e in
                        Text(String(format: "%.4f", e.time)).font(Theme.Font.mono)
                            .foregroundStyle(e.time <= workspace.simulationTime ? Theme.Palette.textPrimary : Theme.Palette.textTertiary)
                    }
                    .width(min: 70, ideal: 80, max: 100)
                    TableColumn("Last device") { e in Text(e.lastDevice) }
                        .width(min: 70, ideal: 90, max: 140)
                    TableColumn("At device") { e in Text(e.atDevice) }
                        .width(min: 70, ideal: 90, max: 140)
                    TableColumn("Type") { e in ProtocolTag(proto: e.proto) }
                        .width(min: 60, ideal: 70, max: 90)
                    TableColumn("Info") { e in
                        Text(e.info).font(Theme.Font.mono).foregroundStyle(Theme.Palette.textSecondary)
                    }
                }
                .font(Theme.Font.body)
            }
        }
    }
}

// MARK: - Capture

private struct CaptureTab: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @State private var selectedPacket: Int? = 3
    @State private var filter = ""

    var body: some View {
        if let id = workspace.captureLink, let link = document.link(id) {
            let a = document.device(link.a.deviceId)?.name ?? "?"
            let b = document.device(link.b.deviceId)?.name ?? "?"
            let packets = PreviewData.packets(on: link, in: document.state).filter {
                filter.isEmpty || $0.proto.rawValue.localizedCaseInsensitiveContains(filter)
                    || $0.info.localizedCaseInsensitiveContains(filter)
                    || $0.source.contains(filter) || $0.destination.contains(filter)
            }
            VStack(spacing: 0) {
                HStack(spacing: Theme.Spacing.m) {
                    StatusDot(color: Theme.Palette.statusDown, size: 7)
                    Text("Capturing on \(a) \(link.a.port.abbreviatedInterface) ⇄ \(b) \(link.b.port.abbreviatedInterface)")
                        .font(Theme.Font.captionMedium)
                        .foregroundStyle(Theme.Palette.textPrimary)
                    SearchField(prompt: "Display filter, e.g. icmp", text: $filter)
                        .frame(maxWidth: 260)
                    Spacer()
                    PreviewBadge(milestone: "M2")
                    Button("Stop", systemImage: "stop.fill") { workspace.captureLink = nil }
                        .controlSize(.small)
                    Button("Export .pcap", systemImage: "square.and.arrow.up") {
                        workspace.flash("pcap export arrives after v1.")
                    }
                    .controlSize(.small)
                }
                .padding(.horizontal, Theme.Spacing.m)
                .padding(.vertical, Theme.Spacing.xs + 2)
                PanelDivider(axis: .horizontal)

                HStack(spacing: 0) {
                    Table(packets, selection: $selectedPacket) {
                        TableColumn("No.") { p in Text("\(p.id)").font(Theme.Font.mono) }.width(36)
                        TableColumn("Time") { p in Text(String(format: "%.6f", p.time)).font(Theme.Font.mono) }.width(78)
                        TableColumn("Source") { p in Text(p.source).font(Theme.Font.mono) }.width(min: 90, ideal: 120)
                        TableColumn("Destination") { p in Text(p.destination).font(Theme.Font.mono) }.width(min: 90, ideal: 120)
                        TableColumn("Protocol") { p in ProtocolTag(proto: p.proto) }.width(64)
                        TableColumn("Length") { p in Text("\(p.length)").font(Theme.Font.mono) }.width(48)
                        TableColumn("Info") { p in Text(p.info).font(Theme.Font.mono).foregroundStyle(Theme.Palette.textSecondary) }
                    }
                    .frame(minWidth: 420)
                    PanelDivider()
                    PacketDetail(packet: packets.first { $0.id == selectedPacket })
                        .frame(width: 320)
                }
            }
        } else {
            VStack(spacing: Theme.Spacing.m) {
                EmptyState(glyph: .custom(.capture), title: "No capture running",
                           message: "Right-click a cable and choose Start Capture to see every frame that crosses it.")
                Button("Capture on First Cable") {
                    if let first = document.state.links.first { workspace.startCapture(on: first.id) }
                }
                .disabled(document.state.links.isEmpty)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

private struct PacketDetail: View {
    let packet: PreviewData.Packet?

    var body: some View {
        ScrollView {
            if let packet {
                VStack(alignment: .leading, spacing: Theme.Spacing.s) {
                    ForEach(Array(PreviewData.detail(of: packet).enumerated()), id: \.offset) { index, layer in
                        DisclosureGroup {
                            VStack(alignment: .leading, spacing: 2) {
                                ForEach(layer.1, id: \.self) { field in
                                    Text(field).font(Theme.Font.monoSmall).foregroundStyle(Theme.Palette.textSecondary)
                                        .textSelection(.enabled)
                                }
                            }
                            .padding(.leading, Theme.Spacing.s)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        } label: {
                            Text(layer.0).font(Theme.Font.captionMedium).foregroundStyle(Theme.Palette.textPrimary)
                                .lineLimit(1)
                        }
                    }
                }
                .padding(Theme.Spacing.m)
            } else {
                Text("Select a packet")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textTertiary)
                    .frame(maxWidth: .infinity)
                    .padding(Theme.Spacing.xl)
            }
        }
        .background(Theme.Palette.surface)
    }
}
