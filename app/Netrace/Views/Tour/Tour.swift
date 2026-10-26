import SwiftUI

/// Parts of the window the tour can point at. Views mark themselves with
/// `.tourAnchor(_:)`; the overlay reads their frames from a preference.
enum TourAnchor: Hashable {
    case library, canvas, rightPane, bottomPanel
}

struct TourAnchorKey: PreferenceKey {
    static let defaultValue: [TourAnchor: Anchor<CGRect>] = [:]
    static func reduce(value: inout [TourAnchor: Anchor<CGRect>], nextValue: () -> [TourAnchor: Anchor<CGRect>]) {
        value.merge(nextValue()) { $1 }
    }
}

extension View {
    func tourAnchor(_ anchor: TourAnchor) -> some View {
        anchorPreference(key: TourAnchorKey.self, value: .bounds) { [anchor: $0] }
    }
}

/// What a step can change so the user sees the part it describes.
@MainActor
struct TourContext {
    let document: ProjectDocument
    let workspace: WorkspaceModel
    let simulation: SimulationController

    func device(_ name: String) -> DeviceInfo? {
        document.state.devices.first { $0.name == name }
    }
}

/// One stop on the tour: what to look at, what it's for, and what it's
/// called in Packet Tracer or GNS3.
struct TourStep {
    let title: String
    let body: String
    /// SF Symbol for the card's icon tile, and the tile's colour.
    var symbol = "sparkles"
    var tint: Swatch = .blue
    /// "Packet Tracer: … · GNS3: …", for people arriving from those.
    var elsewhere: String? = nil
    /// Feature rows under the body (the welcome card).
    var highlights: [(symbol: String, text: String)] = []
    /// Where to point; none centres the card (or, for toolbar items, puts
    /// it under the toolbar).
    var anchor: TourAnchor? = nil
    var underToolbar = false
    var action: (@MainActor (TourContext) -> Void)? = nil
}

