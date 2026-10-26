import SwiftUI

/// Floating controls over the canvas.
struct CanvasOverlays: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    let visibleRect: CGRect
    let onZoomToFit: () -> Void
    let onScrollTo: (CGPoint) -> Void

    var body: some View {
        ZStack {
            if document.state.devices.isEmpty {
                VStack(spacing: Theme.Spacing.s) {
                    IconView(.canvasEmpty, size: 34)
                        .foregroundStyle(Theme.Palette.textTertiary)
                    Text("Drag a device here to start")
                        .font(Theme.Font.emptyTitle)
                        .foregroundStyle(Theme.Palette.textPrimary)
                    Text("Or double-click one in the library. Press ⌘K for every command.")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.textSecondary)
                }
                .allowsHitTesting(false)
            }

            VStack(spacing: 0) {
                if workspace.tool == .rectangle {
                    ShapeToolOptions(workspace: workspace)
                        .padding(.top, Theme.Spacing.m)
                }
                if let hint = workspace.hint ?? toolHint {
                    Text(hint)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.textPrimary)
                        .padding(.horizontal, Theme.Spacing.m)
                        .padding(.vertical, Theme.Spacing.xs + 2)
                        .background(Capsule().fill(Theme.Palette.surfaceRaised))
                        .overlay(Capsule().strokeBorder(Theme.Palette.border))
                        .shadow(color: .black.opacity(0.08), radius: 4, y: 1)
                        .padding(.top, Theme.Spacing.m)
                        .transition(.opacity)
                        .allowsHitTesting(false)
                }
                Spacer()
                HStack(alignment: .bottom) {
                    VStack(alignment: .leading, spacing: Theme.Spacing.s) {
                        if workspace.showsLayer2 { Layer2Legend() }
                        ZoomControl(workspace: workspace, onZoomToFit: onZoomToFit)
                    }
                    Spacer()
                    if workspace.mode == .simulation {
                        SimulationTimeline(document: document, workspace: workspace)
                    }
                    Spacer()
                    if workspace.showsMinimap && !document.state.devices.isEmpty {
                        Minimap(document: document, workspace: workspace, visibleRect: visibleRect, onScrollTo: onScrollTo)
                    } else {
                        Color.clear.frame(width: 1, height: 1)
                    }
                }
                .padding(Theme.Spacing.m)
            }
        }
        .animation(.easeOut(duration: 0.15), value: workspace.hint)
    }

    private var toolHint: String? {
        if workspace.isPickingPDU {
            return workspace.pduSource == nil
                ? "Add Simple PDU: click the device to ping from. Esc to cancel."
                : "Now click the device to ping. Esc to cancel."
        }
        return switch workspace.tool {
        case .cable: "\(workspace.cableKind.title) cable: drag from one device to another, or click both. Esc to finish."
        case .hand: "Drag to pan. Pinch to zoom."
        case .rectangle: "Drag to draw. Devices inside a shape move with it."
        default: nil
        }
    }
}

/// Key to the VLAN / spanning-tree overlay, shown once VLANs, trunks or
/// blocked ports are on the canvas.
private struct Layer2Legend: View {
    @EnvironmentObject private var simulation: SimulationController

