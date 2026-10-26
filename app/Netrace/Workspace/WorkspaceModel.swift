import SwiftUI

/// Interface state for one document window: tools, panels, selection,
/// zoom, simulation controls. Nothing here is saved in the document.
@MainActor
final class WorkspaceModel: ObservableObject {
    enum Tool: String, CaseIterable, Identifiable {
        case select, hand, cable, note, rectangle

        var id: Self { self }

        var title: String {
            switch self {
            case .select: "Select"
            case .hand: "Hand"
            case .cable: "Connect"
            case .note: "Note"
            case .rectangle: "Shape"
            }
        }

        var glyph: Glyph {
            switch self {
            case .select: .system("cursorarrow")
            case .hand: .system("hand.raised")
            case .cable: .custom(.cableCopper)
            case .note: .system("note.text")
            case .rectangle: .system("square.on.circle")
            }
        }

        /// Single-letter shortcut while the canvas has focus.
        var key: Character {
            switch self {
            case .select: "v"
            case .hand: "h"
            case .cable: "c"
            case .note: "n"
            case .rectangle: "r"
            }
        }
    }

    enum Mode: String, CaseIterable, Identifiable {
        case realtime, simulation
        var id: Self { self }
        var title: String { self == .realtime ? "Realtime" : "Simulation" }
        var symbol: String { self == .realtime ? "clock" : "stopwatch" }
    }

    enum CableKind: String, CaseIterable, Identifiable {
        case auto, copper, fiber, console
        var id: Self { self }

        var title: String {
            switch self {
            case .auto: "Automatic"
            case .copper: "Copper"
            case .fiber: "Fibre"
            case .console: "Console"
            }
        }

        var summary: String {
            switch self {
            case .auto: "Picks the right cable"
            case .copper: "Twisted pair, auto-MDIX"
            case .fiber: "Fibre optic"
            case .console: "Rollover to a console port"
            }
        }

        var icon: Icon {
            switch self {
            case .auto: .cableAuto
            case .copper: .cableCopper
            case .fiber: .cableFiber
            case .console: .cableConsole
            }
        }

        var medium: Medium { self == .fiber ? .fiber : .copper }
    }

    enum SidebarTab: String, CaseIterable, Identifiable {
        case library, topology
        var id: Self { self }
        var title: String { self == .library ? "Library" : "Topology" }
    }

    enum BottomTab: String, CaseIterable, Identifiable {
        case console, events, capture
        var id: Self { self }

        var title: String {
            switch self {
            case .console: "Console"
            case .events: "Events"
            case .capture: "Capture"
            }
        }

        var glyph: Glyph {
            switch self {
            case .console: .system("terminal")
            case .events: .system("list.bullet.rectangle")
            case .capture: .custom(.capture)
            }
        }
    }

    /// What the right-hand pane shows. The lab lives here so its
    /// instructions stay visible next to the console in the bottom panel.
    enum RightPane: String, CaseIterable, Identifiable {
        /// The STP pane also turns on the spanning-tree view of the canvas.
        case inspector, lab, spanningTree, ospf
        var id: Self { self }

        var title: String {
            switch self {
            case .inspector: "Inspector"
            case .lab: "Lab"
            case .spanningTree: "STP"
            case .ospf: "OSPF"
            }
        }

        var glyph: Glyph {
            switch self {
            case .inspector: .system("slider.horizontal.3")
            case .lab: .custom(.lab)
            case .spanningTree: .system("point.3.connected.trianglepath.dotted")
            case .ospf: .system("arrow.triangle.branch")
            }
        }
    }

    /// A cable waiting for the user to confirm its ports.
    struct PendingConnection: Equatable {
        let a: UInt64
        let b: UInt64
        var portA: String
        var portB: String
        var medium: Medium
    }

    enum InspectorTab: String, CaseIterable, Identifiable {
        case overview, interfaces, config
        var id: Self { self }
        var title: String { rawValue.capitalized }
    }

    enum PowerState {
        case running, suspended, stopped
    }

    /// Canvas commands from the menu bar (Arrange menu); the canvas performs
    /// them, since it owns the undo manager.
    enum ArrangeAction: Equatable {
        case group, ungroup, lock, unlock
        case stack(ProjectDocument.StackMove)
    }

    // Tools and mode
    @Published var tool: Tool = .select
    /// What the Shape tool draws.
    @Published var shapeKind: ShapeKind = .roundedRectangle
    /// Copper by default so the Connect tool asks for ports, as CCNA labs
    /// specify them. "Automatic" in the library is the no-questions fast path.
    @Published var cableKind: CableKind = .copper
    @Published var mode: Mode = .realtime
    @Published var isPlaying = false
    @Published var simulationSpeed: Double = 1
    @Published var simulationTime: Double = 0
    @Published var visibleProtocols: Set<NetProtocol> = Set(NetProtocol.allCases)

