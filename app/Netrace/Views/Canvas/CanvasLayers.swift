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
    /// VLANs, trunks and spanning tree, when the overlay is shown.
    var layer2: Layer2Overlay?
    /// Port roles in the STP view (replaces the lights).
    var stp: StpDisplay?
    /// ACL and relay badges at interface ends.
    var services: [ServiceBadge] = []

    var body: some View {
        Canvas { context, _ in
            let kinds = Dictionary(uniqueKeysWithValues: document.state.devices.map { ($0.id, $0.kind) })
            let ports = Dictionary((layer2?.ports ?? []).map { (PortKey(device: $0.deviceId, port: $0.port), $0) },
                                   uniquingKeysWith: { a, _ in a })
            for link in document.state.links {
                guard let (a, b) = Self.endpoints(of: link, among: document.state.links, positions: positions),
                      let kindA = kinds[link.a.deviceId], let kindB = kinds[link.b.deviceId] else { continue }
                let isSelected = workspace.selectedLink == link.id
                let isCaptured = workspace.captureLink == link.id

                let length = hypot(b.x - a.x, b.y - a.y)
                guard length > 1 else { continue }
                let dir = CGPoint(x: (b.x - a.x) / length, y: (b.y - a.y) / length)
                if let stp, stp.failedLink == link.id {
                    // The failed link: faded, struck through in the middle.
                    var line = Path()
                    line.move(to: a)
                    line.addLine(to: b)
                    context.stroke(line, with: .color(Theme.Palette.statusDown.opacity(0.45)),
                                   style: StrokeStyle(lineWidth: 1.5, dash: [3, 4]))
                    let mid = CGPoint(x: (a.x + b.x) / 2, y: (a.y + b.y) / 2)
                    var bolt = context.resolve(Image(systemName: "bolt.horizontal.circle.fill"))
                    bolt.shading = .color(Theme.Palette.statusDown)
                    context.draw(bolt, in: CGRect(x: mid.x - 9, y: mid.y - 9, width: 18, height: 18))
                    continue
                }
                let endA = ports[PortKey(device: link.a.deviceId, port: link.a.port)]
                let endB = ports[PortKey(device: link.b.deviceId, port: link.b.port)]
                let trunk = endA?.trunk == true || endB?.trunk == true
                // An access link takes its VLAN's colour (VLAN 1 stays neutral).
                let vlan = trunk ? nil : (endA?.vlan ?? endB?.vlan).flatMap { $0 == 1 ? nil : $0 }
                Self.drawCable(link, from: a, to: b, direction: dir, isSelected: isSelected,
                               tint: vlan.map(Self.vlanColor), trunk: trunk, in: &context)
                let edgeA = Self.edgeDistance(kindA, dir)
                let edgeB = Self.edgeDistance(kindB, CGPoint(x: -dir.x, y: -dir.y))
                let state = linkState(link)

                for (center, edge, sign, end) in [(a, edgeA, 1.0, endA), (b, edgeB, -1.0, endB)] {
                    // Below the device sits its name label; put the light past it.
                    let offset: CGFloat = dir.y * sign > 0.5 ? 28 : 6
                    let p = CGPoint(x: center.x + dir.x * sign * (edge + offset), y: center.y + dir.y * sign * (edge + offset))
                    let endKey = sign > 0 ? PortKey(device: link.a.deviceId, port: link.a.port)
                                          : PortKey(device: link.b.deviceId, port: link.b.port)
                    let badges = services.filter { $0.deviceId == endKey.device && $0.port == endKey.port }
                    if !badges.isEmpty {
                        // Past the light, towards the middle of the cable.
                        var at = CGPoint(x: p.x + dir.x * sign * 18, y: p.y + dir.y * sign * 18)
                        for badge in badges {
                            Self.drawServiceBadge(badge, at: at, in: &context)
                            at = CGPoint(x: at.x + dir.x * sign * 16, y: at.y + dir.y * sign * 16)
                        }
                    }
                    if let stp {
                        let key = endKey
                        if let role = stp.roles[key] {
                            Self.drawRole(role, at: p, highlighted: stp.changed.contains(key), in: &context)
                            continue
                        }
                    }
                    if end?.errDisabled == true {
                        Self.drawLight(in: &context, at: p, state: .stopped)
                    } else if end?.blocked == true {
                        // Spanning tree: an amber bar across the cable at the blocked port.
                        let bar = CGPoint(x: -dir.y * 7, y: dir.x * 7)
                        let at = CGPoint(x: p.x + dir.x * sign * 9, y: p.y + dir.y * sign * 9)
                        var path = Path()
                        path.move(to: CGPoint(x: at.x - bar.x, y: at.y - bar.y))
                        path.addLine(to: CGPoint(x: at.x + bar.x, y: at.y + bar.y))
                        context.stroke(path, with: .color(Theme.Palette.statusWarning),
                                       style: StrokeStyle(lineWidth: 3, lineCap: .round))
                        Self.drawLight(in: &context, at: p, state: .suspended, color: Theme.Palette.statusWarning)
                    } else {
                        Self.drawLight(in: &context, at: p, state: state)
                    }
                }

                if isCaptured {
                    let mid = CGPoint(x: (a.x + b.x) / 2, y: (a.y + b.y) / 2)
                    let badge = context.resolve(Image(Icon.capture.rawValue))
                    context.fill(Path(ellipseIn: CGRect(x: mid.x - 10, y: mid.y - 10, width: 20, height: 20)), with: .color(Theme.Palette.surface))
                    context.stroke(Path(ellipseIn: CGRect(x: mid.x - 10, y: mid.y - 10, width: 20, height: 20)), with: .color(Theme.Palette.accent), lineWidth: 1.5)
                    context.draw(badge, in: CGRect(x: mid.x - 7, y: mid.y - 7, width: 14, height: 14))
                }

                // Semantic zoom: below 70 % the labels would be unreadably small.
                guard stp == nil, workspace.showsInterfaceLabels, workspace.zoom >= 0.7, length > edgeA + edgeB + 100 else { continue }
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

    /// Each medium has its own look so cables can be told apart at a glance:
    /// copper is a single line, fibre a double line. The user's colour and
    /// line pattern apply on top.
    static func drawCable(_ link: LinkInfo, from a: CGPoint, to b: CGPoint, direction dir: CGPoint,
                          isSelected: Bool, tint: Color? = nil, trunk: Bool = false,
                          in context: inout GraphicsContext) {
        var path = Path()
        path.move(to: a)
        path.addLine(to: b)
        if isSelected {
            context.stroke(path, with: .color(Theme.Palette.accent.opacity(0.25)), lineWidth: link.medium == .fiber ? 9 : 7)
        }
        // The user's own colour wins over the VLAN tint.
        let color = isSelected ? Theme.Palette.accent : (link.color == nil ? tint ?? link.displayedColor : link.displayedColor)
        let width: CGFloat = link.color == nil && tint == nil ? (isSelected ? 2 : 1.5) : 2
        if trunk {
            // A trunk carries many VLANs: a wide band with a light stripe down the middle.
            context.stroke(path, with: .color(color), style: StrokeStyle(lineWidth: 4.5, lineCap: .round))
            context.stroke(path, with: .color(Theme.Palette.canvas),
                           style: StrokeStyle(lineWidth: 1.4, lineCap: .round, dash: [5, 4]))
            return
        }
        switch link.medium {
        case .copper:
            let style = StrokeStyle(lineWidth: width, lineCap: .round, dash: link.displayedLine.dash(lineWidth: width))
            context.stroke(path, with: .color(color), style: style)
        case .fiber:
            // Two thin rails, like a duplex fibre pair.
            let normal = CGPoint(x: -dir.y * 1.75, y: dir.x * 1.75)
            let rail = width * 0.7
            let style = StrokeStyle(lineWidth: rail, lineCap: .round, dash: link.displayedLine.dash(lineWidth: rail))
            for sign in [1.0, -1.0] {
                var railPath = Path()
                railPath.move(to: CGPoint(x: a.x + normal.x * sign, y: a.y + normal.y * sign))
                railPath.addLine(to: CGPoint(x: b.x + normal.x * sign, y: b.y + normal.y * sign))
                context.stroke(railPath, with: .color(color), style: style)
            }
        }
    }

    /// A shield for an access list (in or out), an arrow for a DHCP relay,
    /// IN / OUT for the NAT side.
    static func drawServiceBadge(_ badge: ServiceBadge, at p: CGPoint, in context: inout GraphicsContext) {
        if badge.kind.hasPrefix("nat-") {
            let outside = badge.kind == "nat-outside"
            let rect = CGRect(x: p.x - 9, y: p.y - 9, width: 18, height: 18)
            context.fill(Path(ellipseIn: rect), with: .color(Theme.Palette.canvas))
            context.fill(Path(ellipseIn: rect.insetBy(dx: 1, dy: 1)),
                         with: .color(Color(light: 0x1F8A9E, dark: 0x3AAFC4)))
            let text = context.resolve(Text(outside ? "OUT" : "IN")
                .font(.system(size: outside ? 6 : 7, weight: .heavy))
                .foregroundStyle(.white))
            context.draw(text, at: p)
            return
        }
        let (symbol, color): (String, Color) = switch badge.kind {
        case "acl-in": ("shield.lefthalf.filled", Theme.Palette.statusWarning)
        case "acl-out": ("shield.righthalf.filled", Theme.Palette.statusWarning)
        default: ("arrow.uturn.right.circle.fill", Color(light: 0x2F9E5B, dark: 0x3DB46D))
        }
        let back = Path(ellipseIn: CGRect(x: p.x - 8, y: p.y - 8, width: 16, height: 16))
        context.fill(back, with: .color(Theme.Palette.canvas))
        var image = context.resolve(Image(systemName: symbol))
        image.shading = .color(color)
        context.draw(image, in: CGRect(x: p.x - 6.5, y: p.y - 6.5, width: 13, height: 13))
    }

    /// STP view: a lettered badge for the port's role (R, D, or a red ✕).
    static func drawRole(_ role: StpRole, at p: CGPoint, highlighted: Bool, in context: inout GraphicsContext) {
        let r: CGFloat = highlighted ? 10 : 8.5
        let circle = Path(ellipseIn: CGRect(x: p.x - r, y: p.y - r, width: r * 2, height: r * 2))
        if highlighted {
            context.fill(Path(ellipseIn: CGRect(x: p.x - r - 4, y: p.y - r - 4, width: (r + 4) * 2, height: (r + 4) * 2)),
                         with: .color(role.color.opacity(0.3)))
        }
        context.fill(circle, with: .color(role.color))
        context.stroke(circle, with: .color(Theme.Palette.canvas), lineWidth: 1.5)
        let text = context.resolve(Text(role.letter).font(.system(size: highlighted ? 11 : 9.5, weight: .bold)).foregroundColor(.white))
        context.draw(text, at: p)
    }

    /// Colour of a VLAN on the canvas: a stable palette colour per VLAN.
    static func vlanColor(_ vlan: UInt16) -> Color {
        let palette: [Swatch] = [.blue, .green, .orange, .purple, .teal, .pink, .yellow, .red]
        return palette[Int(vlan) % palette.count].color
    }

    struct PortKey: Hashable {
        let device: UInt64
        let port: String
    }

    /// Link light with shape as well as colour (see StatusGlyph).
    static func drawLight(in context: inout GraphicsContext, at p: CGPoint, state: WorkspaceModel.PowerState,
                          color: Color? = nil) {
        let outer = CGRect(x: p.x - 4, y: p.y - 4, width: 8, height: 8)
        let dot = Path(ellipseIn: outer.insetBy(dx: 0.75, dy: 0.75))
        context.fill(Path(ellipseIn: outer.insetBy(dx: -1, dy: -1)), with: .color(Theme.Palette.canvas))
        switch state {
        case .running:
            context.fill(dot, with: .color((color ?? state.color)))
        case .suspended:
            context.stroke(dot, with: .color((color ?? state.color)), lineWidth: 1.6)
            var half = context
            half.clip(to: Path(CGRect(x: outer.minX, y: outer.minY, width: outer.width / 2, height: outer.height)))
            half.fill(dot, with: .color((color ?? state.color)))
        case .stopped:
            context.stroke(dot, with: .color((color ?? state.color)), lineWidth: 1.8)
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
            guard let (a, b) = endpoints(of: link, among: links, positions: positions) else { return false }
            return point.distance(toSegment: a, b) <= tolerance
        }?.id
    }

    /// Where a cable is drawn. Several cables between the same two devices
    /// (redundant links, EtherChannel candidates) fan out side by side
    /// instead of lying on top of each other.
    static func endpoints(of link: LinkInfo, among links: [LinkInfo], positions: [UInt64: CGPoint]) -> (CGPoint, CGPoint)? {
        guard let a = positions[link.a.deviceId], let b = positions[link.b.deviceId] else { return nil }
        let pair = Set([link.a.deviceId, link.b.deviceId])
        let parallel = links.filter { Set([$0.a.deviceId, $0.b.deviceId]) == pair }
        guard parallel.count > 1, let index = parallel.firstIndex(where: { $0.id == link.id }) else { return (a, b) }
        // Offsets measured in a direction fixed by device order, so both ends agree.
        let (from, to) = link.a.deviceId < link.b.deviceId ? (a, b) : (b, a)
        let length = max(1, hypot(to.x - from.x, to.y - from.y))
        let normal = CGPoint(x: -(to.y - from.y) / length, y: (to.x - from.x) / length)
        let shift = (CGFloat(index) - CGFloat(parallel.count - 1) / 2) * 12
        let offset = CGPoint(x: normal.x * shift, y: normal.y * shift)
        return (CGPoint(x: a.x + offset.x, y: a.y + offset.y), CGPoint(x: b.x + offset.x, y: b.y + offset.y))
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
