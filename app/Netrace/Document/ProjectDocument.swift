import SwiftUI
import UniformTypeIdentifiers

extension UTType {
    /// Declared in Info.plist (generated from app/project.yml).
    static let netraceProject = UTType(exportedAs: "app.netrace.project", conformingTo: .package)
}

/// One open `.netrace` document.
///
/// The Rust `Project` is the source of truth. Every mutation goes through a
/// method here, which updates the core, republishes a fresh snapshot for the
/// views and registers the inverse operation with the window's undo manager
/// (which is also how SwiftUI learns the document has unsaved changes).
final class ProjectDocument: ReferenceFileDocument, @unchecked Sendable {
    static let readableContentTypes: [UTType] = [.netraceProject]

    private let project: Project

    /// Latest snapshot of the core model. Views render from this only.
    @MainActor @Published private(set) var state: ProjectSnapshot
    /// Saved states of the topology, newest last.
    @MainActor @Published private(set) var snapshots: [SnapshotInfo] = []

    init() {
        project = Project()
        _state = Published(initialValue: project.snapshot())
        _snapshots = Published(initialValue: project.snapshots())
    }

    /// Opened from Help → Take the Tour: the window starts the tour.
    var startsTour = false

    /// The intro tour's showcase network, as a new untitled document.
    static func tour() -> ProjectDocument? {
        let document = try? ProjectDocument(json: Data(tourProjectJson()))
        document?.startsTour = true
        return document
    }

    /// A new, untitled document holding a copy of a bundled lab.
    init(json: Data) throws {
        project = try Project.fromJson(data: json)
        _state = Published(initialValue: project.snapshot())
        _snapshots = Published(initialValue: project.snapshots())
    }

    required init(configuration: ReadConfiguration) throws {
        guard let data = configuration.file.fileWrappers?[projectFileName()]?.regularFileContents else {
            throw CocoaError(.fileReadCorruptFile)
        }
        do {
            project = try Project.fromJson(data: data)
        } catch let error as CoreError {
            throw CocoaError(.fileReadCorruptFile, userInfo: [NSLocalizedFailureReasonErrorKey: error.message])
        }
        _state = Published(initialValue: project.snapshot())
        _snapshots = Published(initialValue: project.snapshots())
    }

    func snapshot(contentType: UTType) throws -> Data {
        project.toJson()
    }

    func fileWrapper(snapshot: Data, configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(directoryWithFileWrappers: [
            projectFileName(): FileWrapper(regularFileWithContents: snapshot),
        ])
    }
}

// MARK: - Editing

@MainActor
extension ProjectDocument {
    func device(_ id: UInt64) -> DeviceInfo? {
        state.devices.first { $0.id == id }
    }

    func links(attachedTo id: UInt64) -> [LinkInfo] {
        state.links.filter { $0.a.deviceId == id || $0.b.deviceId == id }
    }

    @discardableResult
    func addDevice(_ kind: DeviceKind, at point: CGPoint, undoManager: UndoManager?) -> DeviceInfo {
        let device = project.addDevice(kind: kind, x: point.x, y: point.y)
        refresh()
        registerUndo(undoManager, name: "Add \(kind.displayName)") { doc, undo in
            doc.removeDevices([device.id], undoManager: undo)
        }
        return device
    }

    func moveDevice(_ id: UInt64, to point: CGPoint, undoManager: UndoManager?) {
        guard let old = device(id), old.x != point.x || old.y != point.y else { return }
        guard (try? project.moveDevice(id: id, x: point.x, y: point.y)) != nil else { return }
        refresh()
        registerUndo(undoManager, name: "Move \(old.name)") { doc, undo in
            doc.moveDevice(id, to: CGPoint(x: old.x, y: old.y), undoManager: undo)
        }
    }

    func removeDevices(_ ids: Set<UInt64>, undoManager: UndoManager?) {
        // Removed in reverse stacking order so each restore index stays valid.
        let ordered = state.devices.filter { ids.contains($0.id) }.reversed()
        let removed = ordered.compactMap { try? project.removeDevice(id: $0.id) }
        guard !removed.isEmpty else { return }
        refresh()
        let name = removed.count == 1 ? "Delete \(removed[0].device.name)" : "Delete \(removed.count) Devices"
        registerUndo(undoManager, name: name) { doc, undo in
            doc.restoreDevices(Array(removed.reversed()), undoManager: undo)
        }
    }

