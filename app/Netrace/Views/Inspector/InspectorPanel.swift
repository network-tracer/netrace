import SwiftUI

/// Right-hand inspector: adapts to what is selected (one device, a cable,
/// several devices, or nothing → the project).
struct InspectorPanel: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController

    var body: some View {
        Group {
            if let id = workspace.selectedEvent, let event = simulation.event(id) {
                PacketInspector(event: event, document: document, workspace: workspace)
            } else if workspace.selection.count == 1, workspace.selectedAnnotations.isEmpty,
                      let id = workspace.selection.first, let device = document.device(id) {
                DeviceInspector(device: device, document: document, workspace: workspace)
            } else if let id = workspace.selectedAnnotation, let annotation = document.annotation(id) {
                AnnotationInspector(annotation: annotation, document: document, workspace: workspace)
            } else if let id = workspace.selectedLink, let link = document.link(id) {
                LinkInspector(link: link, document: document, workspace: workspace)
            } else if workspace.selection.count + workspace.selectedAnnotations.count > 1 {
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
    @EnvironmentObject private var simulation: SimulationController
    @Environment(\.undoManager) private var undoManager

    var body: some View {
        let power = workspace.powerState(device.id)
        let size = DeviceSymbol.size(for: device.kind)
        VStack(spacing: 0) {
            InspectorHeader(subtitle: device.kind.displayName) {
                EditableDeviceName(device: device, document: document, workspace: workspace)
            } leading: {
                DeviceSymbolView(kind: device.kind, scale: min(44 / size.width, 36 / size.height), tint: device.color)
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
        if device.kind.isHost {
            HostIPEditor(device: device, document: document, workspace: workspace)
            if device.kind == .server {
                ServerDHCPEditor(device: device, document: document, workspace: workspace)
            }
        }
        InspectorSection("Status") {
            KeyValueRow("Power", power.title)
            KeyValueRow("Hostname", device.name, mono: true)
            if device.kind.isHost {
                KeyValueRow("Address", device.ip.map { "\($0.address)/\($0.prefixLen)" } ?? "Not configured", mono: true)
            }
            if let port = device.kind.ports.first, let mac = simulation.macAddress(of: device.id, port: port) {
                KeyValueRow("MAC", mac, mono: true)
            }
        }
        InspectorSection("Canvas") {
            StyleRow("Colour") {
                SwatchPicker(selection: device.color) { color in
                    document.setDeviceColor([device.id], to: color, undoManager: undoManager)
                }
            }
            KeyValueRow("Position", "\(Int(device.x)), \(Int(device.y))", mono: true)
            KeyValueRow("ID", "#\(device.id)", mono: true)
            if device.group != nil {
                Button("Ungroup", systemImage: "rectangle.3.group") { workspace.arrangeRequest = .ungroup }
                    .controlSize(.small)
            }
        }
    }

    @ViewBuilder
    private var interfaces: some View {
        let links = document.links(attachedTo: device.id)
        let status = device.kind.hasCli ? document.interfaceStatus(of: device.id) : []
        InspectorSection("Interfaces") {
            if device.kind.ports.isEmpty {
                Text("Wireless only. Wi-Fi association arrives after v1.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
            }
            VStack(spacing: 0) {
                let names = device.kind.hasCli ? status.map(\.name) : device.kind.ports
                ForEach(names, id: \.self) { port in
                    let link = links.first { $0.endpoint(for: device.id)?.port == port }
                    let peer = link.flatMap { document.device($0.otherEnd(from: device.id).deviceId) }
                    let row = status.first { $0.name == port }
                    HStack(spacing: Theme.Spacing.s) {
                        IconView(.port, size: 14)
                            .foregroundStyle(portColor(row: row, connected: link != nil))
                        VStack(alignment: .leading, spacing: 1) {
                            Text(port.abbreviatedInterface)
                                .font(Theme.Font.mono)
                                .foregroundStyle(Theme.Palette.textPrimary)
                            Text(row.map(statusLine) ?? interfaceStatus(port: port, connected: link != nil))
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
                    if port != names.last {
                        PanelDivider(axis: .horizontal).padding(.leading, Theme.Spacing.m + 22)
                    }
                }
            }
            .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous))
        }
    }

    private func portColor(row: InterfaceStatusInfo?, connected: Bool) -> Color {
        guard let row else { return connected ? Theme.Palette.statusUp : Theme.Palette.textTertiary }
        if row.status == "up" && row.protocol == "up" { return Theme.Palette.statusUp }
        if row.status == "administratively down" { return Theme.Palette.statusDown }
        return Theme.Palette.textTertiary
    }

    /// "up/up · 192.168.1.1/24", as `show ip interface brief` would say.
    private func statusLine(_ row: InterfaceStatusInfo) -> String {
        let state = row.status == "administratively down" ? "shut down" : "\(row.status)/\(row.protocol)"
        let address = row.address.map { "\($0)/\(row.prefixLen ?? 0)" }
        return [state, address, row.description].compactMap { $0 }.joined(separator: " · ")
    }

    /// Status line for a port; end devices show their real address.
    private func interfaceStatus(port: String, connected: Bool) -> String {
        if device.kind.isHost, port == device.kind.ports.first {
            let address = device.ip.map { "\($0.address)/\($0.prefixLen)" } ?? "no IP address"
            return connected ? "up · \(address)" : "no cable · \(address)"
        }
        return connected ? "up" : "not connected"
    }

    @ViewBuilder
    private var config: some View {
        let text = document.runningConfig(of: device.id) ?? "\(device.name) has no configuration of its own."
        InspectorSection("Running configuration") {
            // Configuration lines never wrap, as in a terminal; scroll sideways for long ones.
            ScrollView(.horizontal) {
                Text(text)
                    .font(Theme.Font.monoSmall)
                    .foregroundStyle(Theme.Palette.textPrimary)
                    .textSelection(.enabled)
                    .fixedSize()
                    .padding(Theme.Spacing.m)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous))
            HStack {
                Button("Copy", systemImage: "doc.on.doc") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(text, forType: .string)
                }
                Button("Open in Console", systemImage: "terminal") { workspace.openConsole(for: device.id) }
                if device.kind.hasCli {
                    Button("Copy To…", systemImage: "doc.on.doc.fill") { workspace.copyConfigSource = device.id }
                        .help("Give other devices this configuration, changing only names and addresses")
                        .disabled(!document.state.devices.contains { $0.kind == device.kind && $0.id != device.id })
                }
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

/// IP configuration form for end devices, like Packet Tracer's Desktop →
/// IP Configuration. Applies on Return or the Apply button; validated by the core.
private struct HostIPEditor: View {
    let device: DeviceInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager
    @State private var address = ""
    @State private var mask = ""
    @State private var gateway = ""

    @EnvironmentObject private var simulation: SimulationController

    var body: some View {
        InspectorSection("IP Configuration") {
            Picker("Address", selection: Binding(
                get: { device.dhcp },
                set: { dhcp in
                    if case .failure(let error) = document.setHostDHCP(device.id, dhcp, undoManager: undoManager) {
                        workspace.flash(error.message)
                    }
                }
            )) {
                Text("Static").tag(false)
                Text("DHCP").tag(true)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .controlSize(.small)
            if device.dhcp {
                dhcpStatus
            } else {
                staticFields
            }
        }
        .onAppear(perform: load)
        .onChange(of: device.ip) { _, _ in load() }
        .onChange(of: device.id) { _, _ in load() }
    }

    /// What DHCP gave the device in the simulation (or why it didn't).
    @ViewBuilder
    private var dhcpStatus: some View {
        if let current = simulation.currentAddress(of: device.id) {
            KeyValueRow(current.apipa ? "APIPA address" : "Leased address", "\(current.address)/\(current.prefixLen)", mono: true)
            KeyValueRow("Default gateway", current.gateway ?? "none", mono: true)
            if let server = current.dhcpServer {
                KeyValueRow("DHCP server", server, mono: true)
            }
            if current.apipa {
                Text("No DHCP server answered, so it gave itself a 169.254 address. Add an ip dhcp pool for this network, or an ip helper-address on the router.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.statusWarning)
                    .fixedSize(horizontal: false, vertical: true)
            }
        } else {
            Text("Asks a DHCP server for its address when the simulation starts. Watch the exchange in the Events tab.")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.Palette.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    @ViewBuilder
    private var staticFields: some View {
            field("IPv4 address", text: $address, prompt: "192.168.1.10")
            field("Subnet mask", text: $mask, prompt: "255.255.255.0")
            field("Default gateway", text: $gateway, prompt: "Optional")
            HStack {
                // No default-button shortcut: the inspector is always on screen and
                // would steal Return from the console and canvas. Fields submit on Return.
                Button("Apply", action: apply)
                    .disabled(!isDirty)
                if device.ip != nil {
                    Button("Clear") {
                        if case .failure(let error) = document.setHostIP(device.id, to: nil, undoManager: undoManager) {
                            workspace.flash(error.message)
                        }
                    }
                }
            }
            .controlSize(.small)
    }

    private func field(_ label: String, text: Binding<String>, prompt: String) -> some View {
        HStack {
            Text(label)
                .font(Theme.Font.body)
                .foregroundStyle(Theme.Palette.textSecondary)
                .frame(width: 104, alignment: .leading)
            TextField(label, text: text, prompt: Text(prompt))
                .textFieldStyle(.roundedBorder)
                .font(Theme.Font.mono)
                .labelsHidden()
                .onSubmit(apply)
        }
    }

    private var isDirty: Bool {
        address != (device.ip?.address ?? "") || mask != (device.ip?.mask ?? "")
            || gateway != (device.ip?.gateway ?? "")
    }

    private func load() {
        address = device.ip?.address ?? ""
        mask = device.ip?.mask ?? (address.isEmpty ? "" : "255.255.255.0")
        gateway = device.ip?.gateway ?? ""
    }

    private func apply() {
        guard isDirty else { return }
        if address.trimmingCharacters(in: .whitespaces).isEmpty {
            _ = document.setHostIP(device.id, to: nil, undoManager: undoManager)
            return
        }
        do {
            let ip = try parseHostIp(address: address, mask: mask.isEmpty ? "255.255.255.0" : mask, gateway: gateway)
            if case .failure(let error) = document.setHostIP(device.id, to: ip, undoManager: undoManager) {
                workspace.flash(error.message)
            }
        } catch let error as CoreError {
            workspace.flash(error.message)
        } catch {
            workspace.flash(error.localizedDescription)
        }
    }
}

// MARK: - Packet

/// One simulation step explained: a plain sentence first, then what each
/// layer did at this device, with the raw fields one click away.
private struct PacketInspector: View {
    let event: TraceEventInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController

    var body: some View {
        let device = document.device(event.deviceId)
        let siblings = simulation.trace.filter { $0.session != nil && $0.session == event.session }
        let index = siblings.firstIndex { $0.id == event.id }
        VStack(spacing: 0) {
            InspectorHeader(title: device?.name ?? "Step",
                            subtitle: "\(event.outcome.title) · \(String(format: "%.3f", event.time)) s") {
                if let device {
                    let size = DeviceSymbol.size(for: device.kind)
                    DeviceSymbolView(kind: device.kind, scale: min(44 / size.width, 36 / size.height), tint: device.color)
                }
            } trailing: {
                if let proto = event.netProtocol { ProtocolTag(proto: proto) }
            }
            PanelDivider(axis: .horizontal)
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.l) {
                    Text(event.summary)
                        .font(.system(size: 14))
                        .foregroundStyle(Theme.Palette.textPrimary)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)

                    if let nat = event.nat {
                        NatDecisionView(decision: nat, device: device?.name ?? "the router")
                    }

                    if let acl = event.acl {
                        AclDecisionView(decision: acl)
                    }

                    if !event.layers.isEmpty {
                        InspectorSection("Layers at \(device?.name ?? "this device")") {
                            VStack(spacing: Theme.Spacing.xs) {
                                ForEach(event.layers.reversed(), id: \.number) { layer in
                                    LayerRow(layer: layer)
                                }
                            }
                        }
                    }

                    if let index {
                        HStack {
                            Button("Previous", systemImage: "chevron.left") {
                                workspace.selectEvent(siblings[index - 1])
                            }
                            .disabled(index == 0)
                            Spacer()
                            Text("Step \(index + 1) of \(siblings.count)")
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.Palette.textSecondary)
                            Spacer()
                            Button("Next", systemImage: "chevron.right") {
                                workspace.selectEvent(siblings[index + 1])
                            }
                            .disabled(index + 1 >= siblings.count)
                        }
                        .controlSize(.small)
                    }
                }
                .padding(Theme.Spacing.l)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

/// One layer's role at a hop: highlighted when used or changed, dimmed when
/// the device never looked at it. Fields expand on demand; changed ones are marked.
private struct LayerRow: View {
    let layer: LayerNoteInfo
    @State private var expanded = false

    var body: some View {
        let active = layer.role != .notUsed
        VStack(alignment: .leading, spacing: Theme.Spacing.xs + 2) {
            HStack(alignment: .firstTextBaseline, spacing: Theme.Spacing.s) {
                Text("L\(layer.number)")
                    .font(.system(size: 11, weight: .bold, design: .rounded))
                    .foregroundStyle(active ? (layer.role == .changed ? Theme.Palette.onAccent : Theme.Palette.surface)
                                            : Theme.Palette.textTertiary)
                    .frame(width: 24, height: 18)
                    .background(RoundedRectangle(cornerRadius: 4).fill(active ? roleColor : Theme.Palette.hover))
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: Theme.Spacing.xs) {
                        Text(layer.title)
                            .font(Theme.Font.label)
                            .foregroundStyle(active ? Theme.Palette.textPrimary : Theme.Palette.textTertiary)
                        Text(roleLabel)
                            .font(Theme.Font.caption)
                            .foregroundStyle(active ? roleColor : Theme.Palette.textTertiary)
                    }
                    Text(layer.note)
                        .font(Theme.Font.caption)
                        .foregroundStyle(active ? Theme.Palette.textSecondary : Theme.Palette.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
                if !layer.fields.isEmpty {
                    Button {
                        expanded.toggle()
                    } label: {
                        Image(systemName: expanded ? "chevron.up" : "chevron.down")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(Theme.Palette.textTertiary)
                            .frame(width: 20, height: 20)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help(expanded ? "Hide fields" : "Show fields")
                    .accessibilityLabel(expanded ? "Hide \(layer.title) fields" : "Show \(layer.title) fields")
                }
            }
            if expanded {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(layer.fields, id: \.name) { field in
                        HStack(alignment: .firstTextBaseline, spacing: Theme.Spacing.xs) {
                            Circle()
                                .fill(field.changed ? Theme.Palette.accent : .clear)
                                .frame(width: 5, height: 5)
                                .help(field.changed ? "Set by this device" : "")
                            Text(field.name)
                                .foregroundStyle(Theme.Palette.textSecondary)
                                .frame(width: 104, alignment: .leading)
                            Text(field.value)
                                .foregroundStyle(Theme.Palette.textPrimary)
                                .textSelection(.enabled)
                        }
                        .font(Theme.Font.monoSmall)
                    }
                }
                .padding(.leading, 32)
            }
        }
        .padding(Theme.Spacing.s)
        .background(
            RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous)
                .fill(active ? Theme.Palette.surfaceSunken : .clear)
        )
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous)
                .strokeBorder(active ? .clear : Theme.Palette.border, style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
        )
        .accessibilityElement(children: .combine)
    }

    private var roleLabel: String {
        switch layer.role {
        case .changed: "Built here"
        case .used: "Read"
        case .notUsed: "Not read"
        }
    }

    private var roleColor: Color {
        layer.role == .changed ? Theme.Palette.accent : Theme.Palette.textSecondary
    }
}

// MARK: - Cable

private struct LinkInspector: View {
    let link: LinkInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager

    // Drafts while a slider moves; committed to the cable when it's let go.
    @State private var latency = 0.0
    @State private var jitter = 0.0
    @State private var loss = 0.0

    private static let bandwidths: [(String, UInt32?)] = [
        ("Unlimited", nil), ("1 Gbit/s", 1_000_000), ("100 Mbit/s", 100_000), ("10 Mbit/s", 10_000), ("1.5 Mbit/s", 1_544),
    ]

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

                    InspectorSection("Appearance") {
                        StyleRow("Colour") {
                            SwatchPicker(selection: link.color) { color in
                                document.setLinkStyle(link.id, to: LinkStyle(color: color, line: link.line),
                                                      undoManager: undoManager)
                            }
                        }
                        StyleRow("Line") {
                            LinePatternPicker(selection: link.displayedLine) { line in
                                document.setLinkStyle(link.id, to: LinkStyle(color: link.color, line: line == .solid ? nil : line),
                                                      undoManager: undoManager)
                            }
                        }
                        Text(link.medium == .fiber ? "Fibre cables are drawn as a double line." : "Copper cables are drawn as a single line.")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textTertiary)
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

                    InspectorSection("Conditioning") {
                        slider("Latency", value: $latency, range: 0...500, unit: "ms")
                        slider("Jitter", value: $jitter, range: 0...100, unit: "ms")
                        slider("Packet loss", value: $loss, range: 0...100, unit: "%")
                        HStack {
                            Text("Bandwidth").font(Theme.Font.body).foregroundStyle(Theme.Palette.textSecondary)
                            Spacer()
                            Picker("Bandwidth", selection: Binding(
                                get: { link.conditioning.bandwidthKbps },
                                set: { kbps in
                                    var c = link.conditioning
                                    c.bandwidthKbps = kbps
                                    document.setLinkConditioning(link.id, c, undoManager: undoManager)
                                }
                            )) {
                                ForEach(Self.bandwidths, id: \.0) { Text($0.0).tag($0.1) }
                            }
                            .labelsHidden()
                            .fixedSize()
                            .controlSize(.small)
                        }
                        Text("Applied to every frame on this cable in the simulation: ping times grow, lost frames show in Events.")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textTertiary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .onAppear(perform: loadConditioning)
                    .onChange(of: link.id) { _, _ in loadConditioning() }
                    .onChange(of: link.conditioning) { _, _ in loadConditioning() }

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
            Slider(value: value, in: range, onEditingChanged: { editing in
                if !editing { commitConditioning() }
            })
            .controlSize(.small)
        }
    }

    private func loadConditioning() {
        latency = Double(link.conditioning.latencyMs)
        jitter = Double(link.conditioning.jitterMs)
        loss = Double(link.conditioning.lossPercent)
    }

    private func commitConditioning() {
        var c = link.conditioning
        c.latencyMs = UInt32(latency.rounded())
        c.jitterMs = UInt32(jitter.rounded())
        c.lossPercent = UInt8(loss.rounded())
        guard c != link.conditioning else { return }
        document.setLinkConditioning(link.id, c, undoManager: undoManager)
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

    private var isNote: Bool { annotation.kind == .note }
    private var style: ShapeStyle { annotation.style }
    private var name: String { isNote ? "Note" : style.shape.title }

    var body: some View {
        VStack(spacing: 0) {
            InspectorHeader(title: name,
                            subtitle: isNote ? "Text on the canvas" : "Outlines a zone; devices inside move with it") {
                if isNote {
                    Image(systemName: "note.text")
                        .font(.system(size: 24, weight: .light))
                        .foregroundStyle(Theme.Palette.textSecondary)
                } else {
                    AnnotationOutline(kind: style.shape)
                        .fill(style.fillColor)
                        .overlay(AnnotationOutline(kind: style.shape)
                            .stroke(style.strokeColor, style: StrokeStyle(lineWidth: 1.5, dash: style.line.dash(lineWidth: 1.5))))
                        .frame(width: 34, height: 30)
                }
            } trailing: { EmptyView() }
            PanelDivider(axis: .horizontal)
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.xl) {
                    InspectorSection(isNote ? "Text" : "Label") {
                        TextField(isNote ? "Note" : "Name", text: $draft, axis: .vertical)
                            .lineLimit(isNote ? 3...10 : 1...2)
                            .textFieldStyle(.roundedBorder)
                            .font(Theme.Font.body)
                            .focused($focused)
                            .onSubmit(commit)
                            .onChange(of: focused) { _, isFocused in if !isFocused { commit() } }
                    }
                    InspectorSection("Style") {
                        if isNote {
                            StyleRow("Colour") {
                                SwatchPicker(selection: style.fill) { color in
                                    var new = style
                                    new.fill = color
                                    setStyle(new, "Change Colour")
                                }
                            }
                        } else {
                            StyleRow("Shape") {
                                ShapeKindPicker(selection: style.shape) { shape in
                                    var new = style
                                    new.shape = shape
                                    setStyle(new, "Change Shape")
                                }
                            }
                            StyleRow("Border") {
                                SwatchPicker(selection: style.stroke) { color in
                                    var new = style
                                    new.stroke = color
                                    setStyle(new, "Change Border Colour")
                                }
                                LinePatternPicker(selection: style.line) { line in
                                    var new = style
                                    new.line = line
                                    setStyle(new, "Change Border Style")
                                }
                            }
                            StyleRow("Label") {
                                SwatchPicker(selection: style.label, defaultHelp: "Automatic: neutral, contrasting with the fill") { color in
                                    var new = style
                                    new.label = color
                                    setStyle(new, "Change Label Colour")
                                }
                            }
                            StyleRow("Fill") {
                                SwatchPicker(selection: style.fill) { color in
                                    var new = style
                                    new.fill = color
                                    // A colour at the faint default opacity is hard to see.
                                    if color != nil && new.fillOpacity < 0.12 { new.fillOpacity = 0.15 }
                                    setStyle(new, "Change Fill")
                                }
                                FillOpacitySlider(value: style.fillOpacity) { opacity in
                                    var new = style
                                    new.fillOpacity = opacity
                                    setStyle(new, "Change Fill Opacity")
                                }
                            }
                        }
                    }
                    InspectorSection("Arrange") {
                        StackButtons { move in document.reorderAnnotation(annotation.id, move, undoManager: undoManager) }
                        Toggle("Locked", isOn: Binding(
                            get: { annotation.locked },
                            set: { document.setAnnotationsLocked([annotation.id], $0, undoManager: undoManager) }
                        ))
                        .toggleStyle(.switch)
                        .controlSize(.small)
                        if annotation.group != nil {
                            Button("Ungroup", systemImage: "rectangle.3.group") { workspace.arrangeRequest = .ungroup }
                                .controlSize(.small)
                        }
                    }
                    InspectorSection("Frame") {
                        KeyValueRow("Position", "\(Int(annotation.x)), \(Int(annotation.y))", mono: true)
                        KeyValueRow("Size", "\(Int(annotation.width)) × \(Int(annotation.height))", mono: true)
                    }
                    Button("Delete \(name)", systemImage: "trash", role: .destructive) {
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

    private func setStyle(_ new: ShapeStyle, _ actionName: String) {
        document.setAnnotationStyle(annotation.id, to: new, actionName: actionName, undoManager: undoManager)
    }

    private func commit() {
        document.updateAnnotation(annotation.id, rect: annotation.rect, text: draft,
                                  actionName: "Edit \(name)", undoManager: undoManager)
    }
}

/// Fill opacity, committed when the drag ends so one drag is one undo step.
private struct FillOpacitySlider: View {
    let value: Double
    let onCommit: (Double) -> Void
    @State private var draft: Double?

    var body: some View {
        HStack(spacing: Theme.Spacing.s) {
            Slider(value: Binding(get: { draft ?? value }, set: { draft = $0 }), in: 0...1) { editing in
                if !editing, let draft {
                    onCommit((draft * 100).rounded() / 100)
                    self.draft = nil
                }
            }
            .controlSize(.small)
            Text("\(Int(((draft ?? value) * 100).rounded())) %")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.Palette.textSecondary)
                .monospacedDigit()
                .frame(width: 38, alignment: .trailing)
        }
    }
}

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
        let annotations = document.state.annotations.filter { workspace.selectedAnnotations.contains($0.id) }
        let total = devices.count + annotations.count
        let groups = Set(devices.compactMap(\.group) + annotations.compactMap(\.group))
        VStack(spacing: 0) {
            InspectorHeader(title: annotations.isEmpty ? "\(devices.count) devices" : "\(total) items",
                            subtitle: groups.count == 1 && devices.allSatisfy({ $0.group != nil })
                                && annotations.allSatisfy({ $0.group != nil }) ? "Group" : "Multiple selection") {
                Image(systemName: "square.on.square.dashed")
                    .font(.system(size: 24, weight: .light))
                    .foregroundStyle(Theme.Palette.textSecondary)
            } trailing: { EmptyView() }
            PanelDivider(axis: .horizontal)
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.xl) {
                    InspectorSection("Group") {
                        HStack {
                            Button("Group", systemImage: "rectangle.3.group") { workspace.arrangeRequest = .group }
                            Button("Ungroup") { workspace.arrangeRequest = .ungroup }
                                .disabled(groups.isEmpty)
                        }
                        .controlSize(.small)
                        Text("Grouped items select and move together.")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textTertiary)
                    }
                    if !devices.isEmpty {
                        InspectorSection("Device colour") {
                            SwatchPicker(selection: Set(devices.map(\.color)).count == 1 ? devices[0].color : nil) { color in
                                document.setDeviceColor(Set(devices.map(\.id)), to: color, undoManager: undoManager)
                            }
                        }
                    }
                    if !annotations.isEmpty {
                        InspectorSection("Shapes and notes") {
                            StackButtons { move in workspace.arrangeRequest = .stack(move) }
                            HStack {
                                Button("Lock", systemImage: "lock") { workspace.arrangeRequest = .lock }
                                Button("Unlock", systemImage: "lock.open") { workspace.arrangeRequest = .unlock }
                            }
                            .controlSize(.small)
                        }
                    }
                    if devices.count > 1 {
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
                    }
                    if !devices.isEmpty {
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
                    SnapshotsSection(document: document, workspace: workspace)
                }
                .padding(Theme.Spacing.l)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

}

/// Saved states of the topology: take one before a risky change, restore
/// it if the change goes wrong. Restoring can be undone.
private struct SnapshotsSection: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager
    @State private var naming = false
    @State private var name = ""
    @State private var confirming: Int?
    @FocusState private var nameFocused: Bool

    var body: some View {
        InspectorSection("Snapshots") {
            if document.snapshots.isEmpty && !naming {
                Text("Save the topology as it is now, and come back to it after you try something.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            ForEach(Array(document.snapshots.enumerated()).reversed(), id: \.offset) { index, snapshot in
                row(snapshot, index: index)
            }
            if naming {
                HStack(spacing: Theme.Spacing.s) {
                    TextField("Snapshot name", text: $name)
                        .textFieldStyle(.roundedBorder)
                        .focused($nameFocused)
                        .onSubmit(take)
                        .onExitCommand { naming = false }
                    Button("Save", action: take).controlSize(.small)
                }
            } else {
                Button("Take Snapshot", systemImage: "camera") {
                    name = "Snapshot \(document.snapshots.count + 1)"
                    naming = true
                    nameFocused = true
                }
                .controlSize(.small)
            }
        }
        .confirmationDialog("Restore this snapshot?", isPresented: Binding(
            get: { confirming != nil }, set: { if !$0 { confirming = nil } }
        )) {
            Button("Restore") {
                if let index = confirming,
                   case .failure(let error) = document.restoreSnapshot(index, undoManager: undoManager) {
                    workspace.flash(error.message)
                } else {
                    workspace.flash("Snapshot restored. Edit → Undo brings back what you had.")
                }
                confirming = nil
            }
        } message: {
            Text("The topology goes back to how it was. You can undo this.")
        }
    }

    private func row(_ snapshot: SnapshotInfo, index: Int) -> some View {
        HStack(spacing: Theme.Spacing.s) {
            Image(systemName: "clock.arrow.circlepath").foregroundStyle(Theme.Palette.textTertiary)
            VStack(alignment: .leading, spacing: 1) {
                Text(snapshot.name).font(Theme.Font.body).foregroundStyle(Theme.Palette.textPrimary)
                Text("\(Self.relative(snapshot.taken)) · \(snapshot.devices) devices, \(snapshot.cables) cables")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textTertiary)
            }
            Spacer(minLength: Theme.Spacing.s)
            Button("Restore") { confirming = index }
                .controlSize(.small)
        }
        .contextMenu {
            Button("Restore…") { confirming = index }
            Button("Delete", role: .destructive) { document.deleteSnapshot(index, undoManager: undoManager) }
        }
    }

    private func take() {
        document.takeSnapshot(named: name, undoManager: undoManager)
        naming = false
    }

    static func relative(_ seconds: UInt64) -> String {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .full
        return formatter.localizedString(for: Date(timeIntervalSince1970: TimeInterval(seconds)), relativeTo: Date())
    }
}

/// What NAT did at this hop: the addresses before and after, side by side,
/// with the changed ones highlighted, and the rule that did it.
private struct NatDecisionView: View {
    let decision: NatDecisionInfo
    let device: String

    private var title: String {
        guard decision.translated else { return "NAT at \(device): not translated" }
        return decision.direction == "inside-to-outside"
            ? "NAT at \(device): inside → outside"
            : "NAT at \(device): outside → inside"
    }

    var body: some View {
        InspectorSection(title) {
            VStack(alignment: .leading, spacing: Theme.Spacing.s) {
                row("Source", decision.beforeSrc, decision.afterSrc)
                row("Destination", decision.beforeDst, decision.afterDst)
            }
            .padding(Theme.Spacing.s)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous))
            if let rule = decision.rule {
                Text(rule)
                    .font(Theme.Font.monoSmall)
                    .foregroundStyle(Theme.Palette.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }
            Label(status.0, systemImage: status.1)
                .font(Theme.Font.labelSemibold)
                .foregroundStyle(decision.translated ? Theme.Palette.textPrimary : Theme.Palette.statusWarning)
        }
    }

    private var status: (String, String) {
        if !decision.translated { return ("Sent untranslated", "exclamationmark.triangle.fill") }
        return decision.created
            ? ("New translation created", "plus.circle.fill")
            : ("Existing translation used", "arrow.triangle.2.circlepath")
    }

    /// "Source" over "before → after", the rewritten address highlighted.
    private func row(_ label: String, _ before: String, _ after: String) -> some View {
        let changed = before != after
        return VStack(alignment: .leading, spacing: 2) {
            Text(label)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.Palette.textTertiary)
            if changed {
                HStack(spacing: Theme.Spacing.xs) {
                    Text(before)
                        .foregroundStyle(Theme.Palette.textSecondary)
                        .strikethrough(color: Theme.Palette.textTertiary)
                    Image(systemName: "arrow.right")
                        .foregroundStyle(Theme.Palette.textTertiary)
                    Text(after)
                        .foregroundStyle(Theme.Palette.textPrimary)
                        .padding(.horizontal, 4)
                        .background(Color(light: 0x1F8A9E, dark: 0x3AAFC4).opacity(0.18),
                                    in: RoundedRectangle(cornerRadius: 4))
                }
                .lineLimit(1)
                .minimumScaleFactor(0.8)
            } else {
                Text("\(before) (unchanged)")
                    .foregroundStyle(Theme.Palette.textSecondary)
                    .lineLimit(1)
            }
        }
        .font(Theme.Font.monoSmall)
    }
}

/// The access list a router checked at this hop, every line shown, the one
/// that decided highlighted (or the implicit deny at the end).
private struct AclDecisionView: View {
    let decision: AclDecisionInfo

    var body: some View {
        InspectorSection("Access list \(decision.acl) \(decision.direction) on \(decision.interface.abbreviatedInterface)") {
            VStack(alignment: .leading, spacing: 2) {
                ForEach(decision.lines, id: \.sequence) { line in
                    row(sequence: "\(line.sequence)", text: line.text, matched: decision.matched == line.sequence)
                }
                row(sequence: "", text: "deny any (implicit)", matched: decision.matched == nil)
            }
            .padding(Theme.Spacing.s)
            .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous))
            Label(decision.permitted ? "Permitted" : "Dropped",
                  systemImage: decision.permitted ? "checkmark.shield.fill" : "xmark.shield.fill")
                .font(Theme.Font.labelSemibold)
                .foregroundStyle(decision.permitted ? Theme.Palette.statusUp : Theme.Palette.statusDown)
        }
    }

    private func row(sequence: String, text: String, matched: Bool) -> some View {
        let color = decision.permitted ? Theme.Palette.statusUp : Theme.Palette.statusDown
        return HStack(spacing: Theme.Spacing.s) {
            Text(sequence)
                .frame(width: 28, alignment: .trailing)
                .foregroundStyle(Theme.Palette.textTertiary)
            Text(text)
                .foregroundStyle(matched ? Theme.Palette.textPrimary : Theme.Palette.textSecondary)
            Spacer(minLength: 0)
            if matched {
                Image(systemName: "arrow.left").foregroundStyle(color)
            }
        }
        .font(Theme.Font.monoSmall)
        .padding(.vertical, 2)
        .padding(.horizontal, 4)
        .background(matched ? color.opacity(0.15) : .clear, in: RoundedRectangle(cornerRadius: 4))
    }
}

/// A server's DHCP service, as in Packet Tracer's Services tab: one pool
/// with its gateway and DNS server, handing out addresses from a start one.
private struct ServerDHCPEditor: View {
    let device: DeviceInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager
    @State private var network = ""
    @State private var mask = "255.255.255.0"
    @State private var gateway = ""
    @State private var dns = ""
    @State private var start = ""

    var body: some View {
        let service = document.serverDHCP(device.id)
        InspectorSection("DHCP Service") {
            Toggle("Hand out addresses", isOn: Binding(
                get: { service != nil },
                set: { on in on ? enable() : save(nil) }
            ))
            .toggleStyle(.switch)
            .controlSize(.mini)
            .font(Theme.Font.body)
            if service != nil {
                field("Network", $network, "192.168.1.0")
                field("Mask", $mask, "255.255.255.0")
                field("Gateway", $gateway, "Optional")
                field("DNS server", $dns, "Optional")
                field("Start address", $start, "192.168.1.100")
                HStack {
                    Spacer()
                    Button("Apply") { save(current()) }
                        .controlSize(.small)
                        .disabled(current() == service)
                }
                Text("PCs set to DHCP on this network get an address from \(start.isEmpty ? "the start address" : start) up.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .onAppear(perform: load)
        .onChange(of: device.id) { _, _ in load() }
    }

    private func field(_ label: String, _ text: Binding<String>, _ prompt: String) -> some View {
        HStack {
            Text(label).font(Theme.Font.body).foregroundStyle(Theme.Palette.textSecondary)
                .frame(width: 104, alignment: .leading)
            TextField(label, text: text, prompt: Text(prompt))
                .textFieldStyle(.roundedBorder)
                .font(Theme.Font.mono)
                .labelsHidden()
                .onSubmit { save(current()) }
        }
    }

    private func load() {
        guard let s = document.serverDHCP(device.id) else { return }
        network = s.network
        mask = Self.mask(s.prefixLen)
        gateway = s.gateway ?? ""
        dns = s.dns ?? ""
        start = s.start
    }

    /// Turning it on suggests settings from the server's own address.
    private func enable() {
        let parts = (device.ip?.address ?? "192.168.1.10").split(separator: ".")
        let base = parts.prefix(3).joined(separator: ".")
        network = "\(base).0"
        mask = device.ip?.mask ?? "255.255.255.0"
        gateway = device.ip?.gateway ?? ""
        start = "\(base).100"
        save(current())
    }

    private func current() -> ServerDhcp? {
        guard let prefix = Self.prefix(mask) else { return nil }
        return ServerDhcp(network: network, prefixLen: prefix, gateway: gateway.isEmpty ? nil : gateway,
                          dns: dns.isEmpty ? nil : dns, start: start)
    }

    private func save(_ service: ServerDhcp?) {
        if case .failure(let error) = document.setServerDHCP(device.id, service, undoManager: undoManager) {
            workspace.flash(error.message)
        } else {
            load()
        }
    }

    static func prefix(_ mask: String) -> UInt8? {
        let octets = mask.split(separator: ".").compactMap { UInt8($0) }
        guard octets.count == 4 else { return nil }
        let bits = octets.reduce(UInt32(0)) { $0 << 8 | UInt32($1) }
        let ones = bits.nonzeroBitCount
        return bits == (ones == 0 ? 0 : UInt32.max << (32 - ones)) ? UInt8(ones) : nil
    }

    static func mask(_ prefix: UInt8) -> String {
        let bits = prefix == 0 ? UInt32(0) : UInt32.max << (32 - UInt32(prefix))
        return (0..<4).map { String((bits >> (24 - 8 * $0)) & 0xff) }.joined(separator: ".")
    }
}