    // Panels
    @Published var showsSidebar = true
    @Published var showsInspector = true
    @Published var showsBottomPanel = true
    @Published var bottomPanelMaximized = false
    @Published var rightPane: RightPane = .inspector
    @Published var sidebarTab: SidebarTab = .library
    @Published var bottomTab: BottomTab = .events
    @Published var inspectorTab: InspectorTab = .overview
    @Published var bottomPanelHeight: CGFloat = Theme.Size.bottomPanelDefault
    @Published var showsCommandPalette = false

    // Canvas
    @Published var zoom: CGFloat = 1
    @Published var showsInterfaceLabels = true
    @Published var showsGrid = true
    @Published var showsMinimap = true
    /// VLAN colours, trunks, blocked ports and root bridges on the canvas.
    @Published var showsLayer2 = true
    /// Dragged items line up with other devices and shapes (off: grid only).
    @Published var snapsToItems = false
    /// STP view: which VLAN's tree, and a link failure being replayed.
    @Published var stpVlan: UInt16 = 1
    @Published var stpFailedLink: UInt64?
    @Published var stpStep = 0

    var showsSpanningTree: Bool { showsInspector && rightPane == .spanningTree }

    func showSpanningTree(_ on: Bool) {
        if on {
            rightPane = .spanningTree
            showsInspector = true
        } else if rightPane == .spanningTree {
            rightPane = .inspector
        }
        stpFailedLink = nil
        stpStep = 0
    }

    /// Replays what spanning tree does when this cable fails (the project isn't changed).
    func failLink(_ id: UInt64) {
        showSpanningTree(true)
        stpFailedLink = id
        stpStep = 0
    }

    /// OSPF view: a link failure being replayed.
    @Published var ospfFailedLink: UInt64?
    @Published var ospfStep = 0

    var showsOspf: Bool { showsInspector && rightPane == .ospf }

    func showOspf(_ on: Bool) {
        if on {
            rightPane = .ospf
            showsInspector = true
        } else if rightPane == .ospf {
            rightPane = .inspector
        }
        ospfFailedLink = nil
        ospfStep = 0
    }

    /// Replays what OSPF does when this cable fails (the project isn't changed).
    func failOspfLink(_ id: UInt64) {
        showOspf(true)
        ospfFailedLink = id
        ospfStep = 0
    }
    @Published var librarySearch = ""
    @Published var topologySearch = ""
    @Published var hint: String?

    // Selection
    @Published var selection: Set<UInt64> = []
    @Published var selectedLink: UInt64?
    /// Selected notes and shapes. Devices and annotations can be selected
    /// together (for grouping and moving).
    @Published var selectedAnnotations: Set<UInt64> = []
    /// The one selected annotation, when it's the whole selection.
    var selectedAnnotation: UInt64? {
        selectedAnnotations.count == 1 && selection.isEmpty ? selectedAnnotations.first : nil
    }
    var hasCanvasSelection: Bool { !selection.isEmpty || !selectedAnnotations.isEmpty }
    @Published var arrangeRequest: ArrangeAction?
    /// The device whose configuration the Copy Configuration sheet copies.
    @Published var copyConfigSource: UInt64?
    /// A simulation step shown in the packet inspector.
    @Published var selectedEvent: UInt64?
    /// Add Simple PDU: picking the source, then the destination.
    @Published var isPickingPDU = false
    @Published var pduSource: UInt64?
    @Published var editingAnnotation: UInt64?
    @Published var renamingDevice: UInt64?
    /// Bumped by the Duplicate command; the canvas performs it (it owns the undo manager).
    @Published var duplicateRequests = 0
    /// World point shown at the top-left corner of the canvas. Together with
    /// `zoom` this is the whole viewport; both change in one update when
    /// zooming around a point, so the view never lands somewhere wrong.
    @Published var viewOrigin: CGPoint = .zero
    /// Size of the canvas on screen. Not published: set by the canvas on resize.
    var viewportSize = CGSize(width: 800, height: 600)

    /// Centre of the visible canvas in world coordinates.
    var viewportCenter: CGPoint {
        CGPoint(x: viewOrigin.x + viewportSize.width / (2 * zoom),
                y: viewOrigin.y + viewportSize.height / (2 * zoom))
    }
    @Published var pendingConnection: PendingConnection?

    // Preview state for features that arrive in later milestones.
    @Published var power: [UInt64: PowerState] = [:]
    @Published var consoleSessions: [UInt64] = []
    @Published var activeConsole: UInt64?
    @Published var captureLink: UInt64?

    static let zoomRange: ClosedRange<CGFloat> = 0.25...3