extension TourStep {
    static let all: [TourStep] = [
        TourStep(
            title: "Welcome to the Shadow Network",
            body: "Four agencies on their own VLANs around a layer 3 core, a data centre they all share, a firewall out to the internet, and a hidden VLAN that quietly watches. Everything netrace simulates is set up and working. The tour takes two minutes and points at each part of the window.",
            symbol: "network",
            tint: .blue,
            elsewhere: "Coming from Packet Tracer or GNS3? Each step says what the same thing is called there.",
            highlights: [
                ("square.stack.3d.up", "VLANs and trunks"),
                ("arrow.triangle.branch", "Rapid spanning tree"),
                ("arrow.left.arrow.right", "Inter-VLAN routing"),
                ("number", "DHCP from the core"),
                ("lock.shield", "ACLs, port security"),
                ("point.3.connected.trianglepath.dotted", "OSPF"),
                ("globe", "PAT at the firewall"),
                ("eye", "SPAN mirroring"),
            ]
        ),
        TourStep(
            title: "The device library",
            body: "Routers, switches, firewalls, the internet cloud and end devices. Drag one onto the canvas to add it. Search finds a device by name; the Topology tab lists what's in this project.",
            symbol: "square.grid.2x2",
            tint: .gray,
            elsewhere: "Packet Tracer: the device-type box at the bottom left. GNS3: the devices toolbar, with nothing to install: no images, no VM, no server.",
            anchor: .library,
            action: { $0.workspace.showsSidebar = true }
        ),
        TourStep(
            title: "The canvas",
            body: "Your topology. Pinch or scroll to zoom, space-drag to pan. Press C and drag from one device to another to cable them: netrace picks a free port and the right cable. The coloured polygons are areas, one per VLAN here; they're only for organising, and the Shape tool draws more.",
            symbol: "point.3.connected.trianglepath.dotted",
            tint: .teal,
            elsewhere: "Packet Tracer: the logical workspace, with automatic cabling always on. GNS3: the topology view and link tool.",
            anchor: .canvas,
            action: { $0.workspace.selectDevices([]) }
        ),
        TourStep(
            title: "Tools and modes",
            body: "Select, hand, cable, note and shape tools sit in the middle of the toolbar. Beside them, Realtime and Simulation: Realtime runs everything at once; Simulation lets you step through every frame on the timeline.",
            symbol: "cursorarrow.rays",
            tint: .gray,
            elsewhere: "Packet Tracer: the same Realtime / Simulation switch, at the bottom right there.",
            underToolbar: true
        ),
        TourStep(
            title: "The inspector",
            body: "Select a device and its details appear here: ports, addresses, interfaces and configuration, power, and snapshots you can roll back to. Edits apply at once and can be undone. CORE is selected: it routes between the VLANs, hands out addresses and keeps Field Ops away from personnel records with ACL 140.",
            symbol: "sidebar.right",
            tint: .blue,
            elsewhere: "Packet Tracer: the device window's Physical and Config tabs, without the modal window. GNS3: the node's configure dialog.",
            anchor: .rightPane,
            action: { context in
                context.workspace.rightPane = .inspector
                context.workspace.showsInspector = true
                if let core = context.device("CORE") { context.workspace.selectDevices([core.id]) }
            }
        ),
        TourStep(
            title: "The console",
            body: "Double-click a router, switch or firewall for its IOS command line: modes, `?` help, Tab completion, `show` commands and the usual errors. Click in it to type; paste a block and it runs line by line. PCs speak `ping`, `tracert`, `ipconfig` and `curl`. Try `show monitor` or `show ip ospf neighbor` here.",
            symbol: "terminal",
            tint: .gray,
            elsewhere: "Packet Tracer: the CLI tab. GNS3: the telnet console, without separate terminal windows.",
            anchor: .bottomPanel,
            action: { context in
                if let core = context.device("CORE") { context.workspace.openConsole(for: core.id) }
            }
        ),
        TourStep(
            title: "Every frame, explained",
            body: "We've just pinged the threat-intel server on the internet from GPS-TRACK in Field Ops. Every frame it caused is listed, with what happened and why. Click one to see each layer: the ARP that came first, the address rewrite at FW, the TTL going down at each hop. Tags filter by protocol.",
            symbol: "list.bullet.rectangle",
            tint: .orange,
            elsewhere: "Packet Tracer: Simulation mode's event list and PDU window, at every hop and in plain words.",
            anchor: .bottomPanel,
            action: { context in
                if let gps = context.device("GPS-TRACK") {
                    _ = try? context.simulation.ping(from: gps.id, to: "198.51.100.10", count: 4,
                                                     at: context.simulation.endTime)
                }
                context.workspace.bottomTab = .events
                context.workspace.showsBottomPanel = true
            }
        ),
        TourStep(
            title: "The Watchers",
            body: "CORE copies everything Internal Affairs sends and receives out a SPAN port to WATCHER, in hidden VLAN 99. We've pinged the vault from AUDITOR and started a capture on WATCHER's cable: every request and reply shows up, though none was addressed to it. Right-click any cable to capture it.",
            symbol: "eye",
            tint: .purple,
            elsewhere: "GNS3: capturing a link into Wireshark, built in. Packet Tracer has no SPAN.",
            anchor: .bottomPanel,
            action: { context in
                if let auditor = context.device("AUDITOR") {
                    _ = try? context.simulation.ping(from: auditor.id, to: "10.50.50.10", count: 4,
                                                     at: context.simulation.endTime)
                }
                if let watcher = context.device("WATCHER"),
                   let link = context.document.state.links.first(where: {
                       $0.a.deviceId == watcher.id || $0.b.deviceId == watcher.id
                   }) {
                    context.workspace.startCapture(on: link.id)
                } else {
                    context.workspace.bottomTab = .capture
                }
            }
        ),
        TourStep(
            title: "Configuration, on the canvas",
            body: "Cables in a VLAN take its colour and trunks are striped. A red bar marks the port spanning tree blocks; ROOT marks the root bridge. Shields show access lists, IN and OUT the NAT sides, DHCP the server. Point at a router to see its routes. View → Hide Feature Overlays turns these off.",
            symbol: "square.3.layers.3d",
            tint: .teal,
            anchor: .canvas,
            action: { context in
                context.workspace.showsLayer2 = true
                context.workspace.selectDevices([])
                context.workspace.rightPane = .inspector
            }
        ),
        TourStep(
            title: "Spanning tree, live",
            body: "One protocol's real state over the topology: which switch is root and why, each port's role, and the SW-IA – SW-FIELD link that blocks to stop a loop. Right-click a cable and choose Fail Link to watch the tree reconverge.",
            symbol: "arrow.triangle.branch",
            tint: .yellow,
            elsewhere: "Neither Packet Tracer nor GNS3 shows this; you'd piece it together from show spanning-tree on each switch.",
            anchor: .rightPane,
            action: { context in
                context.workspace.rightPane = .spanningTree
                context.workspace.selectedVlan = 30
                context.workspace.stpVlan = 30
            }
        ),
        TourStep(
            title: "OSPF, live",
            body: "Each router's ID and why, its neighbours (and why any don't come up), the DR election and every route with its path. FW originates the default route; CORE learns it as O*E2. Green curves are full adjacencies, labelled with interface costs.",
            symbol: "point.3.filled.connected.trianglepath.dotted",
            tint: .green,
            anchor: .rightPane,
            action: { $0.workspace.rightPane = .ospf }
        ),
        TourStep(
            title: "VLANs",
            body: "Pick a VLAN to light it up: its hosts, access ports, the trunks that carry it and its gateway on CORE. Everything else fades back.",
            symbol: "square.stack.3d.up",
            tint: .red,
            anchor: .rightPane,
            action: { context in
                context.workspace.selectedVlan = 20
                context.workspace.rightPane = .vlans
            }
        ),
        TourStep(
            title: "Labs",
            body: "File → New from Lab opens one of 20 guided CCNA labs, from a first ping to NAT and OSPF troubleshooting. Each step checks your work on the simulated network as you go and explains what's missing.",
            symbol: "graduationcap",
            tint: .blue,
            elsewhere: "Packet Tracer: activities with Check Results, but checked on how the network behaves, not just its configuration.",
            anchor: .rightPane,
            action: { $0.workspace.rightPane = .lab }
        ),
        TourStep(
            title: "You're all set",
            body: "Press ⌘K for the command palette: every command, searchable. Things to try: from GPS-TRACK, `ping 10.30.30.20` (ACL 140 stops Field Ops); from SOC-1, `curl 198.51.100.10` (PAT at FW). Right-click a device to power it off, or take a snapshot first. Help → Take the Tour brings this back.",
            symbol: "checkmark.seal.fill",
            tint: .green,
            action: { $0.workspace.rightPane = .inspector }
        ),
    ]
}

