import SwiftUI

/// Window toolbar, grouped Tools · Mode · Simulation · Panels. Native,
/// customisable items so View → Customize Toolbar and "Icon and Text" work.
/// Device power lives in the Devices menu, context menus and the inspector:
/// devices are always on in a CCNA lab, and keeping ▶ off the toolbar means
/// play only ever refers to the simulation.
struct WindowToolbar: CustomizableToolbarContent {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel

    var body: some CustomizableToolbarContent {
        ToolbarItem(id: "sidebar", placement: .navigation) {
            Toggle(isOn: $workspace.showsSidebar) {
                Label("Sidebar", systemImage: "sidebar.left")
            }
            .help("Show or hide the sidebar (⌥⌘1)")
        }

        // Tools
        ToolbarItem(id: "tools", placement: .principal) {
            Picker("Tools", selection: $workspace.tool) {
                ForEach(WorkspaceModel.Tool.allCases) { tool in
                    Label(tool.title, systemImage: tool.toolbarSymbol)
                        .help("\(tool.title) (\(tool.key.uppercased()))")
                        .tag(tool)
                }
            }
            .pickerStyle(.segmented)
            .labelStyle(.iconOnly)
            .help("Select (V) · Hand (H) · Connect (C) · Note (N) · Area (R)")
        }

        // Mode
        ToolbarItem(id: "mode", placement: .principal) {
            Picker("Mode", selection: $workspace.mode) {
                ForEach(WorkspaceModel.Mode.allCases) { mode in
                    Label(mode.title, systemImage: mode.symbol).tag(mode)
                }
            }
            .pickerStyle(.segmented)
            .labelStyle(.titleAndIcon)
            .help("Realtime runs the network continuously (⌃⌘R). Simulation steps through every packet (⌃⌘S).")
        }

        // Simulation
        ToolbarItem(id: "pdu", placement: .primaryAction) {
            Button {
                workspace.mode = .simulation
                workspace.flash("Click a source device, then a destination. (Arrives in M2.)")
            } label: {
                Label("Add PDU", systemImage: "envelope.badge")
            }
            .help("Add Simple PDU: send a ping between two devices (⇧⌘P)")
        }
        ToolbarItem(id: "capture", placement: .primaryAction) {
            Button {
                if let link = workspace.selectedLink ?? document.state.links.first?.id {
                    workspace.startCapture(on: link)
                }
            } label: {
                Label("Capture", systemImage: "waveform.badge.magnifyingglass")
            }
            .disabled(document.state.links.isEmpty)
            .help("Capture packets on the selected cable")
        }

        // Panels
        ToolbarItem(id: "palette", placement: .primaryAction) {
            Button {
                workspace.showsCommandPalette = true
            } label: {
                Label("Commands", systemImage: "command")
            }
            .help("Command palette (⌘K)")
        }
        ToolbarItem(id: "bottomPanel", placement: .primaryAction) {
            Toggle(isOn: $workspace.showsBottomPanel) {
                Label("Panel", systemImage: "rectangle.bottomthird.inset.filled")
            }
            .help("Show or hide the console, events and capture panel (⌥⌘3)")
        }
        ToolbarItem(id: "inspector", placement: .primaryAction) {
            Toggle(isOn: $workspace.showsInspector) {
                Label("Inspector", systemImage: "sidebar.right")
            }
            .help("Show or hide the inspector and lab (⌥⌘2)")
        }
    }
}

extension WorkspaceModel.Tool {
    /// SF Symbol used in the toolbar, where native items render symbols best.
    var toolbarSymbol: String {
        switch self {
        case .select: "cursorarrow"
        case .hand: "hand.raised"
        case .cable: "cable.connector"
        case .note: "note.text"
        case .rectangle: "rectangle.dashed"
        }
    }
}