    private func restoreDevices(_ removed: [RemovedDeviceInfo], undoManager: UndoManager?) {
        for item in removed {
            try? project.restoreDevice(removed: item)
        }
        refresh()
        let ids = Set(removed.map(\.device.id))
        registerUndo(undoManager, name: "Restore Devices") { doc, undo in
            doc.removeDevices(ids, undoManager: undo)
        }
    }

    func link(_ id: UInt64) -> LinkInfo? {
        state.links.first { $0.id == id }
    }

    func freePorts(of id: UInt64) -> [String] {
        guard let device = device(id) else { return [] }
        let used = Set(links(attachedTo: id).compactMap { $0.endpoint(for: id)?.port })
        return device.kind.ports.filter { !used.contains($0) }
    }

    /// Cables two devices. Ports default to the first free port on each.
    func connect(_ a: UInt64, _ b: UInt64, portA: String? = nil, portB: String? = nil,
                 medium: Medium, undoManager: UndoManager?) -> Result<LinkInfo, CoreError> {
        guard a != b, let devA = device(a), let devB = device(b) else {
            return .failure(.InvalidOperation(message: "Choose two different devices."))
        }
        guard let portA = portA ?? freePorts(of: a).first else {
            return .failure(.InvalidOperation(message: "\(devA.name) has no free ports."))
        }
        guard let portB = portB ?? freePorts(of: b).first else {
            return .failure(.InvalidOperation(message: "\(devB.name) has no free ports."))
        }
        do {
            let link = try project.connect(
                a: EndpointInfo(deviceId: a, port: portA),
                b: EndpointInfo(deviceId: b, port: portB),
                medium: medium
            )
            refresh()
            registerUndo(undoManager, name: "Connect \(devA.name) to \(devB.name)") { doc, undo in
                doc.disconnect(link.id, undoManager: undo)
            }
            return .success(link)
        } catch let error as CoreError {
            return .failure(error)
        } catch {
            return .failure(.InvalidOperation(message: error.localizedDescription))
        }
    }

    /// Renames a device (IOS hostname rules, unique ignoring case).
    @discardableResult
    func renameDevice(_ id: UInt64, to name: String, undoManager: UndoManager?) -> Result<Void, CoreError> {
        do {
            let previous = try project.renameDevice(id: id, name: name)
            // The core trims whitespace; an unchanged name needs no undo step.
            guard previous != name.trimmingCharacters(in: .whitespaces) else { return .success(()) }
            refresh()
            registerUndo(undoManager, name: "Rename \(previous)") { doc, undo in
                doc.renameDevice(id, to: previous, undoManager: undo)
            }
            return .success(())
        } catch let error as CoreError {
            return .failure(error)
        } catch {
            return .failure(.InvalidOperation(message: error.localizedDescription))
        }
    }

    func disconnect(_ id: UInt64, undoManager: UndoManager?) {
        guard let link = try? project.disconnect(linkId: id) else { return }
        refresh()
        registerUndo(undoManager, name: "Delete Cable") { doc, undo in
            doc.restoreLink(link, undoManager: undo)
        }
    }

    private func restoreLink(_ link: LinkInfo, undoManager: UndoManager?) {
        guard (try? project.restoreLink(link: link)) != nil else { return }
        refresh()
        registerUndo(undoManager, name: "Restore Cable") { doc, undo in
            doc.disconnect(link.id, undoManager: undo)
        }
    }

    // MARK: IP settings and simulation

    /// Sets or clears an end device's IP settings, with undo.
    func setHostIP(_ id: UInt64, to ip: HostIpInfo?, undoManager: UndoManager?) -> Result<Void, CoreError> {
        do {
            let previous = try project.setHostIp(id: id, ip: ip)
            guard previous != ip else { return .success(()) }
            refresh()
            let name = device(id)?.name ?? "Device"
            registerUndo(undoManager, name: "IP Settings of \(name)") { doc, undo in
                _ = doc.setHostIP(id, to: previous, undoManager: undo)
            }
            return .success(())
        } catch let error as CoreError {
            return .failure(error)
        } catch {
            return .failure(.InvalidOperation(message: error.localizedDescription))
        }
    }

