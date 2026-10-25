import Foundation

/// Owns a window's simulation: the Rust `Simulation` for the current
/// topology, its trace and its ping results. Rebuilt (and its pings
/// cleared) when the topology changes; moving devices doesn't count.
@MainActor
final class SimulationController: ObservableObject {
    @Published private(set) var trace: [TraceEventInfo] = []
    @Published private(set) var sessions: [PingSessionInfo] = []
    @Published private(set) var warnings: [String] = []

    /// Seconds a frame takes to cross one cable.
    let hopLatency = Double(hopLatencyNs()) / 1_000_000_000

    private var simulation: Simulation?
    private var topologyKey = ""

    /// Time of the last thing that happened, in seconds.
    var endTime: Double { trace.last?.time ?? 0 }

    /// Rebuilds the simulation if the topology changed. Returns true if
    /// that discarded pings the user had sent.
    @discardableResult
    func sync(with document: ProjectDocument) -> Bool {
        let key = document.topologyKey
        guard key != topologyKey || simulation == nil else { return false }
        let hadActivity = !sessions.isEmpty
        topologyKey = key
        simulation = document.makeSimulation()
        refresh()
        return hadActivity
    }

    /// Sends `count` pings at `time` (seconds). Returns the new session.
    func ping(from source: UInt64, to destination: String, count: UInt16 = 1, at time: Double) throws -> PingSessionInfo? {
        guard let simulation else { return nil }
        let id = try simulation.ping(source: source, destination: destination, count: count, atNs: nanoseconds(time))
        refresh()
        return sessions.first { $0.id == id }
    }

    /// Runs a command at an end device's prompt (ping, ipconfig, arp -a).
    func hostCommand(_ line: String, on device: UInt64) -> String {
        guard let simulation else { return "" }
        let output = simulation.hostCommand(device: device, line: line, atNs: nanoseconds(endTime))
        refresh()
        return output
    }

    func macAddress(of device: UInt64, port: String) -> String? {
        simulation?.macAddress(device: device, port: port)
    }

    func event(_ id: UInt64) -> TraceEventInfo? {
        trace.first { $0.id == id }
    }

    private func refresh() {
        trace = simulation?.trace() ?? []
        sessions = simulation?.sessions() ?? []
        warnings = simulation?.warnings() ?? []
    }

    private func nanoseconds(_ seconds: Double) -> UInt64 {
        UInt64(max(0, seconds) * 1_000_000_000)
    }
}

extension TraceEventInfo: Identifiable {
    /// Seconds.
    var time: Double { Double(timeNs) / 1_000_000_000 }
    var sentTime: Double? { sentNs.map { Double($0) / 1_000_000_000 } }
    var netProtocol: NetProtocol? { self.protocol.flatMap(NetProtocol.init(rawValue:)) }
    var isRejection: Bool { outcome == .ignored || outcome == .dropped }
}

extension TraceOutcome {
    var title: String {
        switch self {
        case .created: "Created"
        case .forwarded: "Forwarded"
        case .flooded: "Flooded"
        case .replied: "Replied"
        case .accepted: "Accepted"
        case .ignored: "Ignored"
        case .dropped: "Dropped"
        case .completed: "Done"
        }
    }
}

extension PingSessionInfo {
    var succeeded: Bool { received > 0 }
}
