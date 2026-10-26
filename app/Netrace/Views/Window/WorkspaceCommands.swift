import SwiftUI

/// Menu bar commands acting on the focused document window.
struct WorkspaceCommands: Commands {
    @FocusedObject private var workspace: WorkspaceModel?
    @FocusedObject private var document: ProjectDocument?
    @FocusedObject private var simulation: SimulationController?

    var body: some Commands {
        CommandGroup(after: .pasteboard) {
            Button("Duplicate") { workspace?.duplicateRequests += 1 }
                .keyboardShortcut("d")
                .disabled(workspace?.selection.isEmpty ?? true)
        }

        CommandGroup(after: .toolbar) {
            Button("Command Palette…") { workspace?.showsCommandPalette = true }
                .keyboardShortcut("k")
                .disabled(workspace == nil)
            Divider()
            Button(workspace?.showsSidebar == false ? "Show Sidebar" : "Hide Sidebar") { workspace?.showsSidebar.toggle() }
                .keyboardShortcut("1", modifiers: [.command, .option])
            Button(workspace?.showsInspector == false ? "Show Inspector" : "Hide Inspector") { workspace?.showsInspector.toggle() }
                .keyboardShortcut("2", modifiers: [.command, .option])
            Button(workspace?.showsBottomPanel == false ? "Show Bottom Panel" : "Hide Bottom Panel") { workspace?.showsBottomPanel.toggle() }
                .keyboardShortcut("3", modifiers: [.command, .option])
            Button(workspace?.bottomPanelMaximized == true ? "Restore Bottom Panel" : "Maximise Bottom Panel") {
                guard let workspace else { return }
                workspace.showsBottomPanel = true
                workspace.bottomPanelMaximized.toggle()
            }
            .keyboardShortcut(.upArrow, modifiers: [.command, .option])
            Button("Show Lab") { workspace?.openLab() }
                .keyboardShortcut("4", modifiers: [.command, .option])
            Divider()
            Button(workspace?.showsInterfaceLabels == false ? "Show Interface Labels" : "Hide Interface Labels") {
                workspace?.showsInterfaceLabels.toggle()
            }
            .keyboardShortcut("l", modifiers: [.command, .shift])
            Button(workspace?.showsGrid == false ? "Show Grid" : "Hide Grid") { workspace?.showsGrid.toggle() }
            Button(workspace?.showsMinimap == false ? "Show Minimap" : "Hide Minimap") { workspace?.showsMinimap.toggle() }
            Toggle("Spanning Tree View", isOn: Binding(
                get: { workspace?.showsSpanningTree ?? false },
                set: { workspace?.showSpanningTree($0) }
            ))
            .keyboardShortcut("s", modifiers: [.command, .option])
            Toggle("Snap to Items", isOn: Binding(
                get: { workspace?.snapsToItems ?? false },
                set: { workspace?.snapsToItems = $0 }
            ))
            .help("Line up with other devices and shapes while dragging; hold ⌘ to switch it for one drag")
            Button(workspace?.showsLayer2 == false ? "Show VLANs and Spanning Tree" : "Hide VLANs and Spanning Tree") {
                workspace?.showsLayer2.toggle()
            }
            .keyboardShortcut("v", modifiers: [.command, .option])
            Divider()
            Button("Zoom In") { workspace?.zoomIn() }.keyboardShortcut("+")
            Button("Zoom Out") { workspace?.zoomOut() }.keyboardShortcut("-")
            Button("Actual Size") { workspace?.setZoom(1) }.keyboardShortcut("0")
        }

        CommandMenu("Simulation") {
            Button("Realtime") { workspace?.mode = .realtime }
                .keyboardShortcut("r", modifiers: [.command, .control])
            Button("Simulation") { workspace?.mode = .simulation }
                .keyboardShortcut("s", modifiers: [.command, .control])
            Divider()
            Button(workspace?.isPlaying == true ? "Pause" : "Play") {
                workspace?.mode = .simulation
                workspace?.isPlaying.toggle()
            }
            .keyboardShortcut(.return, modifiers: [.command])
            Button("Step Forward") { step(forward: true) }
                .keyboardShortcut("]")
            Button("Step Back") { step(forward: false) }
                .keyboardShortcut("[")
            Button("Reset") {
                workspace?.isPlaying = false
                workspace?.simulationTime = 0
            }
            Divider()
            Button("Add Simple PDU") { workspace?.startPickingPDU() }
            .keyboardShortcut("p", modifiers: [.command, .shift])
        }

        CommandMenu("Arrange") {
            let hasItems = workspace?.hasCanvasSelection ?? false
            let hasAnnotations = !(workspace?.selectedAnnotations.isEmpty ?? true)
            Button("Group") { workspace?.arrangeRequest = .group }
                .keyboardShortcut("g", modifiers: [.command, .option])
                .disabled((workspace?.selection.count ?? 0) + (workspace?.selectedAnnotations.count ?? 0) < 2)
            Button("Ungroup") { workspace?.arrangeRequest = .ungroup }
                .keyboardShortcut("g", modifiers: [.command, .option, .shift])
                .disabled(!hasItems)
            Divider()
            Button("Bring to Front") { workspace?.arrangeRequest = .stack(.front) }
                .keyboardShortcut("f", modifiers: [.command, .shift])
                .disabled(!hasAnnotations)
            Button("Bring Forward") { workspace?.arrangeRequest = .stack(.forward) }
                .keyboardShortcut("f", modifiers: [.command, .option, .shift])
                .disabled(!hasAnnotations)
            Button("Send Backward") { workspace?.arrangeRequest = .stack(.backward) }
                .keyboardShortcut("b", modifiers: [.command, .option, .shift])
                .disabled(!hasAnnotations)
            Button("Send to Back") { workspace?.arrangeRequest = .stack(.back) }
                .keyboardShortcut("b", modifiers: [.command, .shift])
                .disabled(!hasAnnotations)
            Divider()
            Button("Lock") { workspace?.arrangeRequest = .lock }
                .keyboardShortcut("l")
                .disabled(!hasAnnotations)
            Button("Unlock") { workspace?.arrangeRequest = .unlock }
                .keyboardShortcut("l", modifiers: [.command, .option])
                .disabled(!hasAnnotations)
        }

        CommandMenu("Devices") {
            let ids = document?.state.devices.map(\.id) ?? []
            Button("Start All") { workspace?.setPower(.running, for: ids) }
            Button("Suspend All") { workspace?.setPower(.suspended, for: ids) }
            Button("Stop All") { workspace?.setPower(.stopped, for: ids) }
            Button("Reload All") { workspace?.setPower(.running, for: ids) }
            Divider()
            Button("Open Console") {
                if let id = workspace?.selection.first ?? ids.first { workspace?.openConsole(for: id) }
            }
            .keyboardShortcut("t", modifiers: [.command, .shift])
            Button("Rename Device") {
                if let workspace, workspace.selection.count == 1 { workspace.renamingDevice = workspace.selection.first }
            }
            .disabled(workspace?.selection.count != 1)
            Button("Capture on Selected Cable") {
                if let link = workspace?.selectedLink { workspace?.startCapture(on: link) }
            }
            .disabled(workspace?.selectedLink == nil)
        }
    }

    private func step(forward: Bool) {
        guard let workspace, let simulation else { return }
        let events = simulation.trace
        workspace.mode = .simulation
        if forward {
            if let next = events.first(where: { $0.time > workspace.simulationTime + 0.000_001 }) {
                workspace.simulationTime = next.time
            }
        } else {
            workspace.simulationTime = events.last { $0.time < workspace.simulationTime - 0.000_001 }?.time ?? 0
        }
    }
}