    /// Switches an end device between DHCP and its static settings, with undo.
    func setHostDHCP(_ id: UInt64, _ dhcp: Bool, undoManager: UndoManager?) -> Result<Void, CoreError> {
        do {
            let previous = try project.setHostDhcp(id: id, dhcp: dhcp)
            guard previous != dhcp else { return .success(()) }
            refresh()
            registerUndo(undoManager, name: dhcp ? "Use DHCP" : "Use Static Address") { doc, undo in
                _ = doc.setHostDHCP(id, previous, undoManager: undo)
            }
            return .success(())
        } catch let error as CoreError {
            return .failure(error)
        } catch {
            return .failure(.InvalidOperation(message: error.localizedDescription))
        }
    }

    /// A fresh simulation of the current topology.
    /// The lab this project carries, if it is one.
    var lab: LabInfo? { project.lab() }

    func makeSimulation() -> Simulation {
        Simulation(project: project)
    }

    /// Everything the simulation depends on (devices, addresses,
    /// configurations, cables), so moving a device doesn't reset a running
    /// simulation but a configuration change does.
    var topologyKey: String {
        String(state.simulationKey)
    }

    // MARK: Consoles (routers and switches)

    func startConsole(_ console: Console) -> ConsoleResponse {
        console.start(project: project)
    }

    func consolePrompt(_ console: Console) -> String {
        console.prompt(project: project)
    }

    func consoleHelp(_ console: Console, line: String) -> String {
        console.help(line: line, project: project)
    }

    func consoleComplete(_ console: Console, line: String) -> String? {
        console.complete(line: line, project: project)
    }

    /// Runs a command line; a configuration change becomes one undo step.
    func runConsole(_ console: Console, device id: UInt64, line: String, simulation: Simulation, at time: Double,
                    undoManager: UndoManager?) -> ConsoleResponse {
        let response = console.execute(line: line, project: project, simulation: simulation,
                                       atNs: UInt64(max(0, time) * 1_000_000_000))
        if let previous = response.previousSettings {
            refresh()
            let name = device(id)?.name ?? "Device"
            registerUndo(undoManager, name: "Configure \(name)") { doc, undo in
                doc.applySettings(id, previous, undoManager: undo)
            }
        }
        return response
    }

    /// Replaces a device's name, addresses and configuration (from
    /// `settings(of:)`), with undo.
    @discardableResult
    func applySettings(_ id: UInt64, _ settings: String, actionName: String? = nil,
                       undoManager: UndoManager?) -> Result<Void, CoreError> {
        do {
            let current = try project.setDeviceSettings(id: id, settings: settings)
            refresh()
            registerUndo(undoManager, name: actionName ?? "Configure \(device(id)?.name ?? "Device")") { doc, undo in
                doc.applySettings(id, current, actionName: actionName, undoManager: undo)
            }
            return .success(())
        } catch let error as CoreError {
            return .failure(error)
        } catch {
            return .failure(.InvalidOperation(message: error.localizedDescription))
        }
    }

    func runningConfig(of id: UInt64) -> String? {
        project.runningConfig(device: id)
    }

    func interfaceStatus(of id: UInt64) -> [InterfaceStatusInfo] {
        project.interfaceStatus(device: id)
    }

    func configVariables(of id: UInt64) -> [ConfigVariable] {
        project.configVariables(device: id)
    }

    /// Gives `target` the source's configuration with its own values filled in.
    func copyConfig(from source: UInt64, to target: UInt64, values: [ConfigVariable],
                    undoManager: UndoManager?) -> Result<Void, CoreError> {
        do {
            let previous = try project.copyConfig(from: source, to: target, values: values)
            refresh()
            registerUndo(undoManager, name: "Copy Configuration") { doc, undo in
                doc.applySettings(target, previous, actionName: "Copy Configuration", undoManager: undo)
            }
            return .success(())
        } catch let error as CoreError {
            return .failure(error)
        } catch {
            return .failure(.InvalidOperation(message: error.localizedDescription))
        }
    }

