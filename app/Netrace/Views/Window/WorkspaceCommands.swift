import SwiftUI

/// Menu bar commands acting on the focused document window.
struct WorkspaceCommands: Commands {
    @FocusedObject private var workspace: WorkspaceModel?
    @FocusedObject private var document: ProjectDocument?

    var body: some Commands {
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
            Button("Add Simple PDU") {
                workspace?.mode = .simulation
                workspace?.flash("Click a source device, then a destination. (Arrives in M2.)")
            }
            .keyboardShortcut("p", modifiers: [.command, .shift])
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
        guard let workspace, let document else { return }
        let events = PreviewData.events(for: document.state)
        workspace.mode = .simulation
        if forward {
            if let next = events.first(where: { $0.time > workspace.simulationTime }) { workspace.simulationTime = next.time }
        } else {
            workspace.simulationTime = events.last { $0.time < workspace.simulationTime - 0.000_001 }?.time ?? 0
        }
    }
}
