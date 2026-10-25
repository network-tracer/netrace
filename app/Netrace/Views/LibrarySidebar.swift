import SwiftUI

/// Device library. Drag a device onto the canvas, or double-click to drop
/// one near the middle of the canvas.
struct LibrarySidebar: View {
    @ObservedObject var document: ProjectDocument
    @Binding var selection: Set<UInt64>
    @Environment(\.undoManager) private var undoManager

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            Text("DEVICES")
                .font(Theme.Font.sectionHeader)
                .foregroundStyle(Theme.Palette.textTertiary)
                .padding(.horizontal, Theme.Spacing.l)
                .padding(.top, Theme.Spacing.l)

            ScrollView {
                VStack(spacing: Theme.Spacing.xs) {
                    ForEach(DeviceKind.allCases) { kind in
                        LibraryRow(kind: kind)
                            .onTapGesture(count: 2) { add(kind) }
                            .draggable(kind.dragIdentifier) {
                                DeviceTile(kind: kind, name: kind.displayName, isSelected: false)
                            }
                    }
                }
                .padding(.horizontal, Theme.Spacing.s)
            }
        }
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Theme.Palette.surfaceSunken)
    }

    private func add(_ kind: DeviceKind) {
        let device = document.addDevice(kind, at: freeSpot(), undoManager: undoManager)
        selection = [device.id]
    }

    /// First grid position, scanning row by row from the top-left of the
    /// canvas, that doesn't overlap an existing device. (M1 replaces this
    /// with placement at the centre of the visible viewport.)
    private func freeSpot() -> CGPoint {
        let step = Theme.Size.gridSpacing * 4
        let clearance = Theme.Size.deviceTile + Theme.Spacing.l
        let taken = document.state.devices.map(\.position)
        for row in 1...40 {
            for column in 1...8 {
                let candidate = CGPoint(x: CGFloat(column) * step, y: CGFloat(row) * step)
                if taken.allSatisfy({ hypot($0.x - candidate.x, $0.y - candidate.y) >= clearance }) {
                    return candidate
                }
            }
        }
        return CGPoint(x: step, y: step)
    }
}

private struct LibraryRow: View {
    let kind: DeviceKind
    @State private var isHovered = false

    var body: some View {
        HStack(spacing: Theme.Spacing.m) {
            Image(systemName: kind.symbol)
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(kind.tint)
                .frame(width: 32, height: 32)
                .background(kind.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: Theme.Radius.s))
            VStack(alignment: .leading, spacing: 2) {
                Text(kind.displayName)
                    .font(Theme.Font.label)
                    .foregroundStyle(Theme.Palette.textPrimary)
                Text(kind.summary)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
            }
            Spacer(minLength: 0)
        }
        .padding(Theme.Spacing.s)
        .background(
            RoundedRectangle(cornerRadius: Theme.Radius.m)
                .fill(isHovered ? Theme.Palette.surface : .clear)
        )
        .contentShape(Rectangle())
        .onHover { isHovered = $0 }
        .help("Drag onto the canvas, or double-click to add")
    }
}
