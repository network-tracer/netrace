import SwiftUI

/// netrace's own icons for things SF Symbols doesn't cover: 24 pt grid,
/// 1.5 pt round strokes, stored as template SVGs in Assets.xcassets/Icons so
/// they tint with foregroundStyle. Standard actions use SF Symbols.
enum Icon: String, CaseIterable {
    case cableAuto = "cable.auto"
    case cableCopper = "cable.copper"
    case cableFiber = "cable.fiber"
    case cableConsole = "cable.console"
    case packet
    case pdu
    case capture
    case lab
    case port
    case topology
    case canvasEmpty = "canvas.empty"
}

/// Either one of netrace's icons or an SF Symbol, so controls can take both.
enum Glyph: Hashable {
    case custom(Icon)
    case system(String)
}

extension Image {
    init(_ icon: Icon) {
        self.init(icon.rawValue)
    }
}

/// An icon at a given point size, tinted by the surrounding foregroundStyle.
struct IconView: View {
    let glyph: Glyph
    var size: CGFloat

    init(_ icon: Icon, size: CGFloat = Theme.Size.icon) {
        self.glyph = .custom(icon)
        self.size = size
    }

    init(_ glyph: Glyph, size: CGFloat = Theme.Size.icon) {
        self.glyph = glyph
        self.size = size
    }

    init(system name: String, size: CGFloat = Theme.Size.icon) {
        self.glyph = .system(name)
        self.size = size
    }

    var body: some View {
        Group {
            switch glyph {
            case .custom(let icon):
                Image(icon)
                    .renderingMode(.template)
                    .resizable()
                    .interpolation(.high)
                    .frame(width: size, height: size)
            case .system(let name):
                Image(systemName: name)
                    .font(.system(size: size * 0.82, weight: .regular))
                    .frame(width: size, height: size)
            }
        }
        .accessibilityHidden(true)
    }
}
