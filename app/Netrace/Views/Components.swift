import AppKit
import SwiftUI

/// Small uppercase label above groups in panels, with optional trailing content.
struct SectionHeader<Trailing: View>: View {
    let title: String
    let trailing: Trailing

    init(_ title: String, @ViewBuilder trailing: () -> Trailing = { EmptyView() }) {
        self.title = title
        self.trailing = trailing()
    }

    var body: some View {
        HStack(spacing: Theme.Spacing.s) {
            Text(title.uppercased())
                .font(Theme.Font.sectionHeader)
                .tracking(Theme.Font.sectionHeaderTracking)
                .foregroundStyle(Theme.Palette.textSecondary)
            Spacer(minLength: 0)
            trailing
        }
    }
}

/// 1 pt opaque separator in the border colour.
struct PanelDivider: View {
    var axis: Axis = .vertical

    var body: some View {
        Rectangle()
            .fill(Theme.Palette.border)
            .frame(width: axis == .vertical ? 1 : nil, height: axis == .horizontal ? 1 : nil)
    }
}

/// Borderless icon button with a hover wash; shows an "on" state for toggles.
struct IconButton: View {
    let glyph: Glyph
    let help: String
    var size: CGFloat = Theme.Size.icon
    var isOn = false
    var isEnabled = true
    let action: () -> Void

    @State private var isHovered = false

    var body: some View {
        Button(action: action) {
            IconView(glyph, size: size)
                .foregroundStyle(foreground)
                .frame(width: size + 10, height: size + 10)
                .background(
                    RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
                        .fill(isOn ? Theme.Palette.accent.opacity(0.14) : (isHovered ? Theme.Palette.hover : .clear))
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!isEnabled)
        .onHover { isHovered = $0 && isEnabled }
        .help(help)
        .accessibilityLabel(help)
    }

    private var foreground: Color {
        if !isEnabled { return Theme.Palette.textTertiary.opacity(0.6) }
        return isOn ? Theme.Palette.accent : Theme.Palette.textSecondary
    }
}

/// Underlined text tabs used by the sidebar, inspector and bottom panel.
struct TabStrip<Tab: Hashable & Identifiable>: View {
    let tabs: [Tab]
    @Binding var selection: Tab
    let title: (Tab) -> String
    var glyph: ((Tab) -> Glyph)? = nil
    /// Unselected tabs show only their icon (named in a tooltip), for a
    /// narrow strip with many tabs.
    var compact = false

    var body: some View {
        HStack(spacing: compact ? Theme.Spacing.m : Theme.Spacing.l) {
            ForEach(tabs) { tab in
                let isSelected = tab == selection
                Button {
                    selection = tab
                } label: {
                    HStack(spacing: Theme.Spacing.xs + 1) {
                        if let glyph {
                            IconView(glyph(tab), size: 13)
                        }
                        if !compact || isSelected || glyph == nil {
                            Text(title(tab))
                                .font(isSelected ? Theme.Font.captionMedium : Theme.Font.caption)
                                .lineLimit(1)
                                .fixedSize()
                        }
                    }
                    .foregroundStyle(isSelected ? Theme.Palette.textPrimary : Theme.Palette.textSecondary)
                    .padding(.vertical, Theme.Spacing.s)
                    .overlay(alignment: .bottom) {
                        Rectangle()
                            .fill(isSelected ? Theme.Palette.textPrimary : .clear)
                            .frame(height: 1.5)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(title(tab))
                .accessibilityLabel(title(tab))
                .accessibilityAddTraits(isSelected ? .isSelected : [])
            }
        }
    }
}

/// Compact search field in panel style.
struct SearchField: View {
    let prompt: String
    @Binding var text: String

    var body: some View {
        HStack(spacing: Theme.Spacing.xs + 2) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 11))
                .foregroundStyle(Theme.Palette.textTertiary)
            TextField(prompt, text: $text)
                .textFieldStyle(.plain)
                .font(Theme.Font.body)
            if !text.isEmpty {
                Button {
                    text = ""
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(Theme.Palette.textTertiary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear search")
            }
        }
        .padding(.horizontal, Theme.Spacing.s)
        .frame(height: 26)
        .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous).strokeBorder(Theme.Palette.border))
    }
}

