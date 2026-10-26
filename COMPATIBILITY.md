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
| `monitor session N source interface X [both/rx/tx]`, `monitor session N destination interface Y`, `no monitor session N` | ✅ | | | Local SPAN on switches; copies leave untagged; the destination port takes no traffic in |
| `ip default-gateway`, `ip routing`, `no switchport` | ✅ | | | |
| `ip dhcp pool`, `network`, `default-router`, `dns-server`, `domain-name`, `ip dhcp excluded-address` | ✅ | | | No lease timers: leases last for the run |
| `ip helper-address` | ✅ | | | Relays DHCP only |
| `access-list N permit/deny/remark` (standard and extended), `ip access-list standard/extended NAME`, sequence numbers | ✅ | | | TCP and UDP port matches act on real traffic (PC `curl` / `telnet`, DHCP) |
| `ip access-group N in/out` | ✅ | | | |
| `ip nat inside`, `ip nat outside` | ✅ | | | |
| `ip nat pool NAME A B netmask M / prefix-length N` | ✅ | | | |
| `ip nat inside source static L G` | ✅ | | | The router answers ARP for inside global addresses on the outside subnet, as IOS does |
| `ip nat inside source list N pool P [overload]`, `... interface X [overload]` | ✅ | | | Inside source only; ICMP, UDP and TCP translated; translations don't time out; no `clear ip nat translation` |
| `router ospf N`: `network A W area N`, `router-id`, `passive-interface [default]`, `default-information originate`, `auto-cost reference-bandwidth` | ✅ | | | One process per router. Shown converged: a new `router-id` applies at once (IOS waits for `clear ip ospf process`) and a DR election runs as if all routers started together (IOS keeps an existing DR) |
| `ip ospf cost / priority / hello-interval / dead-interval`, `ip ospf N area A`, `ip ospf network point-to-point` | ✅ | | | |
| Multi-area (ABR summaries, `O IA`), `O*E2` default | ✅ | | | No stub / NSSA areas, no area ranges, no authentication, no MTU checks |

## show commands

| Command | netrace | Verified IOS | Verified PT |
|---|---|---|---|
| `show running-config`, `show startup-config` | ✅ | | |
| `show ip interface brief`, `show interfaces [X]` | ✅ | | |
| `show ip dhcp binding`, `show ip dhcp pool` | ✅ | | |
| `show access-lists [X]`, `show ip access-lists [X]` (with match counts) | ✅ | | |
| `show ip nat translations [verbose]`, `show ip nat statistics` | ✅ | | |
| `show ip ospf neighbor`, `show ip ospf interface [brief]`, `show ip ospf database`, `show ip ospf`, `show ip protocols`, `show ip route ospf` | ✅ | | |
| `show ip route` | ✅ | | |
| `show arp`, `show mac address-table` | ✅ | | |
| `show vlan [brief]`, `show interfaces trunk/status/X switchport` | ✅ | | |
| `show spanning-tree [vlan N]` | ✅ | | |
| `show cdp neighbors [detail]`, `show lldp neighbors` | ✅ | | |
| `show port-security [interface X/address]` | ✅ | | |
| `show monitor` | ✅ | | |
| `show version` | 🟡 | | | Simulated platform text |

## EXEC actions

| Command | netrace | Verified IOS | Verified PT | Notes |
|---|---|---|---|---|
| `ping` (router: `!.U`, success rate; PC: Windows format) | ✅ | | | First packet dropped while ARP resolves, as on IOS |
| `traceroute` / `tracert` | ✅ | | | ICMP probes on both (IOS really uses UDP) |
| PC `curl <address>[:port]`, `telnet <address> [port]` | ✅ | | | One request and response over TCP, then close; servers listen on 21, 22, 23, 25, 80 and 443, routers and switches on 23; no remote shell |
| `copy run start`, `write`, `erase startup-config`, `reload` with prompts | ✅ | | | |