/// The tour drawn over the window: everything dimmed except the part being
/// described, which stays usable, with a popover-style card pointing at it.
struct TourOverlay: View {
    @ObservedObject var workspace: WorkspaceModel
    let context: TourContext
    let anchors: [TourAnchor: Anchor<CGRect>]
    @State private var cardSize = CGSize(width: 360, height: 260)
    /// Holds the keyboard so Return and the arrow keys drive the tour even
    /// after a step selects something on the canvas.
    @FocusState private var focused: Bool

    private static let spring = Animation.spring(response: 0.42, dampingFraction: 0.86)

    var body: some View {
        if let index = workspace.tourStep, TourStep.all.indices.contains(index) {
            let step = TourStep.all[index]
            GeometryReader { proxy in
                let bounds = CGRect(origin: .zero, size: proxy.size)
                let hole = step.anchor.flatMap { anchors[$0] }.map { proxy[$0].insetBy(dx: -6, dy: -6) }
                let place = placement(bounds: bounds, hole: hole, underToolbar: step.underToolbar)
                ZStack(alignment: .topLeading) {
                    Spotlight(hole: hole ?? CGRect(x: bounds.midX, y: bounds.midY, width: 0, height: 0))
                        .fill(Color.black.opacity(0.45), style: FillStyle(eoFill: true))
                        .contentShape(Spotlight(hole: hole ?? .zero), eoFill: true)
                        .onTapGesture {}
                    ring(hole ?? CGRect(x: bounds.midX, y: bounds.midY, width: 0, height: 0))
                        .opacity(hole == nil ? 0 : 1)
                    TourCard(step: step, index: index, count: TourStep.all.count, edge: place.edge,
                             arrowOffset: place.arrow, back: { go(index - 1) }, next: { go(index + 1) },
                             close: close)
                        .frame(width: step.highlights.isEmpty ? 360 : 440)
                        .onGeometryChange(for: CGSize.self) { $0.size } action: { cardSize = $0 }
                        .position(place.center)
                        .transition(.scale(scale: 0.94).combined(with: .opacity))
                }
                .animation(Self.spring, value: index)
                .animation(Self.spring, value: hole)
            }
            .transition(.opacity)
            .focusable()
            .focusEffectDisabled()
            .focused($focused)
            .onAppear {
                step.action?(context)
                focused = true
            }
            .onChange(of: index) { _, i in
                TourStep.all[i].action?(context)
                // A step may open the console, which takes the keyboard
                // once it appears; Return keeps driving the tour, and a
                // click in the console gives it the keyboard back.
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) { focused = true }
            }
            .onKeyPress(.return) { go(index + 1); return .handled }
            .onKeyPress(.escape) { close(); return .handled }
            .onKeyPress(.rightArrow) { go(index + 1); return .handled }
            .onKeyPress(.leftArrow) { go(index - 1); return .handled }
        }
    }

    /// A crisp accent outline with a soft halo, around the highlighted part.
    private func ring(_ hole: CGRect) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(Theme.Palette.accent.opacity(0.28), lineWidth: 8)
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(Theme.Palette.accent, lineWidth: 2)
        }
        .frame(width: hole.width, height: hole.height)
        .offset(x: hole.minX, y: hole.minY)
        .allowsHitTesting(false)
    }

    struct Placement {
        var center: CGPoint
        /// The card's edge the arrow sits on, pointing at the highlighted part.
        var edge: Edge?
        /// Where along that edge, from its middle.
        var arrow: CGFloat = 0
    }

    /// Beside the highlighted part where there's room (right, then left,
    /// then below, then above), centred otherwise.
    private func placement(bounds: CGRect, hole: CGRect?, underToolbar: Bool) -> Placement {
        let size = cardSize
        let margin: CGFloat = 18
        let gap = margin + PopoverShape.arrowLength
        guard let hole else {
            return Placement(center: underToolbar ? CGPoint(x: bounds.midX, y: margin + size.height / 2)
                                                  : CGPoint(x: bounds.midX, y: bounds.midY))
        }
        let clampY = { (y: CGFloat) in min(max(y, size.height / 2 + margin), bounds.maxY - size.height / 2 - margin) }
        let clampX = { (x: CGFloat) in min(max(x, size.width / 2 + margin), bounds.maxX - size.width / 2 - margin) }
        if bounds.maxX - hole.maxX > size.width + gap + margin {
            let y = clampY(hole.midY)
            return Placement(center: CGPoint(x: hole.maxX + gap + size.width / 2, y: y), edge: .leading,
                             arrow: hole.midY - y)
        }
        if hole.minX > size.width + gap + margin {
            let y = clampY(hole.midY)
            return Placement(center: CGPoint(x: hole.minX - gap - size.width / 2, y: y), edge: .trailing,
                             arrow: hole.midY - y)
        }
        if bounds.maxY - hole.maxY > size.height + gap + margin {
            let x = clampX(hole.midX)
            return Placement(center: CGPoint(x: x, y: hole.maxY + gap + size.height / 2), edge: .top,
                             arrow: hole.midX - x)
        }
        if hole.minY > size.height + gap + margin {
            let x = clampX(hole.midX)
            return Placement(center: CGPoint(x: x, y: hole.minY - gap - size.height / 2), edge: .bottom,
                             arrow: hole.midX - x)
        }
        // Covers most of the window (the canvas): sit in its top-left corner.
        return Placement(center: CGPoint(x: hole.minX + 2 * margin + size.width / 2,
                                         y: hole.minY + 2 * margin + size.height / 2))
    }

    private func go(_ index: Int) {
        guard TourStep.all.indices.contains(index) else {
            if index >= TourStep.all.count { close() }
            return
        }
        withAnimation(Self.spring) { workspace.tourStep = index }
    }

    private func close() {
        withAnimation(.easeOut(duration: 0.2)) { workspace.tourStep = nil }
    }
}