/// Protocol label with its identity swatch.
struct ProtocolTag: View {
    let proto: NetProtocol

    var body: some View {
        HStack(spacing: Theme.Spacing.xs + 1) {
            RoundedRectangle(cornerRadius: 1.5)
                .fill(proto.color)
                .frame(width: 7, height: 7)
            Text(proto.rawValue)
                .font(Theme.Font.monoSmall)
                .foregroundStyle(Theme.Palette.textPrimary)
        }
    }
}

/// "ROOT" in a small gold rounded rectangle: the spanning-tree root bridge.
struct RootBadge: View {
    var body: some View {
        Text("ROOT")
            .font(.system(size: 7.5, weight: .heavy))
            .tracking(0.4)
            .foregroundStyle(Theme.Palette.onRoot)
            .padding(.horizontal, 4)
            .padding(.vertical, 1.5)
            .background(RoundedRectangle(cornerRadius: 3, style: .continuous).fill(Theme.Palette.root))
            .overlay(RoundedRectangle(cornerRadius: 3, style: .continuous).strokeBorder(Theme.Palette.canvas, lineWidth: 1))
            .accessibilityLabel("Root bridge")
    }
}

/// A protocol tag that shows or hides that protocol's events (in the list
/// and on the canvas). Option-click shows only that protocol.
struct ProtocolFilterTag: View {
    let proto: NetProtocol
    @ObservedObject var workspace: WorkspaceModel
    @State private var hovering = false

    var body: some View {
        let on = workspace.visibleProtocols.contains(proto)
        Button {
            if NSEvent.modifierFlags.contains(.option) {
                workspace.visibleProtocols = [proto]
            } else if on {
                workspace.visibleProtocols.remove(proto)
            } else {
                workspace.visibleProtocols.insert(proto)
            }
        } label: {
            HStack(spacing: Theme.Spacing.xs + 1) {
                RoundedRectangle(cornerRadius: 1.5)
                    .fill(on ? proto.color : .clear)
                    .strokeBorder(proto.color, lineWidth: on ? 0 : 1)
                    .frame(width: 7, height: 7)
                Text(proto.rawValue)
                    .font(Theme.Font.monoSmall)
                    .foregroundStyle(on ? Theme.Palette.textPrimary : Theme.Palette.textTertiary)
                    .strikethrough(!on, color: Theme.Palette.textTertiary)
            }
            .padding(.horizontal, 6)
            .padding(.vertical, 3)
            .background(hovering ? Theme.Palette.hover : .clear, in: RoundedRectangle(cornerRadius: Theme.Radius.s))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .help(on ? "Hide \(proto.rawValue) events (⌥-click to show only \(proto.rawValue))"
                 : "Show \(proto.rawValue) events")
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}

/// Power / link state drawn with shape as well as colour, so it reads
/// without colour vision: filled = up, half = suspended, ring = down.
struct StatusGlyph: View {
    let state: WorkspaceModel.PowerState
    var size: CGFloat = 7

    var body: some View {
        let ring = max(1.5, size * 0.24)
        ZStack {
            switch state {
            case .running:
                Circle().fill(state.color)
            case .suspended:
                Circle().strokeBorder(state.color, lineWidth: ring)
                Circle().fill(state.color).mask(alignment: .leading) {
                    Rectangle().frame(width: size / 2)
                }
            case .stopped:
                Circle().strokeBorder(state.color, lineWidth: ring)
            }
        }
        .frame(width: size, height: size)
        .accessibilityLabel(state.title)
    }
}

struct StatusDot: View {
    let color: Color
    var size: CGFloat = 7

    var body: some View {
        Circle().fill(color).frame(width: size, height: size)
    }
}

/// Marks panels showing placeholder content until their milestone ships.
struct PreviewBadge: View {
    let milestone: String

    var body: some View {
        Text("Preview · \(milestone)")
            .font(Theme.Font.captionMedium)
            .foregroundStyle(Theme.Palette.textSecondary)
            .padding(.horizontal, Theme.Spacing.s - 1)
            .padding(.vertical, Theme.Spacing.xxs)
            .background(Capsule().fill(Theme.Palette.hover))
            .overlay(Capsule().strokeBorder(Theme.Palette.border))
            .help("Placeholder content. This panel becomes live in milestone \(milestone).")
    }
}

struct KeyValueRow: View {
    let label: String
    let value: String
    var mono = false

    init(_ label: String, _ value: String, mono: Bool = false) {
        self.label = label
        self.value = value
        self.mono = mono
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(label)
                .font(Theme.Font.body)
                .foregroundStyle(Theme.Palette.textSecondary)
            Spacer(minLength: Theme.Spacing.s)
            Text(value)
                .font(mono ? Theme.Font.mono : Theme.Font.body)
                .foregroundStyle(Theme.Palette.textPrimary)
                .monospacedDigit()
                .lineLimit(1)
                .textSelection(.enabled)
        }
    }
}

struct EmptyState: View {
    let glyph: Glyph
    let title: String
    let message: String

