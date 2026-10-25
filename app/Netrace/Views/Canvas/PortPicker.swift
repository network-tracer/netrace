import SwiftUI

/// Shown when a cable is drawn with a specific cable type: pick the port on
/// each end. Defaults are the first free ports, so Return accepts quickly.
struct PortPicker: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    let onConnect: () -> Void

    var body: some View {
        if let pending = workspace.pendingConnection,
           let a = document.device(pending.a), let b = document.device(pending.b) {
            VStack(alignment: .leading, spacing: Theme.Spacing.m) {
                HStack(spacing: Theme.Spacing.s) {
                    IconView(pending.medium == .fiber ? .cableFiber : .cableCopper, size: 18)
                        .foregroundStyle(Theme.Palette.textSecondary)
                    Text("Connect \(a.name) to \(b.name)")
                        .font(Theme.Font.headline)
                        .foregroundStyle(Theme.Palette.textPrimary)
                }

                endRow(device: a, selection: Binding(
                    get: { workspace.pendingConnection?.portA ?? pending.portA },
                    set: { workspace.pendingConnection?.portA = $0 }
                ))
                endRow(device: b, selection: Binding(
                    get: { workspace.pendingConnection?.portB ?? pending.portB },
                    set: { workspace.pendingConnection?.portB = $0 }
                ))

                HStack {
                    Text("Medium")
                        .font(Theme.Font.body)
                        .foregroundStyle(Theme.Palette.textSecondary)
                        .frame(width: 70, alignment: .leading)
                    Picker("Medium", selection: Binding(
                        get: { workspace.pendingConnection?.medium ?? pending.medium },
                        set: { workspace.pendingConnection?.medium = $0 }
                    )) {
                        Text("Copper").tag(Medium.copper)
                        Text("Fibre").tag(Medium.fiber)
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .fixedSize()
                }

                HStack {
                    Spacer()
                    Button("Cancel") { workspace.pendingConnection = nil }
                        .keyboardShortcut(.cancelAction)
                    Button("Connect", action: onConnect)
                        .keyboardShortcut(.defaultAction)
                }
            }
            .padding(Theme.Spacing.l)
            .frame(width: 320)
        }
    }

    private func endRow(device: DeviceInfo, selection: Binding<String>) -> some View {
        let size = DeviceSymbol.size(for: device.kind)
        let free = document.freePorts(of: device.id)
        return HStack(spacing: Theme.Spacing.s) {
            DeviceSymbolView(kind: device.kind, scale: min(24 / size.width, 18 / size.height))
                .frame(width: 26)
            Text(device.name)
                .font(Theme.Font.body)
                .foregroundStyle(Theme.Palette.textPrimary)
                .frame(width: 70, alignment: .leading)
                .lineLimit(1)
            Picker(device.name, selection: selection) {
                ForEach(free, id: \.self) { port in
                    Text(port).tag(port)
                }
            }
            .labelsHidden()
            .frame(maxWidth: .infinity)
            Text("\(free.count) free")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.Palette.textSecondary)
                .monospacedDigit()
        }
    }
}
