import SwiftUI

/// Dot grid for the visible part of the infinite canvas, drawn in screen
/// space. Dots stay at least 12 pt apart when zoomed out.
struct GridLayer: View {
    let origin: CGPoint
    let zoom: CGFloat
    let showsGrid: Bool

    var body: some View {
        Canvas { context, size in
            guard showsGrid, zoom > 0 else { return }
            var step = Theme.Size.gridSpacing
            while step * zoom < 12 { step *= 2 }
            let maxX = origin.x + size.width / zoom
            let maxY = origin.y + size.height / zoom
            var dots = Path()
            var y = (origin.y / step).rounded(.down) * step
            while y <= maxY {
                var x = (origin.x / step).rounded(.down) * step
                while x <= maxX {
                    dots.addEllipse(in: CGRect(x: (x - origin.x) * zoom - 0.8, y: (y - origin.y) * zoom - 0.8,
                                               width: 1.6, height: 1.6))
                    x += step
                }
                y += step
            }
            context.fill(dots, with: .color(Theme.Palette.canvasGrid))
        }
        .background(Theme.Palette.canvas)
    }
}

/// Cables, link lights at each end and optional interface labels.
struct LinksLayer: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    let positions: [UInt64: CGPoint]

    var body: some View {
        Canvas { context, _ in
            let kinds = Dictionary(uniqueKeysWithValues: document.state.devices.map { ($0.id, $0.kind) })
            for link in document.state.links {
                guard let a = positions[link.a.deviceId], let b = positions[link.b.deviceId],
                      let kindA = kinds[link.a.deviceId], let kindB = kinds[link.b.deviceId] else { continue }
                let isSelected = workspace.selectedLink == link.id
                let isCaptured = workspace.captureLink == link.id

                var path = Path()
                path.move(to: a)
                path.addLine(to: b)
                if isSelected {
                    context.stroke(path, with: .color(Theme.Palette.accent.opacity(0.25)), lineWidth: 7)
                }
                let dash: [CGFloat] = link.medium == .fiber ? [7, 5] : []
                context.stroke(path, with: .color(isSelected ? Theme.Palette.accent : Theme.Palette.link),
                               style: StrokeStyle(lineWidth: isSelected ? 2 : 1.5, lineCap: .round, dash: dash))

                let length = hypot(b.x - a.x, b.y - a.y)
                guard length > 1 else { continue }
                let dir = CGPoint(x: (b.x - a.x) / length, y: (b.y - a.y) / length)
                let edgeA = Self.edgeDistance(kindA, dir)
                let edgeB = Self.edgeDistance(kindB, CGPoint(x: -dir.x, y: -dir.y))
                let state = linkState(link)

                for (center, edge, sign) in [(a, edgeA, 1.0), (b, edgeB, -1.0)] {
                    // Below the device sits its name label; put the light past it.
                    let offset: CGFloat = dir.y * sign > 0.5 ? 28 : 6
                    let p = CGPoint(x: center.x + dir.x * sign * (edge + offset), y: center.y + dir.y * sign * (edge + offset))
                    Self.drawLight(in: &context, at: p, state: state)
                }

                if isCaptured {
                    let mid = CGPoint(x: (a.x + b.x) / 2, y: (a.y + b.y) / 2)
                    let badge = context.resolve(Image(Icon.capture.rawValue))
                    context.fill(Path(ellipseIn: CGRect(x: mid.x - 10, y: mid.y - 10, width: 20, height: 20)), with: .color(Theme.Palette.surface))
                    context.stroke(Path(ellipseIn: CGRect(x: mid.x - 10, y: mid.y - 10, width: 20, height: 20)), with: .color(Theme.Palette.accent), lineWidth: 1.5)
                    context.draw(badge, in: CGRect(x: mid.x - 7, y: mid.y - 7, width: 14, height: 14))
                }

                // Semantic zoom: below 70 % the labels would be unreadably small.
                guard workspace.showsInterfaceLabels, workspace.zoom >= 0.7, length > edgeA + edgeB + 100 else { continue }
                for (center, edge, sign, port) in [(a, edgeA, 1.0, link.a.port), (b, edgeB, -1.0, link.b.port)] {
                    // Cables leaving downward pass the device's name label; start labels below it.
                    let clearance: CGFloat = dir.y * sign > 0.5 ? 46 : 26
                    let p = CGPoint(x: center.x + dir.x * sign * (edge + clearance), y: center.y + dir.y * sign * (edge + clearance))
                    let text = context.resolve(
                        Text(port.abbreviatedInterface).font(Theme.Font.monoSmall).foregroundColor(Theme.Palette.textSecondary)
                    )
                    let size = text.measure(in: CGSize(width: 200, height: 40))
                    let box = CGRect(x: p.x - size.width / 2 - 4, y: p.y - size.height / 2 - 1.5,
                                     width: size.width + 8, height: size.height + 3)
                    let shape = Path(roundedRect: box, cornerRadius: 4)
                    context.fill(shape, with: .color(Theme.Palette.surface))
                    context.stroke(shape, with: .color(Theme.Palette.border), lineWidth: 1)
                    context.draw(text, at: p)
                }
            }
        }
    }

    private func linkState(_ link: LinkInfo) -> WorkspaceModel.PowerState {
        let states = [workspace.powerState(link.a.deviceId), workspace.powerState(link.b.deviceId)]
        if states.contains(.stopped) { return .stopped }
        if states.contains(.suspended) { return .suspended }
        return .running
    }

    /// Link light with shape as well as colour (see StatusGlyph).
    static func drawLight(in context: inout GraphicsContext, at p: CGPoint, state: WorkspaceModel.PowerState) {
        let outer = CGRect(x: p.x - 4, y: p.y - 4, width: 8, height: 8)
        let dot = Path(ellipseIn: outer.insetBy(dx: 0.75, dy: 0.75))
        context.fill(Path(ellipseIn: outer.insetBy(dx: -1, dy: -1)), with: .color(Theme.Palette.canvas))
        switch state {
        case .running:
            context.fill(dot, with: .color(state.color))
        case .suspended:
            context.stroke(dot, with: .color(state.color), lineWidth: 1.6)
            var half = context
            half.clip(to: Path(CGRect(x: outer.minX, y: outer.minY, width: outer.width / 2, height: outer.height)))
            half.fill(dot, with: .color(state.color))
        case .stopped:
            context.stroke(dot, with: .color(state.color), lineWidth: 1.8)
        }
    }

    /// Distance from a symbol's centre to its edge along a unit direction.
    static func edgeDistance(_ kind: DeviceKind, _ dir: CGPoint) -> CGFloat {
        let size = DeviceSymbol.size(for: kind)
        if kind == .router { return size.width / 2 }
        let tx = dir.x == 0 ? .infinity : (size.width / 2) / abs(dir.x)
        let ty = dir.y == 0 ? .infinity : (size.height / 2) / abs(dir.y)
        return min(tx, ty)
    }

    static func hitTest(_ point: CGPoint, links: [LinkInfo], positions: [UInt64: CGPoint], tolerance: CGFloat) -> UInt64? {
        links.first { link in
            guard let a = positions[link.a.deviceId], let b = positions[link.b.deviceId] else { return false }
            return point.distance(toSegment: a, b) <= tolerance
        }?.id
    }
}

