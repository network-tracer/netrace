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
                         title: \.title, glyph: \.glyph, compact: true)
                Spacer()
            }
            .padding(.horizontal, Theme.Spacing.l)
            .background(Theme.Palette.surfaceRaised)
            PanelDivider(axis: .horizontal)

            switch workspace.rightPane {
            case .inspector: InspectorPanel(document: document, workspace: workspace)
            case .lab: LabPanel(document: document, workspace: workspace)
            case .spanningTree: SpanningTreePanel(document: document, workspace: workspace)
            case .ospf: OspfPanel(document: document, workspace: workspace)
            case .vlans: VlanPanel(document: document, workspace: workspace)
            }
        }
        .background(Theme.Palette.surface)
    }
}

/// A lab, laid out for a narrow pane: the brief, then the tasks in order.
/// Checks run on the simulated network every time the topology changes;
/// the first unfinished task opens to show what's still missing and why.
private struct LabPanel: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController
    @State private var hints: Set<Int> = []
    @State private var showsBrief = true

    var body: some View {
        if let lab = document.lab {
            let results = simulation.labResults()
            let done = results.filter(\.done).count
            let current = results.firstIndex { !$0.done }
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.xl) {
                    header(lab, done: done, total: lab.tasks.count)
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(lab.tasks.enumerated()), id: \.offset) { index, task in
                            taskRow(task, index: index, result: results.indices.contains(index) ? results[index] : nil,
                                    isCurrent: index == current, isLast: index == lab.tasks.count - 1)
                        }
                    }
                    if current == nil, !results.isEmpty {
                        complete
                    }
                }
                .padding(Theme.Spacing.l)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        } else {
            VStack(spacing: Theme.Spacing.m) {
                EmptyState(glyph: .custom(.lab), title: "No lab in this project",
                           message: "Labs walk you through a CCNA topic step by step and check your work on the simulated network as you go.")
                Button("Browse Labs…") { workspace.showsLabBrowser = true }
                    .controlSize(.regular)
            }
            .padding(Theme.Spacing.l)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    @ViewBuilder
    private func header(_ lab: LabInfo, done: Int, total: Int) -> some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            Text(lab.topic.uppercased())
                .font(Theme.Font.sectionHeader)
                .tracking(Theme.Font.sectionHeaderTracking)
                .foregroundStyle(Theme.Palette.textTertiary)
            Text(lab.title)
                .font(Theme.Font.title)
                .foregroundStyle(Theme.Palette.textPrimary)
                .fixedSize(horizontal: false, vertical: true)
            Text(markdown(lab.brief))
                .font(Theme.Font.body)
                .foregroundStyle(Theme.Palette.textSecondary)
                .lineSpacing(2)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
            HStack(spacing: Theme.Spacing.s) {
                ProgressView(value: Double(done), total: Double(max(total, 1)))
                    .tint(done == total ? Theme.Palette.statusUp : Theme.Palette.accent)
                    .controlSize(.small)
                Text("\(done) of \(total)")
                    .font(Theme.Font.caption.monospacedDigit())
                    .foregroundStyle(Theme.Palette.textSecondary)
            }
            .padding(.top, Theme.Spacing.xs)
            .accessibilityElement(children: .combine)
            .accessibilityLabel("\(done) of \(total) tasks done")
        }
    }

    /// One task: a numbered marker joined to the next by a thin line.
    @ViewBuilder
    private func taskRow(_ task: LabTaskInfo, index: Int, result: LabTaskResult?, isCurrent: Bool, isLast: Bool) -> some View {
        let done = result?.done ?? false
        HStack(alignment: .top, spacing: Theme.Spacing.m) {
            VStack(spacing: 0) {
                Marker(number: index + 1, done: done, current: isCurrent)
                if !isLast {
                    Rectangle()
                        .fill(done ? Theme.Palette.statusUp.opacity(0.5) : Theme.Palette.border)
                        .frame(width: 1.5)
                        .frame(maxHeight: .infinity)
                }
            }
            VStack(alignment: .leading, spacing: Theme.Spacing.s) {
                Text(task.title)
                    .font(isCurrent ? Theme.Font.labelSemibold : Theme.Font.label)
                    .foregroundStyle(done || isCurrent ? Theme.Palette.textPrimary : Theme.Palette.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, 2)
                if isCurrent {
                    if !task.detail.isEmpty {
                        Text(markdown(task.detail))
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                            .textSelection(.enabled)
                    }
                    if let result {
                        checks(result)
                    }
                    if let hint = task.hint {
                        if hints.contains(index) {
                            Label { Text(markdown(hint)) } icon: { Image(systemName: "lightbulb") }
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.Palette.textPrimary)
                                .fixedSize(horizontal: false, vertical: true)
                                .padding(Theme.Spacing.s)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m))
                        } else {
                            Button("Show hint", systemImage: "lightbulb") { hints.insert(index) }
                                .buttonStyle(.link)
                                .font(Theme.Font.caption)
                        }
                    }
                }
            }
            .padding(.bottom, isLast ? 0 : Theme.Spacing.l)
            Spacer(minLength: 0)
        }
        .fixedSize(horizontal: false, vertical: true)
    }

    /// What's still missing: each failing check and why, passed ones ticked.
    @ViewBuilder
    private func checks(_ result: LabTaskResult) -> some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            ForEach(Array(result.checks.enumerated()), id: \.offset) { _, check in
                HStack(alignment: .firstTextBaseline, spacing: Theme.Spacing.s) {
                    Image(systemName: check.passed ? "checkmark" : "circle")
                        .font(.system(size: check.passed ? 9 : 6, weight: .bold))
                        .foregroundStyle(check.passed ? Theme.Palette.statusUp : Theme.Palette.textTertiary)
                        .frame(width: 10)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(check.label)
                            .font(Theme.Font.caption)
                            .foregroundStyle(check.passed ? Theme.Palette.textSecondary : Theme.Palette.textPrimary)
                        if !check.passed, !check.feedback.isEmpty {
                            Text(check.feedback)
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.Palette.textTertiary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
            }
        }
        .padding(Theme.Spacing.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m))
    }

    private var complete: some View {
        HStack(spacing: Theme.Spacing.s) {
            Image(systemName: "checkmark.seal.fill").foregroundStyle(Theme.Palette.statusUp)
            VStack(alignment: .leading, spacing: 2) {
                Text("Lab complete").font(Theme.Font.headline)
                Text("Every check passes on the simulated network.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
            }
            Spacer()
            Button("Next Lab…") { workspace.showsLabBrowser = true }
                .controlSize(.small)
        }
        .padding(Theme.Spacing.m)
        .background(Theme.Palette.statusUp.opacity(0.1), in: RoundedRectangle(cornerRadius: Theme.Radius.m))
    }

    private func markdown(_ text: String) -> AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(text)
    }
}

/// A task's number in a circle: hollow ahead, accent for the current
/// task, a green tick once done.
private struct Marker: View {
    let number: Int
    let done: Bool
    let current: Bool

    var body: some View {
        ZStack {
            if done {
                Circle().fill(Theme.Palette.statusUp)
                Image(systemName: "checkmark").font(.system(size: 9, weight: .bold)).foregroundStyle(.white)
            } else if current {
                Circle().fill(Theme.Palette.accent)
                Text("\(number)").font(.system(size: 10, weight: .semibold).monospacedDigit()).foregroundStyle(.white)
            } else {
                Circle().strokeBorder(Theme.Palette.borderStrong, lineWidth: 1.5)
                Text("\(number)").font(.system(size: 10, weight: .medium).monospacedDigit())
                    .foregroundStyle(Theme.Palette.textTertiary)
            }
        }
        .frame(width: 20, height: 20)
        .accessibilityLabel(done ? "Task \(number), done" : "Task \(number)")
    }
}
