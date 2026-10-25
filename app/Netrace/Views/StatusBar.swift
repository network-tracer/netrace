import SwiftUI

struct StatusBar: View {
    @ObservedObject var document: ProjectDocument
    private let version = coreVersion()

    var body: some View {
        HStack(spacing: Theme.Spacing.m) {
            Text("\(document.state.devices.count) devices")
            Text("·")
            Text("\(document.state.links.count) links")
            Spacer()
            Text("netrace core \(version)")
        }
        .font(Theme.Font.caption)
        .foregroundStyle(Theme.Palette.textSecondary)
        .monospacedDigit()
        .padding(.horizontal, Theme.Spacing.m)
        .frame(maxHeight: .infinity)
        .background(Theme.Palette.surfaceSunken)
    }
}
