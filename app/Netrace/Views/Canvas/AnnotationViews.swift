import SwiftUI

/// Callbacks shared by note and area views. Translations are in world points.
struct AnnotationActions {
    var select: () -> Void
    var startEditing: () -> Void
    var move: (_ translation: CGSize, _ ended: Bool) -> Void
    var resize: (_ translation: CGSize, _ ended: Bool) -> Void
    var commitText: (String) -> Void
    var cancelEditing: () -> Void
}

/// A labelled zone (VLAN, site, floor). Drawn behind cables and devices; only
/// its border and label take clicks, so the canvas inside stays usable.
struct AreaView: View {
    let annotation: AnnotationInfo
    let rect: CGRect
    let isSelected: Bool
    let isEditing: Bool
    let actions: AnnotationActions

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous)
        ZStack(alignment: .topLeading) {
            shape.fill(Theme.Palette.textPrimary.opacity(0.035))
            shape.stroke(isSelected ? Theme.Palette.accent : Theme.Palette.textTertiary,
                         style: StrokeStyle(lineWidth: isSelected ? 1.5 : 1.2, dash: isSelected ? [] : [6, 4]))
            AnnotationText(text: annotation.text, placeholder: "Area", isEditing: isEditing, multiline: false,
                           font: Theme.Font.captionMedium, color: Theme.Palette.textSecondary, actions: actions)
                .padding(.horizontal, Theme.Spacing.s + 2)
                .padding(.top, Theme.Spacing.s)
                .frame(maxWidth: min(rect.width - 16, 260), alignment: .leading)
        }
        .frame(width: rect.width, height: rect.height)
        .contentShape(AreaHitShape())
        .annotationGestures(actions: actions, isEditing: isEditing)
        .overlay(alignment: .bottomTrailing) {
            if isSelected && !isEditing { ResizeHandle(actions: actions) }
        }
        .position(x: rect.midX, y: rect.midY)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Area \(annotation.text)")
    }
}

/// A text note on the canvas.
struct NoteView: View {
    let annotation: AnnotationInfo
    let rect: CGRect
    let isSelected: Bool
    let isEditing: Bool
    let actions: AnnotationActions

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous)
        AnnotationText(text: annotation.text, placeholder: "Note", isEditing: isEditing, multiline: true,
                       font: Theme.Font.body, color: Theme.Palette.textPrimary, actions: actions)
            .padding(Theme.Spacing.s + 2)
            .frame(width: rect.width, height: rect.height, alignment: .topLeading)
            .background(shape.fill(Theme.Palette.surfaceRaised))
            .overlay(shape.strokeBorder(isSelected ? Theme.Palette.accent : Theme.Palette.borderStrong,
                                        lineWidth: isSelected ? 1.5 : 1))
            .shadow(color: .black.opacity(0.06), radius: 3, y: 1)
            .contentShape(shape)
            .annotationGestures(actions: actions, isEditing: isEditing)
            .overlay(alignment: .bottomTrailing) {
                if isSelected && !isEditing { ResizeHandle(actions: actions) }
            }
            .position(x: rect.midX, y: rect.midY)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Note: \(annotation.text)")
    }
}

/// Shows the text, or an editor for it while editing. Commits on Return
/// (single line), on clicking away, or discards on Escape.
private struct AnnotationText: View {
    let text: String
    let placeholder: String
    let isEditing: Bool
    let multiline: Bool
    let font: Font
    let color: Color
    let actions: AnnotationActions

    @State private var draft = ""
    @State private var cancelled = false
    @FocusState private var focused: Bool

    var body: some View {
        content
            // Editing can end without a focus change (e.g. a click on the canvas
            // clears the selection), so save whenever it ends, unless Esc was pressed.
            .onChange(of: isEditing) { _, editing in
                if editing {
                    cancelled = false
                } else if !cancelled && draft != text {
                    actions.commitText(draft)
                }
            }
    }

    @ViewBuilder
    private var content: some View {
        if isEditing {
            Group {
                if multiline {
                    TextField(placeholder, text: $draft, axis: .vertical)
                        .lineLimit(1...12)
                } else {
                    TextField(placeholder, text: $draft)
                        .onSubmit { actions.commitText(draft) }
                }
            }
            .textFieldStyle(.plain)
            .font(font)
            .foregroundStyle(Theme.Palette.textPrimary)
            .focused($focused)
            .onAppear {
                draft = text
                // Focusing in the same pass as insertion is ignored; wait one turn.
                DispatchQueue.main.async { focused = true }
            }
            .onExitCommand {
                cancelled = true
                actions.cancelEditing()
            }
            .onChange(of: focused) { _, isFocused in
                if !isFocused { actions.commitText(draft) }
            }
        } else {
            Text(text.isEmpty ? placeholder : text)
                .font(font)
                .foregroundStyle(text.isEmpty ? Theme.Palette.textTertiary : color)
                .lineLimit(multiline ? nil : 1)
                .frame(maxWidth: .infinity, alignment: .topLeading)
        }
    }
}

/// Bottom-right handle for resizing a selected annotation.
private struct ResizeHandle: View {
    let actions: AnnotationActions

    var body: some View {
        RoundedRectangle(cornerRadius: 2)
            .fill(Theme.Palette.accent)
            .overlay(RoundedRectangle(cornerRadius: 2).strokeBorder(Theme.Palette.surface, lineWidth: 1.5))
            .frame(width: 11, height: 11)
            .offset(x: 4, y: 4)
            .contentShape(Rectangle().inset(by: -6))
            .gesture(
                DragGesture(minimumDistance: 1, coordinateSpace: .named("world"))
                    .onChanged { actions.resize($0.translation, false) }
                    .onEnded { actions.resize($0.translation, true) }
            )
            .onHover { inside in
                if inside { NSCursor.crosshair.push() } else { NSCursor.pop() }
            }
            .help("Drag to resize")
    }
}

/// Border band plus the label corner of an area.
private struct AreaHitShape: Shape {
    func path(in rect: CGRect) -> Path {
        var p = Path(roundedRect: rect, cornerRadius: Theme.Radius.l)
            .strokedPath(StrokeStyle(lineWidth: 12))
        p.addRect(CGRect(x: rect.minX, y: rect.minY, width: min(rect.width, 200), height: 30))
        return p
    }
}

private extension View {
    func annotationGestures(actions: AnnotationActions, isEditing: Bool) -> some View {
        self
            .gesture(
                DragGesture(minimumDistance: 2, coordinateSpace: .named("world"))
                    .onChanged { actions.move($0.translation, false) }
                    .onEnded { actions.move($0.translation, true) },
                including: isEditing ? .subviews : .all
            )
            .simultaneousGesture(TapGesture(count: 2).onEnded { actions.startEditing() })
            .simultaneousGesture(TapGesture().onEnded { actions.select() })
    }
}
