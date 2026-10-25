import AppKit
import SwiftUI

/// The user's light/dark preference, applied app-wide through NSApp.appearance
/// (which, unlike preferredColorScheme(nil), reliably returns to "System").
enum AppearanceSetting: String, CaseIterable, Identifiable {
    case system, light, dark

    static let defaultsKey = "appearance"

    var id: Self { self }

    var title: String {
        switch self {
        case .system: "System"
        case .light: "Light"
        case .dark: "Dark"
        }
    }

    @MainActor
    func apply() {
        NSApp.appearance = switch self {
        case .system: nil
        case .light: NSAppearance(named: .aqua)
        case .dark: NSAppearance(named: .darkAqua)
        }
    }
}

struct AppearanceCommands: Commands {
    @AppStorage(AppearanceSetting.defaultsKey) private var appearance = AppearanceSetting.system

    var body: some Commands {
        CommandGroup(after: .sidebar) {
            // Applied in the setter: onChange doesn't fire inside menu commands.
            Picker("Appearance", selection: Binding(
                get: { appearance },
                set: { new in
                    appearance = new
                    new.apply()
                }
            )) {
                ForEach(AppearanceSetting.allCases) { Text($0.title).tag($0) }
            }
        }
    }
}