    /// A device's configuration as an opaque value for copy and paste.
    func settings(of id: UInt64) -> String? {
        try? project.deviceSettings(id: id)
    }

    // MARK: Annotations

    func annotation(_ id: UInt64) -> AnnotationInfo? {
        state.annotations.first { $0.id == id }
    }

    @discardableResult
    func addAnnotation(_ kind: AnnotationKind, in rect: CGRect, text: String, style: ShapeStyle = defaultShapeStyle(),
                       undoManager: UndoManager?) -> AnnotationInfo {
        let annotation = project.addAnnotation(kind: kind, x: rect.minX, y: rect.minY,
                                               width: rect.width, height: rect.height, text: text, style: style)
        refresh()
        registerUndo(undoManager, name: kind == .note ? "Add Note" : "Add \(style.shape.title)") { doc, undo in
            doc.removeAnnotation(annotation.id, undoManager: undo)
        }
        return annotation
    }

    func updateAnnotation(_ id: UInt64, rect: CGRect, text: String, actionName: String, undoManager: UndoManager?) {
        guard let current = annotation(id),
              current.rect != rect || current.text != text,
              let previous = try? project.updateAnnotation(id: id, x: rect.minX, y: rect.minY,
                                                           width: rect.width, height: rect.height, text: text)
        else { return }
        refresh()
        registerUndo(undoManager, name: actionName) { doc, undo in
            doc.updateAnnotation(id, rect: previous.rect, text: previous.text, actionName: actionName, undoManager: undo)
        }
    }

    func removeAnnotation(_ id: UInt64, undoManager: UndoManager?) {
        guard let removed = try? project.removeAnnotation(id: id) else { return }
        refresh()
        registerUndo(undoManager, name: removed.annotation.kind == .note ? "Delete Note" : "Delete Shape") { doc, undo in
            doc.restoreAnnotation(removed, undoManager: undo)
        }
    }

    private func restoreAnnotation(_ removed: RemovedAnnotationInfo, undoManager: UndoManager?) {
        guard (try? project.restoreAnnotation(removed: removed)) != nil else { return }
        refresh()
        registerUndo(undoManager, name: "Restore") { doc, undo in
            doc.removeAnnotation(removed.annotation.id, undoManager: undo)
        }
    }

    // MARK: Styling and arranging

    func setAnnotationStyle(_ id: UInt64, to style: ShapeStyle, actionName: String = "Change Style",
                            undoManager: UndoManager?) {
        guard annotation(id)?.style != style,
              let previous = try? project.setAnnotationStyle(id: id, style: style) else { return }
        refresh()
        registerUndo(undoManager, name: actionName) { doc, undo in
            doc.setAnnotationStyle(id, to: previous, actionName: actionName, undoManager: undo)
        }
    }

    func setAnnotationsLocked(_ ids: Set<UInt64>, _ locked: Bool, undoManager: UndoManager?) {
        let changed = ids.filter { annotation($0)?.locked == !locked }
        for id in changed { _ = try? project.setAnnotationLocked(id: id, locked: locked) }
        guard !changed.isEmpty else { return }
        refresh()
        registerUndo(undoManager, name: locked ? "Lock" : "Unlock") { doc, undo in
            doc.setAnnotationsLocked(changed, !locked, undoManager: undo)
        }
    }

    enum StackMove { case front, forward, backward, back }

    /// Changes an annotation's place in the stacking order.
    func reorderAnnotation(_ id: UInt64, _ move: StackMove, undoManager: UndoManager?) {
        guard let index = state.annotations.firstIndex(where: { $0.id == id }) else { return }
        let target: Int = switch move {
        case .front: state.annotations.count - 1
        case .forward: min(index + 1, state.annotations.count - 1)
        case .backward: max(index - 1, 0)
        case .back: 0
        }
        guard target != index else { return }
        moveAnnotation(id, toIndex: target, actionName: move.title, undoManager: undoManager)
    }

    private func moveAnnotation(_ id: UInt64, toIndex index: Int, actionName: String, undoManager: UndoManager?) {
        guard let previous = try? project.reorderAnnotation(id: id, index: UInt64(index)) else { return }
        refresh()
        registerUndo(undoManager, name: actionName) { doc, undo in
            doc.moveAnnotation(id, toIndex: Int(previous), actionName: actionName, undoManager: undo)
        }
    }

