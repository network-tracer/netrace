import SwiftUI

/// Tabbed panel under the canvas: Console, Events, Capture.
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
    @EnvironmentObject private var simulation: SimulationController
    @State private var input = ""
    @State private var history: [UInt64: [String]] = [:]
    @FocusState private var inputFocused: Bool

    var body: some View {
        let sessions = workspace.consoleSessions.compactMap { document.device($0) }
        if sessions.isEmpty {
            VStack(spacing: Theme.Spacing.m) {
                EmptyState(glyph: .system("terminal"), title: "No console sessions",
                           message: "Double-click a device to open its console, or pick one here.")
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
            let isLive = active.kind.isHost
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
                    if !isLive { PreviewBadge(milestone: "M3") }
                }
                .padding(.horizontal, Theme.Spacing.m)
                .padding(.vertical, Theme.Spacing.xs + 2)
                PanelDivider(axis: .horizontal)

                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        Text(isLive ? hostBanner(for: active) : PreviewData.transcript(for: active, in: document.state))
                            .foregroundStyle(isLive ? Theme.Palette.textSecondary : Theme.Palette.textPrimary)
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
                    Text(prompt(for: active))
                        .font(Theme.Font.mono)
                        .foregroundStyle(Theme.Palette.textSecondary)
                    TextField(isLive ? "ping, ipconfig, arp -a, help" : "Type a command", text: $input)
                        .textFieldStyle(.plain)
                        .font(Theme.Font.mono)
                        .focused($inputFocused)
                        // Deferred one turn: focusing in the same pass as insertion is ignored.
                        .onAppear { DispatchQueue.main.async { inputFocused = true } }
                        .onChange(of: workspace.focusConsoleInput) { _, _ in
                            DispatchQueue.main.async { inputFocused = true }
                        }
                        .onSubmit { run(input, on: active) }
                }
                .padding(.horizontal, Theme.Spacing.m)
                .frame(height: 30)
                .background(Theme.Palette.surfaceSunken)
                .overlay(alignment: .top) { PanelDivider(axis: .horizontal) }
            }
        }
    }

    private func run(_ line: String, on device: DeviceInfo) {
        let command = line.trimmingCharacters(in: .whitespaces)
        input = ""
        guard !command.isEmpty else { return }
        var lines = history[device.id] ?? []
        lines.append("\(prompt(for: device)) \(command)")
        if command.lowercased() == "cls" || command.lowercased() == "clear" {
            history[device.id] = []
            return
        }
        if device.kind.isHost {
            lines.append(simulation.hostCommand(command, on: device.id))
        } else {
            lines.append("% The IOS command line arrives with milestone M3.")
        }
        history[device.id] = lines
    }

    private func prompt(for device: DeviceInfo) -> String {
        device.kind.isHost ? "C:\\>" : PreviewData.prompt(for: device)
    }

    private func hostBanner(for device: DeviceInfo) -> String {
        let address = device.ip.map { "\($0.address)/\($0.prefixLen)" } ?? "no IP address yet"
        return "\(device.name) command prompt (\(address)). Type help for commands.\n"
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
    @EnvironmentObject private var simulation: SimulationController

    var body: some View {
        let events = simulation.trace.filter { $0.netProtocol.map(workspace.visibleProtocols.contains) ?? true }
        let current = workspace.selectedEvent ?? events.last { $0.time <= workspace.simulationTime + 0.000_001 }?.id
        VStack(spacing: 0) {
            HStack(spacing: Theme.Spacing.m) {
                Text(headline(events: events))
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
                Spacer()
                ForEach(NetProtocol.allCases.filter { p in events.contains { $0.netProtocol == p } }) { proto in
                    ProtocolTag(proto: proto)
                }
                Button("Add PDU", systemImage: "envelope.badge") { workspace.startPickingPDU() }
                    .controlSize(.small)
            }
            .padding(.horizontal, Theme.Spacing.m)
            .padding(.vertical, Theme.Spacing.xs + 2)
            if let warning = simulation.warnings.first {
                Label(warning, systemImage: "exclamationmark.triangle.fill")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.statusWarning)
                    .padding(.horizontal, Theme.Spacing.m)
                    .padding(.bottom, Theme.Spacing.xs)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            PanelDivider(axis: .horizontal)

            if events.isEmpty {
                EmptyState(glyph: .custom(.pdu), title: "No traffic yet",
                           message: "Give two PCs IP addresses, then use Add PDU (or type ping in a PC's console) to watch ARP and ICMP step by step.")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                Table(events, selection: Binding(
                    get: { current },
                    set: { id in
                        if let event = events.first(where: { $0.id == id }) { workspace.selectEvent(event) }
                    }
                )) {
                    TableColumn("Time (s)") { e in
                        Text(String(format: "%.3f", e.time)).font(Theme.Font.mono)
                            .foregroundStyle(e.time <= workspace.simulationTime + 0.000_001 ? Theme.Palette.textPrimary : Theme.Palette.textTertiary)
                    }
                    .width(min: 60, ideal: 70, max: 90)
                    TableColumn("Last device") { e in Text(e.fromDeviceId.flatMap { document.device($0)?.name } ?? "–") }
                        .width(min: 60, ideal: 80, max: 120)
                    TableColumn("At device") { e in Text(document.device(e.deviceId)?.name ?? "?") }
                        .width(min: 60, ideal: 80, max: 120)
                    TableColumn("Type") { e in
                        if let proto = e.netProtocol { ProtocolTag(proto: proto) } else { Text("–") }
                    }
                    .width(min: 50, ideal: 60, max: 80)
                    TableColumn("What happened") { e in
                        Text(e.summary)
                            .foregroundStyle(e.isRejection ? Theme.Palette.textSecondary : Theme.Palette.textPrimary)
                            .help(e.summary)
                    }
                }
                .font(Theme.Font.body)
            }
        }
    }

    private func headline(events: [TraceEventInfo]) -> String {
        guard let last = simulation.sessions.last else {
            return workspace.mode == .simulation ? "Simulation" : "Realtime"
        }
        let from = document.device(last.sourceId)?.name ?? "?"
        let status = !last.finished ? "in progress" : (last.succeeded ? "succeeded" : "failed")
        return "Last ping: \(from) → \(last.destination), \(status) · \(events.count) events"
    }
}

