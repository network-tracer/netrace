import SwiftUI

/// Right-hand inspector: adapts to what is selected (one device, a cable,
/// several devices, or nothing → the project).
struct InspectorPanel: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel

    var body: some View {
        Group {
            if workspace.selection.count == 1, let id = workspace.selection.first, let device = document.device(id) {
                DeviceInspector(device: device, document: document, workspace: workspace)
            } else if let id = workspace.selectedAnnotation, let annotation = document.annotation(id) {
                AnnotationInspector(annotation: annotation, document: document, workspace: workspace)
            } else if let id = workspace.selectedLink, let link = document.link(id) {
                LinkInspector(link: link, document: document, workspace: workspace)
            } else if workspace.selection.count > 1 {
                SelectionInspector(document: document, workspace: workspace)
            } else {
                ProjectInspector(document: document, workspace: workspace)
            }
        }
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Theme.Palette.surface)
    }
}

/// Header shared by inspector variants: symbol or icon, title, subtitle.
private struct InspectorHeader<Leading: View, Trailing: View, Title: View>: View {
    let titleView: Title
    let subtitle: String
    @ViewBuilder let leading: Leading
    @ViewBuilder let trailing: Trailing

    init(title: String, subtitle: String, @ViewBuilder leading: () -> Leading, @ViewBuilder trailing: () -> Trailing)
    where Title == AnyView {
        self.titleView = AnyView(
            Text(title)
                .font(Theme.Font.title)
                .foregroundStyle(Theme.Palette.textPrimary)
                .lineLimit(1)
        )
        self.subtitle = subtitle
        self.leading = leading()
        self.trailing = trailing()
    }

    init(subtitle: String, @ViewBuilder titleView: () -> Title, @ViewBuilder leading: () -> Leading,
         @ViewBuilder trailing: () -> Trailing) {
        self.titleView = titleView()
        self.subtitle = subtitle
        self.leading = leading()
        self.trailing = trailing()
    }

    var body: some View {
        HStack(spacing: Theme.Spacing.m) {
            leading.frame(width: 48, height: 40)
            VStack(alignment: .leading, spacing: 1) {
                titleView
                Text(subtitle)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
            trailing
        }
        .padding(Theme.Spacing.l)
    }
}

private struct InspectorSection<Content: View, Trailing: View>: View {
    let title: String
    @ViewBuilder let trailing: Trailing
    @ViewBuilder let content: Content

    init(_ title: String, @ViewBuilder trailing: () -> Trailing = { EmptyView() }, @ViewBuilder content: () -> Content) {
        self.title = title
        self.trailing = trailing()
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            SectionHeader(title) { trailing }
            content
        }
    }
}

// MARK: - Device

private struct DeviceInspector: View {
    let device: DeviceInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel

    var body: some View {
        let power = workspace.powerState(device.id)
        let size = DeviceSymbol.size(for: device.kind)
        VStack(spacing: 0) {
            InspectorHeader(subtitle: device.kind.displayName) {
                EditableDeviceName(device: device, document: document, workspace: workspace)
            } leading: {
                DeviceSymbolView(kind: device.kind, scale: min(44 / size.width, 36 / size.height))
            } trailing: {
                PowerPill(state: power)
            }
            HStack {
                TabStrip(tabs: WorkspaceModel.InspectorTab.allCases, selection: $workspace.inspectorTab, title: \.title)
                Spacer()
            }
            .padding(.horizontal, Theme.Spacing.l)
            PanelDivider(axis: .horizontal)

            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.xl) {
                    switch workspace.inspectorTab {
                    case .overview: overview(power: power)
                    case .interfaces: interfaces
                    case .config: config
                    }
                }
                .padding(Theme.Spacing.l)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    @ViewBuilder
    private func overview(power: WorkspaceModel.PowerState) -> some View {
        let links = document.links(attachedTo: device.id)
        HStack(spacing: Theme.Spacing.s) {
            Button("Console", systemImage: "terminal") { workspace.openConsole(for: device.id) }
            Menu {
                Button("Start", systemImage: "play.fill") { workspace.setPower(.running, for: [device.id]) }
                Button("Suspend", systemImage: "pause.fill") { workspace.setPower(.suspended, for: [device.id]) }
                Button("Stop", systemImage: "stop.fill") { workspace.setPower(.stopped, for: [device.id]) }
            } label: {
                Label("Power", systemImage: "power")
            }
            .fixedSize()
        }
        .controlSize(.small)

        InspectorSection("Hardware") {
            KeyValueRow("Model", device.kind.displayName)
            KeyValueRow("Ports", "\(device.kind.ports.count)")
            KeyValueRow("In use", "\(links.count)")
            Text(device.kind.summary)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.Palette.textTertiary)
        }
        InspectorSection("Status") {
            KeyValueRow("Power", power.title)
            KeyValueRow("Hostname", device.name, mono: true)
            KeyValueRow("Address", device.kind.ports.isEmpty ? "Wi-Fi" : PreviewData.address(of: device, in: document.state), mono: true)
            KeyValueRow("MAC", PreviewData.mac(of: device), mono: true)
        }
        InspectorSection("Canvas") {
            KeyValueRow("Position", "\(Int(device.x)), \(Int(device.y))", mono: true)
            KeyValueRow("ID", "#\(device.id)", mono: true)
        }
    }