    var body: some View {
        VStack(spacing: Theme.Spacing.s) {
            IconView(glyph, size: 28)
                .foregroundStyle(Theme.Palette.textTertiary)
            Text(title)
                .font(Theme.Font.headline)
                .foregroundStyle(Theme.Palette.textPrimary)
            Text(message)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.Palette.textSecondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 280)
        }
        .padding(Theme.Spacing.l)
    }
}

/// A floating control container on the canvas (zoom, minimap, timeline).
struct FloatingPanel<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        content
            .background(Theme.Palette.surfaceRaised, in: RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).strokeBorder(Theme.Palette.border))
            .shadow(color: .black.opacity(0.08), radius: 6, y: 2)
    }
}

extension String {
    /// Cisco-style short interface name: "GigabitEthernet0/1" → "Gi0/1".
    var abbreviatedInterface: String {
        for (long, short) in [("GigabitEthernet", "Gi"), ("FastEthernet", "Fa"), ("Ethernet", "Eth")] where hasPrefix(long) {
            return short + dropFirst(long.count)
        }
        return self
    }
}

extension Double {
    /// Simulation clock text, e.g. 00:00:01.248.
    var simulationClock: String {
        let total = max(0, self)
        let minutes = Int(total) / 60
        let seconds = total - Double(minutes * 60)
        return String(format: "00:%02d:%06.3f", minutes, seconds)
    }
}

/// A row that reveals its explanation when clicked, with a chevron that
/// turns. Explanations sit indented under the row in secondary text.
struct ExplainRow<Header: View, Detail: View>: View {
    let expanded: Bool
    let action: () -> Void
    @ViewBuilder let header: Header
    @ViewBuilder let detail: Detail
    @State private var hovering = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
            HStack(spacing: Theme.Spacing.xs) {
                header
                Image(systemName: "chevron.right")
                    .font(.system(size: 8, weight: .semibold))
                    .foregroundStyle(Theme.Palette.textTertiary)
                    .rotationEffect(.degrees(expanded ? 90 : 0))
                    .opacity(hovering || expanded ? 1 : 0.5)
                    .frame(width: 12)
            }
            if expanded {
                detail
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.Palette.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                    .padding(.leading, Theme.Spacing.s)
                    .overlay(alignment: .leading) {
                        Rectangle().fill(Theme.Palette.border).frame(width: 2)
                    }
                    .transition(.opacity)
            }
        }
        .padding(.vertical, 5)
        .padding(.horizontal, 6)
        .background(hovering ? Theme.Palette.hover : .clear, in: RoundedRectangle(cornerRadius: Theme.Radius.s))
        .padding(.horizontal, -6)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
        .onHover { hovering = $0 }
    }
}

struct Placeholder: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text)
            .font(Theme.Font.caption)
            .foregroundStyle(Theme.Palette.textTertiary)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// A number with its unit: "2 adjacencies".
struct Stat: View {
    let value: String
    let label: String
    var color: Color = Theme.Palette.textPrimary

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 3) {
            Text(value).font(Theme.Font.headline.monospacedDigit()).foregroundStyle(color)
            Text(label).font(Theme.Font.caption).foregroundStyle(Theme.Palette.textSecondary)
        }
    }
}

