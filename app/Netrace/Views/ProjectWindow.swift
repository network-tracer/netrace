import SwiftUI

/// Root layout of a document window: library | canvas | inspector, with a
/// status bar. Built from plain stacks (not NavigationSplitView) so the
/// chrome stays opaque and fully under our control.
struct ProjectWindow: View {
    @ObservedObject var document: ProjectDocument
    @State private var selection: Set<UInt64> = []

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                LibrarySidebar(document: document, selection: $selection)
                    .frame(width: Theme.Size.sidebarWidth)
                Divider().overlay(Theme.Palette.border)
                CanvasView(document: document, selection: $selection)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                Divider().overlay(Theme.Palette.border)
                InspectorPanel(document: document, selection: selection)
                    .frame(width: Theme.Size.inspectorWidth)
            }
            Divider().overlay(Theme.Palette.border)
            StatusBar(document: document)
                .frame(height: Theme.Size.statusBarHeight)
        }
        .background(Theme.Palette.surface)
        .frame(minWidth: 900, minHeight: 560)
        .onChange(of: document.state.devices.map(\.id)) { _, ids in
            // Drop selection of devices that no longer exist (e.g. after undo).
            selection.formIntersection(ids)
        }
    }
}