    @ViewBuilder
    private var interfaces: some View {
        let links = document.links(attachedTo: device.id)
        InspectorSection("Interfaces", trailing: { PreviewBadge(milestone: "M3") }) {
            if device.kind.ports.isEmpty {
                Text("Wireless only. Wi-Fi association arrives after v1.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
            }
            VStack(spacing: 0) {
                ForEach(device.kind.ports, id: \.self) { port in
                    let link = links.first { $0.endpoint(for: device.id)?.port == port }
                    let peer = link.flatMap { document.device($0.otherEnd(from: device.id).deviceId) }
                    HStack(spacing: Theme.Spacing.s) {
                        IconView(.port, size: 14)
                            .foregroundStyle(link != nil ? Theme.Palette.statusUp : Theme.Palette.textTertiary)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(port.abbreviatedInterface)
                                .font(Theme.Font.mono)
                                .foregroundStyle(Theme.Palette.textPrimary)
                            Text(link != nil ? "up · unassigned" : "administratively down")
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.Palette.textTertiary)
                        }
                        Spacer(minLength: Theme.Spacing.s)
                        if let peer, let link {
                            Text("\(peer.name) \(link.otherEnd(from: device.id).port.abbreviatedInterface)")
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.Palette.textSecondary)
                        }
                    }
                    .padding(.horizontal, Theme.Spacing.m)
                    .padding(.vertical, Theme.Spacing.s - 2)
                    .help(port)
                    if port != device.kind.ports.last {
                        PanelDivider(axis: .horizontal).padding(.leading, Theme.Spacing.m + 22)
                    }
                }
            }
            .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous))
        }
    }

    @ViewBuilder
    private var config: some View {
        let text = PreviewData.runningConfig(for: device, in: document.state)
        InspectorSection("Running configuration", trailing: { PreviewBadge(milestone: "M3") }) {
            Text(text)
                .font(Theme.Font.monoSmall)
                .foregroundStyle(Theme.Palette.textPrimary)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(Theme.Spacing.m)
                .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous))
            HStack {
                Button("Copy", systemImage: "doc.on.doc") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(text, forType: .string)
                }
                Button("Open in Console", systemImage: "terminal") { workspace.openConsole(for: device.id) }
            }
            .controlSize(.small)
        }
    }
}

/// The device's hostname as an editable title. Return or clicking away commits.
private struct EditableDeviceName: View {
    let device: DeviceInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager
    @State private var draft = ""
    @FocusState private var focused: Bool

    var body: some View {
        TextField("Hostname", text: $draft)
            .textFieldStyle(.plain)
            .font(Theme.Font.title)
            .foregroundStyle(Theme.Palette.textPrimary)
            .focused($focused)
            .padding(.horizontal, 3)
            .padding(.vertical, 1)
            .background(
                RoundedRectangle(cornerRadius: 4)
                    .strokeBorder(focused ? Theme.Palette.accent : .clear, lineWidth: 1.5)
            )
            .padding(.horizontal, -3)
            .onAppear { draft = device.name }
            .onChange(of: device.name) { _, name in draft = name }
            .onSubmit(commit)
            .onChange(of: focused) { _, isFocused in if !isFocused { commit() } }
            .help("Click to rename. Hostnames use letters, digits and hyphens.")
            .accessibilityLabel("Hostname")
    }

    private func commit() {
        guard draft != device.name else { return }
        if case .failure(let error) = document.renameDevice(device.id, to: draft, undoManager: undoManager) {
            workspace.flash(error.message)
            draft = device.name
        }
    }
}

