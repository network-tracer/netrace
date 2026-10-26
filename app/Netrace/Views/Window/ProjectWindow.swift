import SwiftUI

/// Root layout of a document window:
/// sidebar | (canvas over bottom panel) | right pane, with a status bar.
/// Built from plain stacks so the chrome stays opaque and under our control.
struct ProjectWindow: View {
    @ObservedObject var document: ProjectDocument
    @StateObject private var workspace = WorkspaceModel()
    @StateObject private var simulation = SimulationController()
    @Environment(\.newDocument) private var newDocument
    @Environment(\.undoManager) private var undoManager
    @AppStorage("netrace.offeredTour") private var offeredTour = false
    @State private var showsTourOffer = false

    /// Canvas height kept visible when the bottom panel is maximised.
    private let minimumCanvasHeight: CGFloat = 140

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                if workspace.showsSidebar {
                    SidebarPanel(document: document, workspace: workspace)
                        .frame(width: Theme.Size.sidebarWidth)
                        .tourAnchor(.library)
                    PanelDivider()
                }
                GeometryReader { geometry in
                    VStack(spacing: 0) {
                        CanvasView(document: document, workspace: workspace)
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                            .tourAnchor(.canvas)
                        if workspace.showsBottomPanel {
                            BottomPanel(document: document, workspace: workspace)
                                .tourAnchor(.bottomPanel)
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
                        .tourAnchor(.rightPane)
                }
            }
            PanelDivider(axis: .horizontal)
            StatusBar(document: document, workspace: workspace)
                .frame(height: Theme.Size.statusBarHeight)
        }
        .background(Theme.Palette.surface)
        .frame(minWidth: 1040, minHeight: 640)
        .toolbar(id: "netrace.main") { WindowToolbar(document: document, workspace: workspace) }
        .overlayPreferenceValue(TourAnchorKey.self) { anchors in
            TourOverlay(workspace: workspace,
                        context: TourContext(document: document, workspace: workspace, simulation: simulation),
                        anchors: anchors)
        }
        .overlay(alignment: .bottomTrailing) {
            if showsTourOffer && workspace.tourStep == nil {
                TourOffer(start: {
                    showsTourOffer = false
                    takeTour()
                }, dismiss: { showsTourOffer = false })
                .padding(Theme.Spacing.xl)
                .padding(.bottom, Theme.Size.statusBarHeight)
                .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .onChange(of: workspace.tourRequests) { _, _ in takeTour() }
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
            let undo = undoManager
            workspace.applyPower = { [weak document] state, ids in
                document?.setPower(state.devicePower, for: ids, undoManager: undo)
            }
            workspace.applyReload = { [weak document] ids in
                document?.reload(ids, undoManager: undo)
            }
            mirrorPower(document.state)
            simulation.sync(with: document)
            // A lab opens on its instructions.
            if document.lab != nil {
                workspace.rightPane = .lab
                workspace.showsInspector = true
            }
            if document.startsTour {
                workspace.tourStep = 0
            } else if !offeredTour {
                // Offered once, on the first window ever opened.
                offeredTour = true
                withAnimation(.easeOut.delay(0.6)) { showsTourOffer = true }
            }
        }
        .onChange(of: document.state) { _, state in
            mirrorPower(state)
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

    private func mirrorPower(_ state: ProjectSnapshot) {
        let power = Dictionary(uniqueKeysWithValues: state.devices.map { ($0.id, WorkspaceModel.PowerState($0.power)) })
        if power != workspace.power { workspace.power = power }
    }

    /// Restarts the tour here if this is the tour network, otherwise opens
    /// the tour network in a new window.
    private func takeTour() {
        if document.startsTour {
            workspace.tourStep = 0
        } else if let tour = ProjectDocument.tour() {
            newDocument { tour }
        }
    }

}

/// The first-run offer: a small card, easy to ignore.
private struct TourOffer: View {
    let start: () -> Void
    let dismiss: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Spacing.m) {
            SymbolTile(symbol: "sparkles", tint: Swatch.blue.color, size: 32)
            VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                Text("New to netrace?").font(Theme.Font.headline).foregroundStyle(Theme.Palette.textPrimary)
                Text("A two-minute tour on the Shadow Network, with every feature set up. Handy if you know Packet Tracer or GNS3.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
                    .lineSpacing(1.5)
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: Theme.Spacing.m) {
                    Button(action: start) {
                        Text("Take the Tour")
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundStyle(Theme.Palette.onAccent)
                            .padding(.horizontal, 12)
                            .frame(height: 24)
                            .background(Theme.Palette.accent, in: Capsule())
                            .contentShape(Capsule())
                    }
                    .buttonStyle(.plain)
                    Button("Not Now", action: dismiss)
                        .buttonStyle(.plain)
                        .font(Theme.Font.labelSemibold)
                        .foregroundStyle(Theme.Palette.textSecondary)
                }
                .padding(.top, Theme.Spacing.xs)
            }
        }
        .padding(14)
        .frame(width: 320)
        .background(Theme.Palette.surfaceRaised, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Theme.Palette.border))
        .shadow(color: .black.opacity(0.16), radius: 2, y: 1)
        .shadow(color: .black.opacity(0.2), radius: 20, y: 10)
    }
}