    var body: some View {
        if let overlay = simulation.layer2,
           !overlay.vlans.isEmpty || overlay.ports.contains(where: { $0.trunk || $0.blocked }) {
            FloatingPanel {
                VStack(alignment: .leading, spacing: 3) {
                    ForEach(overlay.vlans, id: \.self) { vlan in
                        legendRow(color: LinksLayer.vlanColor(vlan), text: "VLAN \(vlan)")
                    }
                    if overlay.ports.contains(where: \.trunk) {
                        HStack(spacing: Theme.Spacing.xs) {
                            Capsule().fill(Theme.Palette.link).frame(width: 16, height: 4)
                                .overlay(Capsule().fill(Theme.Palette.canvas).frame(width: 10, height: 1))
                            Text("Trunk").font(Theme.Font.caption)
                        }
                    }
                    if overlay.ports.contains(where: \.blocked) {
                        HStack(spacing: Theme.Spacing.xs) {
                            Capsule().fill(Theme.Palette.statusDown).frame(width: 3, height: 12).frame(width: 16)
                            Text("Blocked by STP").font(Theme.Font.caption)
                        }
                    }
                    if !overlay.roots.isEmpty {
                        HStack(spacing: Theme.Spacing.xs) {
                            RootBadge()
                            Text("Root bridge").font(Theme.Font.caption)
                        }
                    }
                }
                .foregroundStyle(Theme.Palette.textSecondary)
                .padding(.horizontal, Theme.Spacing.s)
                .padding(.vertical, Theme.Spacing.xs)
            }
            .help("VLANs and spanning tree (⌥⌘V to hide)")
        }
    }

    private func legendRow(color: Color, text: String) -> some View {
        HStack(spacing: Theme.Spacing.xs) {
            Capsule().fill(color).frame(width: 16, height: 3)
            Text(text).font(Theme.Font.caption)
        }
    }
}

/// Shape tool: which shape to draw (and how many sides for a polygon).
private struct ShapeToolOptions: View {
    @ObservedObject var workspace: WorkspaceModel

    var body: some View {
        FloatingPanel {
            HStack(spacing: Theme.Spacing.xs) {
                ForEach(ShapeKind.choices, id: \.self) { kind in
                    IconButton(glyph: .system(kind.isPolygon ? "hexagon" : kind.symbol),
                               help: kind.isPolygon ? "Polygon" : kind.title, size: 14,
                               isOn: kind.isSameKind(as: workspace.shapeKind)) {
                        if !kind.isSameKind(as: workspace.shapeKind) { workspace.shapeKind = kind }
                    }
                }
                if case .polygon(let sides) = workspace.shapeKind {
                    PanelDivider(axis: .vertical).frame(height: 18).padding(.horizontal, Theme.Spacing.xs)
                    Text("\(sides) sides")
                        .font(Theme.Font.captionMedium)
                        .monospacedDigit()
                        .foregroundStyle(Theme.Palette.textPrimary)
                        .frame(minWidth: 52)
                    Stepper("Sides", value: Binding(
                        get: { Int(sides) },
                        set: { workspace.shapeKind = .polygon(sides: UInt8($0)) }
                    ), in: Int(ShapeKind.sidesRange.lowerBound)...Int(ShapeKind.sidesRange.upperBound))
                    .labelsHidden()
                    .controlSize(.small)
                }
            }
            // Even inset all round, so a highlighted option sits as far from
            // the panel's left edge as from its top and bottom.
            .padding(3)
        }
    }
}

private struct ZoomControl: View {
    @ObservedObject var workspace: WorkspaceModel
    let onZoomToFit: () -> Void

    var body: some View {
        FloatingPanel {
            HStack(spacing: 0) {
                IconButton(glyph: .system("minus"), help: "Zoom Out (⌘−)", size: 13) { workspace.zoomOut() }
                Button {
                    workspace.setZoom(1)
                } label: {
                    Text("\(Int((workspace.zoom * 100).rounded()))%")
                        .font(Theme.Font.captionMedium)
                        .monospacedDigit()
                        .foregroundStyle(Theme.Palette.textPrimary)
                        .frame(width: 44)
                }
                .buttonStyle(.plain)
                .help("Actual Size (⌘0)")
                IconButton(glyph: .system("plus"), help: "Zoom In (⌘+)", size: 13) { workspace.zoomIn() }
                Rectangle().fill(Theme.Palette.border).frame(width: 1, height: 16).padding(.horizontal, 2)
                IconButton(glyph: .system("arrow.up.left.and.arrow.down.right"), help: "Zoom to Fit (⌘9)", size: 13, action: onZoomToFit)
            }
            .padding(3)
        }
    }
}

