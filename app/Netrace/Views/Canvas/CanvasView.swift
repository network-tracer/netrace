import SwiftUI
import UniformTypeIdentifiers

/// The topology canvas: a scrollable, zoomable world with devices, cables
/// and (in simulation mode) packets, plus floating zoom / minimap / timeline.
struct CanvasView: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController
    @Environment(\.undoManager) private var undoManager

    /// Devices and annotations following the pointer during a drag, and how
    /// far they've moved. Committed to the core on release as one undo step.
    @State private var moving = MoveSet()
    @State private var moveOffset: CGSize = .zero
    /// Alignment lines shown while a drag snaps to other items.
    @State private var snapGuides: [SnapGuide] = []
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
    /// A ⌥-drag that started inside a shape draws a marquee instead of moving it.
    @State private var isShapeMarquee = false
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
            workspace.select(devices: Set(document.state.devices.map(\.id)),
                             annotations: Set(document.state.annotations.map(\.id)))
        }
        .onCopyCommand(perform: copySelection)
        .onCutCommand {
            let items = copySelection()
            deleteSelection()
            return items
        }
        .onPasteCommand(of: [.plainText]) { _ in pasteFromPasteboard() }
        .onChange(of: workspace.duplicateRequests) { _, _ in duplicateSelection() }
        .onChange(of: workspace.arrangeRequest) { _, action in
            guard let action else { return }
            workspace.arrangeRequest = nil
            perform(action)
        }
        .onChange(of: workspace.tool) { _, _ in pendingCableSource = nil }
        .task(id: workspace.isPlaying) { await runSimulationClock() }
    }

    // MARK: World

    private var world: some View {
        let positions = displayedPositions
        let stpRoot = StpDisplay.make(simulation: simulation, workspace: workspace)?.root
        return ZStack(alignment: .topLeading) {
            // Areas sit behind cables and devices.
            ForEach(document.state.annotations.filter { $0.kind == .area }) { area in
                AreaView(annotation: area, rect: displayedRect(area),
                         isSelected: workspace.selectedAnnotations.contains(area.id),
                         isEditing: workspace.editingAnnotation == area.id, zoom: workspace.zoom,
                         interiorClickable: workspace.tool == .select && !spacePanning,
                         actions: actions(for: area))
                    .contextMenu { AnnotationContextMenu(annotation: area, document: document, workspace: workspace) }
            }

            LinksLayer(document: document, workspace: workspace, positions: positions,
                       layer2: workspace.showsLayer2 ? simulation.layer2 : nil,
                       stp: StpDisplay.make(simulation: simulation, workspace: workspace),
                       ospf: OspfDisplay.make(simulation: simulation, workspace: workspace),
                       services: workspace.showsLayer2 ? simulation.services : [])
                .allowsHitTesting(false)

            ForEach(document.state.links) { link in
                if let (a, b) = LinksLayer.endpoints(of: link, among: document.state.links, positions: positions) {
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
                NoteView(annotation: note, rect: displayedRect(note),
                         isSelected: workspace.selectedAnnotations.contains(note.id),
                         isEditing: workspace.editingAnnotation == note.id, actions: actions(for: note))
                    .contextMenu { AnnotationContextMenu(annotation: note, document: document, workspace: workspace) }
            }

            if !snapGuides.isEmpty {
                SnapGuidesView(guides: snapGuides, zoom: workspace.zoom)
            }

            if let drag = cableDrag, let from = positions[drag.from] {
                CableDraftLine(from: from, to: drag.to)
            }

            if let rect = areaDraft {
                let shape = AnnotationOutline(kind: workspace.shapeKind)
                shape.fill(Theme.Palette.accent.opacity(0.08))
                    .overlay(shape.stroke(Theme.Palette.accent, lineWidth: 1 / workspace.zoom))
                    .frame(width: rect.width, height: rect.height)
                    .position(x: rect.midX, y: rect.midY)
                    .allowsHitTesting(false)
            } else if let rect = marquee {
                let shape = RoundedRectangle(cornerRadius: 2)
                shape.fill(Theme.Palette.accent.opacity(0.08))
                    .overlay(shape.stroke(Theme.Palette.accent, lineWidth: 1 / workspace.zoom))
                    .frame(width: rect.width, height: rect.height)
                    .position(x: rect.midX, y: rect.midY)
                    .allowsHitTesting(false)
            }

            ForEach(document.state.devices) { device in
                NodeView(device: device, workspace: workspace,
                         rootVlans: workspace.showsLayer2
                            ? (simulation.layer2?.roots.filter { $0.deviceId == device.id }.map(\.vlan) ?? []) : [],
                         isStpRoot: stpRoot == device.id,
                         isDHCPServer: workspace.showsLayer2
                            && simulation.services.contains { $0.deviceId == device.id && $0.kind == "dhcp-server" },
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
            guard moving.devices.contains(device.id) else { return (device.id, device.position) }
            return (device.id, CGPoint(x: device.x + moveOffset.width, y: device.y + moveOffset.height))
        })
    }

    private func displayedRect(_ annotation: AnnotationInfo) -> CGRect {
        if let live = liveAnnotationRects[annotation.id] { return live }
        guard moving.annotations.contains(annotation.id) else { return annotation.rect }
        return annotation.rect.offsetBy(dx: moveOffset.width, dy: moveOffset.height)
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
                    selectInMarquee(rect)
                }
            }
    }

    /// Selects devices, shapes and notes fully inside `rect`; Shift adds.
    private func selectInMarquee(_ rect: CGRect) {
        let devices = Set(document.state.devices.filter { rect.contains($0.position) }.map(\.id))
        let annotations = Set(document.state.annotations.filter { rect.contains($0.rect) }.map(\.id))
        let picked = document.expandingGroups(devices: devices, annotations: annotations)
        if NSEvent.modifierFlags.contains(.shift) {
            workspace.select(devices: workspace.selection.union(picked.devices),
                             annotations: workspace.selectedAnnotations.union(picked.annotations))
        } else {
            workspace.select(devices: picked.devices, annotations: picked.annotations)
        }
        isFocused = true
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
        var frame = rect.width < 40 || rect.height < 40
            ? CGRect(origin: origin, size: CGSize(width: 264, height: 180))
            : CGRect(origin: origin, size: CGSize(width: rect.width.rounded(), height: rect.height.rounded()))
        var style = defaultShapeStyle()
        style.shape = workspace.shapeKind
        if workspace.shapeKind.isPolygon || workspace.shapeKind == .ellipse {
            // Round and pointed shapes look best roughly as wide as they are tall.
            if rect.width < 40 || rect.height < 40 {
                frame.size = CGSize(width: 220, height: 200)
            }
        }
        let area = document.addAnnotation(.area, in: frame, text: "", style: style, undoManager: undoManager)
        workspace.tool = .select
        workspace.selectAnnotation(area.id)
        workspace.editingAnnotation = area.id
    }

    private func actions(for annotation: AnnotationInfo) -> AnnotationActions {
        let id = annotation.id
        let name = annotation.kind == .note ? "Note" : annotation.style.shape.title
        return AnnotationActions(
            select: {
                isFocused = true
                annotationTapped(id)
            },
            startEditing: {
                workspace.selectAnnotation(id)
                workspace.editingAnnotation = id
            },
            move: { start, location, ended in
                guard workspace.tool == .select else { return }
                if isShapeMarquee || (moving.isEmpty && NSEvent.modifierFlags.contains(.option)) {
                    isShapeMarquee = !ended
                    marquee = ended ? nil : CGRect(corner: start, to: location)
                    if ended { selectInMarquee(CGRect(corner: start, to: location)) }
                    return
                }
                let translation = CGSize(width: location.x - start.x, height: location.y - start.y)
                if annotation.locked {
                    if ended { workspace.flash("\(name) is locked. Unlock it to move it.") }
                    return
                }
                dragMoved(translation, ended: ended, primary: .annotation(id: id))
            },
            resize: { translation, ended in
                let rect = CGRect(origin: annotation.rect.origin,
                                  size: CGSize(width: max(60, annotation.width + translation.width),
                                               height: max(36, annotation.height + translation.height)))
                if ended {
                    liveAnnotationRects[id] = nil
                    let snapped = CGRect(origin: rect.origin,
                                         size: CGSize(width: rect.maxX.snappedToGrid - rect.minX,
                                                      height: rect.maxY.snappedToGrid - rect.minY))
                    document.updateAnnotation(id, rect: snapped.width >= 60 && snapped.height >= 36 ? snapped : rect,
                                              text: annotation.text, actionName: "Resize \(name)", undoManager: undoManager)
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

    /// Click on a note or shape: selects it with the rest of its group;
    /// Shift adds or removes it from the selection.
    private func annotationTapped(_ id: UInt64) {
        let picked = document.expandingGroups(devices: [], annotations: [id])
        if NSEvent.modifierFlags.contains(.shift) {
            if workspace.selectedAnnotations.contains(id) {
                workspace.select(devices: workspace.selection.subtracting(picked.devices),
                                 annotations: workspace.selectedAnnotations.subtracting(picked.annotations))
            } else {
                workspace.select(devices: workspace.selection.union(picked.devices),
                                 annotations: workspace.selectedAnnotations.union(picked.annotations))
            }
        } else if !workspace.selectedAnnotations.contains(id) {
            workspace.select(devices: picked.devices, annotations: picked.annotations)
        }
    }

    // MARK: Moving

    /// Updates a drag of devices and/or annotations. The first change works
    /// out what moves: the selection (or just the dragged item if it isn't
    /// selected), whole groups, and everything inside the shapes being moved.
    private func dragMoved(_ translation: CGSize, ended: Bool, primary: CanvasItem) {
        if moving.isEmpty {
            moving = moveSet(for: primary)
        }
        let (offset, guides) = snapped(translation, primary: primary)
        if ended {
            let items = moving
            moving = MoveSet()
            moveOffset = .zero
            snapGuides = []
            let count = items.devices.count + items.annotations.count
            let name: String
            if count == 1, let id = items.devices.first, let device = document.device(id) {
                name = "Move \(device.name)"
            } else if count == 1, let id = items.annotations.first, let annotation = document.annotation(id) {
                name = "Move \(annotation.kind == .note ? "Note" : annotation.style.shape.title)"
            } else {
                name = "Move \(count) Items"
            }
            document.moveItems(devices: items.devices, annotations: items.annotations, by: offset,
                               actionName: name, undoManager: undoManager)
        } else {
            moveOffset = offset
            snapGuides = guides
        }
    }

    private func moveSet(for primary: CanvasItem) -> MoveSet {
        var devices = workspace.selection
        var annotations = workspace.selectedAnnotations
        switch primary {
        case .device(let id) where !devices.contains(id):
            let picked = document.expandingGroups(devices: [id], annotations: [])
            workspace.select(devices: picked.devices, annotations: picked.annotations)
            (devices, annotations) = picked
        case .annotation(let id) where !annotations.contains(id):
            let picked = document.expandingGroups(devices: [], annotations: [id])
            workspace.select(devices: picked.devices, annotations: picked.annotations)
            (devices, annotations) = picked
        default:
            (devices, annotations) = document.expandingGroups(devices: devices, annotations: annotations)
        }
        annotations = annotations.filter { document.annotation($0)?.locked == false }
        let contents = document.contents(ofAreas: annotations)
        return MoveSet(devices: devices.union(contents.devices), annotations: annotations.union(contents.annotations))
    }

    /// Snaps a drag so the moved items line up with other devices and
    /// shapes (edges and centres) when close; otherwise to the grid on
    /// release. Returns the offset to use and the guides to draw.
    private func snapped(_ translation: CGSize, primary: CanvasItem) -> (CGSize, [SnapGuide]) {
        let movingRects = document.state.annotations.filter { moving.annotations.contains($0.id) }.map(\.rect)
        let primaryPoint: CGPoint? = switch primary {
        case .device(let id): document.device(id)?.position
        case .annotation(let id): document.annotation(id)?.rect.origin
        }
        guard let primaryPoint else { return (translation, []) }

        // Anchors on the moved items: the dragged device's centre, or the
        // moved shapes' combined edges and centre.
        let bounds = movingRects.reduce(CGRect.null) { $0.union($1) }
        let anchorsX: [CGFloat], anchorsY: [CGFloat]
        if case .device = primary {
            anchorsX = [primaryPoint.x]
            anchorsY = [primaryPoint.y]
        } else {
            anchorsX = [bounds.minX, bounds.midX, bounds.maxX]
            anchorsY = [bounds.minY, bounds.midY, bounds.maxY]
        }
        var targetsX: [CGFloat] = [], targetsY: [CGFloat] = []
        for device in document.state.devices where !moving.devices.contains(device.id) {
            targetsX.append(device.x)
            targetsY.append(device.y)
        }
        for annotation in document.state.annotations where !moving.annotations.contains(annotation.id) {
            let r = annotation.rect
            targetsX += [r.minX, r.midX, r.maxX]
            targetsY += [r.minY, r.midY, r.maxY]
        }

        let threshold = 6 / workspace.zoom
        func best(_ anchors: [CGFloat], _ targets: [CGFloat], _ delta: CGFloat) -> (shift: CGFloat, line: CGFloat)? {
            var result: (CGFloat, CGFloat)?
            for anchor in anchors {
                for target in targets {
                    let shift = target - (anchor + delta)
                    if abs(shift) <= threshold, abs(shift) < abs(result?.0 ?? .infinity) { result = (shift, target) }
                }
            }
            return result
        }

        var offset = translation
        var guides: [SnapGuide] = []
        // Snapping to other items is opt-in (View → Snap to Items); holding ⌘
        // flips it for one drag. The grid always applies.
        let toItems = workspace.snapsToItems != NSEvent.modifierFlags.contains(.command)
        if !toItems {
            targetsX = []
            targetsY = []
        }
        if let x = best(anchorsX, targetsX, translation.width) {
            offset.width += x.shift
            guides.append(SnapGuide(vertical: true, position: x.line))
        } else {
            // No alignment: keep the dragged item on the grid.
            offset.width = (primaryPoint.x + translation.width).snappedToGrid - primaryPoint.x
        }
        if let y = best(anchorsY, targetsY, translation.height) {
            offset.height += y.shift
            guides.append(SnapGuide(vertical: false, position: y.line))
        } else {
            offset.height = (primaryPoint.y + translation.height).snappedToGrid - primaryPoint.y
        }
        return (offset, guides)
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
        guard workspace.hasCanvasSelection else { return .ignored }
        var items = moveSet(for: .device(id: workspace.selection.first ?? 0))
        if workspace.selection.isEmpty, let first = workspace.selectedAnnotations.first {
            items = moveSet(for: .annotation(id: first))
        }
        document.moveItems(devices: items.devices, annotations: items.annotations, by: delta,
                           actionName: "Nudge", undoManager: undoManager)
        return .handled
    }

    private func copySelection() -> [NSItemProvider] {
        guard let clipboard = TopologyClipboard(selection: workspace.selection, in: document) else { return [] }
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
        guard let clipboard = TopologyClipboard(selection: workspace.selection, in: document) else { return }
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
        let picked = document.expandingGroups(devices: [device.id], annotations: [])
        if NSEvent.modifierFlags.contains(.shift) {
            if workspace.selection.contains(device.id) {
                workspace.select(devices: workspace.selection.subtracting(picked.devices),
                                 annotations: workspace.selectedAnnotations.subtracting(picked.annotations))
            } else {
                workspace.select(devices: workspace.selection.union(picked.devices),
                                 annotations: workspace.selectedAnnotations.union(picked.annotations))
            }
        } else {
            workspace.select(devices: picked.devices, annotations: picked.annotations)
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
                dragMoved(value.translation, ended: false, primary: .device(id: device.id))
            }
            .onEnded { value in
                if workspace.tool == .cable {
                    cableDrag = nil
                    if let target = deviceID(at: value.location), target != device.id {
                        connect(device.id, target)
                    }
                    return
                }
                guard workspace.tool == .select, !moving.isEmpty else { return }
                dragMoved(value.translation, ended: true, primary: .device(id: device.id))
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
        if workspace.hasCanvasSelection {
            let count = workspace.selection.count + workspace.selectedAnnotations.count
            undoManager?.beginUndoGrouping()
            if !workspace.selection.isEmpty {
                document.removeDevices(workspace.selection, undoManager: undoManager)
            }
            // Front to back, so each restore index stays valid on undo.
            for annotation in document.state.annotations.reversed() where workspace.selectedAnnotations.contains(annotation.id) {
                document.removeAnnotation(annotation.id, undoManager: undoManager)
            }
            if count > 1 { undoManager?.setActionName("Delete \(count) Items") }
            undoManager?.endUndoGrouping()
        } else if let link = workspace.selectedLink {
            document.disconnect(link, undoManager: undoManager)
        }
    }

    // MARK: Arrange

    private func perform(_ action: WorkspaceModel.ArrangeAction) {
        let devices = workspace.selection
        let annotations = workspace.selectedAnnotations
        switch action {
        case .group:
            guard devices.count + annotations.count > 1 else {
                workspace.flash("Select two or more items to group.")
                return
            }
            document.group(devices: devices, annotations: annotations, undoManager: undoManager)
        case .ungroup:
            document.ungroup(devices: devices, annotations: annotations, undoManager: undoManager)
        case .lock, .unlock:
            document.setAnnotationsLocked(annotations, action == .lock, undoManager: undoManager)
        case .stack(let move):
            // Bring forward / send backward keeps the relative order of a multi-selection.
            let ordered = document.state.annotations.filter { annotations.contains($0.id) }.map(\.id)
            let sequence = move == .front || move == .forward ? ordered : ordered.reversed()
            undoManager?.beginUndoGrouping()
            for id in sequence { document.reorderAnnotation(id, move, undoManager: undoManager) }
            undoManager?.endUndoGrouping()
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

/// Devices and annotations moving together in one drag.
struct MoveSet {
    var devices: Set<UInt64> = []
    var annotations: Set<UInt64> = []
    var isEmpty: Bool { devices.isEmpty && annotations.isEmpty }
}

/// An alignment line in world coordinates.
struct SnapGuide: Hashable {
    let vertical: Bool
    let position: CGFloat
}

private struct SnapGuidesView: View {
    let guides: [SnapGuide]
    let zoom: CGFloat

    var body: some View {
        Canvas { context, size in
            for guide in guides {
                var path = Path()
                if guide.vertical {
                    path.move(to: CGPoint(x: guide.position, y: -100_000))
                    path.addLine(to: CGPoint(x: guide.position, y: 100_000))
                } else {
                    path.move(to: CGPoint(x: -100_000, y: guide.position))
                    path.addLine(to: CGPoint(x: 100_000, y: guide.position))
                }
                context.stroke(path, with: .color(Theme.Palette.accent), lineWidth: 1 / zoom)
            }
        }
        .allowsHitTesting(false)
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
        CGPoint(x: x.snappedToGrid, y: y.snappedToGrid)
    }
}

extension CGFloat {
    /// Nearest half grid step.
    var snappedToGrid: CGFloat {
        let step = Theme.Size.gridSpacing / 2
        return (self / step).rounded() * step
    }
}

// MARK: - Node

private struct NodeView: View {
    let device: DeviceInfo
    @ObservedObject var workspace: WorkspaceModel
    /// VLANs this switch is the spanning-tree root for.
    var rootVlans: [UInt16] = []
    /// The root bridge in the STP view: a gold band and halo.
    var isStpRoot = false
    var isDHCPServer = false
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
        DeviceSymbolView(kind: device.kind, isSelected: isSelected, isRoot: isStpRoot,
                         isDimmed: power == .stopped, tint: device.color)
            .overlay(alignment: .bottomTrailing) {
                if isDHCPServer {
                    Text("DHCP")
                        .font(.system(size: 7, weight: .bold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 3)
                        .padding(.vertical, 1)
                        .background(Capsule().fill(Color(light: 0x2F9E5B, dark: 0x3DB46D)))
                        .offset(x: 10, y: 4)
                        .help("This router hands out addresses with DHCP")
                }
            }
            .overlay(alignment: .topLeading) {
                // The STP view draws the halo instead; the badge would repeat it.
                if !rootVlans.isEmpty && !isStpRoot {
                    RootBadge()
                        .offset(x: -8, y: -10)
                        .help("Spanning-tree root bridge for VLAN \(rootVlans.map(String.init).joined(separator: ", "))")
                }
            }
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
