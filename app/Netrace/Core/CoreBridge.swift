import Foundation

// Swift-side conveniences on the types generated from the Rust core
// (Generated/netrace_ffi.swift). Keep UI concerns out of here.

extension DeviceInfo: Identifiable {}
extension LinkInfo: Identifiable {}

extension DeviceInfo {
    var position: CGPoint { CGPoint(x: x, y: y) }
}

extension DeviceKind: CaseIterable, Identifiable {
    public static let allCases: [DeviceKind] = [.router, .l2Switch, .l3Switch, .pc, .server]

    public var id: Self { self }

    /// Stable string used for drag-and-drop between the library and canvas.
    var dragIdentifier: String {
        switch self {
        case .router: "router"
        case .l2Switch: "l2-switch"
        case .l3Switch: "l3-switch"
        case .pc: "pc"
        case .server: "server"
        }
    }

    init?(dragIdentifier: String) {
        guard let kind = Self.allCases.first(where: { $0.dragIdentifier == dragIdentifier }) else { return nil }
        self = kind
    }

    var ports: [String] { portNames(kind: self) }
}

extension CoreError {
    /// Human-readable message from the core. (The generated
    /// `errorDescription` is a debug dump, unsuitable for alerts.)
    var message: String {
        switch self {
        case .InvalidDocument(let message), .InvalidOperation(let message): message
        }
    }
}
