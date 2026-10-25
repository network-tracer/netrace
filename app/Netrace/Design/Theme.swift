import AppKit
import SwiftUI

/// netrace design tokens. Views use these instead of literal colours and
/// sizes so light/dark themes and future restyling stay in one place.
/// Surfaces are opaque on purpose: no vibrancy, materials or Liquid Glass.
enum Theme {
    enum Palette {
        static let canvas = Color(light: 0xF6F7F9, dark: 0x15171B)
        static let canvasGrid = Color(light: 0xD5D9E0, dark: 0x2A2E35)
        static let surface = Color(light: 0xFFFFFF, dark: 0x1D2025)
        static let surfaceSunken = Color(light: 0xF0F2F5, dark: 0x191B1F)
        static let border = Color(light: 0xE2E5EA, dark: 0x2C3037)
        static let textPrimary = Color(light: 0x171A1F, dark: 0xE9ECF1)
        static let textSecondary = Color(light: 0x5F6773, dark: 0x9AA2AE)
        static let textTertiary = Color(light: 0x8C93A0, dark: 0x6B7380)
        static let accent = Color(light: 0x2F6BFF, dark: 0x5B8CFF)
        static let link = Color(light: 0x7A8494, dark: 0x5E6775)
        static let linkUp = Color(light: 0x1F9D55, dark: 0x34C072)
    }

    enum Spacing {
        static let xs: CGFloat = 4
        static let s: CGFloat = 8
        static let m: CGFloat = 12
        static let l: CGFloat = 16
        static let xl: CGFloat = 24
    }

    enum Radius {
        static let s: CGFloat = 6
        static let m: CGFloat = 10
    }

    enum Size {
        static let sidebarWidth: CGFloat = 232
        static let inspectorWidth: CGFloat = 272
        static let statusBarHeight: CGFloat = 26
        static let deviceTile: CGFloat = 56
        static let gridSpacing: CGFloat = 24
    }

    enum Font {
        static let sectionHeader = SwiftUI.Font.system(size: 11, weight: .semibold)
        static let body = SwiftUI.Font.system(size: 13)
        static let label = SwiftUI.Font.system(size: 12, weight: .medium)
        static let caption = SwiftUI.Font.system(size: 11)
        static let mono = SwiftUI.Font.system(size: 12, design: .monospaced)
    }
}

extension Color {
    /// A colour that resolves per appearance, from 0xRRGGBB values.
    init(light: UInt32, dark: UInt32) {
        self.init(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            return NSColor(rgb: isDark ? dark : light)
        })
    }
}

private extension NSColor {
    convenience init(rgb: UInt32) {
        self.init(
            srgbRed: CGFloat((rgb >> 16) & 0xFF) / 255,
            green: CGFloat((rgb >> 8) & 0xFF) / 255,
            blue: CGFloat(rgb & 0xFF) / 255,
            alpha: 1
        )
    }
}

extension DeviceKind {
    var displayName: String {
        switch self {
        case .router: "Router"
        case .l2Switch: "Switch"
        case .l3Switch: "Layer 3 Switch"
        case .pc: "PC"
        case .server: "Server"
        }
    }

    var summary: String {
        switch self {
        case .router: "4 × Gigabit Ethernet"
        case .l2Switch: "24 × Fast Ethernet, 2 × Gigabit"
        case .l3Switch: "24 × Gigabit, routing"
        case .pc: "1 × Fast Ethernet"
        case .server: "1 × Gigabit, services"
        }
    }

    /// Placeholder SF Symbols until the in-house device icon set exists.
    var symbol: String {
        switch self {
        case .router: "arrow.triangle.branch"
        case .l2Switch: "square.split.2x1"
        case .l3Switch: "square.stack.3d.up"
        case .pc: "desktopcomputer"
        case .server: "server.rack"
        }
    }

    var tint: Color {
        switch self {
        case .router: Color(light: 0x2F6BFF, dark: 0x5B8CFF)
        case .l2Switch: Color(light: 0x0E9384, dark: 0x2EC4B0)
        case .l3Switch: Color(light: 0x7A5AF8, dark: 0x9B85FF)
        case .pc: Color(light: 0x475467, dark: 0x98A2B3)
        case .server: Color(light: 0xDC6803, dark: 0xF79009)
        }
    }
}
