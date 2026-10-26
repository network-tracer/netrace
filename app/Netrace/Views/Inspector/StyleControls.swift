import SwiftUI

/// A row of palette colours. The first circle (slashed) is the default.
struct SwatchPicker: View {
    let selection: Swatch?
    var defaultHelp = "Default"
    let onPick: (Swatch?) -> Void

    var body: some View {
        HStack(spacing: 5) {
            chip(nil)
            ForEach(Swatch.allCases) { chip($0) }
        }
    }

    private func chip(_ swatch: Swatch?) -> some View {
        let isSelected = swatch == selection
        return Button { onPick(swatch) } label: {
            ZStack {
                if let swatch {
                    Circle().fill(swatch.color)
                } else {
                    Circle().strokeBorder(Theme.Palette.borderStrong, lineWidth: 1)
                    Path { p in
                        p.move(to: CGPoint(x: 4, y: 14))
                        p.addLine(to: CGPoint(x: 14, y: 4))
                    }
                    .stroke(Theme.Palette.statusDown, lineWidth: 1.2)
                }
            }
            .frame(width: 18, height: 18)
            .padding(2)
            .overlay(Circle().strokeBorder(isSelected ? Theme.Palette.accent : .clear, lineWidth: 2))
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .help(swatch?.title ?? defaultHelp)
        .accessibilityLabel(swatch?.title ?? defaultHelp)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

/// Shape choice (rounded rectangle, rectangle, ellipse, polygon) plus the
/// number of sides when it's a polygon.
struct ShapeKindPicker: View {
    let selection: ShapeKind
    let onPick: (ShapeKind) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
            HStack(spacing: Theme.Spacing.xs) {
                ForEach(ShapeKind.choices, id: \.self) { kind in
                    IconButton(glyph: .system(kind.isPolygon ? "hexagon" : kind.symbol),
                               help: kind.isPolygon ? "Polygon" : kind.title,
                               isOn: kind.isSameKind(as: selection)) {
                        guard !kind.isSameKind(as: selection) else { return }
                        onPick(kind)
                    }
                }
            }
            if case .polygon(let sides) = selection {
                SidesStepper(sides: sides) { onPick(.polygon(sides: $0)) }
            }
        }
    }
}

struct SidesStepper: View {
    let sides: UInt8
    let onChange: (UInt8) -> Void

    var body: some View {
        HStack {
            Text("Sides").font(Theme.Font.body).foregroundStyle(Theme.Palette.textSecondary)
            Spacer()
            Text("\(sides)  \(ShapeKind.polygonName(sides))")
                .font(Theme.Font.body)
                .foregroundStyle(Theme.Palette.textPrimary)
                .monospacedDigit()
            Stepper("Sides", value: Binding(get: { Int(sides) }, set: { onChange(UInt8($0)) }),
                    in: Int(ShapeKind.sidesRange.lowerBound)...Int(ShapeKind.sidesRange.upperBound))
                .labelsHidden()
        }
    }
}

struct LinePatternPicker: View {
    let selection: LinePattern
    let onPick: (LinePattern) -> Void

    var body: some View {
        Picker("Line", selection: Binding(get: { selection }, set: { onPick($0) })) {
            ForEach(LinePattern.allCases) { pattern in
                Text(pattern.title).tag(pattern)
            }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .controlSize(.small)
    }
}

/// A labelled control row in a style section.
struct StyleRow<Control: View>: View {
    let title: String
    @ViewBuilder let control: Control

    init(_ title: String, @ViewBuilder control: () -> Control) {
        self.title = title
        self.control = control()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(title).font(Theme.Font.caption).foregroundStyle(Theme.Palette.textSecondary)
            control
        }
    }
}

/// Front / forward / backward / back buttons for the stacking order.
struct StackButtons: View {
    let onMove: (ProjectDocument.StackMove) -> Void

    var body: some View {
        HStack(spacing: Theme.Spacing.xs) {
            IconButton(glyph: .system("square.3.layers.3d.top.filled"), help: "Bring to Front (⇧⌘F)") { onMove(.front) }
            IconButton(glyph: .system("square.2.layers.3d.top.filled"), help: "Bring Forward (⌥⇧⌘F)") { onMove(.forward) }
            IconButton(glyph: .system("square.2.layers.3d.bottom.filled"), help: "Send Backward (⌥⇧⌘B)") { onMove(.backward) }
            IconButton(glyph: .system("square.3.layers.3d.bottom.filled"), help: "Send to Back (⇧⌘B)") { onMove(.back) }
        }
    }
}
