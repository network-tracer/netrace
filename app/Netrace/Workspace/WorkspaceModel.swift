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
            case .rectangle: "Area"
            }
        }

        var glyph: Glyph {
            switch self {
            case .select: .system("cursorarrow")
            case .hand: .system("hand.raised")
            case .cable: .custom(.cableCopper)
            case .note: .system("note.text")
            case .rectangle: .system("rectangle.dashed")
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
        case inspector, lab
        var id: Self { self }
        var title: String { self == .inspector ? "Inspector" : "Lab" }
        var glyph: Glyph { self == .inspector ? .system("slider.horizontal.3") : .custom(.lab) }
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

    // Tools and mode
    @Published var tool: Tool = .select
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
    @Published var librarySearch = ""
    @Published var topologySearch = ""
    @Published var hint: String?

    // Selection
    @Published var selection: Set<UInt64> = []
    @Published var selectedLink: UInt64?
    @Published var selectedAnnotation: UInt64?
    @Published var editingAnnotation: UInt64?
    @Published var renamingDevice: UInt64?
    /// Bumped by the Duplicate command; the canvas performs it (it owns the undo manager).
    @Published var duplicateRequests = 0
    /// Centre of the visible canvas in world coordinates. Not published:
    /// it changes on every scroll frame and only placement code reads it.
    var viewportCenter = CGPoint(x: 400, y: 300)
    @Published var pendingConnection: PendingConnection?

    // Preview state for features that arrive in later milestones.
    @Published var power: [UInt64: PowerState] = [:]
    @Published var consoleSessions: [UInt64] = []
    @Published var activeConsole: UInt64?
    @Published var captureLink: UInt64?

    static let zoomRange: ClosedRange<CGFloat> = 0.25...3

    func setZoom(_ value: CGFloat) {
        zoom = min(max(value, Self.zoomRange.lowerBound), Self.zoomRange.upperBound)
    }

    func zoomIn() { setZoom((zoom * 1.25 * 100).rounded() / 100) }
    func zoomOut() { setZoom((zoom / 1.25 * 100).rounded() / 100) }

    func selectDevices(_ ids: Set<UInt64>) {
        selection = ids
        selectedLink = nil
        selectedAnnotation = nil
    }

    func selectLink(_ id: UInt64?) {
        selectedLink = id
        selection = []
        selectedAnnotation = nil
    }

    func selectAnnotation(_ id: UInt64?) {
        selectedAnnotation = id
        selection = []
        selectedLink = nil
    }

    func clearSelection() {
        selection = []
        selectedLink = nil
        selectedAnnotation = nil
        editingAnnotation = nil
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
        if let id = selectedAnnotation, !annotations.contains(id) { selectedAnnotation = nil }
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