    func setDeviceColor(_ ids: Set<UInt64>, to color: Swatch?, undoManager: UndoManager?) {
        var previous: [UInt64: Swatch?] = [:]
        for id in ids where device(id).map({ $0.color != color }) ?? false {
            // Not `try?`: it would flatten a previous `nil` colour into a failure.
            do { previous[id] = .some(try project.setDeviceColor(id: id, color: color)) } catch {}
        }
        guard !previous.isEmpty else { return }
        refresh()
        registerUndo(undoManager, name: "Change Colour") { doc, undo in
            doc.restoreDeviceColors(previous, undoManager: undo)
        }
    }

    private func restoreDeviceColors(_ colors: [UInt64: Swatch?], undoManager: UndoManager?) {
        var previous: [UInt64: Swatch?] = [:]
        for (id, color) in colors {
            do { previous[id] = .some(try project.setDeviceColor(id: id, color: color)) } catch {}
        }
        refresh()
        registerUndo(undoManager, name: "Change Colour") { doc, undo in
            doc.restoreDeviceColors(previous, undoManager: undo)
        }
    }

    func setLinkStyle(_ id: UInt64, to style: LinkStyle, undoManager: UndoManager?) {
        guard let link = link(id), link.color != style.color || link.line != style.line,
              let previous = try? project.setLinkStyle(id: id, style: style) else { return }
        refresh()
        registerUndo(undoManager, name: "Change Cable Style") { doc, undo in
            doc.setLinkStyle(id, to: previous, undoManager: undo)
        }
    }

    /// Puts devices and annotations in one new group (two or more items).
    func group(devices: Set<UInt64>, annotations: Set<UInt64>, undoManager: UndoManager?) {
        guard devices.count + annotations.count > 1 else { return }
        let group = project.newGroupId()
        setGroups(items(devices: devices, annotations: annotations).map { GroupMembership(item: $0, group: group) },
                  actionName: "Group", undoManager: undoManager)
    }

    func ungroup(devices: Set<UInt64>, annotations: Set<UInt64>, undoManager: UndoManager?) {
        setGroups(items(devices: devices, annotations: annotations).map { GroupMembership(item: $0, group: nil) },
                  actionName: "Ungroup", undoManager: undoManager)
    }

    private func setGroups(_ changes: [GroupMembership], actionName: String, undoManager: UndoManager?) {
        guard !changes.isEmpty, let previous = try? project.setGroups(changes: changes) else { return }
        refresh()
        registerUndo(undoManager, name: actionName) { doc, undo in
            doc.setGroups(previous, actionName: actionName, undoManager: undo)
        }
    }

    private func items(devices: Set<UInt64>, annotations: Set<UInt64>) -> [CanvasItem] {
        devices.sorted().map { .device(id: $0) } + annotations.sorted().map { .annotation(id: $0) }
    }

    /// Every device and annotation sharing a group with any of the given items.
    func expandingGroups(devices: Set<UInt64>, annotations: Set<UInt64>) -> (devices: Set<UInt64>, annotations: Set<UInt64>) {
        let groups = Set(state.devices.filter { devices.contains($0.id) }.compactMap(\.group)
            + state.annotations.filter { annotations.contains($0.id) }.compactMap(\.group))
        guard !groups.isEmpty else { return (devices, annotations) }
        return (devices.union(state.devices.filter { $0.group.map(groups.contains) ?? false }.map(\.id)),
                annotations.union(state.annotations.filter { $0.group.map(groups.contains) ?? false }.map(\.id)))
    }

