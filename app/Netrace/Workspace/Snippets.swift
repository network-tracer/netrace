import Foundation

/// A reusable block of configuration with fill-in fields, written as
/// `{{Label}}` or `{{Label=default}}`. Built-in snippets cover the setup
/// every CCNA lab repeats; students add their own.
struct Snippet: Codable, Identifiable, Hashable {
    var id = UUID()
    var name: String
    var body: String
    var isBuiltIn = false

    struct Field: Hashable, Identifiable {
        let label: String
        let defaultValue: String
        var id: String { label }
    }

    /// The fields in order of first appearance.
    var fields: [Field] {
        var seen: [Field] = []
        for match in body.matches(of: /\{\{([^}]+)\}\}/) {
            let parts = match.1.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            let field = Field(label: parts[0], defaultValue: parts.count > 1 ? parts[1] : "")
            if !seen.contains(where: { $0.label == field.label }) { seen.append(field) }
        }
        return seen
    }

    /// The body with every field replaced by its value.
    func filled(with values: [String: String]) -> String {
        body.replacing(/\{\{([^}]+)\}\}/) { match in
            let label = match.1.split(separator: "=", maxSplits: 1)[0].trimmingCharacters(in: .whitespaces)
            return values[label] ?? ""
        }
    }
}

/// The snippet library: built-ins plus the user's own, kept in the app's
/// preferences so they're available in every project.
@MainActor
final class SnippetStore: ObservableObject {
    static let shared = SnippetStore()
    private static let key = "netrace.snippets"

    @Published var custom: [Snippet] = [] {
        didSet { save() }
    }

    var all: [Snippet] { Self.builtIn + custom }

    private init() {
        if let data = UserDefaults.standard.data(forKey: Self.key),
           let saved = try? JSONDecoder().decode([Snippet].self, from: data) {
            custom = saved
        }
    }

    private func save() {
        if let data = try? JSONEncoder().encode(custom) {
            UserDefaults.standard.set(data, forKey: Self.key)
        }
    }

    static let builtIn: [Snippet] = [
        Snippet(name: "Basic device setup", body: """
            hostname {{Hostname}}
            no ip domain-lookup
            enable secret {{Enable secret=class}}
            banner motd #{{Banner=Authorised access only}}#
            line console 0
             password {{Console password=cisco}}
             login
             logging synchronous
            line vty 0 4
             password {{VTY password=cisco}}
             login
            service password-encryption
            """, isBuiltIn: true),
        Snippet(name: "Interface address", body: """
            interface {{Interface=g0/0}}
             description {{Description=LAN}}
             ip address {{Address=192.168.1.1}} {{Mask=255.255.255.0}}
             no shutdown
            """, isBuiltIn: true),
        Snippet(name: "Static route", body: """
            ip route {{Network}} {{Mask=255.255.255.0}} {{Next hop}}
            """, isBuiltIn: true),
        Snippet(name: "Default route", body: """
            ip route 0.0.0.0 0.0.0.0 {{Next hop}}
            """, isBuiltIn: true),
        Snippet(name: "Loopback", body: """
            interface loopback {{Number=0}}
             ip address {{Address}} {{Mask=255.255.255.255}}
            """, isBuiltIn: true),
        Snippet(name: "Create VLANs", body: """
            vlan {{First VLAN=10}}
             name {{First name=Sales}}
            vlan {{Second VLAN=20}}
             name {{Second name=Staff}}
            """, isBuiltIn: true),
        Snippet(name: "Access ports", body: """
            interface range {{Ports=f0/1 - 12}}
             switchport mode access
             switchport access vlan {{VLAN=10}}
             spanning-tree portfast
            """, isBuiltIn: true),
        Snippet(name: "Trunk port", body: """
            interface {{Port=g0/1}}
             switchport mode trunk
             switchport trunk native vlan {{Native VLAN=1}}
             switchport trunk allowed vlan {{Allowed VLANs=all}}
            """, isBuiltIn: true),
        Snippet(name: "Router-on-a-stick subinterface", body: """
            interface {{Port=g0/0}}
             no shutdown
            interface {{Port=g0/0}}.{{VLAN=10}}
             encapsulation dot1Q {{VLAN=10}}
             ip address {{Gateway address=192.168.10.1}} {{Mask=255.255.255.0}}
            """, isBuiltIn: true),
        Snippet(name: "Port security", body: """
            interface {{Port=f0/1}}
             switchport mode access
             switchport port-security
             switchport port-security maximum {{Maximum=1}}
             switchport port-security mac-address sticky
             switchport port-security violation {{Violation=shutdown}}
            """, isBuiltIn: true),
        Snippet(name: "Switch management address", body: """
            interface vlan 1
             ip address {{Address}} {{Mask=255.255.255.0}}
             no shutdown
            exit
            ip default-gateway {{Default gateway}}
            """, isBuiltIn: true),
    ]
}
