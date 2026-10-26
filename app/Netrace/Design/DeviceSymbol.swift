import SwiftUI

/// Topology symbols. Each device kind is drawn as its own silhouette: the
/// outline is both the visible container and the hit/selection shape, so a
/// router reads as round, a switch as wide and flat, a PC as a monitor.
///
/// Geometry is described in unit coordinates (0…1 of the symbol's frame),
/// with radii in points relative to the frame height so curves stay round
/// at any aspect ratio.
enum DeviceSymbol {
    /// Natural size on the canvas at 100 % zoom.
    static func size(for kind: DeviceKind) -> CGSize {
        switch kind {
        case .router: CGSize(width: 46, height: 46)
        case .l2Switch: CGSize(width: 68, height: 30)
        case .l3Switch: CGSize(width: 68, height: 40)
        case .hub: CGSize(width: 58, height: 26)
        case .firewall: CGSize(width: 50, height: 40)
        case .accessPoint: CGSize(width: 44, height: 44)
        case .cloud: CGSize(width: 70, height: 44)
        case .pc: CGSize(width: 50, height: 44)
        case .laptop: CGSize(width: 60, height: 40)
        case .server: CGSize(width: 38, height: 50)
        case .printer: CGSize(width: 50, height: 42)
        case .ipPhone: CGSize(width: 52, height: 42)
        case .smartphone: CGSize(width: 28, height: 48)
        }
    }

    /// Silhouette: filled with the symbol colour, stroked as the outline.
    static func outline(_ kind: DeviceKind, in r: CGRect) -> Path {
        let u = Unit(r)
        switch kind {
        case .router:
            return Path(ellipseIn: r)
        case .l2Switch, .hub:
            return Path(roundedRect: r, cornerRadius: r.height * 0.22, style: .continuous)
        case .l3Switch:
            let back = u.rect(0.10, 0, 0.90, 0.64)
            let front = u.rect(0, 0.26, 0.90, 0.74)
            return Path(roundedRect: back, cornerRadius: r.height * 0.18, style: .continuous)
                .union(Path(roundedRect: front, cornerRadius: r.height * 0.18, style: .continuous))
        case .firewall:
            return Path(roundedRect: r, cornerRadius: r.height * 0.14, style: .continuous)
        case .accessPoint:
            return Path(roundedRect: r, cornerRadius: r.height * 0.32, style: .continuous)
        case .cloud:
            let h = r.height
            var cloud = Path(roundedRect: u.rect(0.08, 0.50, 0.84, 0.50), cornerRadius: h * 0.25, style: .continuous)
            for (cx, cy, radius) in [(0.30, 0.56, 0.30), (0.54, 0.40, 0.40), (0.76, 0.58, 0.28)] {
                let c = u.point(cx, cy)
                cloud = cloud.union(Path(ellipseIn: CGRect(x: c.x - h * radius, y: c.y - h * radius,
                                                           width: h * radius * 2, height: h * radius * 2)))
            }
            return cloud
        case .pc:
            let monitor = Path(roundedRect: u.rect(0, 0, 1, 0.74), cornerRadius: r.height * 0.11, style: .continuous)
            let neck = Path(u.rect(0.43, 0.72, 0.14, 0.16))
            let base = Path(roundedRect: u.rect(0.26, 0.86, 0.48, 0.14), cornerRadius: r.height * 0.06, style: .continuous)
            return monitor.union(neck).union(base)
        case .laptop:
            let lid = Path(roundedRect: u.rect(0.11, 0, 0.78, 0.80), cornerRadius: r.height * 0.10, style: .continuous)
            let base = Path(roundedRect: u.rect(0, 0.80, 1, 0.20), cornerRadius: r.height * 0.09, style: .continuous)
            return lid.union(base)
        case .server:
            return Path(roundedRect: r, cornerRadius: r.width * 0.16, style: .continuous)
        case .printer:
            let paper = Path(roundedRect: u.rect(0.22, 0, 0.56, 0.34), cornerRadius: r.height * 0.05, style: .continuous)
            let body = Path(roundedRect: u.rect(0, 0.26, 1, 0.50), cornerRadius: r.height * 0.13, style: .continuous)
            let tray = Path(roundedRect: u.rect(0.16, 0.70, 0.68, 0.30), cornerRadius: r.height * 0.06, style: .continuous)
            return paper.union(body).union(tray)
        case .ipPhone:
            // Desk phone: handset (with ear and mouth pieces) resting on the base.
            let h = r.height
            var handset = Path(roundedRect: u.rect(0.08, 0.02, 0.84, 0.20), cornerRadius: h * 0.10, style: .continuous)
            for x in [0.15, 0.85] {
                let c = u.point(x, 0.16)
                handset = handset.union(Path(ellipseIn: CGRect(x: c.x - h * 0.15, y: c.y - h * 0.15,
                                                               width: h * 0.30, height: h * 0.30)))
            }
            let neck = Path(u.rect(0.34, 0.18, 0.32, 0.22))
            let base = Path(roundedRect: u.rect(0.02, 0.36, 0.96, 0.64), cornerRadius: h * 0.14, style: .continuous)
            return handset.union(neck).union(base)
        case .smartphone:
            return Path(roundedRect: r, cornerRadius: r.width * 0.26, style: .continuous)
        }
    }

