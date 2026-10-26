import SwiftUI

/// "Duplicate with variations": gives other devices of the same model one
/// device's configuration, with only what must differ (hostnames,
/// addresses, gateway) filled in per device in a grid.
struct CopyConfigSheet: View {
    let source: DeviceInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager
    @Environment(\.dismiss) private var dismiss

    @State private var targets: [UInt64] = []
    /// target → variable key → value.
    @State private var values: [UInt64: [String: String]] = [:]
    @State private var errors: [String] = []

    private var variables: [ConfigVariable] { document.configVariables(of: source.id) }
    private var candidates: [DeviceInfo] {
        document.state.devices.filter { $0.kind == source.kind && $0.id != source.id }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.l) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Copy \(source.name)'s Configuration").font(Theme.Font.title)
                Text("Every setting is copied. Fill in only what differs on each device; empty means none.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
            }

            VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                SectionHeader("Copy to")
                HStack(spacing: Theme.Spacing.m) {
                    ForEach(candidates) { device in
                        Toggle(device.name, isOn: Binding(
                            get: { targets.contains(device.id) },
                            set: { on in toggle(device, on: on) }
                        ))
                        .toggleStyle(.checkbox)
                    }
                }
            }

            if targets.isEmpty {
                Text("Choose one or more devices to copy to.")
                    .font(Theme.Font.body)
                    .foregroundStyle(Theme.Palette.textTertiary)
                    .frame(maxWidth: .infinity, minHeight: 80)
            } else {
                ScrollView(.horizontal) {
                    Grid(alignment: .leading, horizontalSpacing: Theme.Spacing.m, verticalSpacing: Theme.Spacing.s) {
                        GridRow {
                            Text("")
                            Text(source.name)
                                .font(Theme.Font.labelSemibold)
                                .foregroundStyle(Theme.Palette.textSecondary)
                            ForEach(targets, id: \.self) { id in
                                Text(document.device(id)?.name ?? "")
                                    .font(Theme.Font.labelSemibold)
                            }
                        }
                        ForEach(variables, id: \.key) { variable in
                            GridRow {
                                Text(variable.label)
                                    .font(Theme.Font.body)
                                    .foregroundStyle(Theme.Palette.textSecondary)
                                    .gridColumnAlignment(.trailing)
                                Text(variable.value)
                                    .font(Theme.Font.mono)
                                    .foregroundStyle(Theme.Palette.textTertiary)
                                ForEach(targets, id: \.self) { id in
                                    TextField(variable.value, text: binding(target: id, key: variable.key))
                                        .textFieldStyle(.roundedBorder)
                                        .font(Theme.Font.mono)
                                        .frame(width: 150)
                                }
                            }
                        }
                    }
                    .padding(.vertical, Theme.Spacing.xs)
                }
                Text("Interfaces, routes, passwords, banner and line settings come along unchanged.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textTertiary)
            }

            ForEach(errors, id: \.self) { error in
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.statusDown)
            }

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(targets.count == 1 ? "Copy to 1 Device" : "Copy to \(targets.count) Devices") { apply() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(targets.isEmpty)
            }
        }
        .padding(Theme.Spacing.xl)
        .frame(minWidth: 520)
        .onAppear {
            // Devices selected alongside the source are the likely targets.
            for device in candidates where workspace.selection.contains(device.id) { toggle(device, on: true) }
        }
    }

    private func binding(target: UInt64, key: String) -> Binding<String> {
        Binding(get: { values[target]?[key] ?? "" }, set: { values[target, default: [:]][key] = $0 })
    }

    private func toggle(_ device: DeviceInfo, on: Bool) {
        if on {
            guard !targets.contains(device.id) else { return }
            targets.append(device.id)
            values[device.id] = suggestions(for: device, index: targets.count)
        } else {
            targets.removeAll { $0 == device.id }
        }
    }

    /// Prefilled values: the target's own where it has them (its name, its
    /// addresses), otherwise the source's with the network moved on by one
    /// per device, ready to correct.
    private func suggestions(for device: DeviceInfo, index: Int) -> [String: String] {
        let own = Dictionary(uniqueKeysWithValues: document.configVariables(of: device.id).map { ($0.key, $0.value) })
        var result: [String: String] = [:]
        for variable in variables {
            if let value = own[variable.key], !value.isEmpty {
                result[variable.key] = value
            } else if variable.key.hasPrefix("address:") {
                result[variable.key] = Self.shifted(variable.value, by: index)
            } else {
                result[variable.key] = variable.value
            }
        }
        return result
    }

    /// "192.168.1.1/24" → "192.168.2.1/24" (the subnet octet of a /24 or
    /// shorter moves; on smaller subnets the host moves).
    static func shifted(_ cidr: String, by step: Int) -> String {
        let parts = cidr.split(separator: "/")
        var octets = parts.first.map { $0.split(separator: ".").compactMap { Int($0) } } ?? []
        guard octets.count == 4, let prefix = parts.count > 1 ? Int(parts[1]) : nil else { return cidr }
        let index = prefix <= 24 ? max(0, (prefix - 1) / 8) : 3
        octets[index] = min(255, octets[index] + step)
        return octets.map(String.init).joined(separator: ".") + "/\(prefix)"
    }

    private func apply() {
        errors = []
        undoManager?.beginUndoGrouping()
        for id in targets {
            let filled = variables.map { ConfigVariable(key: $0.key, label: $0.label, value: values[id]?[$0.key] ?? "") }
            if case .failure(let error) = document.copyConfig(from: source.id, to: id, values: filled, undoManager: undoManager) {
                errors.append(error.message)
            }
        }
        undoManager?.setActionName(targets.count == 1 ? "Copy Configuration" : "Copy Configuration to \(targets.count) Devices")
        undoManager?.endUndoGrouping()
        if errors.isEmpty { dismiss() }
    }
}
