import SwiftUI

struct SidebarPanel: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                TabStrip(tabs: WorkspaceModel.SidebarTab.allCases, selection: $workspace.sidebarTab,
                         title: \.title,
                         glyph: { $0 == .library ? .system("square.grid.2x2") : .custom(.topology) })
                Spacer()
            }
            .padding(.horizontal, Theme.Spacing.m)
            PanelDivider(axis: .horizontal)

            Group {
                if workspace.sidebarTab == .library {
                    SearchField(prompt: "Search devices", text: $workspace.librarySearch)
                } else {
                    SearchField(prompt: "Filter topology", text: $workspace.topologySearch)
                }
            }
            .padding(Theme.Spacing.m)

            switch workspace.sidebarTab {
            case .library: LibraryView(document: document, workspace: workspace)
            case .topology: TopologyOutline(document: document, workspace: workspace)
            }
        }
        .background(Theme.Palette.surface)
    }
}

// MARK: - Library

private struct LibraryView: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel
    @Environment(\.undoManager) private var undoManager

    private let columns = [GridItem(.adaptive(minimum: 68, maximum: 90), spacing: Theme.Spacing.xs)]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Spacing.l) {
                ForEach(DeviceCategory.allCases) { category in
                    let kinds = category.kinds.filter { $0.matches(workspace.librarySearch) }
                    if !kinds.isEmpty {
                        VStack(alignment: .leading, spacing: Theme.Spacing.s) {
                            SectionHeader(category.rawValue)
                            LazyVGrid(columns: columns, spacing: Theme.Spacing.xs) {
                                ForEach(kinds) { kind in
                                    LibraryTile(kind: kind)
                                        .onTapGesture(count: 2) { add(kind) }
                                        .draggable(kind.dragIdentifier) {
                                            DeviceSymbolView(kind: kind)
                                        }
                                }
                            }
                        }
                    }
                }

                if workspace.librarySearch.isEmpty {
                    VStack(alignment: .leading, spacing: Theme.Spacing.xxs) {
                        SectionHeader("Connections")
                            .padding(.bottom, Theme.Spacing.s - Theme.Spacing.xxs)
                        ForEach(WorkspaceModel.CableKind.allCases) { cable in
                            PaletteRow(glyph: .custom(cable.icon), title: cable.title, subtitle: cable.summary,
                                       isActive: workspace.tool == .cable && workspace.cableKind == cable) {
                                workspace.cableKind = cable
                                workspace.tool = .cable
                            }
                        }
                    }

                    VStack(alignment: .leading, spacing: Theme.Spacing.xxs) {
                        SectionHeader("Annotations")
                            .padding(.bottom, Theme.Spacing.s - Theme.Spacing.xxs)
                        PaletteRow(glyph: WorkspaceModel.Tool.note.glyph, title: "Note", subtitle: "Text on the canvas",
                                   isActive: workspace.tool == .note) { workspace.tool = .note }
                        PaletteRow(glyph: WorkspaceModel.Tool.rectangle.glyph, title: "Shape", subtitle: "Outline a zone: box, ellipse, polygon",
                                   isActive: workspace.tool == .rectangle) { workspace.tool = .rectangle }
                    }
                }
            }
            .padding(.horizontal, Theme.Spacing.m)
            .padding(.bottom, Theme.Spacing.l)
        }
    }

    /// Adds the device in view: at the canvas centre, or the nearest free spot.
    private func add(_ kind: DeviceKind) {
        let point = Placement.freeSpot(near: workspace.viewportCenter, avoiding: document.state.devices.map(\.position))
        let device = document.addDevice(kind, at: point, undoManager: undoManager)
        workspace.selectDevices([device.id])
    }
}

private struct LibraryTile: View {
    let kind: DeviceKind
    @State private var isHovered = false

    var body: some View {
        let size = DeviceSymbol.size(for: kind)
        let scale = min(46 / size.width, 30 / size.height)
        VStack(spacing: Theme.Spacing.s - 2) {
            DeviceSymbolView(kind: kind, scale: scale)
                .frame(height: 32)
            Text(kind.displayName)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.Palette.textPrimary)
                .lineLimit(1)
                .minimumScaleFactor(0.85)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, Theme.Spacing.s)
        .background(
            RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous)
                .fill(isHovered ? Theme.Palette.hover : .clear)
        )
        .contentShape(Rectangle())
        .onHover { isHovered = $0 }
        .help("\(kind.displayName): \(kind.summary). Drag onto the canvas or double-click.")
    }
}

/// A clickable row with icon, title and subtitle (cables, annotations).
struct PaletteRow: View {
    let glyph: Glyph
    let title: String
    let subtitle: String
    var isActive = false
    let action: () -> Void

    @State private var isHovered = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: Theme.Spacing.s + 2) {
                IconView(glyph, size: 18)
                    .foregroundStyle(isActive ? Theme.Palette.accent : Theme.Palette.textSecondary)
                    .frame(width: 24)
                VStack(alignment: .leading, spacing: 1) {
                    Text(title)
                        .font(Theme.Font.body)
                        .foregroundStyle(Theme.Palette.textPrimary)
                    Text(subtitle)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.textSecondary)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, Theme.Spacing.s)
            .padding(.vertical, Theme.Spacing.xs + 1)
            .background(
                RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous)
                    .fill(isActive ? Theme.Palette.accent.opacity(0.12) : (isHovered ? Theme.Palette.hover : .clear))
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { isHovered = $0 }
    }
}

