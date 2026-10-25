import SwiftUI

/// Topology canvas. M0 version: fixed viewport (pan/zoom arrive in M1),
/// devices can be dropped from the library, selected, dragged and deleted.
struct CanvasView: View {
    @ObservedObject var document: ProjectDocument
    @Binding var selection: Set<UInt64>
    @Environment(\.undoManager) private var undoManager

    /// Live offset of the devices being dragged. Positions are committed to
    /// the core only when the drag ends, giving one undo step per drag.
    @State private var dragOffset: CGSize = .zero
    @State private var draggedIDs: Set<UInt64> = []
    @FocusState private var isFocused: Bool

    var body: some View {
        ZStack(alignment: .topLeading) {
            GridBackground()
                .contentShape(Rectangle())
                .onTapGesture {
                    selection.removeAll()
                    isFocused = true
                }

            LinksLayer(devices: displayedPositions, links: document.state.links)
                .allowsHitTesting(false)

            ForEach(document.state.devices) { device in
                DeviceTile(kind: device.kind, name: device.name, isSelected: selection.contains(device.id))
                    .position(displayedPositions[device.id] ?? device.position)
                    .gesture(dragGesture(for: device))
                    .simultaneousGesture(TapGesture().onEnded { select(device.id) })
            }

            if document.state.devices.isEmpty {
                EmptyCanvasHint()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .allowsHitTesting(false)
            }
        }
        .coordinateSpace(name: Self.space)
        .clipped()
        .dropDestination(for: String.self) { items, location in
            guard let kind = items.first.flatMap(DeviceKind.init(dragIdentifier:)) else { return false }
            let device = document.addDevice(kind, at: location.snapped, undoManager: undoManager)
            selection = [device.id]
            isFocused = true
            return true
        }
        .focusable()
        .focused($isFocused)
        .focusEffectDisabled()
        .onDeleteCommand {
            document.removeDevices(selection, undoManager: undoManager)
        }
    }

    private static let space = "canvas"

    private var displayedPositions: [UInt64: CGPoint] {
        Dictionary(uniqueKeysWithValues: document.state.devices.map { device in
            let p = device.position
            guard draggedIDs.contains(device.id) else { return (device.id, p) }
            return (device.id, CGPoint(x: p.x + dragOffset.width, y: p.y + dragOffset.height))
        })
    }

    private func select(_ id: UInt64) {
        if NSEvent.modifierFlags.contains(.shift) {
            selection.formSymmetricDifference([id])
        } else {
            selection = [id]
        }
        isFocused = true
    }

    private func dragGesture(for device: DeviceInfo) -> some Gesture {
        DragGesture(minimumDistance: 2, coordinateSpace: .named(Self.space))
            .onChanged { value in
                if draggedIDs.isEmpty {
                    // Dragging an unselected device moves just that device.
                    if !selection.contains(device.id) { selection = [device.id] }
                    draggedIDs = selection
                }
                dragOffset = value.translation
            }
            .onEnded { _ in
                let moved = draggedIDs
                let offset = dragOffset
                for id in moved {
                    guard let d = document.device(id) else { continue }
                    let target = CGPoint(x: d.x + offset.width, y: d.y + offset.height).snapped
                    document.moveDevice(id, to: target, undoManager: undoManager)
                }
                draggedIDs = []
                dragOffset = .zero
            }
    }
}

private extension CGPoint {
    /// Snaps to half the grid spacing so devices line up without feeling locked.
    var snapped: CGPoint {
        let step = Theme.Size.gridSpacing / 2
        return CGPoint(x: (x / step).rounded() * step, y: (y / step).rounded() * step)
    }
}

struct DeviceTile: View {
    let kind: DeviceKind
    let name: String
    let isSelected: Bool

    var body: some View {
        VStack(spacing: Theme.Spacing.xs) {
            Image(systemName: kind.symbol)
                .font(.system(size: 22, weight: .medium))
                .foregroundStyle(kind.tint)
                .frame(width: Theme.Size.deviceTile, height: Theme.Size.deviceTile)
                .background(Theme.Palette.surface, in: RoundedRectangle(cornerRadius: Theme.Radius.m))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.Radius.m)
                        .strokeBorder(isSelected ? Theme.Palette.accent : Theme.Palette.border,
                                      lineWidth: isSelected ? 2 : 1)
                )
                .shadow(color: .black.opacity(0.06), radius: 2, y: 1)
            Text(name)
                .font(Theme.Font.label)
                .foregroundStyle(isSelected ? Theme.Palette.accent : Theme.Palette.textPrimary)
        }
        .contentShape(Rectangle())
    }
}

private struct LinksLayer: View {
    let devices: [UInt64: CGPoint]
    let links: [LinkInfo]

    var body: some View {
        Canvas { context, _ in
            for link in links {
                guard let a = devices[link.a.deviceId], let b = devices[link.b.deviceId] else { continue }
                var path = Path()
                path.move(to: a)
                path.addLine(to: b)
                let style = link.medium == .fiber
                    ? StrokeStyle(lineWidth: 2, dash: [6, 4])
                    : StrokeStyle(lineWidth: 2)
                context.stroke(path, with: .color(Theme.Palette.link), style: style)
            }
        }
    }
}

private struct GridBackground: View {
    var body: some View {
        Canvas { context, size in
            let step = Theme.Size.gridSpacing
            let dot = CGSize(width: 1.5, height: 1.5)
            var y: CGFloat = step
            while y < size.height {
                var x: CGFloat = step
                while x < size.width {
                    context.fill(Path(ellipseIn: CGRect(origin: CGPoint(x: x - 0.75, y: y - 0.75), size: dot)),
                                 with: .color(Theme.Palette.canvasGrid))
                    x += step
                }
                y += step
            }
        }
        .background(Theme.Palette.canvas)
    }
}

private struct EmptyCanvasHint: View {
    var body: some View {
        VStack(spacing: Theme.Spacing.s) {
            Image(systemName: "square.dashed")
                .font(.system(size: 28, weight: .light))
            Text("Drag a device here to start")
                .font(Theme.Font.body)
        }
        .foregroundStyle(Theme.Palette.textTertiary)
    }
}