private struct Minimap: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    let visibleRect: CGRect
    let onScrollTo: (CGPoint) -> Void

    private let size = CGSize(width: 176, height: 112)

    var body: some View {
        let bounds = contentBounds
        let scale = min(size.width / bounds.width, size.height / bounds.height)
        FloatingPanel {
            Canvas { context, _ in
                func map(_ p: CGPoint) -> CGPoint {
                    CGPoint(x: (p.x - bounds.minX) * scale, y: (p.y - bounds.minY) * scale)
                }
                let byId = Dictionary(uniqueKeysWithValues: document.state.devices.map { ($0.id, $0) })
                for link in document.state.links {
                    guard let a = byId[link.a.deviceId], let b = byId[link.b.deviceId] else { continue }
                    var path = Path()
                    path.move(to: map(a.position))
                    path.addLine(to: map(b.position))
                    context.stroke(path, with: .color(Theme.Palette.link), lineWidth: 1)
                }
                for device in document.state.devices {
                    let s = DeviceSymbol.size(for: device.kind)
                    let c = map(device.position)
                    let w = max(4, s.width * scale), h = max(3, s.height * scale)
                    let rect = CGRect(x: c.x - w / 2, y: c.y - h / 2, width: w, height: h)
                    context.fill(DeviceSymbol.outline(device.kind, in: rect),
                                 with: .color(workspace.selection.contains(device.id) ? Theme.Palette.accent : Theme.Palette.textSecondary))
                }
                let viewport = CGRect(x: (visibleRect.minX / workspace.zoom - bounds.minX) * scale,
                                      y: (visibleRect.minY / workspace.zoom - bounds.minY) * scale,
                                      width: visibleRect.width / workspace.zoom * scale,
                                      height: visibleRect.height / workspace.zoom * scale)
                context.fill(Path(roundedRect: viewport, cornerRadius: 3), with: .color(Theme.Palette.accent.opacity(0.08)))
                context.stroke(Path(roundedRect: viewport, cornerRadius: 3), with: .color(Theme.Palette.accent), lineWidth: 1.2)
            }
            .frame(width: size.width, height: size.height)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.m - 2, style: .continuous))
            .padding(4)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0).onChanged { value in
                    onScrollTo(CGPoint(x: (value.location.x - 4) / scale + bounds.minX,
                                       y: (value.location.y - 4) / scale + bounds.minY))
                }
            )
            .help("Minimap: click or drag to move the view")
        }
    }

    /// Devices' extent with padding, so the minimap shows the topology, not the empty world.
    private var contentBounds: CGRect {
        let xs = document.state.devices.map(\.x), ys = document.state.devices.map(\.y)
        let pad: CGFloat = 160
        let minX = max(0, (xs.min() ?? 0) - pad), minY = max(0, (ys.min() ?? 0) - pad)
        let maxX = (xs.max() ?? 0) + pad, maxY = (ys.max() ?? 0) + pad
        return CGRect(x: minX, y: minY, width: max(maxX - minX, 400), height: max(maxY - minY, 260))
    }
}