// MARK: - Topology outline

private struct TopologyOutline: View {
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel

    var body: some View {
        let query = workspace.topologySearch
        let devices = document.state.devices.filter {
            query.isEmpty || $0.name.localizedCaseInsensitiveContains(query) || $0.kind.matches(query)
        }
        if document.state.devices.isEmpty {
            EmptyState(glyph: .custom(.topology), title: "No devices yet",
                       message: "Devices and cables you add appear here with their status.")
                .frame(maxHeight: .infinity, alignment: .top)
        } else {
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.l) {
                    ForEach(DeviceCategory.allCases) { category in
                        let group = devices.filter { $0.kind.category == category }
                        if !group.isEmpty {
                            VStack(alignment: .leading, spacing: 1) {
                                SectionHeader(category.rawValue) {
                                    Text("\(group.count)").font(Theme.Font.caption).foregroundStyle(Theme.Palette.textTertiary)
                                }
                                .padding(.bottom, Theme.Spacing.xs)
                                ForEach(group) { device in
                                    OutlineDeviceRow(device: device, document: document, workspace: workspace)
                                }
                            }
                        }
                    }

                    if !document.state.links.isEmpty && query.isEmpty {
                        VStack(alignment: .leading, spacing: 1) {
                            SectionHeader("Cables") {
                                Text("\(document.state.links.count)").font(Theme.Font.caption).foregroundStyle(Theme.Palette.textTertiary)
                            }
                            .padding(.bottom, Theme.Spacing.xs)
                            ForEach(document.state.links) { link in
                                OutlineLinkRow(link: link, document: document, workspace: workspace)
                            }
                        }
                    }
                }
                .padding(.horizontal, Theme.Spacing.m)
                .padding(.bottom, Theme.Spacing.l)
            }
        }
    }
}

private struct OutlineDeviceRow: View {
    let device: DeviceInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel

    var body: some View {
        let isSelected = workspace.selection.contains(device.id)
        let size = DeviceSymbol.size(for: device.kind)
        let power = workspace.powerState(device.id)
        HStack(spacing: Theme.Spacing.s) {
            DeviceSymbolView(kind: device.kind, scale: min(22 / size.width, 16 / size.height), tint: device.color)
                .frame(width: 24)
            Text(device.name)
                .font(Theme.Font.body)
                .foregroundStyle(Theme.Palette.textPrimary)
            Text(device.kind.displayName)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.Palette.textTertiary)
            Spacer(minLength: 0)
            Text("\(document.links(attachedTo: device.id).count)/\(device.kind.ports.count)")
                .font(Theme.Font.monoSmall)
                .foregroundStyle(Theme.Palette.textTertiary)
                .help("Ports in use")
            StatusGlyph(state: power, size: 7)
                .help(power.title)
        }
        .padding(.horizontal, Theme.Spacing.s)
        .frame(height: Theme.Size.rowHeight)
        .background(
            RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
                .fill(isSelected ? Theme.Palette.accent.opacity(0.14) : .clear)
        )
        .contentShape(Rectangle())
        .onTapGesture { workspace.selectDevices([device.id]) }
    }
}

private struct OutlineLinkRow: View {
    let link: LinkInfo
    @ObservedObject var document: ProjectDocument
    @ObservedObject var workspace: WorkspaceModel

    var body: some View {
        let a = document.device(link.a.deviceId)?.name ?? "?"
        let b = document.device(link.b.deviceId)?.name ?? "?"
        HStack(spacing: Theme.Spacing.s) {
            IconView(link.medium == .fiber ? .cableFiber : .cableCopper, size: 14)
                .foregroundStyle(Theme.Palette.textSecondary)
                .frame(width: 24)
            Text("\(a) \(link.a.port.abbreviatedInterface)")
            Image(systemName: "arrow.left.and.right").font(.system(size: 9)).foregroundStyle(Theme.Palette.textTertiary)
            Text("\(b) \(link.b.port.abbreviatedInterface)")
            Spacer(minLength: 0)
        }
        .font(Theme.Font.caption)
        .foregroundStyle(Theme.Palette.textPrimary)
        .lineLimit(1)
        .padding(.horizontal, Theme.Spacing.s)
        .frame(height: Theme.Size.rowHeight)
        .background(
            RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
                .fill(workspace.selectedLink == link.id ? Theme.Palette.accent.opacity(0.14) : .clear)
        )
        .contentShape(Rectangle())
        .onTapGesture { workspace.selectLink(link.id) }
    }
}

extension WorkspaceModel.PowerState {
    var color: Color {
        switch self {
        case .running: Theme.Palette.statusUp
        case .suspended: Theme.Palette.statusWarning
        case .stopped: Theme.Palette.statusDown
        }
    }

    var title: String {
        switch self {
        case .running: "Running"
        case .suspended: "Suspended"
        case .stopped: "Stopped"
        }
    }
}
