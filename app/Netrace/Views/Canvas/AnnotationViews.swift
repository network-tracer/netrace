import SwiftUI

/// Callbacks shared by note and area views. Translations are in world points.
struct AnnotationActions {
    var select: () -> Void
    var startEditing: () -> Void
    /// Drag start and current point, in world points.
    var move: (_ start: CGPoint, _ location: CGPoint, _ ended: Bool) -> Void
    var resize: (_ translation: CGSize, _ ended: Bool) -> Void
    var commitText: (String) -> Void
    var cancelEditing: () -> Void
}

/// A labelled shape outlining a zone (VLAN, site, floor). Drawn behind
/// cables and devices, which stay clickable on top of it. Anywhere inside
/// selects and drags the shape (with what it contains); ⌥-drag inside draws
/// a selection marquee instead. A locked shape only takes clicks on its
/// border and label, so the canvas inside works as if it weren't there.
struct AreaView: View {
    let annotation: AnnotationInfo
    let rect: CGRect
    let isSelected: Bool
    let isEditing: Bool
    let zoom: CGFloat
    /// Off while drawing (Shape or Note tool), so new items can go inside.
    let interiorClickable: Bool
    let actions: AnnotationActions

    @State private var isHovered = false

    private var style: ShapeStyle { annotation.style }

    var body: some View {
        let outline = AnnotationOutline(kind: style.shape)
        let lineWidth: CGFloat = style.stroke == nil ? 1.2 : 1.6
        let above = style.shape.labelsAbove
        ZStack(alignment: above ? .top : .topLeading) {
            outline.fill(style.fillColor)
            outline.stroke(style.strokeColor,
                           style: StrokeStyle(lineWidth: lineWidth, lineCap: .round, lineJoin: .round,
                                              dash: style.line.dash(lineWidth: lineWidth)))
            if isSelected {
                outline.stroke(Theme.Palette.accent, lineWidth: 1.5 / zoom).padding(-3)
            } else if isHovered {
                // Shows what a click would pick before the click.
                outline.stroke(Theme.Palette.accent.opacity(0.55), lineWidth: 1.5 / zoom).padding(-2)
            }
            AnnotationText(text: annotation.text, placeholder: style.shape.title, isEditing: isEditing, multiline: false,
                           font: Theme.Font.captionMedium,
                           color: style.stroke?.color ?? Theme.Palette.textSecondary, centered: above, actions: actions)
                .padding(.horizontal, Theme.Spacing.s + 2)
                .padding(.top, above ? 0 : Theme.Spacing.s)
                .frame(maxWidth: labelWidth, alignment: above ? .center : .leading)
                .fixedSize(horizontal: false, vertical: true)
                .alignmentGuide(.top) { above ? $0[.bottom] + 3 : $0[.top] }
        }
        .frame(width: rect.width, height: rect.height)
        .contentShape(AreaHitShape(kind: style.shape, labelWidth: labelWidth, interior: interiorClickable && !annotation.locked,
                                   band: max(12, 14 / zoom)))
        .annotationGestures(actions: actions, isEditing: isEditing)
        .onHover { isHovered = $0 }
        .overlay(alignment: .bottomTrailing) {
            if isSelected && !isEditing && !annotation.locked { ResizeHandle(actions: actions) }
        }
        .overlay(alignment: .topTrailing) {
            if isSelected && annotation.locked { LockBadge() }
        }
        .position(x: rect.midX, y: rect.midY)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(style.shape.title) \(annotation.text)")
    }

    private var labelWidth: CGFloat {
        style.shape.labelsAbove ? max(80, rect.width * 0.8) : min(rect.width - 16, 260)
    }
}

/// A text note on the canvas. Its colour tints the paper and the border.
struct NoteView: View {
    let annotation: AnnotationInfo
    let rect: CGRect
    let isSelected: Bool
    let isEditing: Bool
    let actions: AnnotationActions

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous)
        let tint = annotation.style.fill
        AnnotationText(text: annotation.text, placeholder: "Note", isEditing: isEditing, multiline: true,
                       font: Theme.Font.body, color: Theme.Palette.textPrimary, actions: actions)
            .padding(Theme.Spacing.s + 2)
            .frame(width: rect.width, height: rect.height, alignment: .topLeading)
            .background {
                ZStack {
                    shape.fill(Theme.Palette.surfaceRaised)
                    if let tint { shape.fill(tint.color.opacity(0.16)) }
                }
            }
            .overlay(shape.strokeBorder(isSelected ? Theme.Palette.accent
                                            : annotation.style.stroke?.color ?? tint?.color.opacity(0.7) ?? Theme.Palette.borderStrong,
                                        lineWidth: isSelected ? 1.5 : 1))
            .shadow(color: .black.opacity(0.06), radius: 3, y: 1)
            .contentShape(shape)
            .annotationGestures(actions: actions, isEditing: isEditing)
            .overlay(alignment: .bottomTrailing) {
                if isSelected && !isEditing && !annotation.locked { ResizeHandle(actions: actions) }
            }
            .overlay(alignment: .topTrailing) {
                if isSelected && annotation.locked { LockBadge() }
            }
            .position(x: rect.midX, y: rect.midY)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Note: \(annotation.text)")
    }
}

/// Shown on a selected locked annotation instead of the resize handle.
private struct LockBadge: View {
    var body: some View {
        Image(systemName: "lock.fill")
            .font(.system(size: 8, weight: .bold))
            .foregroundStyle(Theme.Palette.onAccent)
            .frame(width: 16, height: 16)
            .background(Circle().fill(Theme.Palette.accent))
            .offset(x: 6, y: -6)
            .help("Locked. Unlock it to move or resize.")
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
    var centered = false
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
                        .multilineTextAlignment(centered ? .center : .leading)
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
                .multilineTextAlignment(centered ? .center : .leading)
                .frame(maxWidth: .infinity, alignment: centered ? .top : .topLeading)
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

/// Border band of the outline plus the label, so clicks inside the shape
/// reach the canvas and devices underneath.
private struct AreaHitShape: Shape {
    let kind: ShapeKind
    let labelWidth: CGFloat
    /// Whether the inside takes clicks too (unlocked shapes).
    let interior: Bool
    /// Width of the clickable band along the border, in world points.
    let band: CGFloat

    func path(in rect: CGRect) -> Path {
        var p = kind.path(in: rect).strokedPath(StrokeStyle(lineWidth: band, lineJoin: .round))
        if interior { p.addPath(kind.path(in: rect)) }
        if kind.labelsAbove {
            p.addRect(CGRect(x: rect.midX - labelWidth / 2, y: rect.minY - 22, width: labelWidth, height: 22))
        } else {
            p.addRect(CGRect(x: rect.minX, y: rect.minY, width: min(rect.width, labelWidth + 16), height: 30))
        }
        return p
    }
}

private extension View {
    func annotationGestures(actions: AnnotationActions, isEditing: Bool) -> some View {
        self
            .gesture(
                DragGesture(minimumDistance: 2, coordinateSpace: .named("world"))
                    .onChanged { actions.move($0.startLocation, $0.location, false) }
                    .onEnded { actions.move($0.startLocation, $0.location, true) },
                including: isEditing ? .subviews : .all
            )
            .simultaneousGesture(TapGesture(count: 2).onEnded { actions.startEditing() })
            .simultaneousGesture(TapGesture().onEnded { actions.select() })
    }
}