    /// Interior line work, stroked with the outline colour.
    static func detail(_ kind: DeviceKind, in r: CGRect) -> Path {
        let u = Unit(r)
        var p = Path()
        switch kind {
        case .router:
            // Two opposing arrows: traffic in, traffic out.
            p.move(to: u.point(0.27, 0.40)); p.addLine(to: u.point(0.73, 0.40))
            p.move(to: u.point(0.61, 0.29)); p.addLine(to: u.point(0.73, 0.40)); p.addLine(to: u.point(0.61, 0.51))
            p.move(to: u.point(0.73, 0.60)); p.addLine(to: u.point(0.27, 0.60))
            p.move(to: u.point(0.39, 0.49)); p.addLine(to: u.point(0.27, 0.60)); p.addLine(to: u.point(0.39, 0.71))
        case .l3Switch:
            // Front body edge over the back plate shows the layering.
            p.addRoundedRect(in: u.rect(0, 0.26, 0.90, 0.74), cornerSize: CGSize(width: r.height * 0.18, height: r.height * 0.18), style: .continuous)
            // Routing arrow on the back plate.
            p.move(to: u.point(0.52, 0.13)); p.addLine(to: u.point(0.78, 0.13))
            p.move(to: u.point(0.72, 0.06)); p.addLine(to: u.point(0.78, 0.13)); p.addLine(to: u.point(0.72, 0.20))
        case .hub:
            // Fan-out: one input repeated to every port.
            p.move(to: u.point(0.62, 0.50)); p.addLine(to: u.point(0.72, 0.50))
            for y in [0.30, 0.50, 0.70] {
                p.move(to: u.point(0.72, 0.50)); p.addLine(to: u.point(0.86, y))
            }
        case .firewall:
            // Brick wall.
            for y in [0.36, 0.64] {
                p.move(to: u.point(0, y)); p.addLine(to: u.point(1, y))
            }
            for (x, y0, y1) in [(0.5, 0.0, 0.36), (0.28, 0.36, 0.64), (0.72, 0.36, 0.64), (0.5, 0.64, 1.0)] {
                p.move(to: u.point(x, y0)); p.addLine(to: u.point(x, y1))
            }
        case .accessPoint:
            // Wireless arcs above a dot.
            let c = u.point(0.5, 0.70)
            for radius in [0.16, 0.30, 0.44].map({ r.height * $0 }) {
                // Move to the arc's start first so arcs don't join up.
                let start = Angle.degrees(-135).radians
                p.move(to: CGPoint(x: c.x + radius * cos(start), y: c.y + radius * sin(start)))
                p.addArc(center: c, radius: radius, startAngle: .degrees(-135), endAngle: .degrees(-45), clockwise: false)
            }
        case .server:
            for y in [0.34, 0.67] {
                p.move(to: u.point(0, y)); p.addLine(to: u.point(1, y))
            }
            for y in [0.17, 0.50, 0.83] {
                p.move(to: u.point(0.42, y)); p.addLine(to: u.point(0.78, y))
            }
        case .printer:
            p.move(to: u.point(0.16, 0.62)); p.addLine(to: u.point(0.84, 0.62))
        case .ipPhone:
            // Gap where the handset rests on the cradle, and the display.
            p.move(to: u.point(0.30, 0.31)); p.addLine(to: u.point(0.70, 0.31))
            p.addRoundedRect(in: u.rect(0.12, 0.50, 0.34, 0.30), cornerSize: CGSize(width: 2, height: 2))
        case .smartphone:
            p.move(to: u.point(0.38, 0.09)); p.addLine(to: u.point(0.62, 0.09))
            p.move(to: u.point(0.36, 0.91)); p.addLine(to: u.point(0.64, 0.91))
        case .l2Switch, .cloud, .pc, .laptop:
            break
        }
        return p
    }

