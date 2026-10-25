import SwiftUI
import UniformTypeIdentifiers

/// The topology canvas: a scrollable, zoomable world with devices, cables
/// and (in simulation mode) packets, plus floating zoom / minimap / timeline.
struct CanvasView: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController
    @Environment(\.undoManager) private var undoManager

    /// Live offset of devices being dragged; committed to the core on release
    /// so each drag is one undo step.
    @State private var dragOffset: CGSize = .zero
    @State private var draggedIDs: Set<UInt64> = []
    @State private var cableDrag: CableDrag?
    @State private var pendingCableSource: UInt64?
    /// Trackpad pinch / scroll handling (kept out of view state so hovering
    /// never re-renders the canvas).
    @State private var input = CanvasInput()
    @State private var panStart: CGPoint?
    @State private var hasFittedOnOpen = false
    /// Live frames of annotations being moved or resized (committed on release).
    @State private var liveAnnotationRects: [UInt64: CGRect] = [:]
    @State private var marquee: CGRect?
    @State private var areaDraft: CGRect?
    /// Space held: drag pans the canvas with any tool (Figma convention).
    @State private var spacePanning = false
    @FocusState private var isFocused: Bool

    struct CableDrag {
        let from: UInt64
        var to: CGPoint
    }

    private static let space = "world"

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .topLeading) {
                GridLayer(origin: workspace.viewOrigin, zoom: workspace.zoom, showsGrid: workspace.showsGrid)
                    .contentShape(Rectangle())
                    .onTapGesture { location in backgroundTapped(at: toWorld(location)) }
                    .gesture(backgroundDrag, including: backgroundDragEnabled ? .all : .subviews)
                    .contextMenu { CanvasContextMenu(document: document, workspace: workspace, onZoomToFit: zoomToFit) }

                world
                    .offset(x: -workspace.viewOrigin.x * workspace.zoom, y: -workspace.viewOrigin.y * workspace.zoom)

                CanvasOverlays(document: document, workspace: workspace, visibleRect: visibleRect,
                               onZoomToFit: zoomToFit, onScrollTo: { workspace.center(on: $0) })
                    .frame(width: geometry.size.width, height: geometry.size.height)
            }
            .frame(width: geometry.size.width, height: geometry.size.height, alignment: .topLeading)
            .clipped()
            .contentShape(Rectangle())
            .simultaneousGesture(panGesture, including: workspace.tool == .hand || spacePanning ? .all : .subviews)
            .dropDestination(for: String.self) { items, location in
                guard let kind = items.first.flatMap(DeviceKind.init(dragIdentifier:)) else { return false }
                let device = document.addDevice(kind, at: toWorld(location).snapped, undoManager: undoManager)
                workspace.selectDevices([device.id])
                isFocused = true
                return true
            }
            .onContinuousHover { phase in
                switch phase {
                case .active(let point): input.hover = point
                case .ended: input.hover = nil
                }
            }
            .onAppear {
                input.install(workspace)
                viewportResized(to: geometry.size)
            }
            .onChange(of: geometry.size) { _, size in viewportResized(to: size) }
            .onDisappear { input.remove() }
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
            if workspace.renamingDevice != nil || workspace.editingAnnotation != nil {
                workspace.renamingDevice = nil
                workspace.editingAnnotation = nil
                return .handled
            }
            if workspace.isPickingPDU {
                workspace.cancelPickingPDU()
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
        .onKeyPress(keys: [.upArrow, .downArrow, .leftArrow, .rightArrow], phases: [.down, .repeat], action: nudge)
        .onKeyPress(.space, phases: [.down, .repeat, .up]) { press in
            guard !isEditingText else { return .ignored }
            spacePanning = press.phase != .up
            return .handled
        }
        .onDeleteCommand(perform: deleteSelection)
        .onCommand(#selector(NSText.selectAll(_:))) {
            workspace.selectDevices(Set(document.state.devices.map(\.id)))
        }
        .onCopyCommand(perform: copySelection)
        .onCutCommand {
            let items = copySelection()
            deleteSelection()
            return items
        }
        .onPasteCommand(of: [.plainText]) { _ in pasteFromPasteboard() }
        .onChange(of: workspace.duplicateRequests) { _, _ in duplicateSelection() }
        .onChange(of: workspace.tool) { _, _ in pendingCableSource = nil }
        .task(id: workspace.isPlaying) { await runSimulationClock() }
    }

    // MARK: World

    private var world: some View {
        let positions = displayedPositions
        return ZStack(alignment: .topLeading) {
            // Areas sit behind cables and devices.
            ForEach(document.state.annotations.filter { $0.kind == .area }) { area in
                AreaView(annotation: area, rect: liveAnnotationRects[area.id] ?? area.rect,
                         isSelected: workspace.selectedAnnotation == area.id,
                         isEditing: workspace.editingAnnotation == area.id, actions: actions(for: area))
            }

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

            ForEach(document.state.annotations.filter { $0.kind == .note }) { note in
                NoteView(annotation: note, rect: liveAnnotationRects[note.id] ?? note.rect,
                         isSelected: workspace.selectedAnnotation == note.id,
                         isEditing: workspace.editingAnnotation == note.id, actions: actions(for: note))
            }

            if let drag = cableDrag, let from = positions[drag.from] {
                CableDraftLine(from: from, to: drag.to)
            }

            if let rect = areaDraft ?? marquee {
                let shape = RoundedRectangle(cornerRadius: areaDraft != nil ? Theme.Radius.l : 2)
                shape.fill(Theme.Palette.accent.opacity(0.08))
                    .overlay(shape.stroke(Theme.Palette.accent, lineWidth: 1 / workspace.zoom))
                    .frame(width: rect.width, height: rect.height)
                    .position(x: rect.midX, y: rect.midY)
                    .allowsHitTesting(false)
            }

            ForEach(document.state.devices) { device in
                NodeView(device: device, workspace: workspace,
                         isSelected: workspace.selection.contains(device.id) || pendingCableSource == device.id
                            || workspace.pduSource == device.id,
                         zoom: workspace.zoom,
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
                PacketsLayer(workspace: workspace, positions: positions)
                    .allowsHitTesting(false)
            }
        }
        .frame(width: worldSize.width, height: worldSize.height, alignment: .topLeading)
        .coordinateSpace(.named(Self.space))
        .scaleEffect(workspace.zoom, anchor: .topLeading)
    }

    /// Big enough for every device and annotation, plus room to grow.
    private var worldSize: CGSize {
        let annotations = document.state.annotations.map(\.rect)
        let maxX = (document.state.devices.map { CGFloat($0.x) } + annotations.map(\.maxX)).max() ?? 0
        let maxY = (document.state.devices.map { CGFloat($0.y) } + annotations.map(\.maxY)).max() ?? 0
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
        workspace.renamingDevice != nil || workspace.editingAnnotation != nil
            || NSApp.keyWindow?.firstResponder is NSText
    }

    private var backgroundDragEnabled: Bool {
        (workspace.tool == .select || workspace.tool == .rectangle) && !spacePanning
    }

    /// Select tool: rubber-band selection. Area tool: draw the area.
    private var backgroundDrag: some Gesture {
        DragGesture(minimumDistance: 3)
            .onChanged { value in
                let rect = CGRect(corner: toWorld(value.startLocation), to: toWorld(value.location))
                if workspace.tool == .rectangle { areaDraft = rect } else { marquee = rect }
            }
            .onEnded { _ in
                if let rect = areaDraft {
                    areaDraft = nil
                    createArea(rect)
                } else if let rect = marquee {
                    marquee = nil
                    let inside = Set(document.state.devices.filter { rect.contains($0.position) }.map(\.id))
                    workspace.selectDevices(NSEvent.modifierFlags.contains(.shift) ? workspace.selection.union(inside) : inside)
                    isFocused = true
                }
            }
    }

    private func createNote(at point: CGPoint) {
        let origin = point.snapped
        let note = document.addAnnotation(.note, in: CGRect(x: origin.x, y: origin.y, width: 180, height: 72),
                                          text: "", undoManager: undoManager)
        workspace.tool = .select
        workspace.selectAnnotation(note.id)
        workspace.editingAnnotation = note.id
    }

    /// Tiny drags (or a click) make a default-sized area.
    private func createArea(_ rect: CGRect) {
        let origin = rect.origin.snapped
        let frame = rect.width < 40 || rect.height < 40
            ? CGRect(origin: origin, size: CGSize(width: 264, height: 180))
            : CGRect(origin: origin, size: CGSize(width: rect.width.rounded(), height: rect.height.rounded()))
        let area = document.addAnnotation(.area, in: frame, text: "", undoManager: undoManager)
        workspace.tool = .select
        workspace.selectAnnotation(area.id)
        workspace.editingAnnotation = area.id
    }

    private func actions(for annotation: AnnotationInfo) -> AnnotationActions {
        let id = annotation.id
        let name = annotation.kind == .note ? "Note" : "Area"
        return AnnotationActions(
            select: {
                isFocused = true
                workspace.selectAnnotation(id)
            },
            startEditing: {
                workspace.selectAnnotation(id)
                workspace.editingAnnotation = id
            },
            move: { translation, ended in
                guard workspace.tool == .select else { return }
                let rect = annotation.rect.offsetBy(dx: translation.width, dy: translation.height)
                if ended {
                    liveAnnotationRects[id] = nil
                    let origin = rect.origin.snapped
                    document.updateAnnotation(id, rect: CGRect(origin: origin, size: rect.size), text: annotation.text,
                                              actionName: "Move \(name)", undoManager: undoManager)
                } else {
                    if workspace.selectedAnnotation != id { workspace.selectAnnotation(id) }
                    liveAnnotationRects[id] = rect
                }
            },
            resize: { translation, ended in
                let rect = CGRect(origin: annotation.rect.origin,
                                  size: CGSize(width: max(60, annotation.width + translation.width),
                                               height: max(36, annotation.height + translation.height)))
                if ended {
                    liveAnnotationRects[id] = nil
                    document.updateAnnotation(id, rect: rect, text: annotation.text,
                                              actionName: "Resize \(name)", undoManager: undoManager)
                } else {
                    liveAnnotationRects[id] = rect
                }
            },
            commitText: { text in
                workspace.editingAnnotation = nil
                document.updateAnnotation(id, rect: annotation.rect, text: text,
                                          actionName: "Edit \(name)", undoManager: undoManager)
            },
            cancelEditing: { workspace.editingAnnotation = nil }
        )
    }

    private func nudge(_ press: KeyPress) -> KeyPress.Result {
        guard !isEditingText else { return .ignored }
        let step: CGFloat = press.modifiers.contains(.shift) ? Theme.Size.gridSpacing * 2 : Theme.Size.gridSpacing / 2
        var delta = CGSize.zero
        switch press.key {
        case .upArrow: delta.height = -step
        case .downArrow: delta.height = step
        case .leftArrow: delta.width = -step
        case .rightArrow: delta.width = step
        default: return .ignored
        }
        if !workspace.selection.isEmpty {
            for id in workspace.selection {
                guard let device = document.device(id) else { continue }
                document.moveDevice(id, to: CGPoint(x: device.x + delta.width, y: device.y + delta.height),
                                    undoManager: undoManager)
            }
            undoManager?.setActionName("Nudge")
            return .handled
        }
        if let id = workspace.selectedAnnotation, let annotation = document.annotation(id) {
            document.updateAnnotation(id, rect: annotation.rect.offsetBy(dx: delta.width, dy: delta.height),
                                      text: annotation.text, actionName: "Nudge", undoManager: undoManager)
            return .handled
        }
        return .ignored
    }

    private func copySelection() -> [NSItemProvider] {
        guard let clipboard = TopologyClipboard(selection: workspace.selection, in: document.state) else { return [] }
        return [NSItemProvider(object: clipboard.json as NSString)]
    }

    private func pasteFromPasteboard() {
        guard let text = NSPasteboard.general.string(forType: .string),
              let clipboard = TopologyClipboard(json: text) else {
            workspace.flash("Nothing to paste. Copy some devices first.")
            return
        }
        let center = Placement.freeSpot(near: workspace.viewportCenter, avoiding: document.state.devices.map(\.position))
        let ids = document.paste(clipboard, centeredAt: center, actionName: "Paste", undoManager: undoManager)
        workspace.selectDevices(Set(ids))
    }

    /// Copies the selection next to itself, keeping the cables between the copies.
    private func duplicateSelection() {
        let selected = document.state.devices.filter { workspace.selection.contains($0.id) }
        guard let clipboard = TopologyClipboard(selection: workspace.selection, in: document.state) else { return }
        let center = CGPoint(x: selected.map(\.x).reduce(0, +) / Double(selected.count) + 48,
                             y: selected.map(\.y).reduce(0, +) / Double(selected.count) + 48)
        let ids = document.paste(clipboard, centeredAt: center, actionName: "Duplicate", undoManager: undoManager)
        workspace.selectDevices(Set(ids))
    }

    private func backgroundTapped(at location: CGPoint) {
        isFocused = true
        pendingCableSource = nil
        switch workspace.tool {
        case .note:
            createNote(at: location)
        case .rectangle:
            createArea(CGRect(origin: location, size: .zero))
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
        if workspace.isPickingPDU {
            pduPicked(device)
            return
        }
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

    /// Add Simple PDU: the first click picks the source, the second fires a
    /// ping at the destination's address.
    private func pduPicked(_ device: DeviceInfo) {
        guard let source = workspace.pduSource else {
            guard device.kind.isHost else {
                workspace.flash("\(device.name) can't send pings yet. Pick a PC, laptop, server, printer or phone.")
                return
            }
            guard device.ip != nil else {
                workspace.flash("\(device.name) has no IP address. Set one in its IP configuration first.")
                return
            }
            workspace.pduSource = device.id
            workspace.flash("Now click the device to ping.")
            return
        }
        guard device.id != source else { return }
        guard let address = device.ip?.address else {
            workspace.flash("\(device.name) has no IP address, so there's nothing to ping.")
            return
        }
        let simulated = workspace.mode == .simulation
        workspace.cancelPickingPDU()
        do {
            let at = simulated ? workspace.simulationTime : simulation.endTime
            guard let session = try simulation.ping(from: source, to: address, at: at) else { return }
            let from = document.device(source)?.name ?? "?"
            if simulated {
                workspace.bottomTab = .events
                workspace.showsBottomPanel = true
                workspace.flash("Ping from \(from) to \(device.name) is ready. Press play or step forward to watch it.")
            } else {
                // Third line of the report is the first reply or failure.
                let result = session.report.split(separator: "\n").dropFirst(1).first.map(String.init) ?? ""
                workspace.flash("\(from) → \(device.name): \(result)")
            }
        } catch let error as CoreError {
            workspace.flash(error.message)
        } catch {
            workspace.flash(error.localizedDescription)
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
        } else if let annotation = workspace.selectedAnnotation {
            document.removeAnnotation(annotation, undoManager: undoManager)
        }
    }

    /// Drag with the Hand tool (or Space held) to pan.
    private var panGesture: some Gesture {
        DragGesture(minimumDistance: 1)
            .onChanged { value in
                if panStart == nil { panStart = workspace.viewOrigin }
                guard let start = panStart else { return }
                workspace.viewOrigin = CGPoint(x: start.x - value.translation.width / workspace.zoom,
                                               y: start.y - value.translation.height / workspace.zoom)
            }
            .onEnded { _ in panStart = nil }
    }

    // MARK: Viewport

    /// Canvas (screen) point → world point.
    private func toWorld(_ point: CGPoint) -> CGPoint {
        CGPoint(x: workspace.viewOrigin.x + point.x / workspace.zoom,
                y: workspace.viewOrigin.y + point.y / workspace.zoom)
    }

    /// The visible area in zoomed content coordinates, as the overlays expect.
    private var visibleRect: CGRect {
        CGRect(x: workspace.viewOrigin.x * workspace.zoom, y: workspace.viewOrigin.y * workspace.zoom,
               width: workspace.viewportSize.width, height: workspace.viewportSize.height)
    }

    private func viewportResized(to size: CGSize) {
        guard size.width > 0, size.height > 0 else { return }
        workspace.viewportSize = size
        // Frame the topology once the canvas has a size.
        if !hasFittedOnOpen {
            hasFittedOnOpen = true
            if !document.state.devices.isEmpty { zoomToFit() }
        }
    }

    private func zoomToFit() {
        guard !document.state.devices.isEmpty else {
            workspace.setZoom(1)
            return
        }
        let margin: CGFloat = 80
        let notes = document.state.annotations.map(\.rect)
        let xs = document.state.devices.map { CGFloat($0.x) } + notes.flatMap { [$0.minX, $0.maxX] }
        let ys = document.state.devices.map { CGFloat($0.y) } + notes.flatMap { [$0.minY, $0.maxY] }
        let bounds = CGRect(x: xs.min()! - margin, y: ys.min()! - margin,
                            width: xs.max()! - xs.min()! + margin * 2, height: ys.max()! - ys.min()! + margin * 2)
        // Leave room for the floating minimap / timeline along the bottom edge.
        let bottomInset: CGFloat = workspace.showsMinimap || workspace.mode == .simulation ? 130 : 0
        workspace.fit(bounds, bottomInset: bottomInset)
    }

    /// Advances the preview simulation clock while playing.
    private func runSimulationClock() async {
        while workspace.isPlaying && !Task.isCancelled {
            try? await Task.sleep(for: .milliseconds(16))
            workspace.simulationTime += 0.016 * 0.002 * workspace.simulationSpeed
            if workspace.simulationTime > simulation.endTime + simulation.hopLatency {
                workspace.isPlaying = false
            }
        }
    }
}

/// Reads trackpad pinch and two-finger scroll straight from AppKit while the
/// pointer is over the canvas. Pinch zooms around the pointer; scrolling pans
/// (with momentum); ⌘-scroll zooms. Each event updates zoom and origin in one
/// step, so the view never jumps.
@MainActor
final class CanvasInput {
    /// Pointer position over the canvas, or nil when it's elsewhere.
    var hover: CGPoint?
    private var monitor: Any?

    func install(_ workspace: WorkspaceModel) {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.scrollWheel, .magnify]) { [weak self, weak workspace] event in
            // Copy what's needed out of the event before hopping onto the main actor.
            let input = Event(event)
            let handled = MainActor.assumeIsolated { () -> Bool in
                guard let self, let workspace else { return false }
                return self.handle(input, in: workspace)
            }
            return handled ? nil : event
        }
    }

    private struct Event: Sendable {
        let isMagnify: Bool
        let magnification: CGFloat
        let deltaX: CGFloat
        let deltaY: CGFloat
        let isPrecise: Bool
        let command: Bool

        init(_ event: NSEvent) {
            isMagnify = event.type == .magnify
            magnification = isMagnify ? event.magnification : 0
            deltaX = isMagnify ? 0 : event.scrollingDeltaX
            deltaY = isMagnify ? 0 : event.scrollingDeltaY
            isPrecise = isMagnify ? true : event.hasPreciseScrollingDeltas
            command = event.modifierFlags.contains(.command)
        }
    }

    private func handle(_ event: Event, in workspace: WorkspaceModel) -> Bool {
        guard let point = hover, !workspace.showsCommandPalette else { return false }
        if event.isMagnify {
            workspace.setZoom(workspace.zoom * (1 + event.magnification), around: point)
        } else if event.command {
            workspace.setZoom(workspace.zoom * exp(event.deltaY * 0.01), around: point)
        } else {
            // Mouse wheels report lines, trackpads report points.
            let scale: CGFloat = event.isPrecise ? 1 : 10
            workspace.pan(by: CGSize(width: event.deltaX * scale, height: event.deltaY * scale))
        }
        return true
    }

    func remove() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
    }
}

extension CGRect {
    /// The rectangle spanning two corner points, in any order.
    init(corner a: CGPoint, to b: CGPoint) {
        self.init(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(b.x - a.x), height: abs(b.y - a.y))
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
    let zoom: CGFloat
    /// Returns false if the name was rejected, keeping the field open.
    let onRename: (String) -> Bool

    /// Semantic zoom: names grow as the canvas shrinks so they stay readable
    /// (about 10 pt on screen at minimum).
    private var labelScale: CGFloat { min(2.2, max(1, 0.85 / zoom)) }

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
                        nameLabel.scaleEffect(labelScale, anchor: .top)
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