private struct PowerPill: View {
    let state: WorkspaceModel.PowerState

    var body: some View {
        HStack(spacing: Theme.Spacing.xs + 1) {
            StatusGlyph(state: state, size: 7)
            Text(state.title)
                .font(Theme.Font.captionMedium)
                .foregroundStyle(Theme.Palette.textSecondary)
        }
        .padding(.horizontal, Theme.Spacing.s)
        .padding(.vertical, 3)
        .background(Capsule().fill(Theme.Palette.hover))
    }
}

// MARK: - Cable

private struct LinkInspector: View {
    let link: LinkInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager

    @State private var latency = 0.0
    @State private var jitter = 0.0
    @State private var loss = 0.0
    @State private var bandwidth = "Unlimited"

    var body: some View {
        VStack(spacing: 0) {
            InspectorHeader(title: "Cable", subtitle: link.medium == .fiber ? "Fibre optic" : "Copper, auto-MDIX") {
                IconView(link.medium == .fiber ? .cableFiber : .cableCopper, size: 28)
                    .foregroundStyle(Theme.Palette.textPrimary)
            } trailing: {
                PowerPill(state: .running).hidden()
            }
            PanelDivider(axis: .horizontal)
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.xl) {
                    InspectorSection("Endpoints") {
                        endpoint(link.a)
                        endpoint(link.b)
                    }

                    InspectorSection("Capture") {
                        if workspace.captureLink == link.id {
                            Button("Stop Capture", systemImage: "stop.fill") { workspace.captureLink = nil }
                            Button("Show Packets") { workspace.startCapture(on: link.id) }
                        } else {
                            Button("Start Capture", image: ImageResource(name: Icon.capture.rawValue, bundle: .main)) {
                                workspace.startCapture(on: link.id)
                            }
                        }
                    }
                    .controlSize(.small)

                    InspectorSection("Conditioning", trailing: { PreviewBadge(milestone: "M5") }) {
                        slider("Latency", value: $latency, range: 0...500, unit: "ms")
                        slider("Jitter", value: $jitter, range: 0...100, unit: "ms")
                        slider("Packet loss", value: $loss, range: 0...100, unit: "%")
                        HStack {
                            Text("Bandwidth").font(Theme.Font.body).foregroundStyle(Theme.Palette.textSecondary)
                            Spacer()
                            Picker("Bandwidth", selection: $bandwidth) {
                                ForEach(["Unlimited", "1 Gbit/s", "100 Mbit/s", "10 Mbit/s", "1.5 Mbit/s"], id: \.self) { Text($0) }
                            }
                            .labelsHidden()
                            .fixedSize()
                            .controlSize(.small)
                        }
                    }

                    Button("Delete Cable", systemImage: "trash", role: .destructive) {
                        document.disconnect(link.id, undoManager: undoManager)
                    }
                    .controlSize(.small)
                }
                .padding(Theme.Spacing.l)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private func endpoint(_ end: EndpointInfo) -> some View {
        let device = document.device(end.deviceId)
        let size = device.map { DeviceSymbol.size(for: $0.kind) } ?? CGSize(width: 1, height: 1)
        return HStack(spacing: Theme.Spacing.s) {
            if let device {
                DeviceSymbolView(kind: device.kind, scale: min(26 / size.width, 20 / size.height))
                    .frame(width: 28)
            }
            Text(device?.name ?? "?")
                .font(Theme.Font.body)
                .foregroundStyle(Theme.Palette.textPrimary)
            Spacer()
            Text(end.port.abbreviatedInterface)
                .font(Theme.Font.mono)
                .foregroundStyle(Theme.Palette.textSecondary)
                .lineLimit(1)
                .help(end.port)
            StatusGlyph(state: workspace.powerState(end.deviceId), size: 7)
        }
        .padding(Theme.Spacing.s)
        .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous))
        .contentShape(Rectangle())
        .onTapGesture { workspace.selectDevices([end.deviceId]) }
    }

    private func slider(_ title: String, value: Binding<Double>, range: ClosedRange<Double>, unit: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(title).font(Theme.Font.body).foregroundStyle(Theme.Palette.textSecondary)
                Spacer()
                Text("\(Int(value.wrappedValue)) \(unit)")
                    .font(Theme.Font.mono)
                    .foregroundStyle(Theme.Palette.textPrimary)
            }
            Slider(value: value, in: range)
                .controlSize(.small)
        }
    }
}