    /// Small solid elements (ports, LEDs, keys), filled with the outline colour.
    static func solids(_ kind: DeviceKind, in r: CGRect) -> Path {
        let u = Unit(r)
        var p = Path()
        func ports(count: Int, in area: CGRect) {
            let gap = area.width * 0.35 / CGFloat(count - 1)
            let w = (area.width - gap * CGFloat(count - 1)) / CGFloat(count)
            for i in 0..<count {
                let x = area.minX + CGFloat(i) * (w + gap)
                p.addRoundedRect(in: CGRect(x: x, y: area.minY, width: w, height: area.height),
                                 cornerSize: CGSize(width: 1, height: 1))
            }
        }
        func dot(_ x: CGFloat, _ y: CGFloat, _ radius: CGFloat) {
            let c = u.point(x, y)
            p.addEllipse(in: CGRect(x: c.x - radius, y: c.y - radius, width: radius * 2, height: radius * 2))
        }
        switch kind {
        case .l2Switch:
            ports(count: 6, in: u.rect(0.11, 0.38, 0.58, 0.24))
            dot(0.84, 0.5, r.height * 0.07)
        case .l3Switch:
            ports(count: 5, in: u.rect(0.10, 0.56, 0.52, 0.20))
            dot(0.76, 0.66, r.height * 0.06)
        case .hub:
            ports(count: 4, in: u.rect(0.12, 0.38, 0.38, 0.24))
        case .accessPoint:
            dot(0.5, 0.70, r.height * 0.06)
        case .server:
            for y in [0.17, 0.50, 0.83] { dot(0.24, y, r.width * 0.055) }
        case .printer:
            dot(0.84, 0.44, r.height * 0.045)
        case .ipPhone:
            for row in 0..<3 {
                for col in 0..<3 {
                    dot(0.60 + CGFloat(col) * 0.12, 0.52 + CGFloat(row) * 0.14, r.height * 0.04)
                }
            }
        default:
            break
        }
        return p
    }

    /// Faint inner shapes (screens), drawn at reduced opacity.
    static func faint(_ kind: DeviceKind, in r: CGRect) -> Path {
        let u = Unit(r)
        switch kind {
        case .pc:
            return Path(roundedRect: u.rect(0.10, 0.11, 0.80, 0.52), cornerRadius: r.height * 0.05)
        case .laptop:
            return Path(roundedRect: u.rect(0.19, 0.11, 0.62, 0.58), cornerRadius: r.height * 0.05)
        case .smartphone:
            return Path(roundedRect: u.rect(0.14, 0.17, 0.72, 0.66), cornerRadius: r.width * 0.08)
        default:
            return Path()
        }
    }
}

/// Maps unit coordinates onto a rect.
private struct Unit {
    let r: CGRect
    init(_ r: CGRect) { self.r = r }

    func point(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
        CGPoint(x: r.minX + x * r.width, y: r.minY + y * r.height)
    }

    func rect(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat) -> CGRect {
        CGRect(x: r.minX + x * r.width, y: r.minY + y * r.height, width: w * r.width, height: h * r.height)
    }
}

// MARK: - Shapes and view

struct DeviceOutlineShape: Shape {
    let kind: DeviceKind
    func path(in rect: CGRect) -> Path { DeviceSymbol.outline(kind, in: rect) }
}

private struct DeviceDetailShape: Shape {
    let kind: DeviceKind
    func path(in rect: CGRect) -> Path { DeviceSymbol.detail(kind, in: rect) }
}

private struct DeviceSolidsShape: Shape {
    let kind: DeviceKind
    func path(in rect: CGRect) -> Path { DeviceSymbol.solids(kind, in: rect) }
}

private struct DeviceFaintShape: Shape {
    let kind: DeviceKind
    func path(in rect: CGRect) -> Path { DeviceSymbol.faint(kind, in: rect) }
}

/// A device silhouette at a given scale (1 = canvas size at 100 %).
struct DeviceSymbolView: View {
    let kind: DeviceKind
    var scale: CGFloat = 1
    var isSelected = false
    var isDimmed = false
    /// The user's colour for this device; `nil` is the default ink.
    var tint: Swatch?

    var body: some View {
        let size = DeviceSymbol.size(for: kind)
        let line = max(1, 1.5 * min(1, scale + 0.25))
        // Solid style, like SF Symbols' filled variants: the silhouette is
        // filled with the ink colour and interior detail is knocked out of it.
        ZStack {
            DeviceOutlineShape(kind: kind).fill(tint?.color ?? Theme.Palette.symbolInk)
            DeviceFaintShape(kind: kind).fill(Theme.Palette.symbolKnockout.opacity(0.22))
            DeviceDetailShape(kind: kind)
                .stroke(Theme.Palette.symbolKnockout, style: StrokeStyle(lineWidth: line, lineCap: .round, lineJoin: .round))
            DeviceSolidsShape(kind: kind).fill(Theme.Palette.symbolKnockout)
        }
        .frame(width: size.width * scale, height: size.height * scale)
        .opacity(isDimmed ? 0.4 : 1)
        .background {
            if isSelected {
                // A solid band hugging the silhouette with no gap: a stroke
                // centred on the outline, whose inner half the symbol covers.
                DeviceOutlineShape(kind: kind)
                    .stroke(Theme.Palette.accent, style: StrokeStyle(lineWidth: 8, lineJoin: .round))
            }
        }
        .contentShape(DeviceOutlineShape(kind: kind))
    }
}
