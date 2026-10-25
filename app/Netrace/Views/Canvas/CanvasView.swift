import SwiftUI

/// The topology canvas: a scrollable, zoomable world with devices, cables
/// and (in simulation mode) packets, plus floating zoom / minimap / timeline.
struct CanvasView: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager

    /// Live offset of devices being dragged; committed to the core on release
    /// so each drag is one undo step.
    @State private var dragOffset: CGSize = .zero
    @State private var draggedIDs: Set<UInt64> = []
    @State private var cableDrag: CableDrag?
    @State private var pendingCableSource: UInt64?
    @State private var scrollPosition = ScrollPosition()
    @State private var visibleRect: CGRect = .zero
    @State private var magnifyBase: CGFloat?
    @State private var panStart: CGPoint?
    @State private var hasFittedOnOpen = false
    @FocusState private var isFocused: Bool

    struct CableDrag {
        let from: UInt64
        var to: CGPoint
    }

    private static let space = "world"

    var body: some View {
        ZStack {
            ScrollView([.horizontal, .vertical]) {
                world
                    .frame(width: worldSize.width * workspace.zoom, height: worldSize.height * workspace.zoom,
                           alignment: .topLeading)
            }
            .scrollPosition($scrollPosition)
            .onScrollGeometryChange(for: CGRect.self, of: { $0.visibleRect }) { _, rect in
                visibleRect = rect
                // Frame the topology once the viewport size is known.
                if !hasFittedOnOpen && rect.width > 0 {
                    hasFittedOnOpen = true
                    if !document.state.devices.isEmpty { zoomToFit() }
                }
            }
            .background(Theme.Palette.canvas)
            .simultaneousGesture(magnifyGesture)
            .simultaneousGesture(panGesture, including: workspace.tool == .hand ? .all : .subviews)
            .dropDestination(for: String.self) { items, location in
                guard let kind = items.first.flatMap(DeviceKind.init(dragIdentifier:)) else { return false }
                let point = CGPoint(x: (location.x + visibleRect.minX) / workspace.zoom,
                                    y: (location.y + visibleRect.minY) / workspace.zoom)
                let device = document.addDevice(kind, at: point.snapped, undoManager: undoManager)
                workspace.selectDevices([device.id])
                isFocused = true
                return true
            }

            CanvasOverlays(document: document, workspace: workspace, visibleRect: visibleRect,
                           onZoomToFit: zoomToFit, onScrollTo: scroll(toWorld:))
        }
        .overlay(alignment: .top) {
            if workspace.mode == .simulation {
                Rectangle().fill(Theme.Palette.accent).frame(height: 2)
            }
        }
        .focusable()
        .focused($isFocused)
        .focusEffectDisabled()
        .onKeyPress(characters: .letters, phases: .down) { press in
            // Letters belong to a text field being edited (e.g. renaming), not to tool shortcuts.
            guard !isEditingText,
                  press.modifiers.isEmpty,
                  let tool = WorkspaceModel.Tool.allCases.first(where: { String($0.key) == press.characters.lowercased() })
            else { return .ignored }
            workspace.tool = tool
            return .handled
        }
        .onKeyPress(.escape) {
            if workspace.renamingDevice != nil {
                workspace.renamingDevice = nil
                return .handled
            }
            if workspace.tool != .select || pendingCableSource != nil {
                workspace.tool = .select
                pendingCableSource = nil
            } else {
                workspace.clearSelection()
            }
            return .handled
        }
        .onKeyPress(.return) {
            guard !isEditingText, workspace.selection.count == 1, let id = workspace.selection.first
            else { return .ignored }
            workspace.openConsole(for: id)
            return .handled
        }
        .onDeleteCommand(perform: deleteSelection)
        .onChange(of: workspace.tool) { _, _ in pendingCableSource = nil }
        .task(id: workspace.isPlaying) { await runSimulationClock() }
    }

    // MARK: World

    private var world: some View {
        let positions = displayedPositions
        return ZStack(alignment: .topLeading) {
            Group {
                if workspace.showsGrid {
                    GridLayer()
                } else {
                    Theme.Palette.canvas
                }
            }
            .contentShape(Rectangle())
            .onTapGesture(coordinateSpace: .named(Self.space)) { location in backgroundTapped(at: location) }
            .contextMenu { CanvasContextMenu(document: document, workspace: workspace, onZoomToFit: zoomToFit) }

            LinksLayer(document: document, workspace: workspace, positions: positions)
                .allowsHitTesting(false)

            ForEach(document.state.links) { link in
                if let a = positions[link.a.deviceId], let b = positions[link.b.deviceId] {
                    // Invisible hit area along the cable for clicks and right-clicks.
                    Color.clear
                        .contentShape(Path { p in p.move(to: a); p.addLine(to: b) }
                            .strokedPath(StrokeStyle(lineWidth: 10 / workspace.zoom, lineCap: .round)))
                        .onTapGesture {
                            isFocused = true
                            workspace.selectLink(link.id)
                        }
                        .contextMenu { LinkContextMenu(link: link, document: document, workspace: workspace) }
                        .accessibilityElement()
                        .accessibilityLabel(cableDescription(link))
                        .accessibilityAddTraits(workspace.selectedLink == link.id ? [.isButton, .isSelected] : .isButton)
                }
            }

            if let drag = cableDrag, let from = positions[drag.from] {
                CableDraftLine(from: from, to: drag.to)
            }

            ForEach(document.state.devices) { device in
                NodeView(device: device, workspace: workspace,
                         isSelected: workspace.selection.contains(device.id) || pendingCableSource == device.id,
                         onRename: { name in rename(device, to: name) })
                    // Before .position(), so the location is relative to the symbol's frame.
                    .simultaneousGesture(SpatialTapGesture(count: 2).onEnded { tap in
                        deviceDoubleTapped(device, at: tap.location)
                    })
                    .popover(isPresented: portPickerBinding(for: device.id), arrowEdge: .bottom) {
                        PortPicker(document: document, workspace: workspace, onConnect: confirmPendingConnection)
                    }
                    .position(positions[device.id] ?? device.position)
                    .gesture(dragGesture(for: device))
                    .simultaneousGesture(TapGesture().onEnded { deviceTapped(device) })
                    .contextMenu { DeviceContextMenu(device: device, document: document, workspace: workspace) }
            }

            // Packets travel on top of devices so they stay visible at each hop.
            if workspace.mode == .simulation {
                PacketsLayer(document: document, workspace: workspace, positions: positions)
                    .allowsHitTesting(false)
            }
        }
        .frame(width: worldSize.width, height: worldSize.height)
        .coordinateSpace(.named(Self.space))
        .scaleEffect(workspace.zoom, anchor: .topLeading)
    }

    /// Big enough for every device plus room to grow.
    private var worldSize: CGSize {
        let maxX = document.state.devices.map(\.x).max() ?? 0
        let maxY = document.state.devices.map(\.y).max() ?? 0
        return CGSize(width: max(2400, maxX + 800), height: max(1600, maxY + 600))
    }

    private var displayedPositions: [UInt64: CGPoint] {
        Dictionary(uniqueKeysWithValues: document.state.devices.map { device in
            guard draggedIDs.contains(device.id) else { return (device.id, device.position) }
            return (device.id, CGPoint(x: device.x + dragOffset.width, y: device.y + dragOffset.height))
        })
    }

    // MARK: Interaction

    /// True while a text field (its field editor) has keyboard focus.
    private var isEditingText: Bool {
        workspace.renamingDevice != nil || NSApp.keyWindow?.firstResponder is NSText
    }

    private func backgroundTapped(at location: CGPoint) {
        isFocused = true
        pendingCableSource = nil
        switch workspace.tool {
        case .note, .rectangle:
            workspace.flash("\(workspace.tool.title)s arrive with the canvas milestone (M1).")
        default:
            if let link = LinksLayer.hitTest(location, links: document.state.links, positions: displayedPositions,
                                              tolerance: 6 / workspace.zoom) {
                workspace.selectLink(link)
            } else {
                workspace.clearSelection()
            }
        }
    }

    private func deviceTapped(_ device: DeviceInfo) {
        isFocused = true
        if workspace.tool == .cable {
            if let source = pendingCableSource, source != device.id {
                connect(source, device.id)
                pendingCableSource = nil
            } else {
                pendingCableSource = device.id
                workspace.flash("Now click the device to connect \(device.name) to.")
            }
            return
        }
        if NSEvent.modifierFlags.contains(.shift) {
            var selection = workspace.selection
            selection.formSymmetricDifference([device.id])
            workspace.selectDevices(selection)
        } else {
            workspace.selectDevices([device.id])
        }
    }

    private func dragGesture(for device: DeviceInfo) -> some Gesture {
        DragGesture(minimumDistance: 2, coordinateSpace: .named(Self.space))
            .onChanged { value in
                if workspace.tool == .cable {
                    cableDrag = CableDrag(from: device.id, to: value.location)
                    return
                }
                guard workspace.tool == .select else { return }
                if draggedIDs.isEmpty {
                    // Dragging an unselected device moves just that device.
                    if !workspace.selection.contains(device.id) { workspace.selectDevices([device.id]) }
                    draggedIDs = workspace.selection
                }
                dragOffset = value.translation
            }
            .onEnded { value in
                if workspace.tool == .cable {
                    cableDrag = nil
                    if let target = deviceID(at: value.location), target != device.id {
                        connect(device.id, target)
                    }
                    return
                }
                let moved = draggedIDs
                let offset = dragOffset
                for id in moved {
                    guard let d = document.device(id) else { continue }
                    document.moveDevice(id, to: CGPoint(x: d.x + offset.width, y: d.y + offset.height).snapped,
                                        undoManager: undoManager)
                }
                draggedIDs = []
                dragOffset = .zero
            }
    }

    /// Automatic cables connect straight away on the first free ports;
    /// every other cable kind asks for the ports first.
    private func connect(_ a: UInt64, _ b: UInt64) {
        guard workspace.cableKind != .auto else {
            finishConnection(document.connect(a, b, medium: workspace.cableKind.medium, undoManager: undoManager))
            return
        }
        guard let devA = document.device(a), let devB = document.device(b) else { return }
        guard let portA = document.freePorts(of: a).first else {
            workspace.flash("\(devA.name) has no free ports.")
            return
        }
        guard let portB = document.freePorts(of: b).first else {
            workspace.flash("\(devB.name) has no free ports.")
            return
        }
        workspace.pendingConnection = .init(a: a, b: b, portA: portA, portB: portB, medium: workspace.cableKind.medium)
    }

    private func confirmPendingConnection() {
        guard let pending = workspace.pendingConnection else { return }
        workspace.pendingConnection = nil
        finishConnection(document.connect(pending.a, pending.b, portA: pending.portA, portB: pending.portB,
                                          medium: pending.medium, undoManager: undoManager))
    }

    private func finishConnection(_ result: Result<LinkInfo, CoreError>) {
        switch result {
        case .success(let link): workspace.selectLink(link.id)
        case .failure(let error): workspace.flash(error.message)
        }
    }

    private func portPickerBinding(for deviceId: UInt64) -> Binding<Bool> {
        Binding(
            get: { workspace.pendingConnection?.b == deviceId },
            set: { shown in if !shown { workspace.pendingConnection = nil } }
        )
    }

    /// Double-click the symbol to open its console; double-click the name to rename.
    private func deviceDoubleTapped(_ device: DeviceInfo, at location: CGPoint) {
        guard workspace.tool == .select else { return }
        if location.y > DeviceSymbol.size(for: device.kind).height {
            workspace.renamingDevice = device.id
        } else {
            workspace.openConsole(for: device.id)
        }
    }

    private func rename(_ device: DeviceInfo, to name: String) -> Bool {
        switch document.renameDevice(device.id, to: name, undoManager: undoManager) {
        case .success:
            return true
        case .failure(let error):
            workspace.flash(error.message)
            return false
        }
    }

    private func cableDescription(_ link: LinkInfo) -> String {
        let a = document.device(link.a.deviceId)?.name ?? "?"
        let b = document.device(link.b.deviceId)?.name ?? "?"
        let medium = link.medium == .fiber ? "Fibre cable" : "Cable"
        return "\(medium), \(a) \(link.a.port.abbreviatedInterface) to \(b) \(link.b.port.abbreviatedInterface)"
    }

    private func deviceID(at point: CGPoint) -> UInt64? {
        document.state.devices.last { device in
            let size = DeviceSymbol.size(for: device.kind)
            return abs(point.x - CGFloat(device.x)) <= size.width / 2 + 6
                && abs(point.y - CGFloat(device.y)) <= size.height / 2 + 6
        }?.id
    }

    private func deleteSelection() {
        if !workspace.selection.isEmpty {
            document.removeDevices(workspace.selection, undoManager: undoManager)
        } else if let link = workspace.selectedLink {
            document.disconnect(link, undoManager: undoManager)
        }
    }

    private var magnifyGesture: some Gesture {
        MagnifyGesture()
            .onChanged { value in
                if magnifyBase == nil { magnifyBase = workspace.zoom }
                workspace.setZoom((magnifyBase ?? 1) * value.magnification)
            }
            .onEnded { _ in magnifyBase = nil }
    }

    private var panGesture: some Gesture {
        DragGesture(minimumDistance: 1)
            .onChanged { value in
                if panStart == nil { panStart = visibleRect.origin }
                guard let start = panStart else { return }
                scrollPosition.scrollTo(point: CGPoint(x: max(0, start.x - value.translation.width),
                                                       y: max(0, start.y - value.translation.height)))
            }
            .onEnded { _ in panStart = nil }
    }

    // MARK: Viewport

    private func zoomToFit() {
        guard !document.state.devices.isEmpty, visibleRect.width > 0 else {
            workspace.setZoom(1)
            return
        }
        let margin: CGFloat = 80
        let xs = document.state.devices.map(\.x), ys = document.state.devices.map(\.y)
        let bounds = CGRect(x: xs.min()! - margin, y: ys.min()! - margin,
                            width: xs.max()! - xs.min()! + margin * 2, height: ys.max()! - ys.min()! + margin * 2)
        let zoom = min(visibleRect.width / bounds.width, visibleRect.height / bounds.height, 1)
        workspace.setZoom(zoom)
        // Scroll after the content has re-laid out at the new zoom.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
            scrollPosition.scrollTo(point: CGPoint(x: max(0, bounds.minX * workspace.zoom),
                                                   y: max(0, bounds.minY * workspace.zoom)))
        }
    }

    /// Centres the viewport on a world point (minimap clicks).
    private func scroll(toWorld point: CGPoint) {
        scrollPosition.scrollTo(point: CGPoint(x: max(0, point.x * workspace.zoom - visibleRect.width / 2),
                                               y: max(0, point.y * workspace.zoom - visibleRect.height / 2)))
    }

    /// Advances the preview simulation clock while playing.
    private func runSimulationClock() async {
        while workspace.isPlaying && !Task.isCancelled {
            try? await Task.sleep(for: .milliseconds(16))
            workspace.simulationTime += 0.016 * 0.002 * workspace.simulationSpeed
            if let last = PreviewData.events(for: document.state).last, workspace.simulationTime > last.time + 0.002 {
                workspace.isPlaying = false
            }
        }
    }
}