/// Packet Tracer-style simulation controls.
private struct SimulationTimeline: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController

    var body: some View {
        let events = simulation.trace.filter { $0.netProtocol.map(workspace.visibleProtocols.contains) ?? true }
        let processed = events.filter { $0.time <= workspace.simulationTime }.count
        FloatingPanel {
            HStack(spacing: Theme.Spacing.xs) {
                IconButton(glyph: .system("backward.end.fill"), help: "Reset Simulation", size: 13) {
                    workspace.isPlaying = false
                    workspace.simulationTime = 0
                }
                IconButton(glyph: .system("backward.frame.fill"), help: "Step Back (⌘[)", size: 13) {
                    workspace.isPlaying = false
                    workspace.simulationTime = events.last { $0.time < workspace.simulationTime - 0.000_001 }.map { $0.time } ?? 0
                }
                Button {
                    workspace.isPlaying.toggle()
                } label: {
                    Image(systemName: workspace.isPlaying ? "pause.fill" : "play.fill")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Theme.Palette.onAccent)
                        .frame(width: 30, height: 26)
                        .background(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).fill(Theme.Palette.accent))
                }
                .buttonStyle(.plain)
                .help(workspace.isPlaying ? "Pause" : "Play")
                IconButton(glyph: .system("forward.frame.fill"), help: "Step Forward (⌘])", size: 13) {
                    workspace.isPlaying = false
                    if let next = events.first(where: { $0.time > workspace.simulationTime + 0.000_001 }) {
                        workspace.simulationTime = next.time
                    }
                }

                VStack(alignment: .leading, spacing: 1) {
                    Text(workspace.simulationTime.simulationClock)
                        .font(Theme.Font.mono)
                        .foregroundStyle(Theme.Palette.textPrimary)
                    Text("Event \(processed) of \(events.count)")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.textSecondary)
                }
                .frame(width: 104, alignment: .leading)
                .padding(.leading, Theme.Spacing.xs)

                Menu {
                    Picker("Speed", selection: $workspace.simulationSpeed) {
                        ForEach([0.25, 0.5, 1, 2, 4], id: \.self) { Text("\($0.formatted())×").tag($0) }
                    }
                    .pickerStyle(.inline)
                } label: {
                    Text("\(workspace.simulationSpeed.formatted())×")
                        .font(Theme.Font.captionMedium)
                        .monospacedDigit()
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
                .help("Playback speed")

                Menu {
                    ForEach(NetProtocol.allCases) { proto in
                        Toggle(proto.rawValue, isOn: Binding(
                            get: { workspace.visibleProtocols.contains(proto) },
                            set: { on in
                                if on { workspace.visibleProtocols.insert(proto) } else { workspace.visibleProtocols.remove(proto) }
                            }
                        ))
                    }
                    Divider()
                    Button("Show All") { workspace.visibleProtocols = Set(NetProtocol.allCases) }
                } label: {
                    Image(systemName: "line.3.horizontal.decrease.circle")
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
                .help("Filter protocols")
            }
            .padding(.horizontal, Theme.Spacing.s)
            .padding(.vertical, Theme.Spacing.xs + 1)
        }
    }
}

// MARK: - Context menus

struct DeviceContextMenu: View {
    let device: DeviceInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager

    var body: some View {
        let targets = workspace.selection.contains(device.id) ? Array(workspace.selection) : [device.id]
        Menu("Power", systemImage: "power") {
            Button("Start") { workspace.setPower(.running, for: targets) }
            Button("Suspend") { workspace.setPower(.suspended, for: targets) }
            Button("Stop") { workspace.setPower(.stopped, for: targets) }
            Button("Reload") { workspace.setPower(.running, for: targets) }
        }
        Divider()
        Button("Open Console", systemImage: "terminal") { workspace.openConsole(for: device.id) }
        Button("Rename…", systemImage: "pencil") {
            workspace.selectDevices([device.id])
            workspace.renamingDevice = device.id
        }
        Button("Configure…", systemImage: "slider.horizontal.3") {
            workspace.selectDevices([device.id])
            workspace.inspectorTab = .config
            workspace.showsInspector = true
        }
        if device.kind.hasCli {
            Button("Copy Configuration To…", systemImage: "doc.on.doc") {
                workspace.copyConfigSource = device.id
            }
            .disabled(!document.state.devices.contains { $0.kind == device.kind && $0.id != device.id })
        }
        if let link = document.links(attachedTo: device.id).first {
            Button("Capture on \(link.endpoint(for: device.id)?.port.abbreviatedInterface ?? "Cable")", image: ImageResource(name: Icon.capture.rawValue, bundle: .main)) {
                workspace.startCapture(on: link.id)
            }
        }
        Divider()
        Menu("Colour", systemImage: "paintpalette") {
            SwatchMenuItems(selection: device.color) { color in
                document.setDeviceColor(Set(targets), to: color, undoManager: undoManager)
            }
        }
        GroupMenuItems(workspace: workspace, isGrouped: device.group != nil)
        Divider()
        Button("Duplicate", systemImage: "plus.square.on.square") {
            workspace.selectDevices(Set(targets))
            workspace.duplicateRequests += 1
        }
        Divider()
        Button("Delete", systemImage: "trash", role: .destructive) {
            document.removeDevices(Set(targets), undoManager: undoManager)
        }
    }
}