private extension CGPoint {
    func distance(toSegment a: CGPoint, _ b: CGPoint) -> CGFloat {
        let dx = b.x - a.x, dy = b.y - a.y
        let lengthSquared = dx * dx + dy * dy
        guard lengthSquared > 0 else { return hypot(x - a.x, y - a.y) }
        let t = max(0, min(1, ((x - a.x) * dx + (y - a.y) * dy) / lengthSquared))
        return hypot(x - (a.x + t * dx), y - (a.y + t * dy))
    }
}

/// Simulation mode: every frame in flight at the current simulation time,
/// travelling along its cable. Frames a device ignores or drops show a red ✕
/// as they arrive.
struct PacketsLayer: View {
    @ObservedObject var workspace: WorkspaceModel
    @EnvironmentObject private var simulation: SimulationController
    let positions: [UInt64: CGPoint]
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Canvas { context, _ in
            let t = workspace.simulationTime
            let epsilon = 1e-9
            for event in simulation.trace {
                guard let proto = event.netProtocol, workspace.visibleProtocols.contains(proto) else { continue }
                let highlighted = workspace.selectedEvent == event.id
                if let sent = event.sentTime, let from = event.fromDeviceId,
                   let a = positions[from], let b = positions[event.deviceId] {
                    guard t >= sent - epsilon, t <= event.time + epsilon else { continue }
                    let progress = min(1, max(0, (t - sent) / max(event.time - sent, epsilon)))
                    // With Reduce Motion the packet jumps between devices instead of gliding.
                    let phase = reduceMotion ? (progress < 0.5 ? 0 : 1) : progress
                    let p = CGPoint(x: a.x + (b.x - a.x) * phase, y: a.y + (b.y - a.y) * phase)
                    Self.drawEnvelope(in: &context, at: p, color: proto.color, highlighted: highlighted,
                                      rejected: event.isRejection && phase >= 1)
                } else if event.outcome == .created, abs(event.time - t) < epsilon, let p = positions[event.deviceId] {
                    Self.drawEnvelope(in: &context, at: p, color: proto.color, highlighted: highlighted, rejected: false)
                }
            }
        }
    }

    static func drawEnvelope(in context: inout GraphicsContext, at p: CGPoint, color: Color,
                             highlighted: Bool = false, rejected: Bool = false) {
        let rect = CGRect(x: p.x - 9, y: p.y - 6.5, width: 18, height: 13)
        let body = Path(roundedRect: rect, cornerRadius: 2.5)
        context.fill(body, with: .color(color))
        context.stroke(body, with: .color(Theme.Palette.canvas), lineWidth: 1.5)
        var flap = Path()
        flap.move(to: CGPoint(x: rect.minX + 2.5, y: rect.minY + 2.5))
        flap.addLine(to: CGPoint(x: rect.midX, y: rect.midY + 1))
        flap.addLine(to: CGPoint(x: rect.maxX - 2.5, y: rect.minY + 2.5))
        context.stroke(flap, with: .color(.white.opacity(0.9)), style: StrokeStyle(lineWidth: 1.3, lineCap: .round, lineJoin: .round))
        if highlighted {
            context.stroke(Path(roundedRect: rect.insetBy(dx: -3, dy: -3), cornerRadius: 4),
                           with: .color(Theme.Palette.accent), lineWidth: 2)
        }
        if rejected {
            let badge = CGRect(x: rect.maxX - 4, y: rect.minY - 6, width: 10, height: 10)
            context.fill(Path(ellipseIn: badge), with: .color(Theme.Palette.statusDown))
            var cross = Path()
            cross.move(to: CGPoint(x: badge.minX + 3, y: badge.minY + 3))
            cross.addLine(to: CGPoint(x: badge.maxX - 3, y: badge.maxY - 3))
            cross.move(to: CGPoint(x: badge.maxX - 3, y: badge.minY + 3))
            cross.addLine(to: CGPoint(x: badge.minX + 3, y: badge.maxY - 3))
            context.stroke(cross, with: .color(.white), style: StrokeStyle(lineWidth: 1.4, lineCap: .round))
        }
    }
}