/// The window with a rounded hole cut out, animating as the hole moves.
private struct Spotlight: Shape {
    var hole: CGRect

    var animatableData: AnimatablePair<AnimatablePair<CGFloat, CGFloat>, AnimatablePair<CGFloat, CGFloat>> {
        get { AnimatablePair(AnimatablePair(hole.minX, hole.minY), AnimatablePair(hole.width, hole.height)) }
        set { hole = CGRect(x: newValue.first.first, y: newValue.first.second,
                            width: newValue.second.first, height: newValue.second.second) }
    }

    func path(in rect: CGRect) -> Path {
        var path = Path(rect)
        if hole.width > 0, hole.height > 0 {
            path.addRoundedRect(in: hole, cornerSize: CGSize(width: 12, height: 12), style: .continuous)
        }
        return path
    }
}

/// A rounded card with a popover arrow on one edge, drawn as one outline
/// so the border runs unbroken around the arrow.
private struct PopoverShape: Shape {
    static let arrowLength: CGFloat = 9
    static let arrowWidth: CGFloat = 22

    var edge: Edge?
    var offset: CGFloat
    var radius: CGFloat = 14

    var animatableData: CGFloat {
        get { offset }
        set { offset = newValue }
    }

    func path(in r: CGRect) -> Path {
        let half = Self.arrowWidth / 2, length = Self.arrowLength
        func along(_ side: CGFloat) -> CGFloat {
            let limit = side / 2 - radius - half
            return min(max(offset, -limit), limit)
        }
        var p = Path()
        p.move(to: CGPoint(x: r.minX + radius, y: r.minY))
        if edge == .top {
            let x = r.midX + along(r.width)
            p.addLine(to: CGPoint(x: x - half, y: r.minY))
            p.addQuadCurve(to: CGPoint(x: x + half, y: r.minY), control: CGPoint(x: x, y: r.minY - 2 * length))
        }
        p.addArc(center: CGPoint(x: r.maxX - radius, y: r.minY + radius), radius: radius,
                 startAngle: .degrees(-90), endAngle: .degrees(0), clockwise: false)
        if edge == .trailing {
            let y = r.midY + along(r.height)
            p.addLine(to: CGPoint(x: r.maxX, y: y - half))
            p.addQuadCurve(to: CGPoint(x: r.maxX, y: y + half), control: CGPoint(x: r.maxX + 2 * length, y: y))
        }
        p.addArc(center: CGPoint(x: r.maxX - radius, y: r.maxY - radius), radius: radius,
                 startAngle: .degrees(0), endAngle: .degrees(90), clockwise: false)
        if edge == .bottom {
            let x = r.midX + along(r.width)
            p.addLine(to: CGPoint(x: x + half, y: r.maxY))
            p.addQuadCurve(to: CGPoint(x: x - half, y: r.maxY), control: CGPoint(x: x, y: r.maxY + 2 * length))
        }
        p.addArc(center: CGPoint(x: r.minX + radius, y: r.maxY - radius), radius: radius,
                 startAngle: .degrees(90), endAngle: .degrees(180), clockwise: false)
        if edge == .leading {
            let y = r.midY + along(r.height)
            p.addLine(to: CGPoint(x: r.minX, y: y + half))
            p.addQuadCurve(to: CGPoint(x: r.minX, y: y - half), control: CGPoint(x: r.minX - 2 * length, y: y))
        }
        p.addArc(center: CGPoint(x: r.minX + radius, y: r.minY + radius), radius: radius,
                 startAngle: .degrees(180), endAngle: .degrees(270), clockwise: false)
        p.closeSubpath()
        return p
    }
}

