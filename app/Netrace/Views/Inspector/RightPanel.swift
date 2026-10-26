import SwiftUI

/// Right-hand pane: the inspector, or the open lab. Keeping the lab here
/// means its instructions stay visible while the console is open below the canvas.
struct RightPanel: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                TabStrip(tabs: WorkspaceModel.RightPane.allCases, selection: $workspace.rightPane,
                         title: \.title, glyph: \.glyph)
                Spacer()
            }
            .padding(.horizontal, Theme.Spacing.l)
            .background(Theme.Palette.surfaceRaised)
            PanelDivider(axis: .horizontal)

            switch workspace.rightPane {
            case .inspector: InspectorPanel(document: document, workspace: workspace)
            case .lab: LabPanel(workspace: workspace)
            case .spanningTree: SpanningTreePanel(document: document, workspace: workspace)
            }
        }
        .background(Theme.Palette.surface)
    }
}

/// Lab instructions and automatic checks, laid out for a narrow pane.
private struct LabPanel: View {
    @ObservedObject var workspace: WorkspaceModel
    @State private var showsHint = false

    var body: some View {
        let done = PreviewData.labTasks.filter(\.isDone).count
        let total = PreviewData.labTasks.count
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Spacing.l) {
                VStack(alignment: .leading, spacing: Theme.Spacing.s) {
                    HStack(alignment: .firstTextBaseline) {
                        Text(PreviewData.labTitle)
                            .font(Theme.Font.headline)
                            .foregroundStyle(Theme.Palette.textPrimary)
                            .fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: Theme.Spacing.s)
                        PreviewBadge(milestone: "M6")
                    }
                    Text(PreviewData.labBrief)
                        .font(Theme.Font.body)
                        .foregroundStyle(Theme.Palette.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }

                VStack(alignment: .leading, spacing: Theme.Spacing.s) {
                    SectionHeader("Checks") {
                        Text("\(done) of \(total)")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textSecondary)
                            .monospacedDigit()
                    }
                    ProgressView(value: Double(done), total: Double(total))
                        .controlSize(.small)
                        .accessibilityLabel("\(done) of \(total) checks passed")
                    ForEach(PreviewData.labTasks) { task in
                        HStack(alignment: .firstTextBaseline, spacing: Theme.Spacing.s) {
                            Image(systemName: task.isDone ? "checkmark.circle.fill" : "circle")
                                .foregroundStyle(task.isDone ? Theme.Palette.statusUp : Theme.Palette.textTertiary)
                                .accessibilityLabel(task.isDone ? "Passed" : "Not yet")
                            VStack(alignment: .leading, spacing: 1) {
                                Text(task.title)
                                    .font(Theme.Font.body)
                                    .foregroundStyle(task.isDone ? Theme.Palette.textSecondary : Theme.Palette.textPrimary)
                                    .strikethrough(task.isDone, color: Theme.Palette.textTertiary)
                                Text(task.detail)
                                    .font(Theme.Font.caption)
                                    .foregroundStyle(Theme.Palette.textTertiary)
                            }
                        }
                        .accessibilityElement(children: .combine)
                    }
                }

                if showsHint {
                    Label("Use the Connect tool, drag from the switch to the router, then pick Gi0/0 on the router.",
                          systemImage: "lightbulb")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.textPrimary)
                        .padding(Theme.Spacing.s)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m))
                }

                HStack {
                    Button("Check My Work", systemImage: "checkmark.circle") {
                        workspace.flash("Automatic checks arrive with labs in M6.")
                    }
                    .buttonStyle(.borderedProminent)
                    Button(showsHint ? "Hide Hint" : "Hint", systemImage: "lightbulb") { showsHint.toggle() }
                }
                .controlSize(.small)
            }
            .padding(Theme.Spacing.l)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