    /// What moves along with an area: devices whose centre lies inside its
    /// outline, and annotations above it in the stack that sit inside it,
    /// recursively (an area inside an area brings its own contents).
    func contents(ofAreas areas: Set<UInt64>) -> (devices: Set<UInt64>, annotations: Set<UInt64>) {
        var devices = Set<UInt64>()
        var annotations = Set<UInt64>()
        var queue = Array(areas)
        while let id = queue.popLast() {
            guard let index = state.annotations.firstIndex(where: { $0.id == id }) else { continue }
            let area = state.annotations[index]
            guard area.kind == .area else { continue }
            let outline = area.style.shape.path(in: area.rect)
            devices.formUnion(state.devices.filter { outline.contains($0.position) }.map(\.id))
            for inner in state.annotations[(index + 1)...]
            where !annotations.contains(inner.id) && !areas.contains(inner.id) && !inner.locked
                && area.rect.contains(inner.rect) && outline.contains(CGPoint(x: inner.rect.midX, y: inner.rect.midY)) {
                annotations.insert(inner.id)
                queue.append(inner.id)
            }
        }
        return (devices, annotations)
    }

    /// Moves devices and annotations by one offset as a single undo step.
    func moveItems(devices: Set<UInt64>, annotations: Set<UInt64>, by delta: CGSize, actionName: String,
                   undoManager: UndoManager?) {
        guard delta != .zero else { return }
        undoManager?.beginUndoGrouping()
        for id in devices {
            guard let device = device(id) else { continue }
            moveDevice(id, to: CGPoint(x: device.x + delta.width, y: device.y + delta.height), undoManager: undoManager)
        }
        for id in annotations {
            guard let annotation = annotation(id), !annotation.locked else { continue }
            updateAnnotation(id, rect: annotation.rect.offsetBy(dx: delta.width, dy: delta.height), text: annotation.text,
                             actionName: actionName, undoManager: undoManager)
        }
        if let undoManager, !undoManager.isUndoing, !undoManager.isRedoing {
            undoManager.setActionName(actionName)
        }
        undoManager?.endUndoGrouping()
    }

    // MARK: Copy and paste

    /// Adds the clipboard's devices around `center`, recreating the cables
    /// between them on the same ports. One undo step; returns the new ids.
    @discardableResult
    func paste(_ clipboard: TopologyClipboard, centeredAt center: CGPoint, actionName: String,
               undoManager: UndoManager?) -> [UInt64] {
        var ids: [UInt64] = []
        for item in clipboard.devices {
            guard let kind = DeviceKind(dragIdentifier: item.kind) else { continue }
            let point = CGPoint(x: center.x + item.dx, y: center.y + item.dy).snapped
            let device = addDevice(kind, at: point, undoManager: undoManager)
            ids.append(device.id)
            // The copy keeps the configuration under its own new hostname.
            if let settings = item.settings, var fields = settings.jsonObject {
                fields["name"] = device.name
                fields["startup"] = nil
                if let json = fields.jsonString { applySettings(device.id, json, undoManager: undoManager) }
            }
            if let color = item.color.flatMap(Swatch.init(identifier:)) {
                setDeviceColor([device.id], to: color, undoManager: undoManager)
            }
        }
        for link in clipboard.links where ids.indices.contains(link.a) && ids.indices.contains(link.b) {
            _ = connect(ids[link.a], ids[link.b], portA: link.portA, portB: link.portB,
                        medium: link.fiber ? .fiber : .copper, undoManager: undoManager)
        }
        if let undoManager, !undoManager.isUndoing, !undoManager.isRedoing, !ids.isEmpty {
            undoManager.setActionName(actionName)
        }
        return ids
    }

    private func refresh() {
        state = project.snapshot()
        snapshots = project.snapshots()
    }

    // MARK: Server DHCP

    func serverDHCP(_ id: UInt64) -> ServerDhcp? {
        project.serverDhcp(id: id)
    }

    func setServerDHCP(_ id: UInt64, _ service: ServerDhcp?, undoManager: UndoManager?) -> Result<Void, CoreError> {
        do {
            let previous = try project.setServerDhcp(id: id, service: service)
            refresh()
            registerUndo(undoManager, name: "Change DHCP Service") { doc, undo in
                _ = doc.setServerDHCP(id, previous, undoManager: undo)
            }
            return .success(())
        } catch let error as CoreError {
            return .failure(error)
        } catch {
            return .failure(.InvalidOperation(message: error.localizedDescription))
        }
    }

    // MARK: Link conditioning

