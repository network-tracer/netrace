import SwiftUI

/// The bundled labs, grouped by topic in teaching order. Opening one makes
/// a new untitled document from its starting topology.
struct LabBrowser: View {
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.newDocument) private var newDocument
    @Environment(\.dismiss) private var dismiss
    @State private var selection: String?
    private let labs = bundledLabs()

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Labs").font(Theme.Font.title)
                    Text("Each lab opens as a new project and checks your work as you go.")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.textSecondary)
                }
                Spacer()
            }
            .padding(Theme.Spacing.l)
            PanelDivider(axis: .horizontal)
            List(selection: $selection) {
                Section("Start here") {
                    HStack(alignment: .firstTextBaseline, spacing: Theme.Spacing.m) {
                        Image(systemName: "sparkles")
                            .foregroundStyle(Theme.Palette.accent)
                            .frame(width: 18, alignment: .trailing)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Tour of netrace").font(Theme.Font.label).foregroundStyle(Theme.Palette.textPrimary)
                            Text("Two minutes around the app, on a network with every feature set up.")
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.Palette.textSecondary)
                        }
                    }
                    .padding(.vertical, 3)
                    .tag("tour")
                }
                ForEach(topics, id: \.self) { topic in
                    Section(topic) {
                        ForEach(labs.filter { $0.topic == topic }, id: \.id) { lab in
                            row(lab)
                                .tag(lab.id)
                        }
                    }
                }
            }
            .listStyle(.inset)
            // Double-click (or Return) opens; a plain click just selects.
            .contextMenu(forSelectionType: String.self, menu: { _ in }, primaryAction: { ids in
                ids.first.map(open)
            })
            PanelDivider(axis: .horizontal)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Open Lab") { selection.map(open) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(selection == nil)
            }
            .padding(Theme.Spacing.m)
        }
        .frame(width: 520, height: 560)
        .background(Theme.Palette.surface)
    }

    /// Topics in the order their first lab appears.
    private var topics: [String] {
        var seen: [String] = []
        for lab in labs where !seen.contains(lab.topic) { seen.append(lab.topic) }
        return seen
    }

    private func row(_ lab: LabSummary) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Theme.Spacing.m) {
            Text(String(lab.id.prefix(2)))
                .font(Theme.Font.monoSmall)
                .foregroundStyle(Theme.Palette.textTertiary)
                .frame(width: 18, alignment: .trailing)
            VStack(alignment: .leading, spacing: 2) {
                Text(lab.title).font(Theme.Font.label).foregroundStyle(Theme.Palette.textPrimary)
                Text((try? AttributedString(markdown: lab.summary)) ?? AttributedString(lab.summary))
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
                    .lineLimit(2)
            }
            Spacer(minLength: Theme.Spacing.s)
            Text("\(lab.taskCount) steps")
                .font(Theme.Font.caption.monospacedDigit())
                .foregroundStyle(Theme.Palette.textTertiary)
        }
        .padding(.vertical, 3)
        .contentShape(Rectangle())
    }

    private func open(_ id: String) {
        if id == "tour", let tour = ProjectDocument.tour() {
            dismiss()
            newDocument { tour }
            return
        }
        guard let json = bundledLabJson(id: id), let document = try? ProjectDocument(json: Data(json)) else {
            workspace.flash("That lab couldn't be opened.")
            return
        }
        dismiss()
        newDocument { document }
    }
}
