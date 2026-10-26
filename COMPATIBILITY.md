# IOS compatibility

netrace's command line follows Cisco IOS 15.x as documented and as seen in CCNA lab
transcripts. **Nothing here has been verified against real IOS or Packet Tracer yet**: the
"Verified" columns are the plan, filled in as golden transcripts are captured (see SCOPE.md,
*Compatibility with Cisco IOS and Packet Tracer*).

Status: ✅ implemented · 🟡 implemented with known differences · ⬜ not yet · — not applicable.
Verified: blank until checked; then the IOS version / Packet Tracer version it was checked against.

## Modes and editing

| Behaviour | netrace | Verified IOS | Verified PT | Notes |
|---|---|---|---|---|
| User / privileged / config / interface / subinterface / line / vlan / interface range modes | ✅ | | | |
| Unique abbreviations, `% Ambiguous command` | ✅ | | | netrace has fewer commands, so some prefixes IOS calls ambiguous are unique here |
| `?` (word and next-token forms), Tab completion | ✅ | | | |
| `% Invalid input detected at '^' marker.` caret position | ✅ | | | |
| `do`, `no`, `end`, `exit`, Ctrl-Z | ✅ | | | |
| Domain lookup on a mistyped word | 🟡 | | | Message shown, no delay |
| History (↑ ↓), `show history` | ✅ | | | |

## Configuration commands

| Command | netrace | Verified IOS | Verified PT | Notes |
|---|---|---|---|---|
| `hostname`, `enable secret/password`, `service password-encryption`, `banner motd` | ✅ | | | Secret shown with a stand-in hash |
| `line console 0` / `line vty 0 4`: `password`, `login`, `logging synchronous`, `exec-timeout` | ✅ | | | |
| `interface`, `ip address`, `shutdown`, `description` | ✅ | | | |
| `ip route` (next hop, exit interface, distance) | ✅ | | | |
| `encapsulation dot1Q N [native]` | ✅ | | | |
| `vlan`, `name`, `switchport mode access/trunk`, `access vlan`, `trunk native/allowed vlan` | ✅ | | | |
| `switchport mode dynamic desirable` | 🟡 | | | No DTP: acts as access |
| `spanning-tree mode`, `vlan N priority`, `root primary/secondary`, `portfast`, `bpduguard` | ✅ | | | Converged tree, no timers |
| `switchport port-security ...` | ✅ | | | Sticky MACs aren't written back to the running-config |
| `cdp run`, `cdp enable`, `lldp run` | ✅ | | | |
| `ip default-gateway`, `ip routing`, `no switchport` | ✅ | | | |
| DHCP, ACLs, NAT, OSPF | ⬜ | | | M5 |

## show commands

| Command | netrace | Verified IOS | Verified PT |
|---|---|---|---|
| `show running-config`, `show startup-config` | ✅ | | |
| `show ip interface brief`, `show interfaces [X]` | ✅ | | |
| `show ip route` | ✅ | | |
| `show arp`, `show mac address-table` | ✅ | | |
| `show vlan [brief]`, `show interfaces trunk/status/X switchport` | ✅ | | |
| `show spanning-tree [vlan N]` | ✅ | | |
| `show cdp neighbors [detail]`, `show lldp neighbors` | ✅ | | |
| `show port-security [interface X/address]` | ✅ | | |
| `show version` | 🟡 | | | Simulated platform text |

## EXEC actions

| Command | netrace | Verified IOS | Verified PT | Notes |
|---|---|---|---|---|
| `ping` (router: `!.U`, success rate; PC: Windows format) | ✅ | | | First packet dropped while ARP resolves, as on IOS |
| `traceroute` / `tracert` | ✅ | | | ICMP probes on both (IOS really uses UDP) |
| `copy run start`, `write`, `erase startup-config`, `reload` with prompts | ✅ | | | |