// MARK: - Annotation

private struct AnnotationInspector: View {
    let annotation: AnnotationInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager
    @State private var draft = ""
    @FocusState private var focused: Bool

    var body: some View {
        let isNote = annotation.kind == .note
        VStack(spacing: 0) {
            InspectorHeader(title: isNote ? "Note" : "Area",
                            subtitle: isNote ? "Text on the canvas" : "Outlines a zone, VLAN or site") {
                Image(systemName: isNote ? "note.text" : "rectangle.dashed")
                    .font(.system(size: 24, weight: .light))
                    .foregroundStyle(Theme.Palette.textSecondary)
            } trailing: { EmptyView() }
            PanelDivider(axis: .horizontal)
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.xl) {
                    InspectorSection(isNote ? "Text" : "Label") {
                        TextField(isNote ? "Note" : "Area name", text: $draft, axis: .vertical)
                            .lineLimit(isNote ? 3...10 : 1...2)
                            .textFieldStyle(.roundedBorder)
                            .font(Theme.Font.body)
                            .focused($focused)
                            .onSubmit(commit)
                            .onChange(of: focused) { _, isFocused in if !isFocused { commit() } }
                    }
                    InspectorSection("Frame") {
                        KeyValueRow("Position", "\(Int(annotation.x)), \(Int(annotation.y))", mono: true)
                        KeyValueRow("Size", "\(Int(annotation.width)) × \(Int(annotation.height))", mono: true)
                    }
                    Button(isNote ? "Delete Note" : "Delete Area", systemImage: "trash", role: .destructive) {
                        document.removeAnnotation(annotation.id, undoManager: undoManager)
                    }
                    .controlSize(.small)
                }
                .padding(Theme.Spacing.l)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .onAppear { draft = annotation.text }
        .onChange(of: annotation.text) { _, text in draft = text }
    }

    private func commit() {
        document.updateAnnotation(annotation.id, rect: annotation.rect, text: draft,
                                  actionName: annotation.kind == .note ? "Edit Note" : "Edit Area", undoManager: undoManager)
    }
}

// MARK: - Multiple devices

