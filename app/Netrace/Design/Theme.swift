import AppKit
import SwiftUI

/// netrace design tokens (see DESIGN.md). Views use these instead of literal
/// colours, sizes and fonts so light/dark themes stay in one place.
/// The interface is monotone: colour only carries meaning (accent, status,
/// protocol). Surfaces are opaque: no vibrancy, materials or Liquid Glass.
enum Theme {
    enum Palette {
        // Light mode stays off pure white and pure black: soft greys for
        // surfaces, charcoal for ink, so long sessions are easy on the eyes.
        static let canvas = Color(light: 0xF0F0F0, dark: 0x161616)
        static let canvasGrid = Color(light: 0xD5D5D5, dark: 0x2B2B2B)
        static let surface = Color(light: 0xF9F9F9, dark: 0x1E1E1E)
        static let surfaceRaised = Color(light: 0xF4F4F4, dark: 0x242424)
        static let surfaceSunken = Color(light: 0xEEEEEE, dark: 0x191919)
        static let hover = Color(light: 0xE9E9E9, dark: 0x2A2A2A)
        static let border = Color(light: 0xDFDFDF, dark: 0x2E2E2E)
        static let borderStrong = Color(light: 0xD4D4D4, dark: 0x3A3A3A)

        static let textPrimary = Color(light: 0x1C1C1C, dark: 0xEDEDED)
        static let textSecondary = Color(light: 0x5C5C5C, dark: 0xA3A3A3)
        /// ≥ 4.8:1 on every surface in both modes (UX-ANALYSIS.md).
        static let textTertiary = Color(light: 0x6B6B6B, dark: 0x8F8F8F)

        /// Device silhouettes are solid ink with detail knocked out in the canvas colour.
        static let symbolInk = Color(light: 0x3A3A3C, dark: 0xD9D9D9)
        static let symbolKnockout = Color(light: 0xF0F0F0, dark: 0x161616)
        /// Device artwork: a bright blue body with white linework in both
        /// appearances (a touch lighter in dark mode to hold its own on the canvas).
        static let deviceFill = Color(light: 0x3A86E6, dark: 0x468FEF)
        static let deviceLine = Color.white
        /// ≥ 3:1 against the canvas, as a meaningful graphic.
        static let link = Color(light: 0x7A7A7A, dark: 0x7A7A7A)

        /// Follows System Settings → Appearance → Accent colour.
        static let accent = Color.accentColor
        static let onAccent = Color.white

        static let statusUp = Color(light: 0x2F9E5B, dark: 0x3DB46D)
        /// The spanning-tree root bridge: a warm gold, deeper in light mode
        /// so it holds its contrast on the pale canvas.
        static let root = Color(light: 0xD9A400, dark: 0xFFD60A)
        static let onRoot = Color(light: 0x1F1A00, dark: 0x1F1A00)
        static let statusWarning = Color(light: 0xA87300, dark: 0xE5A50A)
        static let statusDown = Color(light: 0xD93F3F, dark: 0xEF5350)
    }

    enum Spacing {
        static let xxs: CGFloat = 2
        static let xs: CGFloat = 4
        static let s: CGFloat = 8
        static let m: CGFloat = 12
        static let l: CGFloat = 16
        static let xl: CGFloat = 24
    }

    enum Radius {
        static let s: CGFloat = 5
        static let m: CGFloat = 8
        static let l: CGFloat = 12
    }

    enum Size {
        static let sidebarWidth: CGFloat = 248
        static let inspectorWidth: CGFloat = 292
        static let statusBarHeight: CGFloat = 26
        static let bottomPanelDefault: CGFloat = 220
        static let gridSpacing: CGFloat = 24
        static let iconSmall: CGFloat = 14
        static let icon: CGFloat = 16
        static let rowHeight: CGFloat = 26
    }

    /// SF Pro (the system font) for interface text, SF Mono for interface
    /// names, addresses, CLI and packet fields.
    enum Font {
        static let title = SwiftUI.Font.system(size: 17, weight: .semibold)
        static let headline = SwiftUI.Font.system(size: 13, weight: .semibold)
        static let emptyTitle = SwiftUI.Font.system(size: 15, weight: .semibold)
        static let body = SwiftUI.Font.system(size: 13)
        static let label = SwiftUI.Font.system(size: 12, weight: .medium)
        static let labelSemibold = SwiftUI.Font.system(size: 12, weight: .semibold)
        static let caption = SwiftUI.Font.system(size: 11)
        static let captionMedium = SwiftUI.Font.system(size: 11, weight: .medium)
        /// Small uppercase section headers.
        static let sectionHeader = SwiftUI.Font.system(size: 10.5, weight: .semibold)
        static let sectionHeaderTracking: CGFloat = 0.5
        static let mono = SwiftUI.Font.system(size: 11.5, design: .monospaced)
        static let monoSmall = SwiftUI.Font.system(size: 10.5, design: .monospaced)
    }
}

/// Protocol identity colours, used only in simulation views (events,
/// capture, packets on the canvas).
enum NetProtocol: String, CaseIterable, Identifiable {
    case arp = "ARP"
    case icmp = "ICMP"
    case dhcp = "DHCP"
    case ospf = "OSPF"
    case stp = "STP"
    case cdp = "CDP"
    case tcp = "TCP"
    case udp = "UDP"
    case dns = "DNS"

    var id: Self { self }

    var color: Color {
        switch self {
        case .arp: Color(light: 0xD4930D, dark: 0xF0B429)
        case .icmp: Color(light: 0x1F8FCC, dark: 0x4CB3EB)
        case .dhcp: Color(light: 0x2F9E5B, dark: 0x3DB46D)
        case .ospf: Color(light: 0x7C5CD6, dark: 0x9D85F0)
        case .stp: Color(light: 0x5B6B7F, dark: 0x8696AA)
        case .cdp: Color(light: 0x7A7A7A, dark: 0x9A9A9A)
        case .tcp: Color(light: 0x2F6FDB, dark: 0x5B93F0)
        case .udp: Color(light: 0x14958A, dark: 0x2DBFB2)
        case .dns: Color(light: 0xC2417A, dark: 0xE06A9E)
        }
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
