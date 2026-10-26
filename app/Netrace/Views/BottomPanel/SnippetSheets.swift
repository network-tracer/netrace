import SwiftUI

/// Console toolbar menu: run a snippet on this device, or write a new one.
struct SnippetMenu: View {
    let device: DeviceInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject private var store = SnippetStore.shared
    @State private var running: Snippet?
    @State private var editing: Snippet?
    @State private var managing = false

    var body: some View {
        Menu {
            Section("Built in") {
                ForEach(SnippetStore.builtIn) { snippet in
                    Button(snippet.name) { running = snippet }
                }
            }
            if !store.custom.isEmpty {
                Section("My snippets") {
                    ForEach(store.custom) { snippet in
                        Button(snippet.name) { running = snippet }
                    }
                }
            }
            Divider()
            Button("New Snippet…") { editing = Snippet(name: "", body: "") }
            Button("Edit My Snippets…") { managing = true }
                .disabled(store.custom.isEmpty)
        } label: {
            Label("Snippets", systemImage: "text.badge.plus")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .controlSize(.small)
        .help("Run a saved block of configuration, filling in only what differs")
        .sheet(item: $running) { snippet in
            SnippetRunSheet(snippet: snippet, device: device, document: document)
        }
        .sheet(item: $editing) { snippet in
            SnippetEditor(snippet: snippet)
        }
        .sheet(isPresented: $managing) {
            SnippetManager()
        }
    }
}

/// Fill in a snippet's fields, check the result, run it in the console.
private struct SnippetRunSheet: View {
    let snippet: Snippet
    let device: DeviceInfo
    @ObservedObject var document: ProjectDocument
    @EnvironmentObject private var simulation: SimulationController
    @Environment(\.undoManager) private var undoManager
    @Environment(\.dismiss) private var dismiss
    @State private var values: [String: String] = [:]

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.l) {
            VStack(alignment: .leading, spacing: 2) {
                Text(snippet.name).font(Theme.Font.title)
                Text("Runs on \(device.name), in configuration mode. Fill in what differs.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
            }
            if !snippet.fields.isEmpty {
                Grid(alignment: .leading, horizontalSpacing: Theme.Spacing.m, verticalSpacing: Theme.Spacing.s) {
                    ForEach(snippet.fields) { field in
                        GridRow {
                            Text(field.label)
                                .font(Theme.Font.body)
                                .foregroundStyle(Theme.Palette.textSecondary)
                                .gridColumnAlignment(.trailing)
                            TextField(field.defaultValue.isEmpty ? field.label : field.defaultValue,
                                      text: binding(for: field))
                                .textFieldStyle(.roundedBorder)
                                .font(Theme.Font.mono)
                                .frame(minWidth: 220)
                        }
                    }
                }
            }
            VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                SectionHeader("Commands")
                ScrollView {
                    Text(snippet.filled(with: values))
                        .font(Theme.Font.monoSmall)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(Theme.Spacing.s)
                }
                .frame(height: 150)
                .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous))
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Run on \(device.name)") {
                    simulation.configure(snippet.filled(with: values), on: device, document: document,
                                         undoManager: undoManager, actionName: snippet.name)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(snippet.fields.contains { (values[$0.label] ?? "").trimmingCharacters(in: .whitespaces).isEmpty })
            }
        }
        .padding(Theme.Spacing.xl)
        .frame(width: 460)
        .onAppear {
            for field in snippet.fields where values[field.label] == nil {
                values[field.label] = field.label == "Hostname" ? device.name : field.defaultValue
            }
        }
    }

    private func binding(for field: Snippet.Field) -> Binding<String> {
        Binding(get: { values[field.label] ?? "" }, set: { values[field.label] = $0 })
    }
}

/// Write or change one of the user's snippets.
private struct SnippetEditor: View {
    @State var snippet: Snippet
    @ObservedObject private var store = SnippetStore.shared
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.m) {
            Text(store.custom.contains { $0.id == snippet.id } ? "Edit Snippet" : "New Snippet")
                .font(Theme.Font.title)
            TextField("Name", text: $snippet.name)
                .textFieldStyle(.roundedBorder)
            VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                Text("Configuration commands. Write {{Label}} for a field to fill in, or {{Label=default}}.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
                TextEditor(text: $snippet.body)
                    .font(Theme.Font.mono)
                    .frame(height: 200)
                    .overlay(RoundedRectangle(cornerRadius: Theme.Radius.s).strokeBorder(Theme.Palette.border))
            }
            if !snippet.fields.isEmpty {
                Text("Fields: " + snippet.fields.map(\.label).joined(separator: ", "))
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textTertiary)
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Save") {
                    if let index = store.custom.firstIndex(where: { $0.id == snippet.id }) {
                        store.custom[index] = snippet
                    } else {
                        store.custom.append(snippet)
                    }
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(snippet.name.trimmingCharacters(in: .whitespaces).isEmpty
                          || snippet.body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(Theme.Spacing.xl)
        .frame(width: 480)
    }
}

/// The user's snippets: edit or delete.
private struct SnippetManager: View {
    @ObservedObject private var store = SnippetStore.shared
    @Environment(\.dismiss) private var dismiss
    @State private var editing: Snippet?

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.m) {
            Text("My Snippets").font(Theme.Font.title)
            List {
                ForEach(store.custom) { snippet in
                    HStack {
                        Text(snippet.name)
                        Spacer()
                        Button("Edit") { editing = snippet }
                        Button("Delete", role: .destructive) {
                            store.custom.removeAll { $0.id == snippet.id }
                        }
                    }
                    .controlSize(.small)
                }
            }
            .frame(height: 220)
            HStack {
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(Theme.Spacing.xl)
        .frame(width: 420)
        .sheet(item: $editing) { snippet in
            SnippetEditor(snippet: snippet)
        }
    }
}
