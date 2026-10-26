import Foundation

/// Owns a window's simulation: the Rust `Simulation` for the current
/// topology, its trace and its ping results. Rebuilt (and its pings
/// cleared) when the topology changes; moving devices doesn't count.
@MainActor
final class SimulationController: ObservableObject {
    @Published private(set) var trace: [TraceEventInfo] = []
    @Published private(set) var sessions: [PingSessionInfo] = []
    @Published private(set) var warnings: [String] = []
    /// VLANs, trunks and spanning tree for the canvas overlay.
    @Published private(set) var layer2: Layer2Overlay?

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

    // MARK: Consoles

    /// One console's scrollback and its prompt.
    struct Transcript: Equatable {
        var text = ""
        var prompt = ""
        /// The next line is a password.
        var secret = false
        var history: [String] = []
    }

    @Published private(set) var transcripts: [UInt64: Transcript] = [:]
    /// IOS sessions keep their mode between lines, and across topology changes.
    private var consoles: [UInt64: Console] = [:]

    /// Opens (or keeps) a device's console.
    func openConsole(_ device: DeviceInfo, document: ProjectDocument) {
        guard transcripts[device.id] == nil else { return }
        if device.kind.isHost {
            let address = device.ip.map { "\($0.address)/\($0.prefixLen)" } ?? "no IP address yet"
            transcripts[device.id] = Transcript(
                text: "\(device.name) command prompt (\(address)). Type help for commands.\n",
                prompt: "C:\\>")
            return
        }
        guard device.kind.hasCli else {
            transcripts[device.id] = Transcript(text: "\(device.name) has no command line.\n", prompt: "")
            return
        }
        let console = Console(device: device.id)
        consoles[device.id] = console
        let start = document.startConsole(console)
        transcripts[device.id] = Transcript(
            text: start.output + (start.secret ? "" : "\nPress RETURN to get started.\n\n"),
            prompt: start.prompt, secret: start.secret)
    }

    func closeConsole(_ device: UInt64) {
        transcripts[device] = nil
        consoles[device] = nil
    }

    /// Runs one line in a device's console.
    func submit(_ line: String, on device: DeviceInfo, document: ProjectDocument, undoManager: UndoManager?) {
        guard var transcript = transcripts[device.id] else { return }
        let echoed = transcript.secret ? "" : line
        transcript.text += transcript.prompt + echoed + "\n"
        if !transcript.secret, !line.trimmingCharacters(in: .whitespaces).isEmpty {
            transcript.history.append(line)
        }
        if device.kind.isHost {
            let command = line.trimmingCharacters(in: .whitespaces)
            if ["cls", "clear"].contains(command.lowercased()) {
                transcript.text = ""
            } else if !command.isEmpty {
                transcript.text += hostCommand(command, on: device.id) + "\n"
            }
            transcripts[device.id] = transcript
            return
        }
        guard let console = consoles[device.id] else { return }
        sync(with: document)
        guard let simulation else { return }
        let response = document.runConsole(console, device: device.id, line: line, simulation: simulation,
                                           at: endTime, undoManager: undoManager)
        transcript.text += response.output
        transcript.prompt = response.prompt
        transcript.secret = response.secret
        transcripts[device.id] = transcript
        // A configuration change rebuilds the simulation; anything else
        // (a ping, a traceroute) adds to its trace.
        if !sync(with: document) { refresh() }
    }

    /// Runs each line in turn, as pasting into a real console does: a line
    /// with a mistake reports its error and the next lines still run. One
    /// undo step for all of them.
    func submit(lines text: String, on device: DeviceInfo, document: ProjectDocument, undoManager: UndoManager?,
                actionName: String = "Paste Configuration") {
        var lines = text.split(omittingEmptySubsequences: false, whereSeparator: \.isNewline).map(String.init)
        // The final newline ends the last line; it isn't an extra empty one.
        if lines.last == "" { lines.removeLast() }
        undoManager?.beginUndoGrouping()
        for line in lines where !line.trimmingCharacters(in: .whitespaces).hasPrefix("!") {
            submit(line, on: device, document: document, undoManager: undoManager)
        }
        undoManager?.setActionName(actionName)
        undoManager?.endUndoGrouping()
    }

    /// Runs configuration lines from whatever mode the console is in:
    /// enters configuration mode first and returns to privileged EXEC after.
    func configure(_ text: String, on device: DeviceInfo, document: ProjectDocument, undoManager: UndoManager?,
                   actionName: String) {
        openConsole(device, document: document)
        guard let prompt = transcripts[device.id]?.prompt, !(transcripts[device.id]?.secret ?? false) else { return }
        var lines: [String] = []
        if prompt.hasSuffix(">") { lines += ["enable", "configure terminal"] }
        else if prompt.hasSuffix("#") && !prompt.contains("(config") { lines.append("configure terminal") }
        else if prompt.contains("(config-") { lines.append("exit") }
        lines.append(text)
        lines.append("end")
        submit(lines: lines.joined(separator: "\n"), on: device, document: document, undoManager: undoManager,
               actionName: actionName)
    }

    /// `?` in an IOS console: shows what can come next, keeping the line.
    func help(_ line: String, on device: DeviceInfo, document: ProjectDocument) {
        guard var transcript = transcripts[device.id], let console = consoles[device.id] else { return }
        transcript.text += transcript.prompt + line + "?\n" + document.consoleHelp(console, line: line)
        transcripts[device.id] = transcript
    }

    /// Tab in an IOS console.
    func complete(_ line: String, on device: DeviceInfo, document: ProjectDocument) -> String? {
        guard let console = consoles[device.id] else { return nil }
        return document.consoleComplete(console, line: line)
    }

    /// Follows a rename (`hostname`, the inspector) in an open console's prompt.
    func refreshPrompts(document: ProjectDocument) {
        for (id, console) in consoles {
            guard var transcript = transcripts[id], !transcript.secret,
                  transcript.prompt.hasSuffix(">") || transcript.prompt.hasSuffix("#") else { continue }
            transcript.prompt = document.consolePrompt(console)
            if transcripts[id] != transcript { transcripts[id] = transcript }
        }
    }

    func stpView(vlan: UInt16) -> StpView? {
        simulation?.stpView(vlan: vlan)
    }

    func stpLinkFailure(link: UInt64, vlan: UInt16) -> StpFailure? {
        simulation?.stpLinkFailure(linkId: link, vlan: vlan)
    }

    func macAddress(of device: UInt64, port: String) -> String? {
        simulation?.macAddress(device: device, port: port)
    }

    func event(_ id: UInt64) -> TraceEventInfo? {
        trace.first { $0.id == id }
    }

    func refresh() {
        trace = simulation?.trace() ?? []
        sessions = simulation?.sessions() ?? []
        warnings = simulation?.warnings() ?? []
        let overlay = simulation?.layer2Overlay()
        if overlay != layer2 { layer2 = overlay }
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