extension CGPoint {
    /// Snaps to half the grid spacing so devices line up without feeling locked.
    var snapped: CGPoint {
        let step = Theme.Size.gridSpacing / 2
        return CGPoint(x: (x / step).rounded() * step, y: (y / step).rounded() * step)
    }
}

// MARK: - Node

private struct NodeView: View {
    let device: DeviceInfo
    @ObservedObject var workspace: WorkspaceModel
    let isSelected: Bool
    /// Returns false if the name was rejected, keeping the field open.
    let onRename: (String) -> Bool

    @State private var draft = ""
    @FocusState private var fieldFocused: Bool

    var body: some View {
        let power = workspace.powerState(device.id)
        DeviceSymbolView(kind: device.kind, isSelected: isSelected, isDimmed: power == .stopped)
            .overlay(alignment: .topTrailing) {
                if power != .running {
                    StatusGlyph(state: power, size: 9)
                        .background(Circle().fill(Theme.Palette.canvas).padding(-1.5))
                        .offset(x: 3, y: -3)
                        .help(power.title)
                }
            }
            .overlay(alignment: .bottom) {
                Group {
                    if workspace.renamingDevice == device.id {
                        nameField
                    } else {
                        nameLabel
                    }
                }
                .alignmentGuide(.bottom) { $0[.top] - 6 }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("\(device.kind.displayName) \(device.name)")
            .accessibilityHint("Double-click to open its console")
            .accessibilityAddTraits(isSelected ? .isSelected : [])
    }

    private var nameLabel: some View {
        Text(device.name)
            // Semibold white on solid accent when selected (a deliberate
            // design choice over the tinted variant; see UX-ANALYSIS.md).
            .font(isSelected ? Theme.Font.labelSemibold : Theme.Font.label)
            .foregroundStyle(isSelected ? Theme.Palette.onAccent : Theme.Palette.textPrimary)
            .padding(.horizontal, 5)
            .padding(.vertical, 1)
            .background(Capsule().fill(isSelected ? Theme.Palette.accent : Theme.Palette.canvas.opacity(0.85)))
            .fixedSize()
            .help("Double-click to rename")
    }

    private var nameField: some View {
        TextField("Hostname", text: $draft)
            .textFieldStyle(.plain)
            .font(Theme.Font.label)
            .multilineTextAlignment(.center)
            .frame(width: 120)
            .padding(.vertical, 2)
            .background(RoundedRectangle(cornerRadius: 4).fill(Theme.Palette.surface))
            .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(Theme.Palette.accent, lineWidth: 1.5))
            .focused($fieldFocused)
            .onAppear {
                draft = device.name
                // Focusing in the same pass as insertion is ignored; wait one turn.
                DispatchQueue.main.async { fieldFocused = true }
            }
            .onSubmit { commit(staysOpenOnError: true) }
            .onExitCommand { workspace.renamingDevice = nil }
            .onChange(of: fieldFocused) { _, focused in
                // Clicking away commits; an invalid name is dropped rather than
                // pulling focus back.
                if !focused { commit(staysOpenOnError: false) }
            }
    }

    private func commit(staysOpenOnError: Bool) {
        guard workspace.renamingDevice == device.id else { return }
        if draft == device.name || onRename(draft) || !staysOpenOnError {
            workspace.renamingDevice = nil
        } else {
            fieldFocused = true
        }
    }
}

private struct CableDraftLine: View {
    let from: CGPoint
    let to: CGPoint

    var body: some View {
        Path { p in
            p.move(to: from)
            p.addLine(to: to)
        }
        .stroke(Theme.Palette.accent, style: StrokeStyle(lineWidth: 2, lineCap: .round, dash: [5, 4]))
        .allowsHitTesting(false)
    }
}
