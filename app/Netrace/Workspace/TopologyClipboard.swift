import Foundation

/// Devices and the cables between them, as copied to the pasteboard.
/// Stored as JSON text so it also pastes readably into a text editor.
/// Positions are relative to the selection's centre; names are not kept
/// (pasted devices get fresh hostnames, as hostnames must be unique).
struct TopologyClipboard: Codable {
    static let marker = "netrace-clipboard/1"

    struct Device: Codable {
        var kind: String
        var dx: Double
        var dy: Double
    }

    struct Link: Codable {
        var a: Int
        var portA: String
        var b: Int
        var portB: String
        var fiber: Bool
    }

    var format = Self.marker
    var devices: [Device]
    var links: [Link]

    /// Captures the selected devices and every cable whose both ends are selected.
    init?(selection: Set<UInt64>, in snapshot: ProjectSnapshot) {
        let chosen = snapshot.devices.filter { selection.contains($0.id) }
        guard !chosen.isEmpty else { return nil }
        let cx = chosen.map(\.x).reduce(0, +) / Double(chosen.count)
        let cy = chosen.map(\.y).reduce(0, +) / Double(chosen.count)
        let index = Dictionary(uniqueKeysWithValues: chosen.enumerated().map { ($1.id, $0) })
        devices = chosen.map { Device(kind: $0.kind.dragIdentifier, dx: $0.x - cx, dy: $0.y - cy) }
        links = snapshot.links.compactMap { link in
            guard let a = index[link.a.deviceId], let b = index[link.b.deviceId] else { return nil }
            return Link(a: a, portA: link.a.port, b: b, portB: link.b.port, fiber: link.medium == .fiber)
        }
    }

    init?(json: String) {
        guard let data = json.data(using: .utf8),
              let decoded = try? JSONDecoder().decode(Self.self, from: data),
              decoded.format == Self.marker else { return nil }
        self = decoded
    }

    var json: String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        return (try? encoder.encode(self)).flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
    }
}

/// Where to put new devices: near a point, clear of existing ones.
enum Placement {
    static func freeSpot(near center: CGPoint, avoiding taken: [CGPoint]) -> CGPoint {
        let step = Theme.Size.gridSpacing * 4
        let clearance = step * 0.8
        let origin = center.snapped
        func isFree(_ p: CGPoint) -> Bool {
            taken.allSatisfy { hypot($0.x - p.x, $0.y - p.y) >= clearance }
        }
        if isFree(origin) { return origin }
        // Walk outward in square rings around the centre.
        for ring in 1...12 {
            for dy in -ring...ring {
                for dx in -ring...ring where abs(dx) == ring || abs(dy) == ring {
                    let p = CGPoint(x: origin.x + CGFloat(dx) * step, y: origin.y + CGFloat(dy) * step)
                    if p.x > 0, p.y > 0, isFree(p) { return p }
                }
            }
        }
        return origin
    }
}
