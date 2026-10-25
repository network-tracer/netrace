import SwiftUI

/// Right-hand inspector. Shows the selected device, or a project summary.
/// Read-only in M0; editing arrives with M1.
struct InspectorPanel: View {
    @ObservedObject var document: ProjectDocument
    let selection: Set<UInt64>

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Spacing.l) {
                if selection.count == 1, let id = selection.first, let device = document.device(id) {
                    DeviceDetails(device: device, links: document.links(attachedTo: id), document: document)
                } else if selection.count > 1 {
                    Section("SELECTION") {
                        Row("Devices", "\(selection.count)")
                    }
                } else {
                    Section("PROJECT") {
                        Row("Devices", "\(document.state.devices.count)")
                        Row("Links", "\(document.state.links.count)")
                    }
                }
            }
            .padding(Theme.Spacing.l)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Theme.Palette.surface)
    }
}

private struct DeviceDetails: View {
    let device: DeviceInfo
    let links: [LinkInfo]
    let document: ProjectDocument

    var body: some View {
        HStack(spacing: Theme.Spacing.m) {
            Image(systemName: device.kind.symbol)
                .font(.system(size: 18, weight: .medium))
                .foregroundStyle(device.kind.tint)
                .frame(width: 40, height: 40)
                .background(device.kind.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: Theme.Radius.m))
            VStack(alignment: .leading, spacing: 2) {
                Text(device.name)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Theme.Palette.textPrimary)
                Text(device.kind.displayName)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
            }
        }

        Section("GENERAL") {
            Row("Position", "\(Int(device.x)), \(Int(device.y))")
            Row("Ports", "\(device.kind.ports.count)")
            Row("Connected", "\(links.count)")
        }

        Section("PORTS") {
            ForEach(device.kind.ports, id: \.self) { port in
                HStack(spacing: Theme.Spacing.s) {
                    Circle()
                        .fill(peer(of: port) != nil ? Theme.Palette.linkUp : Theme.Palette.border)
                        .frame(width: 7, height: 7)
                    Text(port)
                        .font(Theme.Font.mono)
                        .foregroundStyle(Theme.Palette.textPrimary)
                    Spacer(minLength: 0)
                    if let peer = peer(of: port) {
                        Text(peer)
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.Palette.textSecondary)
                    }
                }
            }
        }
    }

    /// "SW1 Fa0/1"-style label for whatever is plugged into `port`.
    private func peer(of port: String) -> String? {
        for link in links {
            let (mine, other) = link.a.deviceId == device.id ? (link.a, link.b) : (link.b, link.a)
            guard mine.port == port, let peer = document.device(other.deviceId) else { continue }
            return "\(peer.name) \(other.port)"
        }
        return nil
    }
}

private struct Section<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    init(_ title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            Text(title)
                .font(Theme.Font.sectionHeader)
                .foregroundStyle(Theme.Palette.textTertiary)
            content
        }
    }
}

private struct Row: View {
    let label: String
    let value: String

    init(_ label: String, _ value: String) {
        self.label = label
        self.value = value
    }

    var body: some View {
        HStack {
            Text(label).foregroundStyle(Theme.Palette.textSecondary)
            Spacer()
            Text(value).foregroundStyle(Theme.Palette.textPrimary).monospacedDigit()
        }
        .font(Theme.Font.body)
    }
}