    /// Zooms keeping the world point under `point` (canvas coordinates) still;
    /// without a point, the centre of the canvas stays still.
    func setZoom(_ value: CGFloat, around point: CGPoint? = nil) {
        let new = min(max(value, Self.zoomRange.lowerBound), Self.zoomRange.upperBound)
        guard new != zoom else { return }
        let p = point ?? CGPoint(x: viewportSize.width / 2, y: viewportSize.height / 2)
        let world = CGPoint(x: viewOrigin.x + p.x / zoom, y: viewOrigin.y + p.y / zoom)
        zoom = new
        viewOrigin = CGPoint(x: world.x - p.x / new, y: world.y - p.y / new)
    }

    /// Moves the view by a distance in screen points; content follows the fingers.
    func pan(by delta: CGSize) {
        viewOrigin = CGPoint(x: viewOrigin.x - delta.width / zoom, y: viewOrigin.y - delta.height / zoom)
    }

    /// Centres the view on a world point.
    func center(on point: CGPoint) {
        viewOrigin = CGPoint(x: point.x - viewportSize.width / (2 * zoom),
                             y: point.y - viewportSize.height / (2 * zoom))
    }

    /// Fits a world rectangle in the view, leaving `bottomInset` points free.
    func fit(_ rect: CGRect, bottomInset: CGFloat = 0, maxZoom: CGFloat = 1) {
        let height = max(100, viewportSize.height - bottomInset)
        let fitted = min(viewportSize.width / rect.width, height / rect.height, maxZoom)
        zoom = min(max(fitted, Self.zoomRange.lowerBound), Self.zoomRange.upperBound)
        viewOrigin = CGPoint(x: rect.midX - viewportSize.width / (2 * zoom),
                             y: rect.midY - height / (2 * zoom))
    }

    func zoomIn() { setZoom((zoom * 1.25 * 100).rounded() / 100) }
    func zoomOut() { setZoom((zoom / 1.25 * 100).rounded() / 100) }

    func selectDevices(_ ids: Set<UInt64>) {
        select(devices: ids, annotations: [])
    }

    /// Replaces the canvas selection with devices and annotations.
    func select(devices: Set<UInt64>, annotations: Set<UInt64>) {
        selection = devices
        selectedAnnotations = annotations
        selectedLink = nil
        selectedEvent = nil
    }

    func selectLink(_ id: UInt64?) {
        selectedLink = id
        selection = []
        selectedAnnotations = []
        selectedEvent = nil
    }

    func selectAnnotation(_ id: UInt64?) {
        select(devices: [], annotations: id.map { [$0] } ?? [])
    }

    /// Shows a simulation step in the packet inspector and moves the clock to it.
    func selectEvent(_ event: TraceEventInfo) {
        selection = []
        selectedLink = nil
        selectedAnnotations = []
        selectedEvent = event.id
        mode = .simulation
        isPlaying = false
        simulationTime = event.time
        rightPane = .inspector
        showsInspector = true
    }

    func clearSelection() {
        selection = []
        selectedLink = nil
        selectedAnnotations = []
        selectedEvent = nil
        editingAnnotation = nil
    }

    func startPickingPDU() {
        isPickingPDU = true
        pduSource = nil
        flash("Add Simple PDU: click the device to ping from.")
    }

    func cancelPickingPDU() {
        isPickingPDU = false
        pduSource = nil
    }

    func powerState(_ id: UInt64) -> PowerState { power[id] ?? .running }

    func setPower(_ state: PowerState, for ids: [UInt64]) {
        for id in ids { power[id] = state }
    }

    func openConsole(for id: UInt64) {
        if !consoleSessions.contains(id) { consoleSessions.append(id) }
        activeConsole = id
        bottomTab = .console
        showsBottomPanel = true
        focusConsoleInput &+= 1
    }

    /// Bumped to ask the console's input field to take focus.
    @Published var focusConsoleInput = 0

    func openLab() {
        rightPane = .lab
        showsInspector = true
    }

    func startCapture(on link: UInt64) {
        captureLink = link
        bottomTab = .capture
        showsBottomPanel = true
    }

    func flash(_ message: String) {
        hint = message
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(2.5))
            if self?.hint == message { self?.hint = nil }
        }
    }

    /// Drops references to devices, links and annotations that no longer exist.
    func prune(devices: Set<UInt64>, links: Set<UInt64>, annotations: Set<UInt64>) {
        selectedAnnotations.formIntersection(annotations)
        if let id = editingAnnotation, !annotations.contains(id) { editingAnnotation = nil }
        selection.formIntersection(devices)
        if let link = selectedLink, !links.contains(link) { selectedLink = nil }
        consoleSessions.removeAll { !devices.contains($0) }
        if let id = renamingDevice, !devices.contains(id) { renamingDevice = nil }
        if let pending = pendingConnection, !devices.contains(pending.a) || !devices.contains(pending.b) {
            pendingConnection = nil
        }
        if let active = activeConsole, !devices.contains(active) { activeConsole = consoleSessions.first }
        if let link = captureLink, !links.contains(link) { captureLink = nil }
        power = power.filter { devices.contains($0.key) }
    }
}