struct LinkContextMenu: View {
    let link: LinkInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager

    var body: some View {
        Button("Start Capture", image: ImageResource(name: Icon.capture.rawValue, bundle: .main)) {
            workspace.startCapture(on: link.id)
        }
        Button("Fail Link (Spanning Tree View)", systemImage: "bolt.horizontal.circle") {
            workspace.failLink(link.id)
        }
        .disabled(!(document.device(link.a.deviceId)?.kind.isSwitch ?? false) || !(document.device(link.b.deviceId)?.kind.isSwitch ?? false))
        Button("Fail Link (OSPF View)", systemImage: "bolt.horizontal.circle") {
            workspace.failOspfLink(link.id)
        }
        Button("Link Conditioning…", systemImage: "slider.horizontal.3") {
            workspace.selectLink(link.id)
            workspace.showsInspector = true
        }
        Divider()
        Menu("Colour", systemImage: "paintpalette") {
            SwatchMenuItems(selection: link.color) { color in
                document.setLinkStyle(link.id, to: LinkStyle(color: color, line: link.line), undoManager: undoManager)
            }
        }
        Picker("Line", selection: Binding(
            get: { link.displayedLine },
            set: { document.setLinkStyle(link.id, to: LinkStyle(color: link.color, line: $0 == .solid ? nil : $0),
                                         undoManager: undoManager) }
        )) {
            ForEach(LinePattern.allCases) { Text($0.title).tag($0) }
        }
        Divider()
        Button("Delete Cable", systemImage: "trash", role: .destructive) {
            document.disconnect(link.id, undoManager: undoManager)
        }
    }
}

struct CanvasContextMenu: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    let onZoomToFit: () -> Void

    var body: some View {
        let ids = document.state.devices.map(\.id)
        Button("Start All Devices", systemImage: "power") { workspace.setPower(.running, for: ids) }
        Button("Stop All Devices", systemImage: "poweroff") { workspace.setPower(.stopped, for: ids) }
        Button("Console to All Devices", systemImage: "terminal") {
            for id in ids where document.device(id)?.kind.category == .network { workspace.openConsole(for: id) }
        }
        Divider()
        Toggle("Show Interface Labels", isOn: $workspace.showsInterfaceLabels)
        Toggle("Show Grid", isOn: $workspace.showsGrid)
        Toggle("Show Minimap", isOn: $workspace.showsMinimap)
        Divider()
        Button("Zoom to Fit", systemImage: "arrow.up.left.and.arrow.down.right", action: onZoomToFit)
    }
}

/// Right-click menu of a note or shape. Acts on the whole selection when
/// the item is part of it.
struct AnnotationContextMenu: View {
    let annotation: AnnotationInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager

