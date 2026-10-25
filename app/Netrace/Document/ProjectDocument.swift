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

    init() {
        project = Project()
        _state = Published(initialValue: project.snapshot())
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

    // MARK: Annotations

    func annotation(_ id: UInt64) -> AnnotationInfo? {
        state.annotations.first { $0.id == id }
    }

    @discardableResult
    func addAnnotation(_ kind: AnnotationKind, in rect: CGRect, text: String, undoManager: UndoManager?) -> AnnotationInfo {
        let annotation = project.addAnnotation(kind: kind, x: rect.minX, y: rect.minY,
                                               width: rect.width, height: rect.height, text: text)
        refresh()
        registerUndo(undoManager, name: kind == .note ? "Add Note" : "Add Area") { doc, undo in
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
        registerUndo(undoManager, name: removed.annotation.kind == .note ? "Delete Note" : "Delete Area") { doc, undo in
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
            ids.append(addDevice(kind, at: point, undoManager: undoManager).id)
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