private struct SelectionInspector: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager

    enum Alignment: CaseIterable {
        case left, centerX, right, top, centerY, bottom, distributeX, distributeY

        var symbol: String {
            switch self {
            case .left: "align.horizontal.left"
            case .centerX: "align.horizontal.center"
            case .right: "align.horizontal.right"
            case .top: "align.vertical.top"
            case .centerY: "align.vertical.center"
            case .bottom: "align.vertical.bottom"
            case .distributeX: "distribute.horizontal.center"
            case .distributeY: "distribute.vertical.center"
            }
        }

        var title: String {
            switch self {
            case .left: "Align Left"
            case .centerX: "Align Centres Horizontally"
            case .right: "Align Right"
            case .top: "Align Top"
            case .centerY: "Align Centres Vertically"
            case .bottom: "Align Bottom"
            case .distributeX: "Distribute Horizontally"
            case .distributeY: "Distribute Vertically"
            }
        }
    }

    var body: some View {
        let devices = document.state.devices.filter { workspace.selection.contains($0.id) }
        VStack(spacing: 0) {
            InspectorHeader(title: "\(devices.count) devices", subtitle: "Multiple selection") {
                Image(systemName: "square.on.square.dashed")
                    .font(.system(size: 24, weight: .light))
                    .foregroundStyle(Theme.Palette.textSecondary)
            } trailing: { EmptyView() }
            PanelDivider(axis: .horizontal)
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.xl) {
                    InspectorSection("Arrange") {
                        LazyVGrid(columns: Array(repeating: GridItem(.fixed(30), spacing: Theme.Spacing.xs), count: 6),
                                  alignment: .leading, spacing: Theme.Spacing.xs) {
                            ForEach(Alignment.allCases, id: \.self) { alignment in
                                IconButton(glyph: .system(alignment.symbol), help: alignment.title,
                                           isEnabled: alignment == .distributeX || alignment == .distributeY ? devices.count > 2 : true) {
                                    arrange(devices, alignment)
                                }
                            }
                        }
                    }
                    InspectorSection("Kinds") {
                        ForEach(DeviceKind.allCases.filter { kind in devices.contains { $0.kind == kind } }) { kind in
                            KeyValueRow(kind.displayName, "\(devices.filter { $0.kind == kind }.count)")
                        }
                    }
                    InspectorSection("Power") {
                        HStack {
                            Button("Start", systemImage: "play.fill") { workspace.setPower(.running, for: devices.map(\.id)) }
                            Button("Stop", systemImage: "stop.fill") { workspace.setPower(.stopped, for: devices.map(\.id)) }
                        }
                        .controlSize(.small)
                    }
                }
                .padding(Theme.Spacing.l)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private func arrange(_ devices: [DeviceInfo], _ alignment: Alignment) {
        guard devices.count > 1 else { return }
        let xs = devices.map(\.x), ys = devices.map(\.y)
        var targets: [UInt64: CGPoint] = [:]
        switch alignment {
        case .left: devices.forEach { targets[$0.id] = CGPoint(x: xs.min()!, y: $0.y) }
        case .right: devices.forEach { targets[$0.id] = CGPoint(x: xs.max()!, y: $0.y) }
        case .centerX: devices.forEach { targets[$0.id] = CGPoint(x: (xs.min()! + xs.max()!) / 2, y: $0.y) }
        case .top: devices.forEach { targets[$0.id] = CGPoint(x: $0.x, y: ys.min()!) }
        case .bottom: devices.forEach { targets[$0.id] = CGPoint(x: $0.x, y: ys.max()!) }
        case .centerY: devices.forEach { targets[$0.id] = CGPoint(x: $0.x, y: (ys.min()! + ys.max()!) / 2) }
        case .distributeX:
            let sorted = devices.sorted { $0.x < $1.x }
            let step = (xs.max()! - xs.min()!) / CGFloat(sorted.count - 1)
            for (i, d) in sorted.enumerated() { targets[d.id] = CGPoint(x: xs.min()! + step * CGFloat(i), y: d.y) }
        case .distributeY:
            let sorted = devices.sorted { $0.y < $1.y }
            let step = (ys.max()! - ys.min()!) / CGFloat(sorted.count - 1)
            for (i, d) in sorted.enumerated() { targets[d.id] = CGPoint(x: d.x, y: ys.min()! + step * CGFloat(i)) }
        }
        // All moves land in one undo group (same event loop turn).
        for (id, point) in targets {
            document.moveDevice(id, to: point.snapped, undoManager: undoManager)
        }
        undoManager?.setActionName(alignment.title)
    }
}

// MARK: - Project

private struct ProjectInspector: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @State private var acceleratesTimers = true

    var body: some View {
        VStack(spacing: 0) {
            InspectorHeader(title: "Project", subtitle: "Nothing selected") {
                IconView(.topology, size: 28)
                    .foregroundStyle(Theme.Palette.textSecondary)
            } trailing: { EmptyView() }
            PanelDivider(axis: .horizontal)
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.xl) {
                    InspectorSection("Topology") {
                        ForEach(DeviceCategory.allCases) { category in
                            KeyValueRow(category.rawValue, "\(document.state.devices.filter { $0.kind.category == category }.count)")
                        }
                        KeyValueRow("Cables", "\(document.state.links.count)")
                    }
                    InspectorSection("Simulation") {
                        Toggle("Accelerate protocol timers", isOn: $acceleratesTimers)
                            .font(Theme.Font.body)
                            .toggleStyle(.switch)
                            .controlSize(.mini)
                        Text("Spanning tree and OSPF converge in seconds instead of 30–40 s.")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textTertiary)
                        KeyValueRow("Random seed", "0x2F6B", mono: true)
                    }
                    InspectorSection("Snapshots", trailing: { PreviewBadge(milestone: "M6") }) {
                        snapshotRow("Before VLANs", "Today, 11:02")
                        snapshotRow("Cabled", "Today, 10:47")
                        Button("Take Snapshot", systemImage: "camera") {
                            workspace.flash("Snapshots arrive in M6.")
                        }
                        .controlSize(.small)
                    }
                }
                .padding(Theme.Spacing.l)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private func snapshotRow(_ name: String, _ date: String) -> some View {
        HStack {
            Image(systemName: "clock.arrow.circlepath").foregroundStyle(Theme.Palette.textTertiary)
            Text(name).font(Theme.Font.body).foregroundStyle(Theme.Palette.textPrimary)
            Spacer()
            Text(date).font(Theme.Font.caption).foregroundStyle(Theme.Palette.textTertiary)
        }
    }
}