/// The step's card, in the style of a macOS popover or an iOS tip: an icon
/// tile, title and body, page dots and a prominent Continue button.
private struct TourCard: View {
    let step: TourStep
    let index: Int
    let count: Int
    let edge: Edge?
    let arrowOffset: CGFloat
    let back: () -> Void
    let next: () -> Void
    let close: () -> Void

    var body: some View {
        let shape = PopoverShape(edge: edge, offset: arrowOffset)
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top) {
                SymbolTile(symbol: step.symbol, tint: step.tint.color, size: step.highlights.isEmpty ? 34 : 46)
                Spacer()
                Button(action: close) {
                    Image(systemName: "xmark")
                        .font(.system(size: 9, weight: .bold))
                        .foregroundStyle(Theme.Palette.textSecondary)
                        .frame(width: 20, height: 20)
                        .background(Theme.Palette.hover, in: Circle())
                }
                .buttonStyle(.plain)
                .help("End the tour (Esc)")
            }
            VStack(alignment: .leading, spacing: 4) {
                Text("STEP \(index + 1) OF \(count)")
                    .font(Theme.Font.sectionHeader.monospacedDigit())
                    .tracking(Theme.Font.sectionHeaderTracking)
                    .foregroundStyle(step.tint.color)
                Text(step.title)
                    .font(.system(size: step.highlights.isEmpty ? 17 : 22, weight: .bold))
                    .foregroundStyle(Theme.Palette.textPrimary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Text(markdown(step.body))
                .font(Theme.Font.body)
                .foregroundStyle(Theme.Palette.textSecondary)
                .lineSpacing(2.5)
                .fixedSize(horizontal: false, vertical: true)
            if !step.highlights.isEmpty {
                LazyVGrid(columns: [GridItem(.flexible(), alignment: .leading), GridItem(.flexible(), alignment: .leading)],
                          alignment: .leading, spacing: 10) {
                    ForEach(step.highlights, id: \.text) { item in
                        HStack(spacing: 8) {
                            Image(systemName: item.symbol)
                                .font(.system(size: 12, weight: .semibold))
                                .foregroundStyle(step.tint.color)
                                .frame(width: 18)
                            Text(item.text)
                                .font(Theme.Font.label)
                                .foregroundStyle(Theme.Palette.textPrimary)
                        }
                    }
                }
                .padding(.vertical, 2)
            }
            if let elsewhere = step.elsewhere {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: "arrow.triangle.swap")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Theme.Palette.textTertiary)
                    Text(elsewhere)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.Palette.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Theme.Palette.surfaceSunken, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
            }
            HStack(spacing: 10) {
                PageDots(index: index, count: count, tint: step.tint.color)
                Spacer()
                if index > 0 {
                    Button("Back", action: back)
                        .fixedSize()
                        .buttonStyle(.plain)
                        .font(Theme.Font.labelSemibold)
                        .foregroundStyle(Theme.Palette.textSecondary)
                        .padding(.horizontal, 6)
                }
                Button(action: next) {
                    Text(index == count - 1 ? "Done" : index == 0 ? "Start Tour" : "Continue")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Theme.Palette.onAccent)
                        .fixedSize()
                        .padding(.horizontal, 16)
                        .frame(height: 28)
                        .background(Theme.Palette.accent, in: Capsule())
                        .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(18)
        .background(Theme.Palette.surfaceRaised, in: shape)
        .overlay(shape.stroke(Theme.Palette.border, lineWidth: 1))
        .shadow(color: .black.opacity(0.18), radius: 2, y: 1)
        .shadow(color: .black.opacity(0.22), radius: 24, y: 12)
        .animation(.spring(response: 0.42, dampingFraction: 0.86), value: arrowOffset)
    }

    private func markdown(_ text: String) -> AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(text)
    }
}

/// A white SF Symbol on a solid rounded square, like a Settings icon.
struct SymbolTile: View {
    let symbol: String
    let tint: Color
    var size: CGFloat = 34

    var body: some View {
        Image(systemName: symbol)
            .font(.system(size: size * 0.46, weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(tint, in: RoundedRectangle(cornerRadius: size * 0.26, style: .continuous))
    }
}

/// iOS-style page control: the current page is a stretched capsule.
private struct PageDots: View {
    let index: Int
    let count: Int
    let tint: Color

    var body: some View {
        HStack(spacing: 5) {
            ForEach(0..<count, id: \.self) { i in
                Capsule()
                    .fill(i == index ? tint : Theme.Palette.borderStrong)
                    .frame(width: i == index ? 16 : 6, height: 6)
            }
        }
        .animation(.spring(response: 0.35, dampingFraction: 0.8), value: index)
        .accessibilityElement()
        .accessibilityLabel("Step \(index + 1) of \(count)")
    }
}
