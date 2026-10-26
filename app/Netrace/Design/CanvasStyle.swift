import SwiftUI

// How the core's styling values look on screen: palette colours, line
// patterns and shape outlines. Shared by the canvas and the inspector.

extension Swatch: CaseIterable, Identifiable {
    public static var allCases: [Swatch] { [.gray, .blue, .teal, .green, .yellow, .orange, .red, .pink, .purple] }
    public var id: Self { self }

    /// Apple's system colours, with yellow darkened in light mode so it
    /// still reads on the light canvas.
    var color: Color {
        switch self {
        case .gray: Color(light: 0x8E8E93, dark: 0x98989D)
        case .blue: Color(light: 0x007AFF, dark: 0x0A84FF)
        case .teal: Color(light: 0x30B0C7, dark: 0x40C8E0)
        case .green: Color(light: 0x34C759, dark: 0x30D158)
        case .yellow: Color(light: 0xC99A00, dark: 0xFFD60A)
        case .orange: Color(light: 0xFF9500, dark: 0xFF9F0A)
        case .red: Color(light: 0xFF3B30, dark: 0xFF453A)
        case .pink: Color(light: 0xFF2D55, dark: 0xFF375F)
        case .purple: Color(light: 0xAF52DE, dark: 0xBF5AF2)
        }
    }

    var title: String {
        switch self {
        case .gray: "Gray"
        case .blue: "Blue"
        case .teal: "Teal"
        case .green: "Green"
        case .yellow: "Yellow"
        case .orange: "Orange"
        case .red: "Red"
        case .pink: "Pink"
        case .purple: "Purple"
        }
    }
}

extension LinePattern: CaseIterable, Identifiable {
    public static var allCases: [LinePattern] { [.solid, .dashed, .dotted] }
    public var id: Self { self }

    var title: String {
        switch self {
        case .solid: "Solid"
        case .dashed: "Dashed"
        case .dotted: "Dotted"
        }
    }

    /// Dash lengths for a line of the given width.
    func dash(lineWidth: CGFloat) -> [CGFloat] {
        switch self {
        case .solid: []
        case .dashed: [6 * max(1, lineWidth / 1.5), 4 * max(1, lineWidth / 1.5)]
        case .dotted: [0.01, 3.5 * max(1, lineWidth / 1.5)]
        }
    }
}

extension ShapeKind {
    /// The shapes offered in the Shape tool and inspector. The polygon
    /// entry stands for any number of sides.
    static let choices: [ShapeKind] = [.roundedRectangle, .rectangle, .ellipse, .polygon(sides: 6)]

    var title: String {
        switch self {
        case .rectangle: "Rectangle"
        case .roundedRectangle: "Rounded Rectangle"
        case .ellipse: "Ellipse"
        case .polygon(let sides): Self.polygonName(sides)
        }
    }

    var symbol: String {
        switch self {
        case .rectangle: "rectangle"
        case .roundedRectangle: "app"
        case .ellipse: "circle"
        case .polygon(let sides):
            switch sides {
            case 3: "triangle"
            case 5: "pentagon"
            case 6: "hexagon"
            case 8: "octagon"
            default: "hexagon"
            }
        }
    }

    var isPolygon: Bool {
        if case .polygon = self { return true }
        return false
    }

    /// Same kind of shape, ignoring the number of sides.
    func isSameKind(as other: ShapeKind) -> Bool {
        switch (self, other) {
        case (.polygon, .polygon): true
        default: self == other
        }
    }

    static func polygonName(_ sides: UInt8) -> String {
        switch sides {
        case 3: "Triangle"
        case 4: "Diamond"
        case 5: "Pentagon"
        case 6: "Hexagon"
        case 7: "Heptagon"
        case 8: "Octagon"
        default: "\(sides)-sided Polygon"
        }
    }

    static let sidesRange: ClosedRange<UInt8> = 3...12

    /// The outline fitted to `rect`.
    func path(in rect: CGRect) -> Path {
        switch self {
        case .rectangle:
            Path(rect)
        case .roundedRectangle:
            Path(roundedRect: rect, cornerRadius: min(Theme.Radius.l, rect.width / 4, rect.height / 4), style: .continuous)
        case .ellipse:
            Path(ellipseIn: rect)
        case .polygon(let sides):
            Self.polygon(sides: Int(sides), in: rect)
        }
    }

    /// A regular polygon with its first corner at the top, stretched to fill `rect`.
    private static func polygon(sides: Int, in rect: CGRect) -> Path {
        let n = max(3, sides)
        let points = (0..<n).map { i -> CGPoint in
            let angle = -CGFloat.pi / 2 + CGFloat(i) * 2 * .pi / CGFloat(n)
            return CGPoint(x: cos(angle), y: sin(angle))
        }
        // Scale the unit polygon's bounding box to the rectangle so it fills it.
        let minX = points.map(\.x).min()!, maxX = points.map(\.x).max()!
        let minY = points.map(\.y).min()!, maxY = points.map(\.y).max()!
        var path = Path()
        for (i, p) in points.enumerated() {
            let fitted = CGPoint(x: rect.minX + (p.x - minX) / (maxX - minX) * rect.width,
                                 y: rect.minY + (p.y - minY) / (maxY - minY) * rect.height)
            if i == 0 { path.move(to: fitted) } else { path.addLine(to: fitted) }
        }
        path.closeSubpath()
        return path
    }

    /// Rectangles carry their label inside the top-left corner. Round and
    /// pointed shapes have no corner to spare, so theirs sits centred just
    /// above the outline, where devices inside can't cover it.
    var labelsAbove: Bool {
        switch self {
        case .rectangle, .roundedRectangle: false
        case .ellipse, .polygon: true
        }
    }
}

/// An annotation's outline as a SwiftUI shape.
struct AnnotationOutline: Shape {
    let kind: ShapeKind

    func path(in rect: CGRect) -> Path { kind.path(in: rect) }
}

extension ShapeStyle {
    /// Border colour, falling back to the neutral default.
    var strokeColor: Color { stroke?.color ?? Theme.Palette.textTertiary }

    /// Fill colour at its opacity. Without a colour, a faint neutral wash.
    var fillColor: Color {
        (fill?.color ?? Theme.Palette.textPrimary).opacity(fill == nil ? min(fillOpacity, 0.6) : fillOpacity)
    }
}

extension LinkInfo {
    /// The cable's line pattern: the user's choice, else solid.
    var displayedLine: LinePattern { line ?? .solid }
    var displayedColor: Color { color?.color ?? Theme.Palette.link }
}
