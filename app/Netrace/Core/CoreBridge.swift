import Foundation

// Swift-side conveniences on the types generated from the Rust core
// (Generated/netrace_ffi.swift). Keep UI concerns out of here.

extension DeviceInfo: Identifiable {}
extension LinkInfo: Identifiable {}
extension AnnotationInfo: Identifiable {}

extension AnnotationInfo {
    var rect: CGRect { CGRect(x: x, y: y, width: width, height: height) }
}

extension DeviceInfo {
    var position: CGPoint { CGPoint(x: x, y: y) }
}

extension LinkInfo {
    func endpoint(for deviceId: UInt64) -> EndpointInfo? {
        a.deviceId == deviceId ? a : (b.deviceId == deviceId ? b : nil)
    }

    func otherEnd(from deviceId: UInt64) -> EndpointInfo {
        a.deviceId == deviceId ? b : a
    }
}

extension DeviceKind: CaseIterable, Identifiable {
    public static let allCases: [DeviceKind] = [
        .router, .l2Switch, .l3Switch, .hub, .firewall, .accessPoint, .cloud,
        .pc, .laptop, .server, .printer, .ipPhone, .smartphone,
    ]

    public var id: Self { self }

    /// Stable string used for drag-and-drop between the library and canvas.
    var dragIdentifier: String { "device:\(self)" }

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