    func setLinkConditioning(_ id: UInt64, _ conditioning: LinkConditioning, undoManager: UndoManager?) {
        guard let previous = try? project.setLinkConditioning(id: id, conditioning: conditioning),
              previous != conditioning else { return }
        refresh()
        registerUndo(undoManager, name: "Change Conditioning") { doc, undo in
            doc.setLinkConditioning(id, previous, undoManager: undo)
        }
    }

    // MARK: Power

    /// Starts, suspends or stops devices (one undo step).
    func setPower(_ power: DevicePower, for ids: [UInt64], undoManager: UndoManager?) {
        var previous: [(UInt64, DevicePower)] = []
        for id in ids {
            if let old = try? project.setPower(id: id, power: power), old != power { previous.append((id, old)) }
        }
        guard !previous.isEmpty else { return }
        refresh()
        let name = switch power {
        case .on: "Start"
        case .suspended: "Suspend"
        case .off: "Stop"
        }
        registerUndo(undoManager, name: name) { doc, undo in
            for (id, old) in previous { doc.setPower(old, for: [id], undoManager: undo) }
        }
    }

    /// IOS `reload` on routers and switches: the running configuration
    /// becomes the saved startup one. Undoable.
    func reload(_ ids: [UInt64], undoManager: UndoManager?) {
        for id in ids where device(id)?.kind.hasCli == true {
            guard let before = settings(of: id), (try? project.reloadDevice(id: id)) != nil else { continue }
            refresh()
            registerUndo(undoManager, name: "Reload") { doc, undo in
                doc.applySettings(id, before, actionName: "Reload", undoManager: undo)
            }
        }
        setPower(.on, for: ids, undoManager: undoManager)
    }

    // MARK: Snapshots

    func takeSnapshot(named name: String, undoManager: UndoManager?) {
        let index = project.takeSnapshot(name: name, taken: UInt64(Date().timeIntervalSince1970))
        refresh()
        registerUndo(undoManager, name: "Take Snapshot") { doc, undo in
            doc.deleteSnapshot(Int(index), undoManager: undo)
        }
    }

    func restoreSnapshot(_ index: Int, undoManager: UndoManager?) -> Result<Void, CoreError> {
        do {
            let replaced = try project.restoreSnapshot(index: UInt32(index))
            refresh()
            registerUndo(undoManager, name: "Restore Snapshot") { doc, undo in
                doc.putBack(replaced, undoManager: undo, redo: index)
            }
            return .success(())
        } catch let error as CoreError {
            return .failure(error)
        } catch {
            return .failure(.InvalidOperation(message: error.localizedDescription))
        }
    }

    private func putBack(_ state: SavedState, undoManager: UndoManager?, redo index: Int) {
        try? project.setState(state: state)
        refresh()
        registerUndo(undoManager, name: "Restore Snapshot") { doc, undo in
            _ = doc.restoreSnapshot(index, undoManager: undo)
        }
    }

    func deleteSnapshot(_ index: Int, undoManager: UndoManager?) {
        guard let removed = try? project.deleteSnapshot(index: UInt32(index)) else { return }
        refresh()
        registerUndo(undoManager, name: "Delete Snapshot") { doc, undo in
            doc.project.insertSnapshot(index: UInt32(index), snapshot: removed)
            doc.refresh()
            doc.registerUndo(undo, name: "Delete Snapshot") { doc, undo in
                doc.deleteSnapshot(index, undoManager: undo)
            }
        }
    }

    private func registerUndo(
        _ undoManager: UndoManager?,
        name: String,
        _ inverse: @escaping @MainActor (ProjectDocument, UndoManager?) -> Void
    ) {
        guard let undoManager else { return }
        undoManager.registerUndo(withTarget: self) { doc in
            MainActor.assumeIsolated { inverse(doc, undoManager) }
        }
        // While undoing or redoing, UndoManager carries the original action
        // name over to the inverse; renaming it would mislabel the Edit menu.
        if !undoManager.isUndoing && !undoManager.isRedoing {
            undoManager.setActionName(name)
        }
    }
}

extension ProjectDocument.StackMove {
    var title: String {
        switch self {
        case .front: "Bring to Front"
        case .forward: "Bring Forward"
        case .backward: "Send Backward"
        case .back: "Send to Back"
        }
    }
}
