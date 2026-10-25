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
