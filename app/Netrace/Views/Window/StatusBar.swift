import SwiftUI

struct StatusBar: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    private let version = coreVersion()

    var body: some View {
        HStack(spacing: Theme.Spacing.m) {
            HStack(spacing: Theme.Spacing.xs + 2) {
                StatusDot(color: workspace.mode == .simulation ? Theme.Palette.accent : Theme.Palette.statusUp, size: 6)
                Text(workspace.mode.title)
                    .foregroundStyle(Theme.Palette.textPrimary)
                if workspace.mode == .simulation {
                    Text(workspace.simulationTime.simulationClock)
                        .font(Theme.Font.monoSmall)
                }
            }
            Divider().frame(height: 12)
            Text("\(document.state.devices.count) devices · \(document.state.links.count) cables")
            if !workspace.selection.isEmpty {
                Divider().frame(height: 12)
                Text("\(workspace.selection.count) selected")
            } else if workspace.selectedLink != nil {
                Divider().frame(height: 12)
                Text("Cable selected")
            }
            if workspace.tool != .select {
                Divider().frame(height: 12)
                Text("\(workspace.tool.title) tool")
                    .foregroundStyle(Theme.Palette.accent)
            }
            Spacer()
            Text("\(Int((workspace.zoom * 100).rounded()))%")
                .monospacedDigit()
            Divider().frame(height: 12)
            Text("core \(version)")
                .foregroundStyle(Theme.Palette.textTertiary)
        }
        .font(Theme.Font.caption)
        .foregroundStyle(Theme.Palette.textSecondary)
        .padding(.horizontal, Theme.Spacing.m)
        .frame(maxHeight: .infinity)
        .background(Theme.Palette.surfaceSunken)
    }
}