// MARK: - Capture

private struct CaptureTab: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController
    @State private var selectedPacket: UInt64?
    @State private var filter = ""

    var body: some View {
        if let id = workspace.captureLink, let link = document.link(id) {
            let a = document.device(link.a.deviceId)?.name ?? "?"
            let b = document.device(link.b.deviceId)?.name ?? "?"
            let packets = simulation.trace.filter { $0.linkId == id && $0.protocol != nil }.filter {
                filter.isEmpty || ($0.protocol ?? "").localizedCaseInsensitiveContains(filter)
                    || $0.info.localizedCaseInsensitiveContains(filter)
                    || ($0.source ?? "").contains(filter) || ($0.destination ?? "").contains(filter)
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

                if packets.isEmpty {
                    EmptyState(glyph: .custom(.capture), title: "Nothing captured yet",
                               message: "Send a ping that crosses this cable. Every frame on it shows up here.")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    HStack(spacing: 0) {
                        Table(packets, selection: $selectedPacket) {
                            TableColumn("No.") { p in
                                Text("\((packets.firstIndex { $0.id == p.id } ?? 0) + 1)").font(Theme.Font.mono)
                            }
                            .width(36)
                            TableColumn("Time") { p in Text(String(format: "%.6f", p.time)).font(Theme.Font.mono) }.width(78)
                            TableColumn("Source") { p in Text(p.source ?? "").font(Theme.Font.mono) }.width(min: 90, ideal: 120)
                            TableColumn("Destination") { p in Text(p.destination ?? "").font(Theme.Font.mono) }.width(min: 90, ideal: 120)
                            TableColumn("Protocol") { p in
                                if let proto = p.netProtocol { ProtocolTag(proto: proto) }
                            }
                            .width(64)
                            TableColumn("Length") { p in Text("\(p.length ?? 0)").font(Theme.Font.mono) }.width(48)
                            TableColumn("Info") { p in Text(p.info).font(Theme.Font.mono).foregroundStyle(Theme.Palette.textSecondary) }
                        }
                        .frame(minWidth: 420)
                        PanelDivider()
                        PacketDetail(event: packets.first { $0.id == selectedPacket } ?? packets.first)
                            .frame(width: 320)
                    }
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

/// Wireshark-style field tree for a captured frame.
private struct PacketDetail: View {
    let event: TraceEventInfo?

    var body: some View {
        ScrollView {
            if let event {
                VStack(alignment: .leading, spacing: Theme.Spacing.s) {
                    ForEach(event.layers.filter { !$0.fields.isEmpty }.reversed(), id: \.number) { layer in
                        DisclosureGroup {
                            VStack(alignment: .leading, spacing: 2) {
                                ForEach(layer.fields, id: \.name) { field in
                                    HStack(alignment: .firstTextBaseline, spacing: Theme.Spacing.xs) {
                                        Text("\(field.name):").foregroundStyle(Theme.Palette.textSecondary)
                                        Text(field.value).foregroundStyle(Theme.Palette.textPrimary)
                                    }
                                    .font(Theme.Font.monoSmall)
                                    .textSelection(.enabled)
                                }
                            }
                            .padding(.leading, Theme.Spacing.s)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        } label: {
                            Text("L\(layer.number) \(layer.title) · \(layer.fields.first?.value ?? "")")
                                .font(Theme.Font.captionMedium)
                                .foregroundStyle(Theme.Palette.textPrimary)
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