    var body: some View {
        let isNote = annotation.kind == .note
        Button(isNote ? "Edit Text" : "Edit Label", systemImage: "pencil") {
            workspace.selectAnnotation(annotation.id)
            workspace.editingAnnotation = annotation.id
        }
        Divider()
        Menu(isNote ? "Colour" : "Fill", systemImage: "paintpalette") {
            SwatchMenuItems(selection: annotation.style.fill) { color in
                var style = annotation.style
                style.fill = color
                if !isNote, color != nil, style.fillOpacity < 0.12 { style.fillOpacity = 0.15 }
                document.setAnnotationStyle(annotation.id, to: style, actionName: "Change Colour", undoManager: undoManager)
            }
        }
        if !isNote {
            Menu("Border", systemImage: "square.dashed") {
                SwatchMenuItems(selection: annotation.style.stroke) { color in
                    var style = annotation.style
                    style.stroke = color
                    document.setAnnotationStyle(annotation.id, to: style, actionName: "Change Border Colour",
                                                undoManager: undoManager)
                }
                Divider()
                Picker("Line", selection: Binding(
                    get: { annotation.style.line },
                    set: { line in
                        var style = annotation.style
                        style.line = line
                        document.setAnnotationStyle(annotation.id, to: style, actionName: "Change Border Style",
                                                    undoManager: undoManager)
                    }
                )) {
                    ForEach(LinePattern.allCases) { Text($0.title).tag($0) }
                }
            }
            Menu("Label", systemImage: "textformat") {
                SwatchMenuItems(selection: annotation.style.label) { color in
                    var style = annotation.style
                    style.label = color
                    document.setAnnotationStyle(annotation.id, to: style, actionName: "Change Label Colour",
                                                undoManager: undoManager)
                }
            }
            Menu("Shape", systemImage: "square.on.circle") {
                ForEach([ShapeKind.roundedRectangle, .rectangle, .ellipse] + (3...8).map { ShapeKind.polygon(sides: UInt8($0)) },
                        id: \.self) { kind in
                    Toggle(kind.title, isOn: Binding(
                        get: { annotation.style.shape == kind },
                        set: { _ in
                            var style = annotation.style
                            style.shape = kind
                            document.setAnnotationStyle(annotation.id, to: style, actionName: "Change Shape",
                                                        undoManager: undoManager)
                        }
                    ))
                }
            }
        }
        Divider()
        Menu("Arrange", systemImage: "square.3.layers.3d") {
            ForEach([ProjectDocument.StackMove.front, .forward, .backward, .back], id: \.self) { move in
                Button(move.title) {
                    selectIfNeeded()
                    workspace.arrangeRequest = .stack(move)
                }
            }
        }
        Button(annotation.locked ? "Unlock" : "Lock", systemImage: annotation.locked ? "lock.open" : "lock") {
            selectIfNeeded()
            workspace.arrangeRequest = annotation.locked ? .unlock : .lock
        }
        GroupMenuItems(workspace: workspace, isGrouped: annotation.group != nil)
        Divider()
        Button("Delete", systemImage: "trash", role: .destructive) {
            document.removeAnnotation(annotation.id, undoManager: undoManager)
        }
    }

    private func selectIfNeeded() {
        if !workspace.selectedAnnotations.contains(annotation.id) { workspace.selectAnnotation(annotation.id) }
    }
}

/// Palette entries for a context menu; the first is the default colour.
struct SwatchMenuItems: View {
    let selection: Swatch?
    let onPick: (Swatch?) -> Void

    var body: some View {
        Toggle("Default", isOn: Binding(get: { selection == nil }, set: { _ in onPick(nil) }))
        Divider()
        ForEach(Swatch.allCases) { swatch in
            Toggle(isOn: Binding(get: { selection == swatch }, set: { _ in onPick(swatch) })) {
                Label {
                    Text(swatch.title)
                } icon: {
                    Image(nsImage: swatch.menuImage)
                }
            }
        }
    }
}

/// Group / Ungroup entries for the current selection.
struct GroupMenuItems: View {
    @ObservedObject var workspace: WorkspaceModel
    let isGrouped: Bool

    var body: some View {
        if workspace.selection.count + workspace.selectedAnnotations.count > 1 {
            Button("Group", systemImage: "rectangle.3.group") { workspace.arrangeRequest = .group }
        }
        if isGrouped {
            Button("Ungroup", systemImage: "rectangle.3.group.fill") { workspace.arrangeRequest = .ungroup }
        }
    }
}

extension Swatch {
    /// A colour dot for menus, which can't tint SwiftUI shapes.
    var menuImage: NSImage {
        let image = NSImage(size: NSSize(width: 12, height: 12), flipped: false) { rect in
            NSColor(color).setFill()
            NSBezierPath(ovalIn: rect.insetBy(dx: 1, dy: 1)).fill()
            return true
        }
        return image
    }
}
