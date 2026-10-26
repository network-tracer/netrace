import SwiftUI

/// Root layout of a document window:
/// sidebar | (canvas over bottom panel) | right pane, with a status bar.
/// Built from plain stacks so the chrome stays opaque and under our control.
struct ProjectWindow: View {
    @ObservedObject var document: ProjectDocument
    @StateObject private var workspace = WorkspaceModel()
    @StateObject private var simulation = SimulationController()

    /// Canvas height kept visible when the bottom panel is maximised.
    private let minimumCanvasHeight: CGFloat = 140

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                if workspace.showsSidebar {
                    SidebarPanel(document: document, workspace: workspace)
                        .frame(width: Theme.Size.sidebarWidth)
                    PanelDivider()
                }
                GeometryReader { geometry in
                    VStack(spacing: 0) {
                        CanvasView(document: document, workspace: workspace)
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                        if workspace.showsBottomPanel {
                            BottomPanel(document: document, workspace: workspace)
                                .frame(height: workspace.bottomPanelMaximized
                                       ? max(geometry.size.height - minimumCanvasHeight, workspace.bottomPanelHeight)
                                       : workspace.bottomPanelHeight)
                        }
                    }
                }
                if workspace.showsInspector {
                    PanelDivider()
                    RightPanel(document: document, workspace: workspace)
                        .frame(width: Theme.Size.inspectorWidth)
                }
            }
            PanelDivider(axis: .horizontal)
            StatusBar(document: document, workspace: workspace)
                .frame(height: Theme.Size.statusBarHeight)
        }
        .background(Theme.Palette.surface)
        .frame(minWidth: 1040, minHeight: 640)
        .toolbar(id: "netrace.main") { WindowToolbar(document: document, workspace: workspace) }
        .overlay {
            if workspace.showsCommandPalette {
                CommandPalette(document: document, workspace: workspace)
            }
        }
        .animation(.easeOut(duration: 0.18), value: workspace.bottomPanelMaximized)
        .sheet(isPresented: Binding(
            get: { workspace.copyConfigSource != nil },
            set: { if !$0 { workspace.copyConfigSource = nil } }
        )) {
            if let source = workspace.copyConfigSource.flatMap(document.device) {
                CopyConfigSheet(source: source, document: document, workspace: workspace)
            }
        }
        .environmentObject(simulation)
        .focusedSceneObject(simulation)
        .focusedSceneObject(workspace)
        .focusedSceneObject(document)
        .sheet(isPresented: $workspace.showsLabBrowser) {
            LabBrowser(workspace: workspace)
        }
        .onAppear {
            simulation.sync(with: document)
            // A lab opens on its instructions.
            if document.lab != nil {
                workspace.rightPane = .lab
                workspace.showsInspector = true
            }
        }
        .onChange(of: document.state) { _, state in
            workspace.prune(devices: Set(state.devices.map(\.id)), links: Set(state.links.map(\.id)),
                            annotations: Set(state.annotations.map(\.id)))
            simulation.refreshPrompts(document: document)
            if simulation.sync(with: document) {
                workspace.isPlaying = false
                workspace.simulationTime = 0
                workspace.selectedEvent = nil
                workspace.flash("The topology changed, so the simulation was reset.")
            }
        }
    }
}
